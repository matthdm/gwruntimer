#pragma once

#include <array>
#include <chrono>
#include <cstddef>
#include <optional>
#include <string>
#include <string_view>

// Game-free types for the TIPS tracker. Ported from src/uplink.rs and src/position.rs.
namespace tips {

using Ms = std::chrono::milliseconds;

/// What the tracker does when the runner presses Start.
enum class TrackingMode {
    Run,     // runner tracking: tied to a run (Start run / End run)
    General, // general tracking: position and zone trail with no run behind it
};

const char* to_string(TrackingMode mode);
std::optional<TrackingMode> mode_from_string(std::string_view text);

inline constexpr std::size_t kMaxToken = 100;

/// Trims ASCII whitespace from both ends and cuts the token to kMaxToken characters.
std::string clean_token(std::string_view raw);

/// Saved in the plugin's Toolbox settings. The token is like a password: it can post as this runner.
struct Config {
    std::string token;
    bool share_position = false; // off unless the runner turns it on
    TrackingMode tracking_mode = TrackingMode::Run;
};

/// Anything beyond this is not a coordinate inside a map (the server refuses it).
inline constexpr float kMaxCoord = 100000.f;

struct Pos {
    float x = 0.f;
    float y = 0.f;
    /// A usable coordinate: finite and in range.
    bool is_sane() const;
};

/// A map's coordinate bounds: min_x, min_y, max_x, max_y.
using Bounds = std::array<float, 4>;

/// The bounds if all four are finite and in range and each min is below its max; otherwise nothing.
std::optional<Bounds> bounds_from(const std::array<float, 4>& min_x_min_y_max_x_max_y);

struct NextRun {
    std::string id;
    std::string title;
    std::string status;
    std::optional<std::string> route_name;
};

/// What the panel shows.
struct Status {
    std::optional<NextRun> next_run;
    std::optional<std::string> active_run; // the run this tracker is reporting on
    bool active_general = false;           // general tracking (no run) is on
    std::optional<long long> progress;
    bool off_route = false;
    std::string message;
    bool busy = false;
};

struct Timing {
    Ms heartbeat{30000};     // while a run is active
    Ms position_beat{5000};  // while a fresh position is being shared, so the marker moves
    Ms retry_base{1000};     // backoff base: base * 2^attempt
    Ms sample{1000};         // how often a position is kept for the trail
};

} // namespace tips
