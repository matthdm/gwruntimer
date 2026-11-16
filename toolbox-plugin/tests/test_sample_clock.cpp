#include "test_main.h"
#include "tips/sample_clock.h"

using namespace tips;

// Toolbox passes Update() the frame time in SECONDS (GWToolbox::Update: delta_f = ms / 1000.f).

TEST(sixty_fps_frames_fire_about_four_times_a_second)
{
    SampleClock clock(0.25f);
    int fired = 0;
    for (int frame = 0; frame < 60; ++frame) {
        if (clock.tick(1.f / 60.f)) ++fired;
    }
    CHECK(fired >= 3 && fired <= 5);
}

TEST(it_does_not_fire_before_the_interval)
{
    SampleClock clock(0.25f);
    CHECK(!clock.tick(0.1f));
    CHECK(!clock.tick(0.1f));
    CHECK(clock.tick(0.1f)); // 0.3 s in total
}

TEST(a_long_frame_fires_once_not_many_times)
{
    SampleClock clock(0.25f);
    CHECK(clock.tick(10.f));
    CHECK(!clock.tick(0.01f));
}

TEST(zero_and_negative_deltas_never_fire)
{
    SampleClock clock(0.25f);
    CHECK(!clock.tick(0.f));
    CHECK(!clock.tick(-5.f));
    CHECK(!clock.tick(0.2f));
    CHECK(clock.tick(0.1f));
}
