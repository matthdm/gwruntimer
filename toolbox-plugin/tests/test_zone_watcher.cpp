#include "test_main.h"
#include "tips/zone_watcher.h"

using namespace tips;

TEST(the_first_loaded_frame_emits_the_map)
{
    ZoneWatcher w;
    const auto e = w.update(true, 90, Instance::Explorable);
    CHECK(e.has_value());
    CHECK(e->map_id == 90);
    CHECK(e->explorable);
}

TEST(the_same_loaded_map_does_not_emit_again)
{
    ZoneWatcher w;
    CHECK(w.update(true, 90, Instance::Explorable).has_value());
    CHECK(!w.update(true, 90, Instance::Explorable).has_value());
    CHECK(!w.update(true, 90, Instance::Explorable).has_value());
}

TEST(re_entering_the_same_map_after_a_loading_screen_is_a_new_load)
{
    ZoneWatcher w;
    CHECK(w.update(true, 90, Instance::Explorable).has_value());
    CHECK(!w.update(false, 90, Instance::Loading).has_value());
    CHECK(!w.update(true, 90, Instance::Loading).has_value());
    CHECK(w.update(true, 90, Instance::Explorable).has_value());
}

TEST(a_different_map_emits)
{
    ZoneWatcher w;
    CHECK(w.update(true, 90, Instance::Explorable).has_value());
    const auto e = w.update(true, 20, Instance::Outpost);
    CHECK(e.has_value());
    CHECK(e->map_id == 20);
    CHECK(!e->explorable);
}

TEST(nothing_is_emitted_while_loading_or_for_map_zero)
{
    ZoneWatcher w;
    CHECK(!w.update(false, 90, Instance::Explorable).has_value());
    CHECK(!w.update(true, 90, Instance::Loading).has_value());
    CHECK(!w.update(true, 0, Instance::Explorable).has_value());
    CHECK(!w.update(false, 0, Instance::Loading).has_value());
    CHECK(w.update(true, 90, Instance::Explorable).has_value()); // the first real map still comes through
}

TEST(reset_makes_the_next_loaded_frame_emit)
{
    ZoneWatcher w;
    CHECK(w.update(true, 90, Instance::Explorable).has_value());
    CHECK(!w.update(true, 90, Instance::Explorable).has_value());
    w.reset();
    CHECK(w.update(true, 90, Instance::Explorable).has_value());
}
