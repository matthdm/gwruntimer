#pragma once

#include <memory>
#include <optional>
#include <string>

#include <ToolboxUIPlugin.h>

#include "tips/sample_clock.h"
#include "tips/types.h"
#include "tips/uplink.h"
#include "tips/zone_watcher.h"

/// The TIPS tracker as a Toolbox plugin. It does what the tracker module of this repo's timer does (see
/// TIPS_TRACKER.md): tells the TIPS run board which map the runner is in and, only if they switch it on, where.
/// It uses only the plugin interface and GWCA's public API: no hooks, no window procedure, no chat commands of its own.
class TipsTrackerPlugin : public ToolboxUIPlugin {
public:
    const char* Name() const override { return "TIPS Tracker"; }

    void Initialize(ImGuiContext* ctx, ImGuiAllocFns allocator_fns, HMODULE toolbox_dll) override;
    void SignalTerminate() override;
    bool CanTerminate() override;
    void Terminate() override;

    /// Toolbox calls this every frame with the frame time in seconds (not milliseconds).
    void Update(float delta_seconds) override;
    void Draw(IDirect3DDevice9*) override;

    void LoadSettings(const wchar_t* folder) override;
    void SaveSettings(const wchar_t* folder) override;

private:
    void sample_position();
    std::optional<tips::Bounds> read_bounds() const;
    void draw_panel();

    std::unique_ptr<tips::Uplink> uplink_;
    tips::ZoneWatcher zone_;
    tips::SampleClock sample_clock_{0.25f}; // the position is read about four times a second
    std::wstring settings_folder_; // remembered so the panel's Save button can write the settings at once
    char token_buf_[160] = {};
};
