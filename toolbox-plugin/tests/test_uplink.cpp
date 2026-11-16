#include <atomic>
#include <chrono>
#include <condition_variable>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "test_main.h"
#include "tips/uplink.h"

using namespace tips;
using namespace std::chrono_literals;

namespace {

const char* kRunJson = R"({"run":{"id":"r1","title":"T","status":"scheduled","route_name":null}})";

struct State {
    std::mutex m;
    std::condition_variable cv;
    std::vector<std::string> paths;
    std::vector<std::thread::id> threads;
    bool gate_open = true;      // closed: every request waits
    bool block_zone = false;    // a zone request waits until abort()
    bool block_end = false;     // an end request waits until abort() has been called twice
    std::chrono::milliseconds end_delay{0}; // an end request takes this long to answer
    int end_failures = 0;       // the first N end requests fail with a network error
    bool aborted = false;
    int aborts = 0;

    int count(const std::string& suffix)
    {
        std::lock_guard g(m);
        int n = 0;
        for (const auto& p : paths) {
            if (p.size() >= suffix.size() && p.compare(p.size() - suffix.size(), suffix.size(), suffix) == 0) ++n;
        }
        return n;
    }
    size_t total()
    {
        std::lock_guard g(m);
        return paths.size();
    }
};

class Fake : public Transport {
public:
    explicit Fake(std::shared_ptr<State> s) : st(std::move(s)) {}
    Response get(std::string_view path, std::string_view) override { return handle(path); }
    Response post(std::string_view path, std::string_view, std::string_view) override { return handle(path); }
    void abort() override
    {
        std::lock_guard g(st->m);
        st->aborted = true;
        ++st->aborts;
        st->cv.notify_all();
    }

private:
    Response handle(std::string_view path)
    {
        std::unique_lock l(st->m);
        st->paths.emplace_back(path);
        st->threads.push_back(std::this_thread::get_id());
        st->cv.wait(l, [&] { return st->gate_open || st->aborted; });
        if (st->block_zone && path.size() >= 5 && path.substr(path.size() - 5) == "/zone") {
            st->cv.wait(l, [&] { return st->aborted; });
            return {0, {}, "aborted"};
        }
        const bool is_end = path.size() >= 4 && path.substr(path.size() - 4) == "/end";
        if (is_end && st->block_end) {
            st->cv.wait(l, [&] { return st->aborts >= 2; }); // the first abort is the stop itself, the second cuts this
            return {0, {}, "aborted"};
        }
        if (is_end && st->end_failures > 0) {
            --st->end_failures;
            return {0, {}, "network down"};
        }
        if (is_end && st->end_delay.count() > 0) {
            const auto delay = st->end_delay;
            l.unlock();
            std::this_thread::sleep_for(delay);
            l.lock();
        }
        if (path == "/api/tracker/next-run") return {200, kRunJson, {}};
        return {200, "{\"ok\":true}", {}};
    }
    std::shared_ptr<State> st;
};

bool wait_for(const std::function<bool()>& pred, std::chrono::milliseconds limit = 3000ms)
{
    const auto end = std::chrono::steady_clock::now() + limit;
    while (std::chrono::steady_clock::now() < end) {
        if (pred()) return true;
        std::this_thread::sleep_for(5ms);
    }
    return pred();
}

Config with_token()
{
    Config c;
    c.token = "tips_trk_x";
    return c;
}

Timing fast_samples()
{
    Timing t;
    t.sample = 0ms;
    return t;
}

} // namespace

TEST(commands_run_in_order_off_the_caller_thread)
{
    auto st = std::make_shared<State>();
    Uplink up(std::make_unique<Fake>(st), with_token());
    up.refresh();
    CHECK(wait_for([&] { return up.status().next_run.has_value(); }));
    up.start_run();
    CHECK(wait_for([&] { return up.status().active_run.has_value(); }));
    std::lock_guard g(st->m);
    CHECK(st->paths.size() == 2);
    CHECK(st->paths[0] == "/api/tracker/next-run");
    CHECK(st->paths[1] == "/api/tracker/runs/r1/start");
    for (const auto& id : st->threads) CHECK(id != std::this_thread::get_id());
}

TEST(the_queue_is_bounded_and_the_caller_never_blocks)
{
    auto st = std::make_shared<State>();
    st->gate_open = false;
    Uplink up(std::make_unique<Fake>(st), with_token());
    const auto t0 = std::chrono::steady_clock::now();
    for (int i = 0; i < 300; ++i) up.refresh();
    CHECK(std::chrono::steady_clock::now() - t0 < 1000ms);
    {
        std::lock_guard g(st->m);
        st->gate_open = true;
        st->cv.notify_all();
    }
    CHECK(wait_for([&] { return up.status().next_run.has_value(); }));
    std::this_thread::sleep_for(300ms);
    CHECK(st->total() <= 70); // one in flight plus a queue of 64, never the 300 asked for
}

TEST(the_mode_cannot_change_while_tracking)
{
    auto st = std::make_shared<State>();
    Config c = with_token();
    c.tracking_mode = TrackingMode::General;
    Uplink up(std::make_unique<Fake>(st), c);
    up.start_general();
    CHECK(wait_for([&] { return up.status().active_general; }));
    up.update_config([](Config& cfg) { cfg.tracking_mode = TrackingMode::Run; });
    CHECK(up.config().tracking_mode == TrackingMode::General);
}

TEST(update_config_cleans_the_token)
{
    auto st = std::make_shared<State>();
    Uplink up(std::make_unique<Fake>(st), Config{});
    up.update_config([](Config& cfg) { cfg.token = "  tips_trk_abc \n"; });
    CHECK(up.config().token == "tips_trk_abc");
    up.update_config([](Config& cfg) { cfg.token = std::string(300, 'z'); });
    CHECK(up.config().token.size() == 100);
}

TEST(push_zone_clears_the_old_position_and_trail_and_bumps_the_generation)
{
    auto st = std::make_shared<State>();
    Config c = with_token();
    c.share_position = true;
    Uplink up(std::make_unique<Fake>(st), c, fast_samples());
    CHECK(!up.knows_map());
    up.set_position(Pos{1.f, 2.f});
    up.set_position(Pos{3.f, 4.f});
    const auto before = up.latest();
    CHECK(before.pos.has_value());
    CHECK(before.samples.size() == 2);
    up.push_zone(90, true, Bounds{0.f, 0.f, 1.f, 1.f});
    const auto after = up.latest();
    CHECK(up.knows_map());
    CHECK(after.map_id == std::optional<uint32_t>(90));
    CHECK(after.explorable);
    CHECK(!after.pos.has_value());
    CHECK(after.samples.empty());
    CHECK(after.zone_gen == before.zone_gen + 1);
    CHECK(after.bounds.has_value());
}

TEST(an_insane_position_is_dropped)
{
    auto st = std::make_shared<State>();
    Uplink up(std::make_unique<Fake>(st), with_token());
    up.set_position(Pos{1.f, 2.f});
    CHECK(up.latest().pos.has_value());
    up.set_position(Pos{std::nanf(""), 2.f});
    CHECK(!up.latest().pos.has_value());
    up.set_position(Pos{9.0e6f, 2.f});
    CHECK(!up.latest().pos.has_value());
    up.set_position(std::nullopt);
    CHECK(!up.latest().pos.has_value());
}

TEST(trail_samples_are_kept_only_while_sharing)
{
    auto st = std::make_shared<State>();
    Uplink up(std::make_unique<Fake>(st), with_token(), fast_samples());
    up.set_position(Pos{1.f, 2.f});
    CHECK(up.latest().samples.empty());
    up.update_config([](Config& c) { c.share_position = true; });
    up.set_position(Pos{1.f, 2.f});
    up.set_position(Pos{3.f, 4.f});
    CHECK(up.latest().samples.size() == 2);
    up.update_config([](Config& c) { c.share_position = false; });
    up.set_position(Pos{5.f, 6.f});
    CHECK(up.latest().samples.empty());
}

TEST(wants_bounds_only_when_sharing_with_a_map_and_no_bounds)
{
    auto st = std::make_shared<State>();
    Uplink up(std::make_unique<Fake>(st), with_token());
    CHECK(!up.wants_bounds()); // no map, not sharing
    up.push_zone(90, true, std::nullopt);
    CHECK(!up.wants_bounds()); // map known but not sharing
    up.update_config([](Config& c) { c.share_position = true; });
    CHECK(up.wants_bounds());
    up.set_bounds(Bounds{0.f, 0.f, 1.f, 1.f});
    CHECK(!up.wants_bounds());
}

TEST(set_bounds_is_ignored_while_not_sharing)
{
    auto st = std::make_shared<State>();
    Uplink up(std::make_unique<Fake>(st), with_token());
    up.push_zone(90, true, std::nullopt);
    up.set_bounds(Bounds{0.f, 0.f, 1.f, 1.f});
    CHECK(!up.latest().bounds.has_value());
}

TEST(stopping_aborts_the_request_in_flight_and_sends_exactly_one_end)
{
    auto st = std::make_shared<State>();
    st->block_zone = true;
    Uplink up(std::make_unique<Fake>(st), with_token());
    up.push_zone(90, true, std::nullopt);
    up.refresh();
    CHECK(wait_for([&] { return up.status().next_run.has_value(); }));
    up.start_run();
    CHECK(wait_for([&] { return st->count("/zone") >= 1; })); // the worker is now stuck in the zone request
    up.signal_stop();
    CHECK(wait_for([&] { return up.stopped(); }, 2000ms));
    CHECK(st->count("/end") == 1);
    CHECK(st->count("/zone") == 1); // Start's zone only (push_zone's ran while idle and sent nothing); no retry after stop
}

TEST(an_idle_uplink_sends_nothing_when_destroyed)
{
    auto st = std::make_shared<State>();
    {
        Uplink up(std::make_unique<Fake>(st), with_token());
        up.push_zone(90, true, std::nullopt); // known map, but nothing is tracking
        std::this_thread::sleep_for(300ms);
    }
    CHECK(st->total() == 0);
}

// Toolbox closes the game with ForceTerminate: it calls the plugin's SignalTerminate() and asks CanTerminate() once,
// at once, then terminates the plugin module regardless (PluginModule::Terminate asserts no plugin is still loaded).
// So the plugin must be finished when SignalTerminate() returns, not some frames later.

TEST(stop_and_wait_returns_only_after_the_worker_finished_a_slow_end_request)
{
    auto st = std::make_shared<State>();
    st->end_delay = 300ms;
    Config c = with_token();
    c.tracking_mode = TrackingMode::General;
    Uplink up(std::make_unique<Fake>(st), c);
    up.start_general();
    CHECK(wait_for([&] { return up.status().active_general; }));
    const bool stopped = up.stop_and_wait(1500ms, 2000ms);
    CHECK(stopped);
    CHECK(up.stopped()); // what Toolbox's CanTerminate() sees, immediately
    CHECK(st->count("/presence/end") == 1);
}

TEST(stop_and_wait_cuts_a_hanging_end_request_and_still_finishes_in_bounded_time)
{
    auto st = std::make_shared<State>();
    st->block_end = true;
    Config c = with_token();
    c.tracking_mode = TrackingMode::General;
    Uplink up(std::make_unique<Fake>(st), c);
    up.start_general();
    CHECK(wait_for([&] { return up.status().active_general; }));
    const auto t0 = std::chrono::steady_clock::now();
    const bool stopped = up.stop_and_wait(300ms, 2000ms);
    const auto took = std::chrono::steady_clock::now() - t0;
    CHECK(stopped);
    CHECK(up.stopped());
    CHECK(took < 1500ms);
}

TEST(stop_and_wait_on_an_idle_uplink_is_immediate_and_sends_nothing)
{
    auto st = std::make_shared<State>();
    Uplink up(std::make_unique<Fake>(st), with_token());
    const auto t0 = std::chrono::steady_clock::now();
    CHECK(up.stop_and_wait(1500ms, 2000ms));
    CHECK(std::chrono::steady_clock::now() - t0 < 1000ms);
    CHECK(up.stopped());
    CHECK(st->total() == 0);
}

TEST(signal_stop_alone_returns_before_the_worker_has_finished_which_is_why_close_game_crashed_toolbox)
{
    auto st = std::make_shared<State>();
    st->end_delay = 300ms;
    Config c = with_token();
    c.tracking_mode = TrackingMode::General;
    Uplink up(std::make_unique<Fake>(st), c);
    up.start_general();
    CHECK(wait_for([&] { return up.status().active_general; }));
    up.signal_stop();
    CHECK(!up.stopped()); // Toolbox's one CanTerminate() check, right after SignalTerminate(), would say "not yet"
    CHECK(wait_for([&] { return up.stopped(); }, 3000ms)); // it does finish, just not synchronously
}

// The runner must not stay on the TIPS map after the game closes: the final end request is retried (briefly) if the
// first attempt fails, but never past the bounded wait that keeps Toolbox from crashing.

TEST(the_final_end_request_is_retried_once_if_the_first_attempt_fails)
{
    auto st = std::make_shared<State>();
    st->end_failures = 1;
    Config c = with_token();
    c.tracking_mode = TrackingMode::General;
    Timing t;
    t.retry_base = 20ms;
    Uplink up(std::make_unique<Fake>(st), c, t);
    up.start_general();
    CHECK(wait_for([&] { return up.status().active_general; }));
    CHECK(up.stop_and_wait(1500ms, 2000ms));
    CHECK(st->count("/presence/end") == 2); // the first failed, the retry went through
}

TEST(the_final_end_request_is_not_retried_once_the_wait_has_given_up)
{
    auto st = std::make_shared<State>();
    st->block_end = true;
    Config c = with_token();
    c.tracking_mode = TrackingMode::General;
    Uplink up(std::make_unique<Fake>(st), c);
    up.start_general();
    CHECK(wait_for([&] { return up.status().active_general; }));
    const auto t0 = std::chrono::steady_clock::now();
    CHECK(up.stop_and_wait(300ms, 2000ms));
    CHECK(std::chrono::steady_clock::now() - t0 < 1500ms);
    CHECK(st->count("/presence/end") == 1); // cut at 300 ms and not tried again
}
