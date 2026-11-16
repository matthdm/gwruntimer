#pragma once

#include <cstddef>
#include <mutex>
#include <string>
#include <string_view>

#include "tips/transport.h"

class HttpRequest; // Toolbox's RestClient/HttpClient.h

namespace tips {

inline constexpr std::size_t kMaxResponse = 8 * 1024;

/// Appends data to body but never past `cap` bytes. Returns false when something did not fit (the part that fits
/// is still appended).
bool capped_append(std::string& body, const char* data, std::size_t n, std::size_t cap = kMaxResponse);

/// The real Transport, built on Toolbox's own HttpRequest (a thin wrapper over WinHTTP, compiled into this DLL from
/// the pinned Toolbox source). Blocking: call it from the worker thread only. HTTPS only (a debug build may also use
/// http://localhost), no redirects, certificate checks on, 10 s timeout, replies capped at kMaxResponse.
class RestClientTransport : public Transport {
public:
    RestClientTransport(std::string base, bool debug_build);

    Response get(std::string_view path, std::string_view token) override;
    Response post(std::string_view path, std::string_view token, std::string_view json_body) override;
    void abort() override;

private:
    Response perform(bool is_post, std::string_view path, std::string_view token, std::string_view body);

    std::string base_;
    bool debug_build_;
    std::mutex m_;
    HttpRequest* current_ = nullptr; // the request in flight, so abort() can cancel it
};

} // namespace tips
