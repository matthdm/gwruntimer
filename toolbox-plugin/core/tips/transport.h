#pragma once

#include <string>
#include <string_view>

namespace tips {

/// One HTTP reply. status 0 means the request never got an answer (network failure, refused URL, oversize body);
/// `error` then says why.
struct Response {
    int status = 0;
    std::string body;
    std::string error;
};

/// How the tracker talks to the TIPS server. `path` is the part after the compiled-in host. Implementations must be
/// HTTPS-only, never follow redirects and cap the reply size (see plugin/RestClientTransport.h).
class Transport {
public:
    virtual ~Transport() = default;
    virtual Response get(std::string_view path, std::string_view token) = 0;
    virtual Response post(std::string_view path, std::string_view token, std::string_view json_body) = 0;
    /// Cancels a request in flight on another thread (called at shutdown).
    virtual void abort() = 0;
};

} // namespace tips
