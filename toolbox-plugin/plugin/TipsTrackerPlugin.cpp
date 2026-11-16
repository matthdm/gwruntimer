#include "TipsTrackerPlugin.h"

#include <cstring>

#include <GWCA/Constants/Constants.h>
#include <GWCA/Context/MapContext.h>
#include <GWCA/GameEntities/Agent.h>
#include <GWCA/Managers/AgentMgr.h>
#include <GWCA/Managers/MapMgr.h>

#include <imgui.h>

#include "RestClientTransport.h"

namespace {

constexpr float kSampleEveryMs = 250.f; // how often the character's position is read

#if defined(_DEBUG) || defined(TIPS_ALLOW_LOCALHOST)
constexpr bool kDebugBuild = true; // may talk to http://localhost
#else
constexpr bool kDebugBuild = false; // HTTPS only
#endif

tips::Instance to_instance(const GW::Constants::InstanceType t)
{
    switch (t) {
        case GW::Constants::InstanceType::Outpost: return tips::Instance::Outpost;
        case GW::Constants::InstanceType::Explorable: return tips::Instance::Explorable;
        default: return tips::Instance::Loading;
    }
}

constexpr ImVec4 kGreen{0.2f, 0.9f, 0.35f, 1.f};
constexpr ImVec4 kAmber{1.f, 0.75f, 0.2f, 1.f};

} // namespace

DLLAPI ToolboxPlugin* ToolboxPluginInstance()
{
    static TipsTrackerPlugin instance;
    return &instance;
}

// ---------- lifecycle ----------

void TipsTrackerPlugin::Initialize(ImGuiContext* ctx, const ImGuiAllocFns allocator_fns, const HMODULE toolbox_dll)
{
    ToolboxUIPlugin::Initialize(ctx, allocator_fns, toolbox_dll);
    // Settings are loaded right after this, which fills in the token and options.
    uplink_ = std::make_unique<tips::Uplink>(std::make_unique<tips::RestClientTransport>(TIPS_API_BASE, kDebugBuild), tips::Config{});
    zone_.reset();
}

void TipsTrackerPlugin::SignalTerminate()
{
    ToolboxUIPlugin::SignalTerminate();
    if (uplink_) uplink_->signal_stop(); // aborts a request in flight and sends one tidy end if a run is on
}

bool TipsTrackerPlugin::CanTerminate()
{
    return !uplink_ || uplink_->stopped(); // Toolbox must not unload the DLL while our thread still runs in it
}

void TipsTrackerPlugin::Terminate()
{
    ToolboxUIPlugin::Terminate();
    uplink_.reset();
}

void TipsTrackerPlugin::LoadSettings(const wchar_t* folder)
{
    ToolboxUIPlugin::LoadSettings(folder);
    settings_folder_ = folder;
    std::string token;
    bool share = false;
    std::string mode = tips::to_string(tips::TrackingMode::Run);
    LoadSetting("token", token);
    LoadSetting("share_position", share);
    LoadSetting("tracking_mode", mode);
    if (!uplink_) return;
    uplink_->update_config([&](tips::Config& c) {
        c.token = token;
        c.share_position = share;
        c.tracking_mode = tips::mode_from_string(mode).value_or(tips::TrackingMode::Run);
    });
    const auto cleaned = uplink_->config().token;
    strncpy_s(token_buf_, sizeof(token_buf_), cleaned.c_str(), _TRUNCATE);
}

void TipsTrackerPlugin::SaveSettings(const wchar_t* folder)
{
    if (uplink_) {
        const auto c = uplink_->config();
        SaveSetting("token", c.token);
        SaveSetting("share_position", c.share_position);
        SaveSetting("tracking_mode", std::string(tips::to_string(c.tracking_mode)));
    }
    ToolboxUIPlugin::SaveSettings(folder);
}

// ---------- reading the game (only coordinates and map ids, never a name) ----------

std::optional<tips::Bounds> TipsTrackerPlugin::read_bounds() const
{
    const auto* map = GW::GetMapContext();
    if (!map) return std::nullopt;
    return tips::bounds_from({map->start_pos.x, map->start_pos.y, map->end_pos.x, map->end_pos.y});
}

void TipsTrackerPlugin::sample_position()
{
    const auto* me = GW::Agents::GetControlledCharacter();
    uplink_->set_position(me ? std::optional(tips::Pos{me->x, me->y}) : std::nullopt);
    if (uplink_->wants_bounds()) uplink_->set_bounds(read_bounds()); // sharing was switched on after this map loaded
}

void TipsTrackerPlugin::Update(const float delta_ms)
{
    if (!uplink_) return;
    const bool loaded = GW::Map::GetIsMapLoaded();
    const auto instance = to_instance(GW::Map::GetInstanceType());
    const auto map_id = static_cast<uint32_t>(GW::Map::GetMapID());

    if (const auto zone = zone_.update(loaded, map_id, instance)) {
        // The map's bounds are read only while the runner is sharing their position.
        uplink_->push_zone(zone->map_id, zone->explorable, uplink_->sharing() ? read_bounds() : std::nullopt);
    }

    since_sample_ms_ += delta_ms;
    if (since_sample_ms_ < kSampleEveryMs) return;
    since_sample_ms_ = 0.f;
    if (!(loaded && instance != tips::Instance::Loading)) {
        uplink_->set_position(std::nullopt);
        return;
    }
    const bool panel_open = GetVisiblePtr() && *GetVisiblePtr();
    if (panel_open || uplink_->sharing()) sample_position();
}

// ---------- the panel ----------

void TipsTrackerPlugin::Draw(IDirect3DDevice9*)
{
    if (!uplink_) return;
    ImGui::SetNextWindowSize(ImVec2(380.f, 0.f), ImGuiCond_FirstUseEver);
    if (ImGui::Begin(Name(), GetVisiblePtr(), GetWinFlags())) draw_panel();
    ImGui::End();
}

void TipsTrackerPlugin::draw_panel()
{
    const auto cfg = uplink_->config();
    const auto status = uplink_->status();
    const bool tracking = status.active_run.has_value() || status.active_general;
    const bool general_mode = status.active_general || (!status.active_run && cfg.tracking_mode == tips::TrackingMode::General);

    ImGui::TextUnformatted("Tracker token (from My shop on the TIPS site)");
    ImGui::SetNextItemWidth(-60.f);
    ImGui::InputText("##tips_token", token_buf_, sizeof(token_buf_), ImGuiInputTextFlags_Password);
    ImGui::SameLine();
    if (ImGui::Button("Save##tips_token")) {
        const std::string token = token_buf_;
        uplink_->update_config([&](tips::Config& c) { c.token = token; });
        if (!settings_folder_.empty()) SaveSettings(settings_folder_.c_str());
    }
    ImGui::TextDisabled("Treat the token like a password: it can post as you.");
    ImGui::Separator();

    // Runner tracking (a run) or general tracking (no run). The mode is fixed while tracking.
    int mode = general_mode ? 1 : 0;
    ImGui::BeginDisabled(tracking);
    const bool picked_run = ImGui::RadioButton("Runner tracking (a run)##tips_mode", &mode, 0);
    ImGui::SameLine();
    const bool picked_general = ImGui::RadioButton("General tracking (no run)##tips_mode", &mode, 1);
    ImGui::EndDisabled();
    if (picked_run || picked_general) {
        const auto chosen = mode == 1 ? tips::TrackingMode::General : tips::TrackingMode::Run;
        uplink_->update_config([&](tips::Config& c) { c.tracking_mode = chosen; });
        if (!settings_folder_.empty()) SaveSettings(settings_folder_.c_str());
    }

    if (general_mode) {
        ImGui::TextWrapped("Starting general tracking makes your position and zone trail public on the TIPS map until you stop. It is not tied to a run.");
        if (status.active_general) {
            if (ImGui::Button("Stop tracking##tips")) uplink_->end_general();
            ImGui::TextColored(kGreen, "Tracking: broadcasting your position.");
        }
        else if (ImGui::Button("Start tracking##tips")) {
            uplink_->start_general();
        }
    }
    else {
        if (status.next_run) {
            ImGui::TextWrapped("Next run: %s (%s)", status.next_run->title.c_str(), status.next_run->status.c_str());
            if (status.next_run->route_name) ImGui::TextDisabled("Route: %s", status.next_run->route_name->c_str());
        }
        else {
            ImGui::TextDisabled("No run loaded. Press Refresh.");
        }
        ImGui::BeginDisabled(tracking);
        if (ImGui::Button("Refresh##tips")) uplink_->refresh();
        ImGui::EndDisabled();
        ImGui::SameLine();
        if (status.active_run) {
            if (ImGui::Button("End run##tips")) uplink_->end_run();
        }
        else if (ImGui::Button("Start run##tips")) {
            uplink_->start_run();
        }
        if (status.active_run) {
            ImGui::TextColored(kGreen, "Tracking: sending your map to TIPS.");
            if (status.progress) ImGui::Text("Progress: %lld%%", *status.progress);
            if (status.off_route) ImGui::TextColored(kAmber, "Off route");
        }

        bool share = cfg.share_position;
        if (ImGui::Checkbox("Share my position on the map (x/y)", &share)) {
            uplink_->update_config([&](tips::Config& c) { c.share_position = share; });
            if (!settings_folder_.empty()) SaveSettings(settings_folder_.c_str());
        }
        if (share) ImGui::TextDisabled("Your position and the trail in this zone are public on the TIPS site while this is on.");
    }

    if (status.busy) ImGui::TextDisabled("Working...");
    if (!status.message.empty()) ImGui::TextWrapped("%s", status.message.c_str());

    if (ImGui::CollapsingHeader("Position check (should move when you walk)")) {
        const auto latest = uplink_->latest();
        if (latest.map_id) ImGui::Text("Map %u (%s)", *latest.map_id, latest.explorable ? "explorable" : "town/outpost");
        else ImGui::TextDisabled("No map seen yet.");
        if (latest.pos) ImGui::Text("x %.1f   y %.1f", latest.pos->first.x, latest.pos->first.y);
        else ImGui::TextDisabled("Position: not read (shown only while this panel is open or sharing).");
        if (latest.bounds) ImGui::Text("Bounds [%.0f %.0f %.0f %.0f]", (*latest.bounds)[0], (*latest.bounds)[1], (*latest.bounds)[2], (*latest.bounds)[3]);
        else ImGui::TextDisabled("Bounds: read only while sharing.");
    }
    ImGui::TextDisabled("Sent to TIPS only while a run is active: map ID, town or explorable, and (if sharing) x/y.");
}
