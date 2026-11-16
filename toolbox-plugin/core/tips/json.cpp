#include "tips/json.h"

#include <charconv>
#include <cmath>

#include <glaze/glaze.hpp>

namespace tips {

namespace {

void append_string(std::string& out, const std::string_view s)
{
    out += '"';
    for (const char ch : s) {
        const auto c = static_cast<unsigned char>(ch);
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (c < 0x20) {
                    static constexpr char hex[] = "0123456789abcdef";
                    out += "\\u00";
                    out += hex[c >> 4];
                    out += hex[c & 0xF];
                }
                else {
                    out += ch;
                }
        }
    }
    out += '"';
}

void append_float(std::string& out, const float v)
{
    char buf[48];
    const auto r = std::to_chars(buf, buf + sizeof(buf), v);
    out.append(buf, r.ptr);
}

void append_uint(std::string& out, const uint32_t v)
{
    out += std::to_string(v);
}

void append_pos(std::string& out, const Pos& p)
{
    out += ",\"x\":";
    append_float(out, p.x);
    out += ",\"y\":";
    append_float(out, p.y);
}

/// At most `max_chars` Unicode characters of `s` (never cuts a UTF-8 sequence in half).
std::string truncate_chars(const std::string& s, const std::size_t max_chars)
{
    std::size_t chars = 0;
    std::size_t i = 0;
    while (i < s.size()) {
        if ((static_cast<unsigned char>(s[i]) & 0xC0) != 0x80) {
            if (chars == max_chars) break;
            ++chars;
        }
        ++i;
    }
    return s.substr(0, i);
}

/// The value at `key` of a JSON object, or null.
const glz::generic* member(const glz::generic& v, const char* key)
{
    if (!v.is_object()) return nullptr;
    const auto& obj = v.get_object();
    const auto it = obj.find(key);
    return it == obj.end() ? nullptr : &it->second;
}

std::string string_member(const glz::generic& v, const char* key, const std::size_t max_chars)
{
    const auto* m = member(v, key);
    return m && m->is_string() ? truncate_chars(m->get_string(), max_chars) : std::string();
}

bool parse_json(const std::string_view text, glz::generic& out)
{
    return !glz::read_json(out, text);
}

} // namespace

std::string body_session(const std::string_view session)
{
    std::string out = "{\"client_session\":";
    append_string(out, session);
    out += '}';
    return out;
}

std::string body_zone(const std::string_view session, const uint32_t map_id, const bool explorable, const std::optional<Pos> pos, const std::optional<Bounds> bounds)
{
    std::string out = "{\"client_session\":";
    append_string(out, session);
    out += ",\"map_id\":";
    append_uint(out, map_id);
    out += ",\"is_explorable\":";
    out += explorable ? "true" : "false";
    if (pos) append_pos(out, *pos);
    if (bounds) {
        out += ",\"bounds\":[";
        for (std::size_t i = 0; i < bounds->size(); ++i) {
            if (i) out += ',';
            append_float(out, (*bounds)[i]);
        }
        out += ']';
    }
    out += '}';
    return out;
}

std::string body_heartbeat(const std::string_view session, const std::optional<Pos> pos, const std::vector<std::pair<float, float>>& points)
{
    std::string out = "{\"client_session\":";
    append_string(out, session);
    if (pos) append_pos(out, *pos);
    if (!points.empty()) {
        out += ",\"points\":[";
        for (std::size_t i = 0; i < points.size(); ++i) {
            if (i) out += ',';
            out += '[';
            append_float(out, points[i].first);
            out += ',';
            append_float(out, points[i].second);
            out += ']';
        }
        out += ']';
    }
    out += '}';
    return out;
}

std::optional<NextRun> parse_next_run(const std::string_view json)
{
    glz::generic v;
    if (!parse_json(json, v)) return std::nullopt;
    const auto* run = member(v, "run");
    if (!run || !run->is_object()) return std::nullopt;
    NextRun r;
    r.id = string_member(*run, "id", 64);
    r.title = string_member(*run, "title", 80);
    r.status = string_member(*run, "status", 16);
    if (const auto* route = member(*run, "route_name"); route && route->is_string()) {
        r.route_name = truncate_chars(route->get_string(), 80);
    }
    return r;
}

ZoneReply parse_reply(const std::string_view json)
{
    ZoneReply reply;
    glz::generic v;
    if (!parse_json(json, v) || !v.is_object()) return reply;
    reply.ok_json = true;
    const auto flag = [&](const char* key) {
        const auto* m = member(v, key);
        return m && m->is_boolean() && m->get_boolean();
    };
    reply.ignored = flag("ignored");
    reply.finished = flag("finished");
    reply.off_route = flag("off_route");
    if (const auto* p = member(v, "progress"); p && p->is_number()) {
        const double d = p->get_number();
        if (std::isfinite(d) && d == std::floor(d) && std::fabs(d) < 1.0e9) reply.progress = static_cast<long long>(d);
    }
    return reply;
}

std::string error_text(const std::string_view json, const std::string_view fallback)
{
    glz::generic v;
    if (parse_json(json, v)) {
        if (const auto* e = member(v, "error"); e && e->is_string()) return truncate_chars(e->get_string(), 120);
    }
    return std::string(fallback);
}

} // namespace tips
