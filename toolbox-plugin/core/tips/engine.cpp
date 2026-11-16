#include "tips/engine.h"

#include <cmath>

#include "tips/json.h"

namespace tips {

namespace {
constexpr Ms kPositionFresh{15000};  // a position older than this is not sent
constexpr int kMaxZoneRetries = 3;   // how often a zone is re-reported to get its bounds to the server

std::string run_path(const std::string& id) { return "/api/tracker/runs/" + id; }
} // namespace

std::string Engine::ApiError::text() const
{
    return code == 0 ? "Can't reach TIPS: " + msg : msg + " (" + std::to_string(code) + ")";
}

Engine::Engine(Shared& shared, Transport& transport, Timing timing, std::function<Tp()> now, std::function<void(Ms)> sleep, std::string session)
    : sh_(shared), tr_(transport), timing_(timing), now_(std::move(now)), sleep_(std::move(sleep)), session_(std::move(session))
{
    last_beat_ = now_();
}

// ---------- plumbing ----------

void Engine::say(std::string msg)
{
    std::lock_guard g(sh_.m);
    sh_.status.message = std::move(msg);
}

bool Engine::sharing()
{
    std::lock_guard g(sh_.m);
    return general_ || sh_.config.share_position;
}

std::optional<std::string> Engine::track_base() const
{
    if (general_) return std::string("/api/tracker/presence");
    if (active_) return run_path(*active_);
    return std::nullopt;
}

std::optional<Pos> Engine::position_to_send()
{
    std::lock_guard g(sh_.m);
    if (!(general_ || sh_.config.share_position)) return std::nullopt;
    if (!sh_.latest.pos) return std::nullopt;
    const auto& [p, at] = *sh_.latest.pos;
    if (std::chrono::duration_cast<Ms>(now_() - at) > kPositionFresh || !p.is_sane()) return std::nullopt;
    return p;
}

std::pair<uint64_t, std::vector<std::pair<float, float>>> Engine::take_samples()
{
    const bool share = sharing();
    std::lock_guard g(sh_.m);
    if (!share) {
        sh_.latest.clear_samples();
        return {sh_.latest.zone_gen, {}};
    }
    std::vector<std::pair<float, float>> out(sh_.latest.samples.begin(), sh_.latest.samples.end());
    sh_.latest.samples.clear();
    return {sh_.latest.zone_gen, std::move(out)};
}

Engine::CallResult Engine::call(const int attempts, const std::function<Response(const std::string&)>& send)
{
    std::string token;
    {
        std::lock_guard g(sh_.m);
        token = sh_.config.token;
    }
    if (token.empty()) return {ApiError{401, "Paste your tracker token first", true}, {}};

    std::optional<ApiError> last;
    for (int n = 0; n < attempts; ++n) {
        const Response r = send(token);
        ApiError e;
        if (r.status == 0) {
            e = {0, r.error.empty() ? "no response" : r.error, false};
        }
        else if (r.status >= 200 && r.status < 300) {
            return {std::nullopt, r.body};
        }
        else {
            e = {r.status, error_text(r.body), false};
        }
        if (e.retryable() && n + 1 < attempts) {
            sleep_(timing_.retry_base * (1 << n));
            last = e;
            continue;
        }
        return {e, {}};
    }
    return {last.value_or(ApiError{0, "no attempts", false}), {}};
}

Engine::CallResult Engine::get(const int attempts, const std::string& path)
{
    return call(attempts, [&](const std::string& token) { return tr_.get(path, token); });
}

Engine::CallResult Engine::post(const int attempts, const std::string& path, const std::string& body)
{
    return call(attempts, [&](const std::string& token) { return tr_.post(path, token, body); });
}

void Engine::stop_run()
{
    active_.reset();
    general_ = false;
    std::lock_guard g(sh_.m);
    sh_.status.active_run.reset();
    sh_.status.active_general = false;
    sh_.status.progress.reset();
    sh_.status.off_route = false;
}

/// Records a failure in the panel; some end the run.
void Engine::failed(const ApiError& e)
{
    if (e.no_token) {
        say("Paste your tracker token first.");
    }
    else if (e.code == 401) {
        say("TIPS rejected the token. Make a new one on your shop page.");
    }
    else if (e.code == 409) {
        const std::string msg = e.msg + (general_ ? ". Tracking stopped." : ". The run is over.");
        stop_run();
        say(msg);
    }
    else if (e.code == 404) {
        say("That run isn't yours any more. Refresh.");
        stop_run();
    }
    else {
        say(e.text());
    }
}

// ---------- commands ----------

void Engine::handle(const Cmd cmd)
{
    switch (cmd) {
        case Cmd::Refresh: refresh(false); break;
        case Cmd::Start: start_run(); break;
        case Cmd::End: end_run(); break;
        case Cmd::Zone: zone(); break;
        case Cmd::StartGeneral: start_general(); break;
        case Cmd::EndGeneral: end_general(); break;
    }
}

void Engine::shutdown()
{
    if (active_) end_run();
    else if (general_) end_general();
}

/// Looks up the runner's next run. `keep_message` leaves the status line alone when a run was found (used right
/// after a run ends, so "Route finished" is not wiped by the lookup that follows).
void Engine::refresh(const bool keep_message)
{
    const auto r = get(2, "/api/tracker/next-run");
    if (!r.ok()) {
        failed(*r.err);
        return;
    }
    auto run = parse_next_run(r.body);
    std::lock_guard g(sh_.m);
    if (!run) sh_.status.message = "No open runs. Post one on the TIPS site.";
    else if (!keep_message) sh_.status.message.clear();
    sh_.status.next_run = std::move(run);
}

void Engine::start_general()
{
    if (general_) return;
    // The mode can be flipped between pressing Start tracking and the worker getting to it. Only start what the
    // panel still offers, or the runner would broadcast with the wrong controls on screen.
    TrackingMode mode;
    {
        std::lock_guard g(sh_.m);
        mode = sh_.config.tracking_mode;
    }
    if (mode != TrackingMode::General) {
        say("Choose General tracking first.");
        return;
    }
    if (active_) {
        say("End your run before starting general tracking.");
        return;
    }
    const auto r = post(3, "/api/tracker/presence/start", body_session(session_));
    if (!r.ok()) {
        failed(*r.err);
        return;
    }
    general_ = true;
    last_beat_ = now_();
    {
        std::lock_guard g(sh_.m);
        sh_.latest.clear_samples();
        sh_.status.active_general = true;
        sh_.status.message = "Tracking.";
    }
    zone(); // report the map we are standing in right now
}

void Engine::end_general()
{
    if (!general_) return;
    const auto r = post(2, "/api/tracker/presence/end", body_session(session_));
    stop_run();
    if (r.ok()) {
        say("Tracking stopped.");
    }
    else {
        // The server still has the dot until it times out, so do not let the runner think it is gone.
        say(r.err->text() + " Your dot stays on the map until it expires (about 15 minutes). Press Start tracking, then Stop tracking, to try again.");
    }
}

void Engine::start_run()
{
    if (general_) {
        say("Stop general tracking before starting a run.");
        return;
    }
    std::optional<NextRun> run;
    TrackingMode mode;
    {
        std::lock_guard g(sh_.m);
        run = sh_.status.next_run;
        mode = sh_.config.tracking_mode;
    }
    if (mode == TrackingMode::General) {
        say("Choose Runner tracking first.");
        return;
    }
    if (!run) {
        say("Press Refresh to find your next run first.");
        return;
    }
    const auto r = post(3, run_path(run->id) + "/start", body_session(session_));
    if (!r.ok()) {
        failed(*r.err);
        return;
    }
    if (parse_reply(r.body).ignored) {
        say("Another game client is already tracking this run.");
        return;
    }
    active_ = run->id;
    last_beat_ = now_();
    {
        std::lock_guard g(sh_.m);
        sh_.latest.clear_samples(); // whatever was walked before Start run is not part of this run
        sh_.status.active_run = run->id;
        sh_.status.message = "Tracking.";
    }
    zone(); // report the map we are standing in right now
}

void Engine::end_run()
{
    if (!active_) return;
    const auto r = post(2, run_path(*active_) + "/end", body_session(session_));
    stop_run();
    if (r.ok()) {
        say("Run ended.");
        refresh(true);
    }
    else {
        failed(*r.err);
    }
}

void Engine::zone()
{
    const auto base = track_base();
    if (!base) return;
    uint32_t map_id;
    bool explorable;
    std::optional<Bounds> bounds;
    {
        std::lock_guard g(sh_.m);
        if (!sh_.latest.map_id) return;
        map_id = *sh_.latest.map_id;
        explorable = sh_.latest.explorable;
        bounds = sh_.latest.bounds;
    }
    // The bounds ride with the zone only while sharing: the site needs them to place the position on the map.
    const bool with_bounds = sharing() && bounds.has_value();
    const auto body = body_zone(session_, map_id, explorable, position_to_send(), with_bounds ? bounds : std::nullopt);
    bounds_sent_ = false; // until the server says it has them
    const auto r = post(4, *base + "/zone", body);
    if (!r.ok()) {
        failed(*r.err);
        return;
    }
    last_beat_ = now_();
    bounds_sent_ = with_bounds;
    const auto reply = parse_reply(r.body);
    if (reply.ignored) {
        stop_run();
        say("Another game client took over this run.");
        return;
    }
    {
        std::lock_guard g(sh_.m);
        sh_.status.progress = reply.progress;
        sh_.status.off_route = reply.off_route;
    }
    if (reply.finished) {
        say("Route finished. The run is complete.");
        stop_run();
        refresh(true);
    }
}

void Engine::tick()
{
    const auto base = track_base();
    if (!base) return;
    const bool has_position = position_to_send().has_value();
    const Ms every = has_position ? timing_.position_beat : timing_.heartbeat;
    const bool share = sharing();
    // Sharing just went off: do not wait for the next beat to clear the dot and trail on the site.
    const bool just_off = was_sharing_ && !share;
    was_sharing_ = share;
    if (!just_off && std::chrono::duration_cast<Ms>(now_() - last_beat_) < every) return;
    last_beat_ = now_();

    // Sharing was switched on after this zone was reported, so the server has no bounds for it: report the zone
    // again (with position and bounds) instead of a plain heartbeat.
    uint64_t zone_gen;
    bool has_bounds;
    {
        std::lock_guard g(sh_.m);
        zone_gen = sh_.latest.zone_gen;
        has_bounds = sh_.latest.bounds.has_value();
    }
    if (!bounds_sent_ && share && has_bounds && position_to_send()) {
        if (retry_gen_ != zone_gen) {
            retry_gen_ = zone_gen;
            zone_retries_ = 0;
        }
        if (zone_retries_ < kMaxZoneRetries) {
            ++zone_retries_;
            zone();
            return;
        }
    }
    auto [gen, samples] = take_samples();
    const auto body = body_heartbeat(session_, position_to_send(), samples);
    const auto r = post(1, *base + "/heartbeat", body);
    if (r.ok()) {
        if (parse_reply(r.body).ignored) {
            stop_run();
            say("Another game client took over this run.");
        }
        return;
    }
    {
        std::lock_guard g(sh_.m);
        sh_.latest.restore_samples(gen, std::move(samples)); // they were not delivered
    }
    failed(*r.err);
}

} // namespace tips
