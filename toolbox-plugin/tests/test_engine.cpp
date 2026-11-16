#include <chrono>
#include <string>
#include <vector>

#include "fake_transport.h"
#include "test_main.h"
#include "tips/engine.h"

using namespace tips;
using namespace std::chrono_literals;
using testing::FakeTransport;

namespace {

constexpr const char* kSession = "SESSION0123456789";
const std::string kRun = "/api/tracker/runs/r1";
const std::string kPresence = "/api/tracker/presence";

bool has(const std::string& body, const std::string& needle) { return body.find(needle) != std::string::npos; }

struct Rig {
    Shared shared;
    FakeTransport tr;
    Tp now{};
    std::vector<Ms> sleeps;
    Engine engine;

    Rig()
        : engine(shared, tr, Timing{}, [this] { return now; }, [this](Ms d) { sleeps.push_back(d); }, kSession)
    {
        shared.config.token = "tips_trk_x";
    }

    void have_run(const std::string& id = "r1")
    {
        shared.status.next_run = NextRun{id, "Beacon's -> Drok's", "scheduled", std::nullopt};
    }
    void in_map(uint32_t id, bool explorable, std::optional<Bounds> bounds = std::nullopt)
    {
        shared.latest.map_id = id;
        shared.latest.explorable = explorable;
        shared.latest.bounds = bounds;
    }
    void position(float x, float y) { shared.latest.pos = std::make_pair(Pos{x, y}, now); }
    void advance(Ms d) { now += d; }
    void start() { engine.handle(Cmd::Start); }
    std::string message() const { return shared.status.message; }
};

const Bounds kBounds{-18432.f, -21504.f, 21504.f, 21504.f};

} // namespace

TEST(start_without_next_run_says_refresh_first)
{
    Rig r;
    r.start();
    CHECK(r.message() == "Press Refresh to find your next run first.");
    CHECK(r.tr.requests.empty());
}

TEST(start_in_general_mode_says_choose_runner)
{
    Rig r;
    r.have_run();
    r.shared.config.tracking_mode = TrackingMode::General;
    r.start();
    CHECK(r.message() == "Choose Runner tracking first.");
    CHECK(r.tr.requests.empty());
}

TEST(start_without_token_says_paste_token_and_sends_nothing)
{
    Rig r;
    r.have_run();
    r.shared.config.token.clear();
    r.start();
    CHECK(r.message() == "Paste your tracker token first.");
    CHECK(r.tr.requests.empty());
    CHECK(!r.shared.status.active_run.has_value());
}

TEST(start_posts_start_then_zone_with_current_map)
{
    Rig r;
    r.have_run();
    r.in_map(90, true);
    r.shared.latest.samples.push_back({1.f, 1.f});
    r.start();
    CHECK(r.tr.requests.size() == 2);
    CHECK(r.tr.requests[0].path == kRun + "/start");
    CHECK(r.tr.requests[0].token == "tips_trk_x");
    CHECK(has(r.tr.requests[0].body, std::string("\"client_session\":\"") + kSession + "\""));
    CHECK(r.tr.requests[1].path == kRun + "/zone");
    CHECK(has(r.tr.requests[1].body, "\"map_id\":90"));
    CHECK(has(r.tr.requests[1].body, "\"is_explorable\":true"));
    CHECK(r.shared.status.active_run == std::optional<std::string>("r1"));
    CHECK(r.message() == "Tracking.");
    CHECK(r.shared.latest.samples.empty());
}

TEST(start_with_no_known_map_sends_only_start)
{
    Rig r;
    r.have_run();
    r.start();
    CHECK(r.tr.requests.size() == 1);
    CHECK(r.shared.status.active_run.has_value());
}

TEST(start_ignored_reply_does_not_start)
{
    Rig r;
    r.have_run();
    r.tr.reply(kRun + "/start", 200, R"({"ok":true,"ignored":true})");
    r.start();
    CHECK(!r.shared.status.active_run.has_value());
    CHECK(r.message() == "Another game client is already tracking this run.");
    CHECK(r.tr.requests.size() == 1);
}

TEST(zone_reply_sets_progress_and_off_route)
{
    Rig r;
    r.have_run();
    r.in_map(90, true);
    r.tr.reply(kRun + "/zone", 200, R"({"ok":true,"progress":33,"off_route":true,"finished":false})");
    r.start();
    CHECK(r.shared.status.progress == std::optional<long long>(33));
    CHECK(r.shared.status.off_route);
}

TEST(zone_finished_ends_the_run_and_refreshes)
{
    Rig r;
    r.have_run();
    r.in_map(90, true);
    r.tr.reply(kRun + "/zone", 200, R"({"ok":true,"progress":100,"off_route":false,"finished":true})");
    r.tr.reply("/api/tracker/next-run", 200, R"({"run":{"id":"r2","title":"Next","status":"queued","route_name":null}})");
    r.start();
    CHECK(!r.shared.status.active_run.has_value());
    CHECK(r.message() == "Route finished. The run is complete.");
    CHECK(r.tr.requests.back().path == "/api/tracker/next-run");
    CHECK(r.shared.status.next_run.has_value() && r.shared.status.next_run->id == "r2");
}

TEST(zone_ignored_stops_with_takeover_message)
{
    Rig r;
    r.have_run();
    r.in_map(90, true);
    r.tr.reply(kRun + "/zone", 200, R"({"ok":true,"ignored":true})");
    r.start();
    CHECK(!r.shared.status.active_run.has_value());
    CHECK(r.message() == "Another game client took over this run.");
}

TEST(a_401_says_the_token_was_rejected)
{
    Rig r;
    r.have_run();
    r.tr.reply(kRun + "/start", 401, R"({"error":"bad token"})");
    r.start();
    CHECK(r.message() == "TIPS rejected the token. Make a new one on your shop page.");
    CHECK(!r.shared.status.active_run.has_value());
    CHECK(r.tr.requests.size() == 1);
}

TEST(a_409_on_start_says_the_run_is_over)
{
    Rig r;
    r.have_run();
    r.tr.reply(kRun + "/start", 409, R"({"error":"Finish your current live run first"})");
    r.start();
    CHECK(r.message() == "Finish your current live run first. The run is over.");
    CHECK(!r.shared.status.active_run.has_value());
}

TEST(a_404_says_the_run_is_not_yours)
{
    Rig r;
    r.have_run();
    r.tr.reply(kRun + "/start", 404, R"({"error":"nope"})");
    r.start();
    CHECK(r.message() == "That run isn't yours any more. Refresh.");
}

TEST(a_400_is_shown_and_not_retried)
{
    Rig r;
    r.have_run();
    r.tr.reply(kRun + "/start", 400, R"({"error":"Bad input"})");
    r.start();
    CHECK(r.message() == "Bad input (400)");
    CHECK(r.tr.requests.size() == 1);
    CHECK(r.sleeps.empty());
}

TEST(a_503_on_zone_is_retried_with_doubling_backoff_then_shown)
{
    Rig r;
    r.have_run();
    r.in_map(90, true);
    for (int i = 0; i < 4; ++i) r.tr.reply(kRun + "/zone", 503, R"({"error":"Boom"})");
    r.start();
    CHECK(r.tr.count("/zone") == 4);
    CHECK((r.sleeps == std::vector<Ms>{1000ms, 2000ms, 4000ms}));
    CHECK(r.message() == "Boom (503)");
    CHECK(r.shared.status.active_run.has_value()); // a server hiccup does not end the run
}

TEST(a_network_failure_on_refresh_is_retried_once_and_shown)
{
    Rig r;
    r.tr.network_error("/api/tracker/next-run", "timed out");
    r.tr.network_error("/api/tracker/next-run", "timed out");
    r.engine.handle(Cmd::Refresh);
    CHECK(r.tr.count("/next-run") == 2);
    CHECK((r.sleeps == std::vector<Ms>{1000ms}));
    CHECK(r.message() == "Can't reach TIPS: timed out");
}

TEST(refresh_reads_the_next_run_and_clears_the_message)
{
    Rig r;
    r.shared.status.message = "old";
    r.tr.reply("/api/tracker/next-run", 200, R"({"run":{"id":"r9","title":"T","status":"live","route_name":"Route"}})");
    r.engine.handle(Cmd::Refresh);
    CHECK(r.shared.status.next_run.has_value());
    CHECK(r.shared.status.next_run->id == "r9");
    CHECK(r.shared.status.next_run->route_name == std::optional<std::string>("Route"));
    CHECK(r.message().empty());
}

TEST(refresh_with_no_run_says_so)
{
    Rig r;
    r.tr.reply("/api/tracker/next-run", 200, R"({"run":null})");
    r.engine.handle(Cmd::Refresh);
    CHECK(!r.shared.status.next_run.has_value());
    CHECK(r.message() == "No open runs. Post one on the TIPS site.");
}

TEST(end_posts_end_and_refreshes)
{
    Rig r;
    r.have_run();
    r.start();
    r.tr.reply("/api/tracker/next-run", 200, R"({"run":{"id":"r2","title":"T","status":"queued"}})");
    r.engine.handle(Cmd::End);
    CHECK(r.tr.count("/end") == 1);
    CHECK(r.tr.requests.back().path == "/api/tracker/next-run");
    CHECK(!r.shared.status.active_run.has_value());
    CHECK(r.message() == "Run ended.");
}

TEST(a_failed_end_still_leaves_the_engine_idle)
{
    Rig r;
    r.have_run();
    r.start();
    r.tr.reply(kRun + "/end", 400, R"({"error":"nope"})");
    r.engine.handle(Cmd::End);
    CHECK(!r.shared.status.active_run.has_value());
    CHECK(!r.engine.tracking());
    CHECK(r.message() == "nope (400)");
}

TEST(heartbeat_every_30_seconds_without_sharing_carries_no_position)
{
    Rig r;
    r.have_run();
    r.in_map(90, true);
    r.start();
    const auto before = r.tr.requests.size();
    r.position(5.f, 6.f);
    r.advance(29999ms);
    r.engine.tick();
    CHECK(r.tr.requests.size() == before);
    r.advance(1ms);
    r.engine.tick();
    CHECK(r.tr.requests.size() == before + 1);
    CHECK(r.tr.requests.back().path == kRun + "/heartbeat");
    CHECK(!has(r.tr.requests.back().body, "\"x\""));
    CHECK(!has(r.tr.requests.back().body, "points"));
}

TEST(heartbeat_every_5_seconds_with_a_fresh_position_when_sharing)
{
    Rig r;
    r.have_run();
    r.shared.config.share_position = true;
    r.start();
    r.position(5.f, 6.f);
    r.advance(4999ms);
    r.engine.tick();
    CHECK(r.tr.count("/heartbeat") == 0);
    r.advance(1ms);
    r.engine.tick();
    CHECK(r.tr.count("/heartbeat") == 1);
    CHECK(has(r.tr.last("/heartbeat")->body, "\"x\":5"));
    CHECK(has(r.tr.last("/heartbeat")->body, "\"y\":6"));
}

TEST(a_position_older_than_15_seconds_is_not_sent)
{
    Rig r;
    r.have_run();
    r.shared.config.share_position = true;
    r.start();
    r.position(5.f, 6.f);
    r.advance(30s); // no fresh position: the slow 30 s beat
    r.engine.tick();
    CHECK(r.tr.count("/heartbeat") == 1);
    CHECK(!has(r.tr.last("/heartbeat")->body, "\"x\""));
}

TEST(heartbeat_points_are_oldest_first_and_drained)
{
    Rig r;
    r.have_run();
    r.shared.config.share_position = true;
    r.start();
    r.shared.latest.add_sample(1.f, 2.f, r.now, 1000ms);
    r.shared.latest.add_sample(3.f, 4.f, r.now + 1s, 1000ms);
    r.advance(5s);
    r.position(9.f, 9.f);
    r.engine.tick();
    CHECK(has(r.tr.last("/heartbeat")->body, "\"points\":[[1,2],[3,4]]"));
    CHECK(r.shared.latest.samples.empty());
}

TEST(a_failed_heartbeat_puts_its_samples_back)
{
    Rig r;
    r.have_run();
    r.shared.config.share_position = true;
    r.start();
    r.shared.latest.add_sample(1.f, 2.f, r.now, 1000ms);
    r.advance(5s);
    r.position(9.f, 9.f);
    r.tr.reply(kRun + "/heartbeat", 500, R"({"error":"Boom"})");
    r.engine.tick();
    CHECK(r.shared.latest.samples.size() == 1);
    CHECK(r.message() == "Boom (500)");
    CHECK(r.tr.count("/heartbeat") == 1); // a heartbeat is tried once
}

TEST(samples_from_an_earlier_zone_are_not_put_back)
{
    Rig r;
    r.have_run();
    r.shared.config.share_position = true;
    r.start();
    r.shared.latest.add_sample(1.f, 2.f, r.now, 1000ms);
    r.advance(5s);
    r.position(9.f, 9.f);
    r.tr.reply(kRun + "/heartbeat", 500, R"({"error":"Boom"})");
    r.tr.on_request = [&] { ++r.shared.latest.zone_gen; r.tr.on_request = nullptr; }; // the game changed map mid-request
    r.engine.tick();
    CHECK(r.shared.latest.samples.empty());
}

TEST(switching_sharing_off_sends_a_bare_heartbeat_at_once)
{
    Rig r;
    r.have_run();
    r.shared.config.share_position = true;
    r.start();
    r.position(5.f, 6.f);
    r.shared.latest.add_sample(1.f, 2.f, r.now, 1000ms);
    r.advance(1s);
    r.engine.tick(); // sharing is on; too early for a beat
    CHECK(r.tr.count("/heartbeat") == 0);
    r.shared.config.share_position = false;
    r.advance(1s);
    r.engine.tick();
    CHECK(r.tr.count("/heartbeat") == 1);
    CHECK(!has(r.tr.last("/heartbeat")->body, "\"x\""));
    CHECK(!has(r.tr.last("/heartbeat")->body, "points"));
    CHECK(r.shared.latest.samples.empty());
}

TEST(sharing_switched_on_midzone_rereports_the_zone_with_bounds_at_most_three_times)
{
    Rig r;
    r.have_run();
    r.in_map(90, true, kBounds);
    r.start(); // sharing is off: the zone goes without bounds
    CHECK(!has(r.tr.last("/zone")->body, "bounds"));
    r.shared.config.share_position = true;
    for (int i = 0; i < 4; ++i) r.tr.reply(kRun + "/zone", 400, R"({"error":"no"})");
    for (int i = 0; i < 3; ++i) {
        r.position(5.f, 6.f);
        r.advance(5s);
        r.engine.tick();
    }
    CHECK(r.tr.count("/zone") == 4); // the original plus three re-reports
    CHECK(has(r.tr.last("/zone")->body, "\"x\":5"));
    CHECK(has(r.tr.last("/zone")->body, "\"bounds\":[-18432,-21504,21504,21504]"));
    r.position(5.f, 6.f);
    r.advance(5s);
    r.engine.tick();
    CHECK(r.tr.count("/zone") == 4); // the cap is reached: a plain heartbeat goes instead
    CHECK(r.tr.count("/heartbeat") == 1);
}

TEST(a_successful_rereport_stops_the_rereporting)
{
    Rig r;
    r.have_run();
    r.in_map(90, true, kBounds);
    r.start();
    r.shared.config.share_position = true;
    r.position(5.f, 6.f);
    r.advance(5s);
    r.engine.tick(); // re-report succeeds (default 200)
    r.position(5.f, 6.f);
    r.advance(5s);
    r.engine.tick();
    CHECK(r.tr.count("/zone") == 2);
    CHECK(r.tr.count("/heartbeat") == 1);
}

TEST(bounds_are_sent_with_a_zone_only_while_sharing)
{
    Rig off;
    off.have_run();
    off.in_map(90, true, kBounds);
    off.start();
    CHECK(!has(off.tr.last("/zone")->body, "bounds"));

    Rig on;
    on.have_run();
    on.shared.config.share_position = true;
    on.in_map(90, true, kBounds);
    on.start();
    CHECK(has(on.tr.last("/zone")->body, "bounds"));
}

TEST(general_mode_uses_the_presence_endpoints_and_shares_by_definition)
{
    Rig r;
    r.shared.config.tracking_mode = TrackingMode::General;
    r.in_map(20, true, kBounds);
    r.position(7.f, 8.f);
    r.engine.handle(Cmd::StartGeneral);
    CHECK(r.tr.requests[0].path == kPresence + "/start");
    CHECK(r.tr.requests[1].path == kPresence + "/zone");
    CHECK(has(r.tr.requests[1].body, "\"x\":7"));
    CHECK(has(r.tr.requests[1].body, "bounds"));
    CHECK(r.shared.status.active_general);
    CHECK(r.message() == "Tracking.");
    r.position(7.f, 8.f);
    r.advance(5s);
    r.engine.tick();
    CHECK(r.tr.last("/heartbeat")->path == kPresence + "/heartbeat");
    r.engine.handle(Cmd::EndGeneral);
    CHECK(r.tr.last("/end")->path == kPresence + "/end");
    CHECK(!r.shared.status.active_general);
    CHECK(r.message() == "Tracking stopped.");
}

TEST(a_409_while_in_general_mode_says_tracking_stopped)
{
    Rig r;
    r.shared.config.tracking_mode = TrackingMode::General;
    r.engine.handle(Cmd::StartGeneral);
    r.advance(30s);
    r.tr.reply(kPresence + "/heartbeat", 409, R"({"error":"No presence"})");
    r.engine.tick();
    CHECK(!r.shared.status.active_general);
    CHECK(r.message() == "No presence. Tracking stopped.");
}

TEST(a_failed_end_of_general_tracking_warns_the_dot_stays)
{
    Rig r;
    r.shared.config.tracking_mode = TrackingMode::General;
    r.engine.handle(Cmd::StartGeneral);
    r.tr.reply(kPresence + "/end", 400, R"({"error":"nope"})");
    r.engine.handle(Cmd::EndGeneral);
    CHECK(!r.shared.status.active_general);
    CHECK(has(r.message(), "nope (400)"));
    CHECK(has(r.message(), "stays on the map"));
}

TEST(mode_conflicts_are_explained_and_send_nothing)
{
    Rig a; // general requested while the mode is Runner
    a.engine.handle(Cmd::StartGeneral);
    CHECK(a.message() == "Choose General tracking first.");
    CHECK(a.tr.requests.empty());

    Rig b; // a run is active, the mode was flipped, general requested
    b.have_run();
    b.start();
    b.shared.config.tracking_mode = TrackingMode::General;
    const auto n = b.tr.requests.size();
    b.engine.handle(Cmd::StartGeneral);
    CHECK(b.message() == "End your run before starting general tracking.");
    CHECK(b.tr.requests.size() == n);

    Rig c; // general is on, the mode was flipped, a run requested
    c.have_run();
    c.shared.config.tracking_mode = TrackingMode::General;
    c.engine.handle(Cmd::StartGeneral);
    c.shared.config.tracking_mode = TrackingMode::Run;
    const auto m = c.tr.requests.size();
    c.start();
    CHECK(c.message() == "Stop general tracking before starting a run.");
    CHECK(c.tr.requests.size() == m);
}

TEST(nothing_is_sent_when_idle)
{
    Rig r;
    r.in_map(90, true, kBounds);
    r.position(1.f, 2.f);
    r.shared.config.share_position = true;
    for (int i = 0; i < 20; ++i) {
        r.advance(10s);
        r.engine.tick();
    }
    r.engine.handle(Cmd::Zone);
    CHECK(r.tr.requests.empty());
    CHECK(!r.engine.tracking());
}

TEST(non_json_replies_do_not_break_the_run)
{
    Rig r;
    r.have_run();
    r.in_map(90, true);
    r.tr.reply(kRun + "/start", 200, "<html>captive portal</html>");
    r.tr.reply(kRun + "/zone", 200, "<html>captive portal</html>");
    r.start();
    CHECK(r.shared.status.active_run.has_value());
    CHECK(!r.shared.status.progress.has_value());

    Rig q;
    q.tr.reply("/api/tracker/next-run", 200, "<html>captive portal</html>");
    q.engine.handle(Cmd::Refresh);
    CHECK(q.message() == "No open runs. Post one on the TIPS site.");
}

TEST(shutdown_ends_an_active_run_once)
{
    Rig r;
    r.have_run();
    r.start();
    r.engine.shutdown();
    CHECK(r.tr.count("/end") == 1);
    r.engine.shutdown();
    CHECK(r.tr.count("/end") == 1);
}
