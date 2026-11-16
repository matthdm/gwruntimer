#include "tips/zone_watcher.h"

namespace tips {

std::optional<ZoneEvent> ZoneWatcher::update(const bool map_loaded, const uint32_t map_id, const Instance instance)
{
    const bool real_map = map_loaded && map_id != 0 && instance != Instance::Loading;
    if (!real_map) {
        armed_ = false; // a loading screen: whatever loads next is a new load
        return std::nullopt;
    }
    if (armed_ && map_id == last_id_) return std::nullopt;
    armed_ = true;
    last_id_ = map_id;
    return ZoneEvent{map_id, instance == Instance::Explorable};
}

} // namespace tips
