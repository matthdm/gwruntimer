#pragma once
#include <vector>

namespace testing {
struct Case {
    const char* name;
    void (*fn)();
};
std::vector<Case>& registry();
struct Registrar {
    Registrar(const char* name, void (*fn)()) { registry().push_back({name, fn}); }
};
void fail(const char* expr, const char* file, int line);
} // namespace testing

#define TEST(name)                                          \
    static void name();                                     \
    static ::testing::Registrar registrar_##name(#name, name); \
    static void name()

#define CHECK(expr)                                         \
    do {                                                    \
        if (!(expr)) ::testing::fail(#expr, __FILE__, __LINE__); \
    } while (0)
