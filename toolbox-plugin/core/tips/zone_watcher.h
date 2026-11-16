#pragma once

#include <cstdint>
#include <optional>

namespace tips {

/// Mirrors GW::Constants::InstanceType, so the core does not depend on GWCA.
enum class Instance { Outpost, Explorable, Loading };

struct ZoneEvent {
    uint32_t map_id = 0;
    bool explorable = false;
};

/// Watches the game's map state frame by frame and reports each completed map load once. Polling (instead of
/// hooking a game event) means nothing to register and nothing to clash with Toolbox.
class ZoneWatcher {
public:
    /// Emits the first frame a real map is loaded, and again after every loading screen (even back into the same
    /// map) or when the map id changes. Never emits while loading, while the map is not loaded, or for map 0.
    std::optional<ZoneEvent> update(bool map_loaded, uint32_t map_id, Instance instance);

    /// Forget the current map: the next loaded frame emits.
    void reset() { armed_ = false; }

private:
    bool armed_ = false; // a loaded map has been reported and no loading screen has been seen since
    uint32_t last_id_ = 0;
};

} // namespace tips
