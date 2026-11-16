#include "test_main.h"

#include <cstdio>

namespace testing {
namespace {
int g_failures_in_case = 0;
}

std::vector<Case>& registry()
{
    static std::vector<Case> cases;
    return cases;
}

void fail(const char* expr, const char* file, int line)
{
    ++g_failures_in_case;
    std::printf("    CHECK failed: %s (%s:%d)\n", expr, file, line);
}
} // namespace testing

int main()
{
    int failed = 0;
    for (const auto& c : testing::registry()) {
        testing::g_failures_in_case = 0;
        c.fn();
        if (testing::g_failures_in_case == 0) {
            std::printf("PASS %s\n", c.name);
        }
        else {
            std::printf("FAIL %s\n", c.name);
            ++failed;
        }
    }
    std::printf("%d tests, %d failed\n", static_cast<int>(testing::registry().size()), failed);
    return failed == 0 ? 0 : 1;
}
