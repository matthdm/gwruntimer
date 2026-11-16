#pragma once

#include <chrono>
#include <cstdint>
#include <deque>
#include <optional>
#include <utility>
#include <vector>

#include "tips/types.h"

namespace tips {

using Tp = std::chrono::steady_clock::time_point;

/// Most position samples kept for the next heartbeat (the server accepts 20 per beat).
inline constexpr std::size_t kMaxPoints = 20;

/// What the game side has most recently told the tracker. Always accessed under Shared::m (see engine.h).
struct Latest {
    std::optional<uint32_t> map_id;
    bool explorable = false;
    /// The current map's coordinate bounds. Only ever filled while sharing.
    std::optional<Bounds> bounds;
    std::optional<std::pair<Pos, Tp>> pos;
    /// One position per sample interval since the last heartbeat, oldest first. Only filled while sharing.
    std::deque<std::pair<float, float>> samples;
    std::optional<Tp> last_sample;
    /// Bumped on every map change, so samples taken in one zone are never filed under another.
    uint64_t zone_gen = 0;

    /// Keeps (x, y) if a sample interval has passed since the last kept one; never more than kMaxPoints.
    void add_sample(float x, float y, Tp now, Ms every);

    /// Puts samples that could not be sent back in front of any newer ones, keeping the newest kMaxPoints.
    /// Samples from an earlier zone (`zone` is no longer current) are dropped: they belong to a map we have left.
    void restore_samples(uint64_t zone, std::vector<std::pair<float, float>> unsent);

    void clear_samples();
};

} // namespace tips
