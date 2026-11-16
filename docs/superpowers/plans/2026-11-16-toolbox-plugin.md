# TIPS Tracker Toolbox Plugin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A GWToolbox++ plugin DLL (`TipsTracker.dll`) that does what this repo's Rust TIPS tracker does today, and nothing else.

**Architecture:** A game-free C++ core (`core/`: types, JSON, sample buffer, a protocol `Engine`, a threaded `Uplink`, a `Transport` interface) that is unit-tested with a fake transport and clock. A `RestClientTransport` built on Toolbox's `HttpRequest`. A thin Toolbox plugin (`plugin/`) that polls GWCA, draws the panel and saves settings. The plugin is built inside Toolbox's own CMake tree through a directory junction and a deferred CMake include, so no Toolbox file is edited.

**Tech Stack:** C++23, MSVC v143+/Visual Studio 18 (2026) Win32, CMake, vcpkg (x86-windows-mixed), GWCA, ImGui (from Toolbox), glaze (JSON), Toolbox `RestClient/HttpClient`.

**Spec:** `docs/superpowers/specs/2026-11-16-toolbox-plugin-design.md`. Behavioural reference: `src/uplink.rs`, `src/position.rs`, `src/lib.rs` (`render_tips`). Protocol: `../docs/tracker-protocol.md` (tips-run repo).

## Global Constraints

- Win32 (32-bit) only. MSVC `/W4 /WX` on the plugin target (Toolbox's `add_tb_plugin`), so core code must be warning-clean at `/W4`.
- Toolbox pinned at release **8.34** (commit `1ce867e55`); local clone is `reference_material/GWToolboxpp` (git-ignored).
- HTTPS only to the compiled-in host (`TIPS_API_BASE`); a Debug build may use `http://localhost`. Never follow redirects; certificate checks never relaxed.
- Request timeout 10 s; response cap 8 KB (8192 bytes); error text from server capped at 120 chars; title/route_name 80, id 64, status 16.
- Token max 100 chars, trimmed, never logged, never placed in a header if it contains CR or LF.
- `client_session`: 32 hex chars (16 random bytes), fresh per plugin load.
- Heartbeat 30 s; 5 s while sharing a fresh position; position fresh for 15 s; sample every 1 s; at most 20 points per beat; zone bounds re-reported at most 3 times per zone; coordinates within +-100000; bounds min<max on both axes.
- Retry: network, 429 and >=500 retry with backoff `1 s * 2^n`; other 4xx do not. Attempts: next-run 2, start 3, end 2, zone 4, heartbeat 1.
- Queue to the worker: bounded at 64, drops when full, never blocks the caller.
- Sends only: `client_session`, `map_id`, `is_explorable`, and (sharing) `x`, `y`, `bounds`, `points`. Nothing before Start or after End.
- Commit messages end with the line: `Authored owned entirely by Matthew Morales`
- Plugin window name `TIPS Tracker`; settings keys `token`, `share_position`, `tracking_mode`.

## Review Focus

1. A token containing CR/LF or spaces, or longer than 100 chars: trimmed/truncated, CR/LF rejected, never sent in a header (Task 1, Task 6).
2. A server reply that is not JSON, is HTML (captive portal), is empty, or exceeds 8 KB: no crash, shown as an error, state sane (Task 2, Task 6).
3. The map still loading (`InstanceType::Loading`, map id 0) when Start is pressed or the plugin enables: no zone sent for it; the first real map is reported when it loads (Task 7).
4. Plugin unloaded or the game closed with a request in flight or a run active: the request is aborted, one best-effort end is sent, `CanTerminate()` is false until the worker has stopped (Task 5, Task 8).
5. NaN/inf/huge coordinates and bounds, and samples taken in one zone arriving after the next zone began: dropped, never sent, never filed under the wrong zone (Task 3, Task 4).

## File Structure

```
toolbox-plugin/
  TOOLBOX_VERSION            pinned tag + commit (checked by build.ps1)
  build.ps1                  configure/build/test/dist driver
  cmake/tips.cmake           deferred include: adds TipsTracker + tips_tests targets inside Toolbox's tree
  core/                      game-free sources, namespace tips (compiled into plugin AND tests)
    types.h/.cpp             Config, TrackingMode, Status, NextRun, Timing, Pos, bounds_from, is_sane
    json.h/.cpp              request body builders, response parsers
    samples.h/.cpp           Latest (position, bounds, samples, zone generation)
    transport.h              Response, Transport
    engine.h/.cpp            Shared, Cmd, Engine (the protocol state machine, no threads)
    uplink.h/.cpp            Uplink (queue + worker thread around Engine)
    zone_watcher.h/.cpp      ZoneWatcher (frame-by-frame map-load detection)
    url_policy.h/.cpp        is_allowed_base(), valid_token_for_header()
  plugin/                    junctioned to <toolbox>/plugins/TipsTracker (add_tb_plugin globs it)
    TipsTrackerPlugin.h/.cpp
    RestClientTransport.h/.cpp
  tests/                     test_main.cpp (CHECK/TEST mini-runner) + one test_*.cpp per core unit
  README.md                  install + build + test
```

---

### Task 1: Build harness and types

**Files:**
- Create: `toolbox-plugin/TOOLBOX_VERSION`, `build.ps1`, `cmake/tips.cmake`, `core/types.h`, `core/types.cpp`, `core/url_policy.h`, `core/url_policy.cpp`, `plugin/TipsTrackerPlugin.h/.cpp` (skeleton: Name only), `tests/test_main.cpp`, `tests/test_types.cpp`
- Modify: `.gitignore` (`/toolbox-plugin/build`, `/toolbox-plugin/dist`)

**Interfaces:**
- Produces (`core/types.h`, namespace `tips`): `enum class TrackingMode { Run, General }`; `const char* to_string(TrackingMode)`; `std::optional<TrackingMode> mode_from_string(std::string_view)`; `struct Config { std::string token; bool share_position=false; TrackingMode tracking_mode=TrackingMode::Run; }`; `std::string clean_token(std::string_view)` (trim ASCII whitespace, cut to 100 chars); `struct Pos { float x, y; bool is_sane() const; }`; `inline constexpr float kMaxCoord = 100000.f`; `using Bounds = std::array<float,4>`; `std::optional<Bounds> bounds_from(std::array<float,4> min_x_min_y_max_x_max_y)`; `struct NextRun { std::string id, title, status; std::optional<std::string> route_name; }`; `struct Status { std::optional<NextRun> next_run; std::optional<std::string> active_run; bool active_general=false; std::optional<long long> progress; bool off_route=false; std::string message; bool busy=false; }`; `struct Timing { ms heartbeat{30000}, position_beat{5000}, retry_base{1000}, sample{1000}; }`.
- Produces (`core/url_policy.h`): `bool is_allowed_base(std::string_view base, bool debug_build)` (https:// always; `http://localhost` only when `debug_build`); `bool valid_token_for_header(std::string_view)` (non-empty, no CR/LF/NUL, printable ASCII only).
- Produces: `build.ps1 -Target <TipsTracker|tips_tests> [-Run] [-Config RelWithDebInfo] [-ApiBase <url>] [-Dist]`.

- [ ] **Step 1: Write failing tests** in `tests/test_types.cpp` using `TEST(name)`/`CHECK(expr)` from `test_main.cpp` (a ~30-line runner: registers tests, runs all, returns non-zero on failure, prints names):
  - `clean_token("  tips_trk_abc \n") == "tips_trk_abc"`; a 150-char token is cut to 100; empty stays empty.
  - `mode_from_string("run")==Run`, `"general"==General`, `"x"==nullopt`; `to_string` round-trips.
  - `Config{}` defaults: `share_position==false`, `tracking_mode==Run`.
  - `Pos{100,-2500.5}.is_sane()`; NaN, inf, `5.0e6` are not sane; `+-100000` is sane, `100001` is not.
  - `bounds_from({-18432,-21504,21504,21504})` returns those four; `{5,0,5,10}` (min==max), `{0,9,10,5}`, a NaN, `1e6`, and `{0,0,0,0}` all return `nullopt`; `{-100000,-100000,100000,100000}` is accepted.
  - `is_allowed_base("https://x.example", false)` true; `"http://x.example"` false; `"http://localhost:4280"` true only with `debug_build=true`; `"https://x.example/"` accepted.
  - `valid_token_for_header`: `"tips_trk_ok"` true; `"a\r\nX: y"`, `"a\nb"`, `""`, a token with a non-ASCII byte: false.
- [ ] **Step 2: Build harness.** `TOOLBOX_VERSION` holds `8.34_Release` and `1ce867e55`. `build.ps1` finds VS 18's `cmake.exe`/`ninja`/`vcpkg` (`C:\Program Files\Microsoft Visual Studio\18\Community`, falling back to 2022), sets `VCPKG_ROOT` to the VS-bundled vcpkg, checks the Toolbox clone's HEAD against `TOOLBOX_VERSION` (default clone `reference_material/GWToolboxpp`, override `-ToolboxDir`), creates the junction `<toolbox>/plugins/TipsTracker -> toolbox-plugin/plugin` if missing, configures Toolbox's tree like its `vcpkg` preset (generator `Visual Studio 18 2026`, `-A Win32`, vcpkg toolchain, overlay triplets, `x86-windows-mixed`, `-DGWTOOLBOX_BUILD_EXAMPLE_PLUGIN=OFF`, binary dir `toolbox-plugin/build`, `-DCMAKE_PROJECT_gwtoolbox_INCLUDE=toolbox-plugin/cmake/tips.cmake`, `-DTIPS_API_BASE=<url>`), then builds the target; with `-Run` runs `tips_tests.exe`; with `-Dist` copies `TipsTracker.dll` to `toolbox-plugin/dist/`. `cmake/tips.cmake` uses `cmake_language(DEFER DIRECTORY ${CMAKE_SOURCE_DIR} CALL tips_setup)` so Toolbox's `add_tb_plugin` macro exists; `tips_setup` calls `add_tb_plugin(TipsTracker)`, adds `core/*.cpp` and `<toolbox>/RestClient/HttpClient.cpp` to it (include dirs `core`, `<toolbox>/RestClient`; define `TIPS_API_BASE` as a string), and defines `tips_tests` (an executable of `core/*.cpp` + `tests/*.cpp`, C++23, `/W4 /WX`, no GWCA/ImGui).
- [ ] **Step 3: Run the tests to see them fail.** Run: `toolbox-plugin\build.ps1 -Target tips_tests -Run` (the first run configures and fetches vcpkg dependencies for Toolbox's whole tree, which takes a long time once). Expected: compile error, `tips/types.h` symbols undefined.
- [ ] **Step 4: Implement** `core/types.*` and `core/url_policy.*` to the signatures above. `TipsTrackerPlugin` is a skeleton deriving `ToolboxUIPlugin` with `Name() == "TIPS Tracker"` and an exported `ToolboxPluginInstance()`.
- [ ] **Step 5: Run.** `build.ps1 -Target tips_tests -Run` -> all tests pass. `build.ps1 -Target TipsTracker` -> produces `TipsTracker.dll` with no warnings.
- [ ] **Step 6: Commit** `feat: toolbox plugin build harness, tracker types and URL/token policy`

---

### Task 2: JSON bodies and response parsing

**Files:** Create `core/json.h`, `core/json.cpp`, `tests/test_json.cpp`

**Interfaces:**
- Consumes: Task 1 types.
- Produces (`tips`): 
  - `std::string body_session(std::string_view session)` -> `{"client_session":"..."}`
  - `std::string body_zone(std::string_view session, uint32_t map_id, bool explorable, std::optional<Pos> pos, std::optional<Bounds> bounds)` (x,y added only with pos; bounds only with pos and bounds)
  - `std::string body_heartbeat(std::string_view session, std::optional<Pos> pos, const std::vector<std::pair<float,float>>& points)` (`points` key omitted when empty)
  - `std::optional<NextRun> parse_next_run(std::string_view json)` returns nullopt for `{"run":null}`, non-JSON, or no object
  - `struct ZoneReply { bool ok_json=false; bool ignored=false; bool finished=false; bool off_route=false; std::optional<long long> progress; }`; `ZoneReply parse_reply(std::string_view)`
  - `std::string error_text(std::string_view json, std::string_view fallback="Request failed")` (server's `error` string, 120 chars max, else fallback)
  - Strings escaped correctly; use glaze for parsing/writing.

- [ ] **Step 1: Write failing tests:** `body_zone("s",90,true,nullopt,nullopt)` parses back to exactly the three keys; with pos adds `x`,`y`; `bounds` is omitted when `pos` is absent even if bounds given; `body_heartbeat` with 2 points has `"points":[[1,2],[3,4]]`, with none has no `points`; a session or title containing `"` and `\` round-trips; `parse_next_run` on the protocol example yields id/title/status/route_name, a `route_name:null` gives nullopt, over-long title is cut to 80; `parse_next_run("{\"run\":null}")`, `""`, `"<html>"`, `"[]"` -> nullopt; `parse_reply` on `{"ok":true,"progress":33,"off_route":false,"finished":false}`, on `{"ok":true,"ignored":true}`, on `{"progress":null}`, on garbage (`ok_json==false`); `error_text("{\"error\":\"Nope\"}")=="Nope"`, a 500-char error is cut to 120, non-JSON gives the fallback, a non-string `error` gives the fallback.
- [ ] **Step 2: Run** `build.ps1 -Target tips_tests -Run`; expected FAIL (undefined).
- [ ] **Step 3: Implement** the six functions in `core/json.cpp` with glaze (`glz::json_t` for reading).
- [ ] **Step 4: Run;** expected PASS.
- [ ] **Step 5: Commit** `feat: tracker JSON bodies and response parsing`

---

### Task 3: Sample buffer

**Files:** Create `core/samples.h`, `core/samples.cpp`, `tests/test_samples.cpp`

**Interfaces:**
- Consumes: Task 1 (`Pos`, `Bounds`).
- Produces (`tips`, port of Rust `Latest`): `using Tp = std::chrono::steady_clock::time_point;` `struct Latest { std::optional<uint32_t> map_id; bool explorable=false; std::optional<Bounds> bounds; std::optional<std::pair<Pos,Tp>> pos; std::deque<std::pair<float,float>> samples; std::optional<Tp> last_sample; uint64_t zone_gen=0; void add_sample(float x,float y,Tp now,std::chrono::milliseconds every); void restore_samples(uint64_t zone, std::vector<std::pair<float,float>> unsent); void clear_samples(); }; constexpr size_t kMaxPoints = 20;`

- [ ] **Step 1: Write failing tests** (ports of the Rust behaviour): first sample is kept; a second within `every` is not; one after `every` is; never more than 20 (add 25 spaced samples -> size 20, oldest dropped, newest kept); `restore_samples` for the current zone puts unsent samples in front of newer ones and keeps the newest 20; for an older `zone` it changes nothing; `clear_samples` empties and resets `last_sample`.
- [ ] **Step 2: Run;** expected FAIL.
- [ ] **Step 3: Implement** as specified.
- [ ] **Step 4: Run;** expected PASS.
- [ ] **Step 5: Commit** `feat: trail sample buffer`

---

### Task 4: Protocol engine

**Files:** Create `core/transport.h`, `core/engine.h`, `core/engine.cpp`, `tests/fake_transport.h`, `tests/test_engine.cpp`

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces (`core/transport.h`): `struct Response { int status=0; std::string body; std::string error; }` (status 0 = network failure, `error` set); `class Transport { public: virtual ~Transport()=default; virtual Response get(std::string_view path, std::string_view token)=0; virtual Response post(std::string_view path, std::string_view token, std::string_view json_body)=0; virtual void abort()=0; };`
- Produces (`core/engine.h`): `struct Shared { std::mutex m; Config config; Status status; Latest latest; std::chrono::milliseconds sample_every{1000}; };` `enum class Cmd { Refresh, Start, End, Zone, StartGeneral, EndGeneral };` `class Engine { public: Engine(Shared&, Transport&, Timing, std::function<Tp()> now, std::function<void(std::chrono::milliseconds)> sleep, std::string session); void handle(Cmd); void tick(); bool tracking() const; };` `Engine` has no threads. `sleep` is injected so tests do not wait.
- Behaviour is the Rust `Worker` (see `src/uplink.rs` lines 461-829) with paths `/api/tracker/next-run`, `/api/tracker/runs/{id}/{start,zone,heartbeat,end}` and `/api/tracker/presence/{start,zone,heartbeat,end}`; header token via the transport; the same panel messages.

- [ ] **Step 1: Write failing tests** with a `FakeTransport` (scripted responses per path, records requests) and a manual clock, one `TEST` each:
  - `start_without_next_run_says_refresh_first`; `start_in_general_mode_says_choose_runner`; `start_without_token_says_paste_token_and_sends_nothing`.
  - `start_posts_start_then_zone_with_current_map`: after `Refresh` (next-run reply) and a known map, `Start` sends `POST /runs/<id>/start` body `{"client_session":S}` then `POST /runs/<id>/zone` with `map_id` and `is_explorable`; `status.active_run==id`, message "Tracking."; samples cleared.
  - `start_ignored_reply_does_not_start` ("Another game client is already tracking this run.").
  - `zone_reply_sets_progress_and_off_route`; `zone_finished_ends_run_and_refreshes` (message "Route finished. The run is complete.", active_run empty, a `GET next-run` follows); `zone_ignored_stops_with_takeover_message`.
  - `errors`: 401 -> "TIPS rejected the token. Make a new one on your shop page." and run stays; 409 -> "<server msg>. The run is over." and idle; 404 -> "That run isn't yours any more. Refresh." and idle; 400 shown not retried; 503 retried up to the attempt count with sleeps 1 s, 2 s, 4 s then fails with `"<msg> (503)"`; network failure text "Can't reach TIPS: ...".
  - `end_posts_end_and_refreshes`; a failed end still leaves the engine idle.
  - `heartbeat_every_30s_without_sharing` (nothing before 30 s; one at 30 s; no `x`/`y`); `heartbeat_every_5s_with_fresh_position_when_sharing` with `x`,`y`; a position older than 15 s is not sent; `points` carry the buffered samples oldest first and are drained; a failed heartbeat restores the samples (same zone) and shows the error; samples from an earlier zone are not restored.
  - `sharing_switched_off_sends_bare_heartbeat_immediately` (no x/y/points, clears samples).
  - `sharing_on_midzone_rereports_zone_with_bounds_at_most_3_times`; the zone body then carries `x`,`y`,`bounds`.
  - `bounds_only_sent_while_sharing`.
  - `general_mode`: Start tracking posts `/presence/start` then `/presence/zone` with position and bounds (sharing is implied); heartbeats go to `/presence/heartbeat`; End posts `/presence/end`; a 409 says "<msg>. Tracking stopped."; Start tracking while a run is active says "End your run before starting general tracking."; Start run while general says "Stop general tracking before starting a run."; mode changed in config between press and handling says "Choose General tracking first." / "Choose Runner tracking first."
  - `nothing_sent_when_idle`: `tick()` with no run and a position pushed sends nothing.
  - `invalid_json_replies_do_not_crash` (HTML body with 200 on start/zone/next-run: treated as success without progress / "No open runs" respectively; no exception).
- [ ] **Step 2: Run;** expected FAIL.
- [ ] **Step 3: Implement** `core/engine.cpp` to the interfaces above, ported from the Rust `Worker`.
- [ ] **Step 4: Run;** expected all PASS.
- [ ] **Step 5: Commit** `feat: tracker protocol engine with tests against a fake transport`

---

### Task 5: Threaded uplink

**Files:** Create `core/uplink.h`, `core/uplink.cpp`, `tests/test_uplink.cpp`

**Interfaces:**
- Consumes: Task 4 (`Engine`, `Shared`, `Transport`).
- Produces (`tips::Uplink`, API mirrors Rust `Uplink`): `Uplink(std::unique_ptr<Transport>, Config initial, Timing = {})`; `Config config() const`; `bool sharing() const`; `Status status() const`; `void update_config(std::function<void(Config&)>)` (mode cannot change while tracking; token cleaned with `clean_token`; returns after updating; **persistence is the caller's job**); `void push_zone(uint32_t map_id, bool explorable, std::optional<Bounds>)`; `bool knows_map() const`; `bool wants_bounds() const`; `void set_bounds(std::optional<Bounds>)`; `void set_position(std::optional<Pos>)`; `void refresh(); void start_run(); void end_run(); void start_general(); void end_general();` `void signal_stop()` (asks the worker to end, calls `Transport::abort()` on an in-flight request); `bool stopped() const` (worker thread has exited). Destructor calls `signal_stop()` then joins.
- The worker waits up to 250 ms for a command, runs `tick()` each loop, and on shutdown sends one best-effort end for an active run or presence.

- [ ] **Step 1: Write failing tests** (real thread, fake transport, short `Timing`): `commands_run_in_order_off_the_caller_thread`; `queue_is_bounded_and_drops_when_full` (block the transport, push 200 commands, caller never blocks, no crash); `update_config_cannot_change_mode_while_tracking`; `update_config_cleans_token`; `push_zone_clears_position_and_samples_and_bumps_generation`; `set_position_ignores_insane_values`; `set_position_keeps_samples_only_while_sharing`; `wants_bounds_true_only_when_sharing_and_map_known_and_bounds_missing`; `signal_stop_aborts_inflight_request_and_sends_one_end` (transport blocks in `post` until `abort()`; after `signal_stop()`, `stopped()` becomes true within 2 s and exactly one end was posted); `destructor_with_idle_uplink_sends_nothing`.
- [ ] **Step 2: Run;** expected FAIL.
- [ ] **Step 3: Implement** with `std::thread`, a mutex+condition-variable bounded deque (capacity 64, `try_push`), shared state under `Shared::m`.
- [ ] **Step 4: Run;** expected PASS.
- [ ] **Step 5: Commit** `feat: threaded uplink with bounded queue and clean shutdown`

---

### Task 6: RestClient transport

**Files:** Create `plugin/RestClientTransport.h`, `plugin/RestClientTransport.cpp`; Create `tests/test_transport_policy.cpp` (tests only the pure parts)

**Interfaces:**
- Consumes: `tips::Transport`, `is_allowed_base`, `valid_token_for_header`; Toolbox `HttpRequest` (`RestClient/HttpClient.h`).
- Produces: `class RestClientTransport : public tips::Transport { RestClientTransport(std::string base, bool debug_build); Response get(...); Response post(...); void abort(); }`. Internally a private `CappedRequest : HttpRequest` whose `OnContent` stops appending past 8192 bytes and records overflow (overflow is returned as `Response{status=0, error="response too large"}`).
- Rules enforced here: refuse (status 0) any base failing `is_allowed_base` or a token failing `valid_token_for_header` without making a request; `SetUserAgent("TIPS-Tracker/1")`; headers `X-Tracker-Token`, and for POST `Content-Type: application/json`; `SetTimeoutMs(10000)`, `SetConnectTimeoutMs(10000)`; redirects off (the `HttpRequest` default; never call `SetFollowLocation(true)`, `SetVerifyPeer(false)` or `SetVerifyHost(false)`); a non-`Completed` `ResponseStatus` becomes status 0 with an `error` string; `abort()` calls `RequestAbort()` on the current request (guarded by a mutex).

- [ ] **Step 1: Write failing tests** for what is testable without a network (`tests/test_transport_policy.cpp`, building `RestClientTransport.cpp` into `tips_tests` too): constructing with a `http://x.example` base then calling `get` returns `status==0` and a message mentioning HTTPS without touching the network; a token with `\r\n` is refused the same way; the 8 KB cap helper (`CappedAppend` free function in the header, used by `OnContent`) keeps exactly 8192 bytes and reports overflow when given 10000.
- [ ] **Step 2: Run;** expected FAIL.
- [ ] **Step 3: Implement** to the interface; `RestClientTransport.cpp` compiles into both `TipsTracker` and `tips_tests` (add to `cmake/tips.cmake`).
- [ ] **Step 4: Run;** expected PASS. Then an **optional live check**: if `TIPS_E2E_BASE` and `TIPS_E2E_TOKEN` are set (local TIPS API from `npm run dev:lite`, Debug build), a `TEST(e2e_next_run_against_local_api)` calls `get("/api/tracker/next-run")` and expects status 200; skipped (and printed as skipped) otherwise.
- [ ] **Step 5: Commit** `feat: RestClient-based HTTPS transport with response cap and policy checks`

---

### Task 7: Zone watcher

**Files:** Create `core/zone_watcher.h`, `core/zone_watcher.cpp`, `tests/test_zone_watcher.cpp`

**Interfaces:**
- Produces (`tips`): `enum class Instance { Outpost, Explorable, Loading };` `struct ZoneEvent { uint32_t map_id; bool explorable; };` `class ZoneWatcher { public: std::optional<ZoneEvent> update(bool map_loaded, uint32_t map_id, Instance); void reset(); };` Emits one event the first frame a real map is loaded, and again whenever a load completes into a different load (including re-entering the same map after a loading screen); never emits for `Loading`, `!map_loaded`, or `map_id==0`.

- [ ] **Step 1: Write failing tests:** the first loaded frame emits; the same loaded frame repeated emits nothing; a `Loading` frame then the same map again emits again (re-entering a map is a new load); changing map ids emits; `Explorable` -> `explorable==true`, `Outpost` -> false; `map_loaded=false` and `map_id=0` never emit; `reset()` makes the next loaded frame emit.
- [ ] **Step 2: Run;** expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run;** expected PASS.
- [ ] **Step 5: Commit** `feat: zone watcher`

---

### Task 8: The Toolbox plugin

**Files:** Modify `plugin/TipsTrackerPlugin.h/.cpp`

**Interfaces:**
- Consumes: `Uplink`, `RestClientTransport`, `ZoneWatcher`; GWCA `GW::Map::GetIsMapLoaded()`, `GW::Map::GetMapID()`, `GW::Map::GetInstanceType()` (`Constants::InstanceType::{Outpost,Explorable,Loading}`), `GW::Agents::GetControlledCharacter()` (`->x`, `->y`), `GW::GetMapContext()` (`->start_pos`, `->end_pos` as min and max); Toolbox `ToolboxUIPlugin`.
- Produces: `class TipsTrackerPlugin : public ToolboxUIPlugin` exporting `ToolboxPluginInstance()`. `Name() == "TIPS Tracker"`.
  - `Initialize`: base call; build `Uplink` (token from loaded settings) with a `RestClientTransport(TIPS_API_BASE, debug_build)`; 32-hex session id from `BCryptGenRandom` (in `Uplink`'s `Engine` constructor argument).
  - `LoadSettings`/`SaveSettings`: keys `token`, `share_position`, `tracking_mode`; remember the settings folder so the Save button can call `SaveSettings(folder)`.
  - `Update(float)`: each frame call `ZoneWatcher::update(...)`; on an event call `push_zone` with bounds read only while sharing; seed the first loaded map; about four times a second (while the panel is visible or sharing) call `set_position` from `GetControlledCharacter()` (null -> `nullopt`) and, if `wants_bounds()`, `set_bounds`. Reads only coordinates and map ids, never names.
  - `Draw`: the panel per the design sketch, drawn only while visible. Token box is `ImGuiInputTextFlags_Password`; Save calls `update_config` then `SaveSettings`; mode radios disabled while tracking; Refresh; Start/End run; Start/Stop tracking with the public-trail note; Share checkbox; progress/off-route/message/busy; collapsible "Position check" showing x, y, bounds; the line "Sent to TIPS only while a run is active: map ID, town or explorable, and (if sharing) x/y."
  - `SignalTerminate`: base call; `uplink->signal_stop()`. `CanTerminate`: `uplink->stopped()`. `Terminate`: destroy the uplink.
  - No chat commands of its own (the base class already adds `/tb`), no hooks, no `WndProc`.

- [ ] **Step 1: Build** `build.ps1 -Target TipsTracker` -> compiles and links with `/W4 /WX` clean (this task has no unit tests; the logic it calls is tested in Tasks 1-7).
- [ ] **Step 2: Check exports:** `dumpbin /exports dist\TipsTracker.dll` lists `ToolboxPluginInstance`.
- [ ] **Step 3: Run all tests:** `build.ps1 -Target tips_tests -Run` -> PASS.
- [ ] **Step 4: Commit** `feat: TIPS Tracker Toolbox plugin`

---

### Task 9: Packaging, README, final verification

**Files:** Create `toolbox-plugin/README.md`; Modify `TIPS_TRACKER.md` (add a pointer to the plugin), `build.ps1` (`-Dist`)

- [ ] **Step 1: README** covers: what it is; install (close the game, copy `TipsTracker.dll` to `%USERPROFILE%\Documents\GWToolboxpp\plugins\` (create the folder), start Toolbox, Settings -> Plugins -> tick TIPS Tracker, or use the Refresh button); first use (token, Save, Refresh, Start run); build and test commands; the pinned Toolbox version and that the DLL must be rebuilt for a new Toolbox release; the unsigned-DLL note; how to test against a local TIPS server (Debug build with `-ApiBase http://localhost:4280`, `TIPS_E2E_*`).
- [ ] **Step 2: Release build:** `build.ps1 -Target TipsTracker -Dist -ApiBase https://<site>` (use the placeholder host if the real one is not decided) -> `dist/TipsTracker.dll`.
- [ ] **Step 3: Full verification:** `build.ps1 -Target tips_tests -Run` prints all tests passing; the release DLL imports no `ureq`/`rustls` (it is C++), and `dumpbin /dependents` lists only system DLLs plus `gwca.dll`.
- [ ] **Step 4: Commit** `docs: toolbox plugin readme and packaging`
