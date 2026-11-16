#include <chrono>
#include <utility>
#include <vector>

#include "test_main.h"
#include "tips/samples.h"

using namespace tips;
using namespace std::chrono_literals;

namespace {
const Tp t0{};
using Pt = std::pair<float, float>;
} // namespace

TEST(first_sample_is_kept_and_one_inside_the_interval_is_not)
{
    Latest l;
    l.add_sample(1.f, 2.f, t0, 1000ms);
    l.add_sample(3.f, 4.f, t0 + 500ms, 1000ms);
    CHECK(l.samples.size() == 1);
    CHECK((l.samples.front() == Pt{1.f, 2.f}));
}

TEST(a_sample_after_the_interval_is_kept)
{
    Latest l;
    l.add_sample(1.f, 2.f, t0, 1000ms);
    l.add_sample(3.f, 4.f, t0 + 1000ms, 1000ms);
    CHECK(l.samples.size() == 2);
    CHECK((l.samples.back() == Pt{3.f, 4.f}));
}

TEST(never_more_than_twenty_samples_and_the_newest_survive)
{
    Latest l;
    for (int i = 0; i < 25; ++i) l.add_sample(static_cast<float>(i), 0.f, t0 + std::chrono::seconds(i), 1000ms);
    CHECK(l.samples.size() == kMaxPoints);
    CHECK(l.samples.front().first == 5.f);
    CHECK(l.samples.back().first == 24.f);
}

TEST(restore_puts_unsent_samples_in_front_of_newer_ones_for_the_current_zone)
{
    Latest l;
    l.zone_gen = 3;
    l.add_sample(10.f, 0.f, t0, 1000ms);
    l.restore_samples(3, {{1.f, 0.f}, {2.f, 0.f}});
    CHECK(l.samples.size() == 3);
    CHECK(l.samples[0].first == 1.f);
    CHECK(l.samples[1].first == 2.f);
    CHECK(l.samples[2].first == 10.f);
}

TEST(restore_keeps_only_the_newest_twenty)
{
    Latest l;
    l.zone_gen = 1;
    std::vector<Pt> unsent;
    for (int i = 0; i < 18; ++i) unsent.push_back({static_cast<float>(i), 0.f});
    l.add_sample(100.f, 0.f, t0, 1000ms);
    l.add_sample(101.f, 0.f, t0 + 1s, 1000ms);
    l.add_sample(102.f, 0.f, t0 + 2s, 1000ms);
    l.restore_samples(1, unsent);
    CHECK(l.samples.size() == kMaxPoints);
    CHECK(l.samples.back().first == 102.f);
    CHECK(l.samples.front().first == 1.f);
}

TEST(restore_for_an_earlier_zone_is_dropped)
{
    Latest l;
    l.zone_gen = 5;
    l.restore_samples(4, {{1.f, 1.f}});
    CHECK(l.samples.empty());
}

TEST(clear_empties_and_resets_the_interval)
{
    Latest l;
    l.add_sample(1.f, 2.f, t0, 1000ms);
    l.clear_samples();
    CHECK(l.samples.empty());
    l.add_sample(9.f, 9.f, t0 + 10ms, 1000ms); // would be inside the old interval
    CHECK(l.samples.size() == 1);
}
