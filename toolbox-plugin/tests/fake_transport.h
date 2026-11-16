#pragma once

#include <deque>
#include <functional>
#include <map>
#include <string>
#include <vector>

#include "tips/transport.h"

namespace testing {

struct Req {
    std::string method;
    std::string path;
    std::string token;
    std::string body;
};

/// Records every request; replies from a per-path script (front first), else 200 {"ok":true}.
class FakeTransport : public tips::Transport {
public:
    std::vector<Req> requests;
    std::map<std::string, std::deque<tips::Response>> script;
    std::function<void()> on_request; // runs before each reply (to simulate the game thread moving on)
    int aborted = 0;

    void reply(const std::string& path, int status, std::string body = "{\"ok\":true}")
    {
        script[path].push_back({status, std::move(body), {}});
    }
    void network_error(const std::string& path, std::string error)
    {
        script[path].push_back({0, {}, std::move(error)});
    }

    tips::Response get(std::string_view path, std::string_view token) override { return handle("GET", path, token, {}); }
    tips::Response post(std::string_view path, std::string_view token, std::string_view body) override
    {
        return handle("POST", path, token, body);
    }
    void abort() override { ++aborted; }

    int count(const std::string& path_suffix) const
    {
        int n = 0;
        for (const auto& r : requests) {
            if (r.path.size() >= path_suffix.size() && r.path.compare(r.path.size() - path_suffix.size(), path_suffix.size(), path_suffix) == 0) ++n;
        }
        return n;
    }
    const Req* last(const std::string& path_suffix) const
    {
        for (auto it = requests.rbegin(); it != requests.rend(); ++it) {
            if (it->path.size() >= path_suffix.size() && it->path.compare(it->path.size() - path_suffix.size(), path_suffix.size(), path_suffix) == 0) return &*it;
        }
        return nullptr;
    }

private:
    tips::Response handle(const char* method, std::string_view path, std::string_view token, std::string_view body)
    {
        requests.push_back({method, std::string(path), std::string(token), std::string(body)});
        if (on_request) on_request();
        auto it = script.find(std::string(path));
        if (it != script.end() && !it->second.empty()) {
            auto r = std::move(it->second.front());
            it->second.pop_front();
            return r;
        }
        return {200, "{\"ok\":true}", {}};
    }
};

} // namespace testing
