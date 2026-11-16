#pragma once

#include <functional>
#include <mutex>
#include <optional>
#include <string>

#include "tips/samples.h"
#include "tips/transport.h"
#include "tips/types.h"

namespace tips {

/// State shared between the game thread (the plugin) and the worker. Everything is guarded by `m`; the engine
/// never holds it across a network call.
struct Shared {
    std::mutex m;
    Config config;
    Status status;
    Latest latest;
    Ms sample_every{1000};
};

enum class Cmd { Refresh, Start, End, Zone, StartGeneral, EndGeneral };

/// The tracker protocol (docs/tracker-protocol.md): start/zone/heartbeat/end for a run, and the presence
/// endpoints for general tracking. A port of `Worker` in src/uplink.rs. It owns no threads: the Uplink calls
/// handle() for each command and tick() regularly, from one worker thread only.
class Engine {
public:
    Engine(Shared& shared, Transport& transport, Timing timing, std::function<Tp()> now, std::function<void(Ms)> sleep, std::string session);

    void handle(Cmd cmd);
    /// Heartbeats while tracking: every 30 s, or every 5 s while a fresh position is being shared.
    void tick();
    /// A run or general tracking is on.
    bool tracking() const { return active_.has_value() || general_; }
    /// Best-effort tidy end of whatever is on. Safe to call more than once.
    void shutdown();
    /// While this returns true a failed request is not retried (the Uplink is shutting down).
    void set_cancel(std::function<bool()> cancelled) { cancelled_ = std::move(cancelled); }

private:
    struct ApiError {
        int code = 0; // 0: the server could not be reached
        std::string msg;
        bool no_token = false;
        bool retryable() const { return code == 0 || code == 429 || code >= 500; }
        std::string text() const;
    };
    struct CallResult {
        std::optional<ApiError> err;
        std::string body;
        bool ok() const { return !err; }
    };

    CallResult call(int attempts, const std::function<Response(const std::string& token)>& send);
    CallResult get(int attempts, const std::string& path);
    CallResult post(int attempts, const std::string& path, const std::string& body);

    void say(std::string msg);
    void failed(const ApiError& e);
    void stop_run();
    bool sharing();
    std::optional<std::string> track_base() const;
    std::optional<Pos> position_to_send();
    std::pair<uint64_t, std::vector<std::pair<float, float>>> take_samples();

    void refresh(bool keep_message);
    void start_run();
    void end_run();
    void start_general();
    void end_general();
    void zone();

    Shared& sh_;
    Transport& tr_;
    Timing timing_;
    std::function<Tp()> now_;
    std::function<void(Ms)> sleep_;
    std::function<bool()> cancelled_;
    std::string session_;

    std::optional<std::string> active_; // the run being reported on
    bool general_ = false;              // general tracking: requests go to the presence endpoints
    Tp last_beat_;
    bool bounds_sent_ = false;    // the server has this zone's bounds
    uint64_t retry_gen_ = 0;      // the zone whose bounds are being re-reported
    int zone_retries_ = 0;        // capped, so a zone the server keeps refusing cannot starve the heartbeats
    bool was_sharing_ = false;    // to notice the moment sharing is switched off
};

} // namespace tips
