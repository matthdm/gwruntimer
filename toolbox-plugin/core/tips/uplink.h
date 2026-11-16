#pragma once

#include <atomic>
#include <condition_variable>
#include <deque>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <thread>

#include "tips/engine.h"

namespace tips {

/// The game-facing side of the tracker. Every method is cheap and non-blocking (a mutex and a bounded queue); the
/// network happens on a background thread running the Engine. A port of `Uplink` in src/uplink.rs.
class Uplink {
public:
    /// `session` is the random per-load client_session; empty means generate one (32 hex characters).
    Uplink(std::unique_ptr<Transport> transport, Config initial, Timing timing = {}, std::string session = {});
    ~Uplink(); // stops and joins

    Uplink(const Uplink&) = delete;
    Uplink& operator=(const Uplink&) = delete;

    Config config() const;
    Status status() const;
    /// A copy of what the game side last reported (for the panel's "Position check").
    Latest latest() const;
    /// Whether position sharing is on (general tracking is public position by definition).
    bool sharing() const;

    /// Updates the settings (the token is cleaned; the mode cannot change while tracking). Saving them is the caller's job.
    void update_config(const std::function<void(Config&)>& change);

    /// A map finished loading. `bounds` is read by the caller only while sharing (else nullopt).
    void push_zone(uint32_t map_id, bool explorable, std::optional<Bounds> bounds);
    /// Whether the game loop has reported a map yet.
    bool knows_map() const;
    /// Sharing is on, a map is known and its bounds are not: the caller should read them and set_bounds().
    bool wants_bounds() const;
    void set_bounds(std::optional<Bounds> bounds);
    /// The character's latest position, if it could be read. Kept for the trail only while sharing.
    void set_position(std::optional<Pos> pos);

    void refresh();
    void start_run();
    void end_run();
    void start_general();
    void end_general();

    /// Asks the worker to stop: aborts a request in flight, sends one best-effort end, exits. Returns at once.
    void signal_stop();
    /// signal_stop(), then waits until the worker has finished: up to `polite` for the final end request to go
    /// out, then aborts it and waits up to `hard` more. Returns stopped(). Toolbox's close-game path
    /// (ForceTerminate) asks CanTerminate() once, right after SignalTerminate(), and terminates the plugin module
    /// whatever the answer, so a plugin that is still running then makes Toolbox crash.
    bool stop_and_wait(Ms polite, Ms hard);
    /// The worker thread has finished (safe to unload the DLL).
    bool stopped() const { return stopped_; }

private:
    void send(Cmd cmd);
    void run();
    bool wait_stopped(Ms limit) const;
    bool sharing_locked() const;

    mutable Shared shared_;
    std::unique_ptr<Transport> transport_;
    Engine engine_;

    std::mutex queue_m_;
    std::condition_variable queue_cv_;
    std::deque<Cmd> queue_;
    bool stop_ = false;
    std::atomic<bool> stopped_{false};
    // While stopping, retries are cancelled and backoff sleeps cut short, except during the final end request
    // (`final_phase_`), which is retried briefly so a runner does not stay on the map; `giveup_` ends even that.
    std::atomic<bool> final_phase_{false};
    std::atomic<bool> giveup_{false};
    bool stop_cancels_retries() const; // call with queue_m_ held or not: only reads atomics
    std::thread worker_;
};

} // namespace tips
