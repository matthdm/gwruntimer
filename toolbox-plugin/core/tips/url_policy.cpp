#include "tips/url_policy.h"

namespace tips {

namespace {
bool starts_with(const std::string_view s, const std::string_view prefix)
{
    return s.size() >= prefix.size() && s.substr(0, prefix.size()) == prefix;
}
} // namespace

bool is_allowed_base(const std::string_view base, const bool debug_build)
{
    constexpr std::string_view https = "https://";
    if (starts_with(base, https)) return base.size() > https.size();
    constexpr std::string_view local = "http://localhost";
    if (debug_build && starts_with(base, local)) {
        const auto rest = base.substr(local.size());
        return rest.empty() || rest.front() == ':' || rest.front() == '/';
    }
    return false;
}

bool valid_token_for_header(const std::string_view token)
{
    if (token.empty()) return false;
    for (const char c : token) {
        const auto u = static_cast<unsigned char>(c);
        if (u < 0x21 || u > 0x7E) return false;
    }
    return true;
}

} // namespace tips
