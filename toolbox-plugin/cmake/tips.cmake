# Included by Toolbox's CMake right after project(gwtoolbox) via -DCMAKE_PROJECT_gwtoolbox_INCLUDE.
# Our targets are created at the end of Toolbox's top-level directory, once its own
# dependencies and the add_tb_plugin() macro exist. No Toolbox file is edited.
if(NOT TIPS_ROOT)
    message(FATAL_ERROR "TIPS_ROOT is not set (use toolbox-plugin/build.ps1)")
endif()
if(NOT TIPS_API_BASE)
    set(TIPS_API_BASE "https://tips-runs.invalid")
endif()

function(tips_setup)
    file(GLOB TIPS_CORE CONFIGURE_DEPENDS "${TIPS_ROOT}/core/tips/*.cpp")
    file(GLOB TIPS_TEST_SOURCES CONFIGURE_DEPENDS "${TIPS_ROOT}/tests/*.cpp")

    # The plugin: Toolbox's macro globs plugins/TipsTracker/*.cpp (a junction to toolbox-plugin/plugin).
    add_tb_plugin(TipsTracker)
    target_sources(TipsTracker PRIVATE ${TIPS_CORE})
    target_include_directories(TipsTracker PRIVATE "${TIPS_ROOT}/core")
    target_compile_definitions(TipsTracker PRIVATE "TIPS_API_BASE=\"${TIPS_API_BASE}\"")

    # The tests: the game-free core only (no GWCA, no ImGui).
    add_executable(tips_tests ${TIPS_CORE} ${TIPS_TEST_SOURCES})
    target_include_directories(tips_tests PRIVATE "${TIPS_ROOT}/core" "${TIPS_ROOT}/tests")
    target_link_libraries(tips_tests PRIVATE glaze::glaze)
    target_compile_options(tips_tests PRIVATE /W4 /WX /utf-8)
    set_target_properties(tips_tests PROPERTIES FOLDER "tips/")
endfunction()

cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL tips_setup)
