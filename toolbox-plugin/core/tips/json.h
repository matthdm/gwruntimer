#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "tips/types.h"

// Request bodies and response parsing for the TIPS tracker protocol (docs/tracker-protocol.md in the tips-run repo).
// Bodies are written by hand (a handful of fixed shapes); replies are read with glaze and only the documented
// fields are ever looked at.
namespace tips {

/// {"client_session": ...}
std::string body_session(std::string_view session);

/// {"client_session","map_id","is_explorable"} plus x/y when `pos` is given, plus bounds only when both are given.
std::string body_zone(std::string_view session, uint32_t map_id, bool explorable, std::optional<Pos> pos, std::optional<Bounds> bounds);

/// {"client_session"} plus x/y when `pos` is given, plus "points": [[x,y],...] (oldest first) when there are any.
std::string body_heartbeat(std::string_view session, std::optional<Pos> pos, const std::vector<std::pair<float, float>>& points);

/// The run in a GET /next-run reply, or nothing for {"run":null}, a non-object run, or anything that is not JSON.
std::optional<NextRun> parse_next_run(std::string_view json);

struct ZoneReply {
    bool ok_json = false; // the reply was a JSON object
    bool ignored = false;
    bool finished = false;
    bool off_route = false;
    std::optional<long long> progress;
};

/// The documented fields of a zone/start/heartbeat/end reply. Unknown or mistyped fields are ignored.
ZoneReply parse_reply(std::string_view json);

/// The server's "error" text (at most 120 characters), else `fallback`.
std::string error_text(std::string_view json, std::string_view fallback = "Request failed");

} // namespace tips
