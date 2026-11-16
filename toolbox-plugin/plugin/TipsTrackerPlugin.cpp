#include "TipsTrackerPlugin.h"

DLLAPI ToolboxPlugin* ToolboxPluginInstance()
{
    static TipsTrackerPlugin instance;
    return &instance;
}

void TipsTrackerPlugin::Draw(IDirect3DDevice9*) {}
