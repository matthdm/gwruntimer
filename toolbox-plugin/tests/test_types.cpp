#include <cmath>
#include <limits>
#include <string>

#include "test_main.h"
#include "tips/types.h"
#include "tips/url_policy.h"

using namespace tips;

TEST(clean_token_trims_and_truncates)
{
    CHECK(clean_token("  tips_trk_abc \n") == "tips_trk_abc");
    CHECK(clean_token(std::string(150, 'a')).size() == 100);
    CHECK(clean_token("").empty());
}

TEST(tracking_mode_strings_round_trip)
{
    CHECK(mode_from_string("run") == TrackingMode::Run);
    CHECK(mode_from_string("general") == TrackingMode::General);
    CHECK(!mode_from_string("x").has_value());
    CHECK(mode_from_string(to_string(TrackingMode::Run)) == TrackingMode::Run);
    CHECK(mode_from_string(to_string(TrackingMode::General)) == TrackingMode::General);
}

TEST(config_defaults_are_private_and_runner_mode)
{
    const Config c{};
    CHECK(!c.share_position);
    CHECK(c.tracking_mode == TrackingMode::Run);
    CHECK(c.token.empty());
}

TEST(pos_sanity_rejects_garbage)
{
    CHECK((Pos{100.f, -2500.5f}.is_sane()));
    CHECK(!(Pos{std::nanf(""), 0.f}.is_sane()));
    CHECK(!(Pos{0.f, std::numeric_limits<float>::infinity()}.is_sane()));
    CHECK(!(Pos{5.0e6f, 0.f}.is_sane()));
    CHECK((Pos{100000.f, -100000.f}.is_sane()));
    CHECK(!(Pos{100001.f, 0.f}.is_sane()));
}

TEST(bounds_come_from_min_then_max)
{
    const auto b = bounds_from({-18432.f, -21504.f, 21504.f, 21504.f});
    CHECK(b.has_value());
    CHECK((*b == Bounds{-18432.f, -21504.f, 21504.f, 21504.f}));
}

TEST(bounds_must_be_finite_with_min_below_max_and_in_range)
{
    CHECK(!bounds_from({5.f, 0.f, 5.f, 10.f}).has_value());    // minX == maxX
    CHECK(!bounds_from({0.f, 9.f, 10.f, 5.f}).has_value());    // minY > maxY
    CHECK(!bounds_from({std::nanf(""), 0.f, 10.f, 10.f}).has_value());
    CHECK(!bounds_from({-1.0e6f, 0.f, 10.f, 10.f}).has_value()); // the server would 400 this
    CHECK(!bounds_from({0.f, 0.f, 0.f, 0.f}).has_value());     // an unset context
    CHECK(bounds_from({-100000.f, -100000.f, 100000.f, 100000.f}).has_value());
}

TEST(base_url_must_be_https_except_localhost_in_debug)
{
    CHECK(is_allowed_base("https://x.example", false));
    CHECK(is_allowed_base("https://x.example/", false));
    CHECK(!is_allowed_base("http://x.example", false));
    CHECK(!is_allowed_base("http://x.example", true));
    CHECK(!is_allowed_base("http://localhost:4280", false));
    CHECK(is_allowed_base("http://localhost:4280", true));
    CHECK(!is_allowed_base("", true));
    CHECK(!is_allowed_base("ftp://x.example", true));
}

TEST(token_for_header_refuses_control_characters_and_non_ascii)
{
    CHECK(valid_token_for_header("tips_trk_ok"));
    CHECK(!valid_token_for_header("a\r\nX: y"));
    CHECK(!valid_token_for_header("a\nb"));
    CHECK(!valid_token_for_header(""));
    CHECK(!valid_token_for_header("caf\xC3\xA9"));
    CHECK(!valid_token_for_header(std::string("a\0b", 3)));
}
