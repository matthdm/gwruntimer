#pragma once

#include <ToolboxUIPlugin.h>

class TipsTrackerPlugin : public ToolboxUIPlugin {
public:
    const char* Name() const override { return "TIPS Tracker"; }
    void Draw(IDirect3DDevice9*) override;
};
