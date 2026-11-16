#include <cstdio>
#include <cstdlib>
#include <string>

#include "test_main.h"
#include "RestClientTransport.h"

using namespace tips;

namespace {
bool mentions(const std::string& text, const std::string& needle) { return text.find(needle) != std::string::npos; }
} // namespace

TEST(plain_http_is_refused_without_touching_the_network)
{
    RestClientTransport t("http://x.example.invalid", false);
    const auto r = t.get("/api/tracker/next-run", "tips_trk_ok");
    CHECK(r.status == 0);
    CHECK(mentions(r.error, "HTTPS"));
}

TEST(localhost_http_is_refused_in_a_release_build)
{
    RestClientTransport t("http://localhost:4280", false);
    const auto r = t.post("/api/tracker/presence/start", "tips_trk_ok", "{}");
    CHECK(r.status == 0);
    CHECK(mentions(r.error, "HTTPS"));
}

TEST(a_token_with_line_breaks_is_refused_before_any_request)
{
    RestClientTransport t("https://x.example.invalid", false);
    const auto r = t.get("/api/tracker/next-run", "tips_trk_ok\r\nX-Evil: 1");
    CHECK(r.status == 0);
    CHECK(mentions(r.error, "token"));
}

TEST(an_empty_token_is_refused)
{
    RestClientTransport t("https://x.example.invalid", false);
    CHECK(t.get("/x", "").status == 0);
}

TEST(the_response_cap_keeps_exactly_8192_bytes_and_reports_overflow)
{
    std::string body;
    const std::string chunk(10000, 'a');
    CHECK(!capped_append(body, chunk.data(), chunk.size()));
    CHECK(body.size() == kMaxResponse);
    CHECK(kMaxResponse == 8192);
}

TEST(the_response_cap_accepts_a_body_that_just_fits)
{
    std::string body;
    const std::string chunk(8192, 'b');
    CHECK(capped_append(body, chunk.data(), chunk.size()));
    CHECK(body.size() == 8192);
    CHECK(!capped_append(body, "x", 1)); // one more byte is over the cap
    CHECK(body.size() == 8192);
}

TEST(live_check_against_a_local_tips_api_when_configured)
{
    char* base = nullptr;
    char* token = nullptr;
    size_t n = 0;
    _dupenv_s(&base, &n, "TIPS_E2E_BASE");
    _dupenv_s(&token, &n, "TIPS_E2E_TOKEN");
    if (!base || !token) {
        std::printf("    SKIPPED: set TIPS_E2E_BASE and TIPS_E2E_TOKEN to check against a local TIPS API\n");
        std::free(base);
        std::free(token);
        return;
    }
    RestClientTransport t(base, true);
    const auto r = t.get("/api/tracker/next-run", token);
    CHECK(r.status == 200);
    CHECK(mentions(r.body, "run"));
    std::free(base);
    std::free(token);
}
