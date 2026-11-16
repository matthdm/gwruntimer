#include "tips/types.h"

#include <cmath>

namespace tips {

const char* to_string(const TrackingMode mode)
{
    return mode == TrackingMode::General ? "general" : "run";
}

std::optional<TrackingMode> mode_from_string(const std::string_view text)
{
    if (text == "run") return TrackingMode::Run;
    if (text == "general") return TrackingMode::General;
    return std::nullopt;
}

std::string clean_token(const std::string_view raw)
{
    constexpr std::string_view blanks = " \t\r\n\v\f";
    const auto first = raw.find_first_not_of(blanks);
    if (first == std::string_view::npos) return {};
    const auto last = raw.find_last_not_of(blanks);
    auto trimmed = raw.substr(first, last - first + 1);
    if (trimmed.size() > kMaxToken) trimmed = trimmed.substr(0, kMaxToken);
    return std::string(trimmed);
}

bool Pos::is_sane() const
{
    return std::isfinite(x) && std::isfinite(y) && std::fabs(x) <= kMaxCoord && std::fabs(y) <= kMaxCoord;
}

std::optional<Bounds> bounds_from(const std::array<float, 4>& b)
{
    for (const float v : b) {
        if (!std::isfinite(v) || std::fabs(v) > kMaxCoord) return std::nullopt;
    }
    if (!(b[0] < b[2]) || !(b[1] < b[3])) return std::nullopt;
    return Bounds{b[0], b[1], b[2], b[3]};
}

} // namespace tips
