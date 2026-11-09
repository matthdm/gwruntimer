//! TIPS live tracking: tells the TIPS run board which map the runner is in, and (only if they switch
//! it on) where inside it. Everything network-related lives here, on its own thread.
//!
//! Rules (docs/tracker-protocol.md in the tips-run repo):
//! - Nothing is sent until the runner presses Start run, and nothing after End run.
//! - Only these fields are sent: client_session (random per launch), map_id, is_explorable, and x/y
//!   when "Share position" is on. Never a name, account, party, chat or anything else from game memory.
//! - The game's render thread never waits on the network: it only touches a mutex and a bounded channel.
//! - HTTPS only, to the compiled-in host. Responses are size-capped and only a few fields are read.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::position::{Pos, MAX_COORD};

/// The TIPS site. Set `TIPS_API_BASE` when building a release (the Azure host isn't fixed yet).
pub const API_BASE: &str = match option_env!("TIPS_API_BASE") {
    Some(v) => v,
    None => "https://tips-runs.invalid",
};

const MAX_RESPONSE: u64 = 8 * 1024;
const MAX_TOKEN: usize = 100;
/// A position older than this is not sent.
const POSITION_FRESH: Duration = Duration::from_secs(15);

// ---------- settings ----------

/// Saved next to the DLL as `tips_tracker.cfg`. The token is like a password: it can post as this runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub token: String,
    /// Off unless the runner turns it on. Sends x/y inside the map.
    pub share_position: bool,
    /// Index into `position::Source::ALL`.
    pub position_source: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config { token: String::new(), share_position: false, position_source: crate::position::Source::MovementA.to_index() }
    }
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "token" if v.len() <= MAX_TOKEN => c.token = v.to_string(),
                "share_position" => c.share_position = v == "1",
                "position_source" => {
                    if let Ok(i) = v.parse() {
                        c.position_source = i;
                    }
                }
                _ => {}
            }
        }
        c
    }

    pub fn serialize(&self) -> String {
        format!("token={}\nshare_position={}\nposition_source={}\n", self.token, u8::from(self.share_position), self.position_source)
    }
}

// ---------- what the panel shows ----------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NextRun {
    pub id: String,
    pub title: String,
    pub status: String,
    pub route_name: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub next_run: Option<NextRun>,
    /// The run this tracker is reporting on (between Start run and End run).
    pub active_run: Option<String>,
    pub progress: Option<i64>,
    pub off_route: bool,
    pub message: String,
    pub busy: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub heartbeat: Duration,
    /// Heartbeat interval while sharing position, so the marker moves.
    pub position_beat: Duration,
    pub retry_base: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { heartbeat: Duration::from_secs(30), position_beat: Duration::from_secs(5), retry_base: Duration::from_secs(1) }
    }
}

#[derive(Default)]
struct Latest {
    map_id: Option<u32>,
    explorable: bool,
    pos: Option<(Pos, Instant)>,
}

struct Shared {
    status: Mutex<Status>,
    config: Mutex<Config>,
    latest: Mutex<Latest>,
    config_path: Option<PathBuf>,
}

enum Cmd {
    Refresh,
    Start,
    End,
    Zone,
}

pub struct Uplink {
    tx: SyncSender<Cmd>,
    shared: Arc<Shared>,
}

impl Uplink {
    /// The real thing: compiled-in host, HTTPS only (a debug build may talk to http://localhost).
    pub fn new(config_path: Option<PathBuf>) -> Uplink {
        let base = std::env::var("TIPS_API_BASE").ok().filter(|_| cfg!(debug_assertions)).unwrap_or_else(|| API_BASE.to_string());
        let allow_http = cfg!(debug_assertions) && base.starts_with("http://localhost");
        Uplink::with_base(&base, allow_http, config_path, Timing::default())
    }

    pub fn with_base(base: &str, allow_http: bool, config_path: Option<PathBuf>, timing: Timing) -> Uplink {
        let config = config_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Config::parse(&t)).unwrap_or_default();
        let shared = Arc::new(Shared {
            status: Mutex::new(Status::default()),
            config: Mutex::new(config),
            latest: Mutex::new(Latest::default()),
            config_path,
        });
        let (tx, rx) = sync_channel(64);
        let worker = Worker::new(base, allow_http, timing, shared.clone());
        thread::Builder::new().name("tips-uplink".into()).spawn(move || worker.run(rx)).ok();
        Uplink { tx, shared }
    }

    fn send(&self, cmd: Cmd) {
        // Never block the game: if the queue is somehow full, drop the command.
        if let Err(TrySendError::Full(_)) = self.tx.try_send(cmd) {
            tracing::warn!("tips uplink queue full; dropped a command");
        }
    }

    pub fn config(&self) -> Config {
        self.shared.config.lock().unwrap().clone()
    }

    /// Whether position sharing is on (cheap enough to call every frame).
    pub fn sharing(&self) -> bool {
        self.shared.config.lock().unwrap().share_position
    }

    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }

    /// Updates and saves the settings.
    pub fn update_config(&self, f: impl FnOnce(&mut Config)) {
        let mut c = self.shared.config.lock().unwrap();
        f(&mut c);
        c.token = c.token.trim().chars().take(MAX_TOKEN).collect();
        if let Some(path) = &self.shared.config_path {
            if let Err(e) = std::fs::write(path, c.serialize()) {
                tracing::error!("couldn't save tips_tracker.cfg: {e}");
            }
        }
    }

    /// A map finished loading. Cheap and non-blocking; call it from the render loop.
    pub fn push_zone(&self, map_id: u32, explorable: bool) {
        {
            let mut l = self.shared.latest.lock().unwrap();
            l.map_id = Some(map_id);
            l.explorable = explorable;
            l.pos = None; // the old position belongs to the old map
        }
        self.send(Cmd::Zone);
    }

    /// The character's latest position, if it could be read. Only ever sent when sharing is on.
    pub fn set_position(&self, pos: Option<Pos>) {
        self.shared.latest.lock().unwrap().pos = pos.filter(Pos::is_sane).map(|p| (p, Instant::now()));
    }

    pub fn refresh(&self) {
        self.send(Cmd::Refresh);
    }
    pub fn start_run(&self) {
        self.send(Cmd::Start);
    }
    pub fn end_run(&self) {
        self.send(Cmd::End);
    }
}

// ---------- HTTP ----------

#[derive(Debug)]
enum ApiError {
    /// The server answered with an error status.
    Http(u16, String),
    /// Couldn't reach it, or the answer wasn't usable.
    Network(String),
}

impl ApiError {
    fn retryable(&self) -> bool {
        matches!(self, ApiError::Network(_)) || matches!(self, ApiError::Http(c, _) if *c == 429 || *c >= 500)
    }
    fn text(&self) -> String {
        match self {
            ApiError::Http(c, m) => format!("{m} ({c})"),
            ApiError::Network(m) => format!("Can't reach TIPS: {m}"),
        }
    }
}

struct Api {
    agent: ureq::Agent,
    base: String,
}

impl Api {
    fn new(base: &str, allow_http: bool) -> Result<Api, String> {
        let base = base.trim_end_matches('/').to_string();
        if !base.starts_with("https://") && !(allow_http && base.starts_with("http://")) {
            return Err("The tracker only talks to TIPS over HTTPS.".into());
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(0)
            .https_only(!allow_http)
            .http_status_as_error(false)
            .build();
        Ok(Api { agent: config.into(), base })
    }

    fn finish(res: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Value, ApiError> {
        let mut resp = res.map_err(|e| ApiError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE)
            .read_to_string()
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            Ok(value)
        } else {
            // Only the server's `error` text is ever shown, and only as plain text.
            let msg = value.get("error").and_then(Value::as_str).unwrap_or("Request failed").chars().take(120).collect();
            Err(ApiError::Http(status, msg))
        }
    }

    fn get(&self, path: &str, token: &str) -> Result<Value, ApiError> {
        Self::finish(self.agent.get(format!("{}{path}", self.base)).header("X-Tracker-Token", token).call())
    }

    fn post(&self, path: &str, token: &str, body: &Value) -> Result<Value, ApiError> {
        Self::finish(
            self.agent
                .post(format!("{}{path}", self.base))
                .header("X-Tracker-Token", token)
                .header("Content-Type", "application/json")
                .send(body.to_string()),
        )
    }
}

/// 32 hex characters from the OS-seeded hasher. Not a secret, just unique per launch.
fn new_session_id() -> String {
    let s = RandomState::new();
    format!("{:016x}{:016x}", s.hash_one(1u8), s.hash_one(2u8))
}

// ---------- worker ----------

struct Worker {
    api: Result<Api, String>,
    timing: Timing,
    shared: Arc<Shared>,
    session: String,
    active: Option<String>,
    last_beat: Instant,
}

impl Worker {
    fn new(base: &str, allow_http: bool, timing: Timing, shared: Arc<Shared>) -> Worker {
        Worker { api: Api::new(base, allow_http), timing, shared, session: new_session_id(), active: None, last_beat: Instant::now() }
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(cmd) => {
                    self.set_busy(true);
                    self.handle(cmd);
                    self.set_busy(false);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
        }
        // The game or the tracker is closing: best-effort tidy end of any run we started.
        if self.active.is_some() {
            self.end_run();
        }
    }

    fn set_busy(&self, busy: bool) {
        self.shared.status.lock().unwrap().busy = busy;
    }

    fn say(&self, msg: impl Into<String>) {
        self.shared.status.lock().unwrap().message = msg.into();
    }

    fn token(&self) -> Option<String> {
        let t = self.shared.config.lock().unwrap().token.clone();
        (!t.is_empty()).then_some(t)
    }

    fn sharing(&self) -> bool {
        self.shared.config.lock().unwrap().share_position
    }

    /// The fresh position to send, but only when the runner has switched sharing on.
    fn position_to_send(&self) -> Option<(f32, f32)> {
        if !self.sharing() {
            return None;
        }
        let l = self.shared.latest.lock().unwrap();
        let (p, at) = l.pos?;
        (at.elapsed() <= POSITION_FRESH && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD).then_some((p.x, p.y))
    }

    fn with_position(&self, mut body: Value) -> Value {
        if let Some((x, y)) = self.position_to_send() {
            body["x"] = json!(x);
            body["y"] = json!(y);
        }
        body
    }

    /// One request with retries for network trouble, 429 and 5xx. `attempts` includes the first try.
    fn call(&self, attempts: u32, f: impl Fn(&Api, &str) -> Result<Value, ApiError>) -> Result<Value, ApiError> {
        let Some(token) = self.token() else { return Err(ApiError::Http(401, "Paste your tracker token first".into())) };
        let api = match &self.api {
            Ok(a) => a,
            Err(e) => return Err(ApiError::Network(e.clone())),
        };
        let mut last = None;
        for n in 0..attempts {
            match f(api, &token) {
                Ok(v) => return Ok(v),
                Err(e) if e.retryable() && n + 1 < attempts => {
                    thread::sleep(self.timing.retry_base * 2u32.pow(n));
                    last = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or(ApiError::Network("no attempts".into())))
    }

    /// Records a failure in the panel; some end the run.
    fn failed(&mut self, e: &ApiError) {
        match e {
            ApiError::Http(401, _) => self.say("TIPS rejected the token. Make a new one on your shop page."),
            ApiError::Http(409, m) => {
                self.say(format!("{m}. The run is over."));
                self.stop_run();
            }
            ApiError::Http(404, _) => {
                self.say("That run isn't yours any more. Refresh.");
                self.stop_run();
            }
            other => self.say(other.text()),
        }
    }

    fn stop_run(&mut self) {
        self.active = None;
        let mut s = self.shared.status.lock().unwrap();
        s.active_run = None;
        s.progress = None;
        s.off_route = false;
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Refresh => self.refresh(false),
            Cmd::Start => self.start_run(),
            Cmd::End => self.end_run(),
            Cmd::Zone => self.zone(),
        }
    }

    /// Looks up the runner's next run. `keep_message` leaves the current status line alone (used right after
    /// a run ends, so "Route finished" isn't wiped by the lookup that follows).
    fn refresh(&mut self, keep_message: bool) {
        match self.call(2, |api, t| api.get("/api/tracker/next-run", t)) {
            Ok(v) => {
                let run = v.get("run").filter(|r| r.is_object()).map(|r| NextRun {
                    id: str_field(r, "id", 64),
                    title: str_field(r, "title", 80),
                    status: str_field(r, "status", 16),
                    route_name: r.get("route_name").and_then(Value::as_str).map(|s| s.chars().take(80).collect()),
                });
                let mut s = self.shared.status.lock().unwrap();
                if run.is_none() {
                    s.message = "No open runs. Post one on the TIPS site.".into();
                } else if !keep_message {
                    s.message.clear();
                }
                s.next_run = run;
            }
            Err(e) => self.failed(&e),
        }
    }

    fn start_run(&mut self) {
        let Some(run) = self.shared.status.lock().unwrap().next_run.clone() else {
            self.say("Press Refresh to find your next run first.");
            return;
        };
        let body = json!({ "client_session": self.session });
        let path = format!("/api/tracker/runs/{}/start", run.id);
        match self.call(3, |api, t| api.post(&path, t, &body)) {
            Ok(v) if v.get("ignored").and_then(Value::as_bool) == Some(true) => {
                self.say("Another game client is already tracking this run.");
            }
            Ok(_) => {
                self.active = Some(run.id.clone());
                self.last_beat = Instant::now();
                {
                    let mut s = self.shared.status.lock().unwrap();
                    s.active_run = Some(run.id);
                    s.message = "Tracking.".into();
                }
                self.zone(); // report the map we're standing in right now
            }
            Err(e) => self.failed(&e),
        }
    }

    fn end_run(&mut self) {
        let Some(id) = self.active.clone() else { return };
        let body = json!({ "client_session": self.session });
        let path = format!("/api/tracker/runs/{id}/end");
        let result = self.call(2, |api, t| api.post(&path, t, &body));
        self.stop_run();
        match result {
            Ok(_) => {
                self.say("Run ended.");
                self.refresh(true);
            }
            Err(e) => self.failed(&e),
        }
    }

    fn zone(&mut self) {
        let Some(id) = self.active.clone() else { return };
        let (map_id, explorable) = {
            let l = self.shared.latest.lock().unwrap();
            match l.map_id {
                Some(m) => (m, l.explorable),
                None => return,
            }
        };
        let body = self.with_position(json!({ "client_session": self.session, "map_id": map_id, "is_explorable": explorable }));
        let path = format!("/api/tracker/runs/{id}/zone");
        match self.call(4, |api, t| api.post(&path, t, &body)) {
            Ok(v) => {
                self.last_beat = Instant::now();
                if v.get("ignored").and_then(Value::as_bool) == Some(true) {
                    self.say("Another game client took over this run.");
                    self.stop_run();
                    return;
                }
                let finished = v.get("finished").and_then(Value::as_bool) == Some(true);
                let mut s = self.shared.status.lock().unwrap();
                s.progress = v.get("progress").and_then(Value::as_i64);
                s.off_route = v.get("off_route").and_then(Value::as_bool) == Some(true);
                drop(s);
                if finished {
                    self.say("Route finished. The run is complete.");
                    self.stop_run();
                    self.refresh(true);
                }
            }
            Err(e) => self.failed(&e),
        }
    }

    /// Heartbeats while a run is active: every 30 s, or every 5 s while a position is being shared.
    fn tick(&mut self) {
        let Some(id) = self.active.clone() else { return };
        let every = if self.position_to_send().is_some() { self.timing.position_beat } else { self.timing.heartbeat };
        if self.last_beat.elapsed() < every {
            return;
        }
        self.last_beat = Instant::now();
        let body = self.with_position(json!({ "client_session": self.session }));
        let path = format!("/api/tracker/runs/{id}/heartbeat");
        match self.call(1, |api, t| api.post(&path, t, &body)) {
            Ok(v) if v.get("ignored").and_then(Value::as_bool) == Some(true) => {
                self.say("Another game client took over this run.");
                self.stop_run();
            }
            Ok(_) => {}
            Err(e) => self.failed(&e),
        }
    }
}

/// A string field, capped in length, or empty.
fn str_field(v: &Value, key: &str, max: usize) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[derive(Clone, Debug)]
    struct Req {
        method: String,
        path: String,
        token: Option<String>,
        body: Value,
    }

    /// A tiny HTTP server that records requests and answers like the TIPS API.
    fn mock_server(finish_on_map: Option<u64>) -> (String, Arc<Mutex<Vec<Req>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let log: Arc<Mutex<Vec<Req>>> = Arc::default();
        let log2 = log.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let log = log2.clone();
                thread::spawn(move || {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 2048];
                    let (head_end, len) = loop {
                        let n = s.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                            let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0usize);
                            break (i + 4, len);
                        }
                    };
                    while buf.len() < head_end + len {
                        let n = s.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let mut lines = head.lines();
                    let mut first = lines.next().unwrap_or("").split(' ');
                    let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
                    let token = head.lines().find_map(|l| l.to_lowercase().starts_with("x-tracker-token:").then(|| l[16..].trim().to_string()));
                    let body: Value = serde_json::from_slice(&buf[head_end..]).unwrap_or(Value::Null);
                    log.lock().unwrap().push(Req { method, path: path.clone(), token: token.clone(), body: body.clone() });
                    let (status, reply) = if token.as_deref() != Some("tips_trk_test") {
                        ("401 Unauthorized", json!({ "error": "Invalid tracker token" }))
                    } else if path.ends_with("/next-run") {
                        ("200 OK", json!({ "run": { "id": "run1", "title": "Beacon's to Drok's", "status": "scheduled", "start_utc": null, "route_name": "Test route" } }))
                    } else if path.ends_with("/zone") {
                        let finished = Some(body["map_id"].as_u64().unwrap_or(0)) == finish_on_map;
                        ("200 OK", json!({ "ok": true, "progress": if finished { 100 } else { 40 }, "off_route": false, "finished": finished }))
                    } else {
                        ("200 OK", json!({ "ok": true }))
                    };
                    let text = reply.to_string();
                    let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                });
            }
        });
        (base, log)
    }

    fn fast() -> Timing {
        Timing { heartbeat: Duration::from_millis(300), position_beat: Duration::from_millis(100), retry_base: Duration::from_millis(5) }
    }

    fn wait(what: &str, mut ok: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if ok() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for {what}");
    }

    fn requests(log: &Arc<Mutex<Vec<Req>>>, suffix: &str) -> Vec<Req> {
        log.lock().unwrap().iter().filter(|r| r.path.ends_with(suffix)).cloned().collect()
    }

    fn start_uplink(finish_on_map: Option<u64>) -> (Uplink, Arc<Mutex<Vec<Req>>>) {
        let (base, log) = mock_server(finish_on_map);
        let up = Uplink::with_base(&base, true, None, fast());
        up.update_config(|c| c.token = "tips_trk_test".into());
        (up, log)
    }

    fn track(up: &Uplink) {
        up.refresh();
        wait("next run", || up.status().next_run.is_some());
        up.start_run();
        wait("active run", || up.status().active_run.is_some());
    }

    #[test]
    fn config_round_trips_and_ignores_junk() {
        let c = Config { token: "tips_trk_abc".into(), share_position: true, position_source: 2 };
        assert_eq!(Config::parse(&c.serialize()), c);
        let junk = Config::parse("nonsense\ntoken=short\n=\nshare_position=maybe\nposition_source=x\nunknown=1\n");
        assert_eq!(junk, Config { token: "short".into(), share_position: false, position_source: crate::position::Source::MovementA.to_index() });
        assert_eq!(Config::parse(&format!("token={}\n", "a".repeat(500))).token, "");
        assert!(!Config::default().share_position, "position sharing must be off by default");
        assert_eq!(Config::default().position_source, crate::position::Source::MovementA.to_index(), "the movement vector is the default source");
    }

    #[test]
    fn session_ids_are_valid_for_the_api_and_differ() {
        let (a, b) = (new_session_id(), new_session_id());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn refuses_plain_http_in_a_real_build() {
        assert!(Api::new("http://example.com", false).is_err());
        assert!(Api::new("https://example.com", false).is_ok());
    }

    #[test]
    fn nothing_is_sent_before_start_run() {
        let (up, log) = start_uplink(None);
        up.push_zone(55, false);
        up.set_position(Some(Pos { x: 1.0, y: 2.0, z: 3.0 }));
        thread::sleep(Duration::from_millis(700));
        assert!(log.lock().unwrap().is_empty(), "sent {:?}", log.lock().unwrap());
    }

    #[test]
    fn tracks_a_run_with_map_only_by_default() {
        let (up, log) = start_uplink(None);
        up.push_zone(55, false); // standing in Lion's Arch before pressing Start
        track(&up);
        wait("first zone", || !requests(&log, "/zone").is_empty());
        let z = &requests(&log, "/zone")[0];
        assert_eq!(z.method, "POST");
        assert_eq!(z.path, "/api/tracker/runs/run1/zone");
        assert_eq!(z.body["map_id"], 55);
        assert_eq!(z.body["is_explorable"], false);
        assert_eq!(z.body["client_session"].as_str().unwrap().len(), 32);
        wait("progress", || up.status().progress == Some(40));

        up.set_position(Some(Pos { x: 10.0, y: 20.0, z: 0.0 })); // available, but sharing is off
        up.push_zone(90, true);
        wait("second zone", || requests(&log, "/zone").len() >= 2);
        wait("a heartbeat", || !requests(&log, "/heartbeat").is_empty());

        // Every request carried the token, only expected fields were sent, and no position leaked.
        for r in log.lock().unwrap().iter() {
            assert_eq!(r.token.as_deref(), Some("tips_trk_test"));
            let keys: Vec<_> = r.body.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
            for k in keys {
                assert!(["client_session", "map_id", "is_explorable"].contains(&k.as_str()), "unexpected field {k} in {r:?}");
            }
        }
        up.end_run();
        wait("end", || up.status().active_run.is_none());
        wait("end request", || !requests(&log, "/end").is_empty());
    }

    #[test]
    fn sends_position_only_when_sharing_is_on() {
        let (up, log) = start_uplink(None);
        up.update_config(|c| c.share_position = true);
        up.push_zone(55, false);
        track(&up);
        up.set_position(Some(Pos { x: 123.5, y: -456.25, z: 7.0 }));
        wait("a heartbeat with a position", || requests(&log, "/heartbeat").iter().any(|r| r.body["x"] == 123.5 && r.body["y"] == -456.25));
        // z is never sent
        assert!(requests(&log, "/heartbeat").iter().all(|r| r.body.get("z").is_none()));

        // Turning sharing off stops it straight away.
        up.update_config(|c| c.share_position = false);
        let before = requests(&log, "/heartbeat").len();
        wait("a later heartbeat", || requests(&log, "/heartbeat").len() > before + 1);
        let later = requests(&log, "/heartbeat");
        assert!(later[before + 1..].iter().all(|r| r.body.get("x").is_none() && r.body.get("y").is_none()));
    }

    #[test]
    fn a_zone_change_does_not_resend_the_old_maps_position() {
        let (up, log) = start_uplink(None);
        up.update_config(|c| c.share_position = true);
        up.push_zone(55, false);
        track(&up);
        up.set_position(Some(Pos { x: 5.0, y: 6.0, z: 0.0 }));
        up.push_zone(90, true);
        wait("zone 90", || requests(&log, "/zone").iter().any(|r| r.body["map_id"] == 90));
        let z = requests(&log, "/zone").into_iter().find(|r| r.body["map_id"] == 90).unwrap();
        assert!(z.body.get("x").is_none(), "old position sent with the new map: {z:?}");
    }

    #[test]
    fn stops_when_the_route_finishes() {
        let (up, _log) = start_uplink(Some(20));
        up.push_zone(55, false);
        track(&up);
        up.push_zone(20, false);
        wait("run to stop", || up.status().active_run.is_none());
        assert!(up.status().message.contains("finished"), "{}", up.status().message);
    }

    #[test]
    fn a_bad_token_says_so_and_never_starts() {
        let (base, _log) = mock_server(None);
        let up = Uplink::with_base(&base, true, None, fast());
        up.update_config(|c| c.token = "tips_trk_wrong".into());
        up.refresh();
        wait("message", || up.status().message.contains("rejected"));
        assert!(up.status().next_run.is_none());
    }

    #[test]
    fn no_token_means_no_requests() {
        let (base, log) = mock_server(None);
        let up = Uplink::with_base(&base, true, None, fast());
        up.refresh();
        wait("message", || up.status().message.contains("token"));
        assert!(log.lock().unwrap().is_empty());
    }

    /// Against a running local TIPS server (`npm run dev:lite`) with its sample data, to check the
    /// tracker and the real API agree. Run: cargo test -- --ignored e2e --nocapture
    /// Needs TIPS_E2E_BASE (e.g. http://localhost:4291) and TIPS_E2E_TOKEN, and a scheduled run to exist.
    #[test]
    #[ignore]
    fn e2e_against_the_local_server() {
        let base = std::env::var("TIPS_E2E_BASE").expect("TIPS_E2E_BASE");
        let token = std::env::var("TIPS_E2E_TOKEN").expect("TIPS_E2E_TOKEN");
        let up = Uplink::with_base(&base, true, None, Timing { heartbeat: Duration::from_secs(1), position_beat: Duration::from_millis(500), retry_base: Duration::from_millis(50) });
        up.update_config(|c| {
            c.token = token;
            c.share_position = true;
        });
        up.refresh();
        wait("next run", || up.status().next_run.is_some());
        println!("next run: {:?}", up.status().next_run);
        up.push_zone(133, false);
        up.start_run();
        wait("active run", || up.status().active_run.is_some());
        up.push_zone(90, true);
        up.set_position(Some(Pos { x: 1000.0, y: -2000.0, z: 0.0 })); // after the zone change, as the game loop does
        wait("progress", || up.status().progress.is_some());
        println!("progress {:?}, message {:?}", up.status().progress, up.status().message);
        thread::sleep(Duration::from_millis(1500));
        up.end_run();
        wait("end", || up.status().active_run.is_none());
    }
}
