#pragma once

#include <string_view>

namespace tips {

/// HTTPS always; plain http only to localhost, and only in a debug build.
bool is_allowed_base(std::string_view base, bool debug_build);

/// A token that is safe to put in an HTTP header: non-empty, printable ASCII, no spaces, CR, LF or NUL.
bool valid_token_for_header(std::string_view token);

} // namespace tips
