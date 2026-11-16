#include "RestClientTransport.h"

#include <HttpClient.h>

#include "tips/url_policy.h"

namespace tips {

bool capped_append(std::string& body, const char* data, const std::size_t n, const std::size_t cap)
{
    const std::size_t room = body.size() < cap ? cap - body.size() : 0;
    const std::size_t take = n < room ? n : room;
    body.append(data, take);
    return take == n;
}

namespace {

constexpr int kTimeoutMs = 10000;

/// An HttpRequest whose body stops at kMaxResponse; anything past it marks the reply too large.
class CappedRequest : public HttpRequest {
public:
    bool overflow = false;

protected:
    void OnContent(const char* bytes, const size_t count) override
    {
        if (!capped_append(m_Content, bytes, count)) overflow = true;
    }
};

std::string failure_text(const ResponseStatus status)
{
    switch (status) {
        case ResponseStatus::TimedOut: return "timed out";
        case ResponseStatus::Aborted: return "cancelled";
        default: return "connection failed";
    }
}

} // namespace

RestClientTransport::RestClientTransport(std::string base, const bool debug_build)
    : base_(std::move(base)), debug_build_(debug_build)
{
    while (!base_.empty() && base_.back() == '/') base_.pop_back();
}

Response RestClientTransport::get(const std::string_view path, const std::string_view token)
{
    return perform(false, path, token, {});
}

Response RestClientTransport::post(const std::string_view path, const std::string_view token, const std::string_view json_body)
{
    return perform(true, path, token, json_body);
}

void RestClientTransport::abort()
{
    std::lock_guard g(m_);
    if (current_) current_->RequestAbort();
}

Response RestClientTransport::perform(const bool is_post, const std::string_view path, const std::string_view token, const std::string_view body)
{
    if (!is_allowed_base(base_, debug_build_)) return {0, {}, "The tracker only talks to TIPS over HTTPS."};
    if (!valid_token_for_header(token)) return {0, {}, "The tracker token is not valid. Paste it again."};

    CappedRequest req;
    const std::string url = base_ + std::string(path);
    const std::string tok(token);
    req.SetUrl(url.c_str());
    req.SetMethod(is_post ? HttpMethod::Post : HttpMethod::Get);
    req.SetUserAgent("TIPS-Tracker/1"); // never the default "GWToolbox"
    req.SetHeader("X-Tracker-Token", tok.c_str());
    if (is_post) {
        req.SetHeader("Content-Type", "application/json");
        req.SetPostContent(std::string(body), ContentFlag::Copy);
    }
    req.SetTimeoutMs(kTimeoutMs);
    req.SetConnectTimeoutMs(kTimeoutMs);
    // Left at their safe defaults on purpose: redirects are never followed, the certificate and host are verified.

    {
        std::lock_guard g(m_);
        current_ = &req;
    }
    req.Perform();
    {
        std::lock_guard g(m_);
        current_ = nullptr;
    }

    if (req.GetStatus() != ResponseStatus::Completed) return {0, {}, failure_text(req.GetStatus())};
    if (req.overflow) return {0, {}, "reply too large"};
    return {req.GetStatusCode(), std::move(req.GetContent()), {}};
}

} // namespace tips
