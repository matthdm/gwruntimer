#include <string>
#include <utility>
#include <vector>

#include <glaze/glaze.hpp>

#include "test_main.h"
#include "tips/json.h"

using namespace tips;

namespace {
glz::generic parse(const std::string& text)
{
    glz::generic v;
    const auto ec = glz::read_json(v, text);
    CHECK(!ec);
    return v;
}
} // namespace

TEST(body_session_has_only_client_session)
{
    auto v = parse(body_session("abc"));
    CHECK(v.is_object());
    CHECK(v.get_object().size() == 1);
    CHECK(v["client_session"].get_string() == "abc");
}

TEST(body_zone_without_position_has_three_keys)
{
    auto v = parse(body_zone("s", 90, true, std::nullopt, std::nullopt));
    CHECK(v.get_object().size() == 3);
    CHECK(v["client_session"].get_string() == "s");
    CHECK(v["map_id"].get_number() == 90.0);
    CHECK(v["is_explorable"].get_boolean() == true);
}

TEST(body_zone_with_position_and_bounds_adds_x_y_bounds)
{
    auto v = parse(body_zone("s", 20, false, Pos{-1234.5f, 9876.25f}, Bounds{-18432.f, -21504.f, 21504.f, 21504.f}));
    CHECK(v["is_explorable"].get_boolean() == false);
    CHECK(v["x"].get_number() == -1234.5);
    CHECK(v["y"].get_number() == 9876.25);
    CHECK(v["bounds"].is_array());
    CHECK(v["bounds"].get_array().size() == 4);
    CHECK(v["bounds"][0].get_number() == -18432.0);
    CHECK(v["bounds"][3].get_number() == 21504.0);
}

TEST(body_zone_never_sends_bounds_without_a_position)
{
    auto v = parse(body_zone("s", 20, true, std::nullopt, Bounds{0.f, 0.f, 1.f, 1.f}));
    CHECK(!v.contains("bounds"));
    CHECK(!v.contains("x"));
}

TEST(body_heartbeat_carries_points_oldest_first_only_when_present)
{
    auto with = parse(body_heartbeat("s", Pos{1.f, 2.f}, {{1.f, 2.f}, {3.f, 4.f}}));
    CHECK(with["x"].get_number() == 1.0);
    CHECK(with["points"].get_array().size() == 2);
    CHECK(with["points"][0][0].get_number() == 1.0);
    CHECK(with["points"][1][1].get_number() == 4.0);

    auto bare = parse(body_heartbeat("s", std::nullopt, {}));
    CHECK(bare.get_object().size() == 1);
    CHECK(!bare.contains("points"));
    CHECK(!bare.contains("x"));
}

TEST(strings_with_quotes_and_backslashes_round_trip)
{
    const std::string nasty = "a\"b\\c\n\t\x01 end";
    auto v = parse(body_session(nasty));
    CHECK(v["client_session"].get_string() == nasty);
}

TEST(parse_next_run_reads_the_protocol_example)
{
    const auto run = parse_next_run(
        R"({"run":{"id":"r1","title":"Beacon's -> Drok's","status":"live","start_utc":null,"route_name":"Beacon's Perch to Droknar's Forge"}})");
    CHECK(run.has_value());
    CHECK(run->id == "r1");
    CHECK(run->title == "Beacon's -> Drok's");
    CHECK(run->status == "live");
    CHECK(run->route_name == std::optional<std::string>("Beacon's Perch to Droknar's Forge"));
}

TEST(parse_next_run_null_route_and_long_fields)
{
    const std::string long_title(200, 'T');
    const auto run = parse_next_run(R"({"run":{"id":"r","title":")" + long_title + R"(","status":"queued","route_name":null}})");
    CHECK(run.has_value());
    CHECK(!run->route_name.has_value());
    CHECK(run->title.size() == 80);
}

TEST(parse_next_run_rejects_non_runs)
{
    CHECK(!parse_next_run(R"({"run":null})").has_value());
    CHECK(!parse_next_run("").has_value());
    CHECK(!parse_next_run("<html>captive portal</html>").has_value());
    CHECK(!parse_next_run("[]").has_value());
    CHECK(!parse_next_run(R"({"run":5})").has_value());
}

TEST(parse_reply_reads_progress_and_flags)
{
    const auto a = parse_reply(R"({"ok":true,"progress":33,"off_route":false,"finished":false})");
    CHECK(a.ok_json);
    CHECK(a.progress == std::optional<long long>(33));
    CHECK(!a.off_route && !a.finished && !a.ignored);

    const auto b = parse_reply(R"({"ok":true,"ignored":true})");
    CHECK(b.ignored);

    const auto c = parse_reply(R"({"ok":true,"progress":null,"off_route":true,"finished":true})");
    CHECK(!c.progress.has_value());
    CHECK(c.off_route && c.finished);
}

TEST(parse_reply_survives_garbage)
{
    CHECK(!parse_reply("").ok_json);
    CHECK(!parse_reply("<html>").ok_json);
    CHECK(!parse_reply("[1,2]").ok_json);
    const auto odd = parse_reply(R"({"progress":"high","finished":"yes"})");
    CHECK(odd.ok_json);
    CHECK(!odd.progress.has_value());
    CHECK(!odd.finished);
}

TEST(error_text_uses_the_servers_error_capped_at_120)
{
    CHECK(error_text(R"({"error":"Nope"})") == "Nope");
    CHECK(error_text("{\"error\":\"" + std::string(500, 'e') + "\"}").size() == 120);
    CHECK(error_text("<html>") == "Request failed");
    CHECK(error_text(R"({"error":7})") == "Request failed");
    CHECK(error_text("", "Custom") == "Custom");
}
