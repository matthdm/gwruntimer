
mod gw;
mod run_log;
mod map_names;
mod categories;
mod mapdump;
mod position;
mod uplink;

pub mod version {
    include!(concat!(env!("OUT_DIR"), "/version.rs"));
}

use run_log::RunLog;

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hudhook::ImguiRenderLoop;
use hudhook::RenderContext;
use hudhook::imgui::{
    self, Condition, FontConfig, FontId, FontSource, StyleColor, TableColumnFlags, TableColumnSetup,
    TableFlags, TreeNodeFlags, WindowFlags,
};

use run_log::{RouteRecord, RunEntry};
use categories::CategorySummary;

/// Target that can be pinned to display its PB on the main timer readout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinnedTarget {
    None,
    Route(RouteRecord),
}

/// Pixel size of the large timer font, loaded once in `initialize`.
const TIMER_FONT_PX: f32 = 48.0;

/// TrueType font used for the large timer readout, embedded at compile time so
/// there's no runtime file dependency inside the injected process.
static TIMER_FONT_TTF: &[u8] = include_bytes!("../res/default.ttf");

/// `FontId` holds a raw `*const Font`, which is neither `Send` nor `Sync`, but
/// hudhook requires the render loop to be both. The id is only ever created and
/// used on the render thread, so asserting these bounds is sound.
struct SyncFont(FontId);
unsafe impl Send for SyncFont {}
unsafe impl Sync for SyncFont {}

struct ZoneTimerState {
    last_load_state: bool,
    mapid_before_run: u32,
    mapid_during_run: u32,
    mapid_now: u32
}

/// A stopwatch-style overlay timer.
///
/// It sits on top of the game as a borderless, click-through readout. Holding
/// the edit chord (Ctrl+Shift) reveals a bounding box and makes it draggable,
/// with a right-click context menu for start/pause/reset.
struct ZoneTimer {
    /// Time banked from previous run segments (before the current pause point).
    accumulated: Duration,
    /// If running, the instant the current segment started; `None` when paused.
    running_since: Option<Instant>,
    /// Top-left screen position of the timer window, in pixels.
    pos: [f32; 2],
    /// The large font, set up in `initialize`.
    big_font: Option<SyncFont>,
    /// Persistent log of completed runs, opened in `initialize`.
    run_log: Option<RunLog>,
    /// Runs logged during this game session, newest last.
    session_runs: Vec<RunEntry>,
    /// Whether the session-log window is open.
    show_log: bool,
    /// Aggregated route records loaded from the database cache.
    records: Vec<RouteRecord>,
    /// Precomputed summaries for each category to eliminate per-frame allocations/calculations.
    category_summaries: Vec<(&'static str, CategorySummary)>,
    /// Unmatched custom records.
    unmatched_records: Vec<RouteRecord>,
    /// Pre-loaded route run histories for instant subtable rendering without DB queries.
    route_history_cache: HashMap<(u32, u32, u32), Vec<RunEntry>>,
    /// Whether the personal-records window is open.
    show_records: bool,
    /// Previous frame state of `show_records` to detect window opening.
    was_records_open: bool,
    /// Set when "Exit Timer" is picked, to open the confirm popup next.
    confirm_exit: bool,
    /// Target route awaiting confirmation to delete (from, run, to).
    pending_delete_route: Option<(u32, u32, u32)>,
    /// Set when a route delete button is clicked, to open the confirm modal next.
    confirm_delete_route: bool,
    /// The currently pinned route or category whose PB is displayed under the timer.
    pinned_target: PinnedTarget,
    /// Edit-mode state from the previous frame, for detecting enter/leave.
    was_edit: bool,

    in_full_edit_mode: bool,

    show_inst_timer: bool,

    state: ZoneTimerState,

    /// TIPS live tracking (see uplink.rs). Created in `initialize`.
    uplink: Option<uplink::Uplink>,
    show_tips: bool,
    /// The TIPS panel takes the mouse and keyboard (so a token can be typed or pasted) without holding
    /// Ctrl+Shift. Opened from the context menu, left with the panel's Lock button.
    tips_edit: bool,
    was_tips_edit: bool,
    /// Text box contents for the tracker token.
    tips_token_input: String,
    probe: position::Probe,
    recorder: mapdump::Recorder,
    last_pos_read: Option<Instant>,
    last_reading: Option<position::Reading>,
}

impl Default for ZoneTimer {
    fn default() -> Self {
        Self {
            accumulated: Duration::ZERO,
            running_since: None,
            pos: [40.0, 40.0],
            big_font: None,
            run_log: None,
            session_runs: Vec::new(),
            show_log: false,
            records: Vec::new(),
            category_summaries: Vec::new(),
            unmatched_records: Vec::new(),
            route_history_cache: HashMap::new(),
            show_records: false,
            was_records_open: false,
            confirm_exit: false,
            pending_delete_route: None,
            confirm_delete_route: false,
            pinned_target: PinnedTarget::None,
            show_inst_timer: false,
            was_edit: false,
            in_full_edit_mode: false,
            uplink: None,
            show_tips: false,
            tips_edit: false,
            was_tips_edit: false,
            tips_token_input: String::new(),
            probe: position::Probe::default(),
            recorder: mapdump::Recorder::default(),
            last_pos_read: None,
            last_reading: None,
            state: ZoneTimerState {
                last_load_state: false,
                mapid_before_run: 0,
                mapid_during_run: 0,
                mapid_now: 0,
            },
        }
    }
}

impl ZoneTimer {
    /// Total elapsed time, including the in-progress segment if running.
    fn elapsed(&self) -> Duration {
        self.accumulated + self.running_since.map_or(Duration::ZERO, |t| t.elapsed())
    }

    fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    fn start(&mut self) {
        if self.running_since.is_none() {
            self.running_since = Some(Instant::now());
        }
    }

    fn pause(&mut self) {
        if let Some(t) = self.running_since.take() {
            self.accumulated += t.elapsed();
        }
    }

    fn reset(&mut self) {
        self.accumulated = Duration::ZERO;
        self.running_since = None;
    }

    /// Current time on the timer, formatted `MM:SS.d`.
    fn format_time(&self) -> String {
        format_duration(self.elapsed())
    }

    /// Log a completed run to the database and this session's list.
    /// Runs that start and end in the same location are ignored to prevent mistakes.
    #[allow(dead_code)]
    fn record_run(&mut self, from_map_id: u32, run_map_id: u32, to_map_id: u32, duration: Duration) {
        if from_map_id == to_map_id {
            return;
        }
        let Some(log) = &self.run_log else { return };
        match log.log_run(from_map_id, run_map_id, to_map_id, duration) {
            Ok(entry) => {
                self.session_runs.push(entry);
                self.refresh_records();
            }
            Err(e) => tracing::error!("failed to log run: {e}"),
        }
    }

    /// Refresh aggregated route records and precompute category summaries in memory.
    fn refresh_records(&mut self) {
        if let Some(log) = &self.run_log {
            let _ = log.refresh_cache();
            self.records = log.cached_records();
            self.route_history_cache = log.cached_route_history();
        } else {
            self.records.clear();
            self.route_history_cache.clear();
        }

        self.category_summaries = categories::ALL_CATEGORIES
            .iter()
            .map(|cat| (cat.name, cat.summarize(&self.records)))
            .collect();

        self.unmatched_records = self.records
            .iter()
            .copied()
            .filter(|rec| {
                !categories::ALL_CATEGORIES
                    .iter()
                    .any(|cat| cat.matches(rec.from_map_id, rec.run_map_id, rec.to_map_id))
            })
            .collect();
    }

    /// Reads the character's position about four times a second, while the panel is open or sharing is on,
    /// and hands it to the uplink. Reading is best-effort and can never fault (see position.rs).
    fn sample_position(&mut self) {
        let Some(up) = &self.uplink else { return };
        if !(self.show_tips || up.sharing()) {
            return;
        }
        if self.last_pos_read.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return;
        }
        self.last_pos_read = Some(Instant::now());
        let Some(agent_id) = gw::CharControlledCharacterData::current().map(|c| c.agent_id) else { return };
        let reading = self.probe.read(agent_id, gw::agent_context_addr());
        let source = position::Source::from_index(up.config().position_source);
        up.set_position(reading.pick(source));
        if self.recorder.recording {
            let map_id = gw::MissionMapData::current().map_or(0, |m| m.map_id);
            self.recorder.push(map_id, &reading, mapdump::read_bounds());
        }
        self.last_reading = Some(reading);
    }

    /// The TIPS Tracker panel: token, next run, Start/End, and the position-sharing switch.
    fn render_tips(&mut self, ui: &imgui::Ui, edit: bool) {
        let Some(up) = &self.uplink else {
            ui.text("The tracker isn't available.");
            return;
        };
        if edit {
            if self.tips_edit {
                if ui.button("Lock (click-through)") {
                    self.tips_edit = false;
                }
                ui.same_line();
                if ui.button("Close##tips_close") {
                    self.show_tips = false;
                    self.tips_edit = false;
                }
                ui.same_line();
                ui.text_disabled("Locked, the panel stays on screen but the mouse and keyboard go to the game.");
            }
        } else {
            ui.text_disabled("Locked. Hold Ctrl+Shift, right-click the timer, TIPS Tracker to edit.");
        }
        let status = up.status();
        let cfg = up.config();

        ui.text("Tracker token (from My shop on the TIPS site)");
        ui.set_next_item_width(320.0);
        ui.input_text("##tips_token", &mut self.tips_token_input).password(true).build();
        ui.same_line();
        if ui.button("Save##tips_token") {
            let token = self.tips_token_input.clone();
            up.update_config(|c| c.token = token);
            up.refresh();
        }

        ui.separator();
        if let Some(run) = &status.next_run {
            ui.text(format!("Next run: {} ({})", run.title, run.status));
            if let Some(route) = &run.route_name {
                ui.text(format!("Route: {route}"));
            }
        } else {
            ui.text("No run found yet.");
        }
        if ui.button("Refresh##tips") {
            up.refresh();
        }
        ui.same_line();
        if status.active_run.is_some() {
            if ui.button("End run##tips") {
                up.end_run();
            }
        } else if ui.button("Start run##tips") {
            up.start_run();
        }
        if status.busy {
            ui.same_line();
            ui.text_disabled("working...");
        }
        if status.active_run.is_some() {
            ui.text_colored([0.2, 0.9, 0.35, 1.0], "Tracking: sending your map to TIPS.");
            if let Some(p) = status.progress {
                imgui::ProgressBar::new(p as f32 / 100.0).size([320.0, 0.0]).overlay_text(format!("{p}%")).build(ui);
            }
            if status.off_route {
                ui.text_colored([1.0, 0.75, 0.2, 1.0], "Off route");
            }
        }
        if !status.message.is_empty() {
            ui.text_wrapped(&status.message);
        }

        ui.separator();
        let mut share = cfg.share_position;
        if ui.checkbox("Share my position on the map (x/y)", &mut share) {
            up.update_config(|c| c.share_position = share);
        }
        ui.text_disabled("Off by default. When on, your x/y inside the map is sent so the site can place you on it.");
        let labels: Vec<&str> = position::Source::ALL.iter().map(|s| s.label()).collect();
        let mut idx = cfg.position_source.min(labels.len() - 1);
        ui.set_next_item_width(320.0);
        if ui.combo_simple_string("Position source", &mut idx, &labels) {
            up.update_config(|c| c.position_source = idx);
        }

        if ui.collapsing_header("Position debug (check these move when you walk)", TreeNodeFlags::empty()) {
            match &self.last_reading {
                None => ui.text("No reading yet. Enter an explorable area or town."),
                Some(r) => {
                    ui.text(format!("Agent id: {}   agent array found: {}", r.agent_id, r.array_found));
                    let show = |ui: &imgui::Ui, name: &str, p: &Option<position::Pos>| match p {
                        Some(p) => ui.text(format!("{name}: x {:.1}  y {:.1}  z {:.1}", p.x, p.y, p.z)),
                        None => ui.text(format!("{name}: not readable")),
                    };
                    show(ui, "Agent position", &r.agent);
                    if let Some(plane) = r.plane {
                        ui.text(format!("Plane: {plane}"));
                    }
                    show(ui, "Movement A (+0x64)", &r.movement_a);
                    show(ui, "Movement B (+0x74)", &r.movement_b);
                }
            }
        }

        if ui.collapsing_header("Map data spike (throwaway experiment)", TreeNodeFlags::empty()) {
            ui.text_wrapped("Saves the game's own map rectangles, this map's bounds and a recording of your walk to tips_mapdump.json next to the DLL. Nothing is sent anywhere.");
            if self.recorder.recording {
                if ui.button("Stop recording") {
                    self.recorder.recording = false;
                }
                ui.same_line();
                ui.text(format!("recording, {} samples", self.recorder.len()));
            } else if ui.button("Record my walk") {
                self.recorder.start();
            }
            if ui.button("Save map data file") {
                let path = hudhook::util::get_dll_path().map(|p| p.parent().unwrap().join("tips_mapdump.json"));
                let current = gw::MissionMapData::current().map(|m| (m.map_id, m.is_explorable != 0));
                self.recorder.save(path, current);
            }
            if !self.recorder.message.is_empty() {
                ui.text_wrapped(&self.recorder.message);
            }
        }

        ui.separator();
        ui.text_disabled("Sent to TIPS only while a run is active: map ID, town or explorable,");
        ui.text_disabled("a random session ID, and x/y if sharing is on. Never your name or account.");
    }

    /// Draw the table of personal records and route statistics, grouped by
    /// running categories with collapsible dropdowns and aggregate totals.
    fn render_records_table(&mut self, ui: &imgui::Ui, edit: bool) {
        if self.records.is_empty() {
            ui.text_disabled("No records logged in database.");
            return;
        }

        let mut delete_run_id: Option<i64> = None;

        for (cat_name, summary) in &self.category_summaries {
            if !summary.records.is_empty() {
                let header_title = format!("{cat_name}##cat_{cat_name}");
                let is_open = ui.collapsing_header(header_title, TreeNodeFlags::empty());
                let rect_min = ui.item_rect_min();
                let rect_max = ui.item_rect_max();
                let title_size = ui.calc_text_size(cat_name);

                let mut x = rect_min[0] + title_size[0] + 32.0;
                let y = rect_min[1] + (rect_max[1] - rect_min[1] - title_size[1]) * 0.5;

                let draw_list = ui.get_window_draw_list();
                let text_gray = [0.6, 0.6, 0.6, 1.0];
                let text_green = [0.2, 0.9, 0.35, 1.0];

                draw_list.add_text([x, y], text_gray, "[Sum of Best: ");
                x += ui.calc_text_size("[Sum of Best: ")[0];

                let pb_str = format_duration(summary.total_pb);
                draw_list.add_text([x, y], text_green, &pb_str);
                x += ui.calc_text_size(&pb_str)[0];

                let rest = format!(" | Avg: {} | Zones: {}]", format_duration(summary.total_avg), summary.zone_count);
                draw_list.add_text([x, y], text_gray, &rest);

                if is_open {
                    render_records_subtable(
                        ui,
                        edit,
                        &summary.records,
                        cat_name,
                        &self.route_history_cache,
                        &mut self.pinned_target,
                        &mut self.pending_delete_route,
                        &mut self.confirm_delete_route,
                        &mut delete_run_id,
                    );
                    ui.spacing();
                }
            }
        }

        if !self.unmatched_records.is_empty() {
            let header_title = "Other / Custom Runs##cat_other";
            let is_open = ui.collapsing_header(header_title, TreeNodeFlags::empty());
            let rect_min = ui.item_rect_min();
            let rect_max = ui.item_rect_max();
            let title_size = ui.calc_text_size("Other / Custom Runs");

            let x = rect_min[0] + title_size[0] + 32.0;
            let y = rect_min[1] + (rect_max[1] - rect_min[1] - title_size[1]) * 0.5;

            let draw_list = ui.get_window_draw_list();
            let text_gray = [0.6, 0.6, 0.6, 1.0];
            let label = format!("[Zones: {}]", self.unmatched_records.len());
            draw_list.add_text([x, y], text_gray, label);

            if is_open {
                render_records_subtable(
                    ui,
                    edit,
                    &self.unmatched_records,
                    "other",
                    &self.route_history_cache,
                    &mut self.pinned_target,
                    &mut self.pending_delete_route,
                    &mut self.confirm_delete_route,
                    &mut delete_run_id,
                );
                ui.spacing();
            }
        }

        if let Some(id) = delete_run_id {
            if let Some(log) = &self.run_log {
                let _ = log.delete_run(id);
            }
            self.refresh_records();
        }
    }

    /// Draw the table of runs recorded this session. Meant to be called inside
    /// the main window, under the session-log collapsing header.
    fn render_session_table(&self, ui: &imgui::Ui) {
        if self.session_runs.is_empty() {
            ui.text_disabled("No runs logged yet this session.");
            return;
        }

        let time_width = ui.calc_text_size("00:00.00")[0] + 5.0;
        let table = ui.begin_table_header_with_flags(
            "session_runs",
            [
                TableColumnSetup {
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: 160.0,
                    ..TableColumnSetup::new("From")
                },
                TableColumnSetup {
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: 160.0,
                    ..TableColumnSetup::new("Through")
                },
                TableColumnSetup {
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: 160.0,
                    ..TableColumnSetup::new("To")
                },
                TableColumnSetup {
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: time_width,
                    ..TableColumnSetup::new("Time")
                },
            ],
            TableFlags::SIZING_FIXED_FIT,
        );
        if let Some(_table) = table {
            for run in self.session_runs.iter().rev() {
                ui.table_next_row();
                ui.table_next_column();
                ui.text(map_names::map_name_by_id(run.from_map_id));
                ui.table_next_column();
                ui.text(map_names::map_name_by_id(run.run_map_id));
                ui.table_next_column();
                ui.text(map_names::map_name_by_id(run.to_map_id));
                ui.table_next_column();
                ui.text(format_duration(run.duration));
            }
        }
    }
}

fn render_records_subtable(
    ui: &imgui::Ui,
    edit: bool,
    records: &[RouteRecord],
    cat_key: &str,
    route_history_cache: &HashMap<(u32, u32, u32), Vec<RunEntry>>,
    pinned_target: &mut PinnedTarget,
    pending_delete_route: &mut Option<(u32, u32, u32)>,
    confirm_delete_route: &mut bool,
    delete_run_id: &mut Option<i64>,
) {
    let time_width = ui.calc_text_size("00:00.00")[0] + 5.0;
    let runs_width = ui.calc_text_size("Runs")[0] + 10.0;
    let actions_width = if edit { 85.0 } else { 1.0 };
    let table_id = format!("records_table_{cat_key}");
    let table = ui.begin_table_header_with_flags(
        table_id,
        [
            TableColumnSetup {
                flags: TableColumnFlags::WIDTH_FIXED,
                init_width_or_weight: 160.0,
                ..TableColumnSetup::new("From")
            },
            TableColumnSetup {
                flags: TableColumnFlags::WIDTH_FIXED,
                init_width_or_weight: 160.0,
                ..TableColumnSetup::new("Through")
            },
            TableColumnSetup {
                flags: TableColumnFlags::WIDTH_FIXED,
                init_width_or_weight: 160.0,
                ..TableColumnSetup::new("To")
            },
            TableColumnSetup {
                flags: TableColumnFlags::WIDTH_FIXED,
                init_width_or_weight: runs_width,
                ..TableColumnSetup::new("Runs")
            },
            TableColumnSetup {
                flags: TableColumnFlags::WIDTH_FIXED,
                init_width_or_weight: time_width,
                ..TableColumnSetup::new("PB")
            },
            TableColumnSetup {
                flags: TableColumnFlags::WIDTH_FIXED,
                init_width_or_weight: time_width,
                ..TableColumnSetup::new("Avg")
            },
            TableColumnSetup {
                flags: TableColumnFlags::WIDTH_FIXED,
                init_width_or_weight: actions_width,
                ..TableColumnSetup::new("")
            },
        ],
        TableFlags::SIZING_FIXED_FIT,
    );
    if let Some(_table) = table {
        for rec in records {
            ui.table_next_row();
            ui.table_next_column();

            let route_from_name = map_names::map_name_by_id(rec.from_map_id);
            let is_open = ui.tree_node_config(format!("{route_from_name}##node_{cat_key}_{}_{}_{}", rec.from_map_id, rec.run_map_id, rec.to_map_id))
                .push();

            ui.table_next_column();
            ui.text(map_names::map_name_by_id(rec.run_map_id));
            ui.table_next_column();
            ui.text(map_names::map_name_by_id(rec.to_map_id));
            ui.table_next_column();
            ui.text(rec.run_count.to_string());
            ui.table_next_column();
            ui.text_colored([0.2, 0.9, 0.35, 1.0], format_duration(rec.pb));
            ui.table_next_column();
            ui.text(format_duration(rec.avg));
            ui.table_next_column();
            if edit {
                let _btn_color = ui.push_style_color(StyleColor::Text, [0.9, 0.4, 0.4, 1.0]);
                if ui.small_button(format!("x##del_route_{cat_key}_{}_{}_{}", rec.from_map_id, rec.run_map_id, rec.to_map_id)) {
                    *pending_delete_route = Some((rec.from_map_id, rec.run_map_id, rec.to_map_id));
                    *confirm_delete_route = true;
                }
                drop(_btn_color);
                if ui.is_item_hovered() {
                    ui.tooltip_text("Delete all runs for this route");
                }

                ui.same_line();
                let pin_color = if let PinnedTarget::Route(pinned_rec) = *pinned_target && *rec == pinned_rec {
                    [0.7, 0.7, 0.7, 1.0]
                } else {
                    [0.2, 0.9, 0.35, 1.0]
                };
                let _pin_btn_color = ui.push_style_color(StyleColor::Text, pin_color);
                if ui.small_button(format!("Pin##pin_route_{cat_key}_{}_{}_{}", rec.from_map_id, rec.run_map_id, rec.to_map_id)) {
                    *pinned_target = PinnedTarget::Route(*rec);
                }
                drop(_pin_btn_color);
                if ui.is_item_hovered() {
                    if let PinnedTarget::None = *pinned_target {
                        ui.tooltip_text("Pin PB under timer");
                    } else {
                        ui.tooltip_text("Unpin PB from timer");
                    }
                }
            }

            if let Some(_token) = is_open {
                if let Some(runs) = route_history_cache.get(&(rec.from_map_id, rec.run_map_id, rec.to_map_id)) {
                    for (idx, run) in runs.iter().enumerate() {
                        ui.table_next_row();
                        ui.table_next_column();
                        let item_label = format!("  Run #{}", idx + 1);
                        ui.text_disabled(&item_label);

                        ui.table_next_column();
                        ui.text_disabled(format_system_date(run.completed_at));
                        ui.table_next_column();
                        ui.text_disabled("");
                        ui.table_next_column();
                        ui.text_disabled("");
                        ui.table_next_column();
                        if run.duration == rec.pb {
                            ui.text_colored([0.2, 0.9, 0.35, 1.0], format_duration(run.duration));
                        } else {
                            ui.text_disabled(format_duration(run.duration));
                        }
                        ui.table_next_column();
                        ui.text_disabled("");
                        ui.table_next_column();
                        if edit {
                            let _btn_color = ui.push_style_color(StyleColor::Text, [0.9, 0.4, 0.4, 1.0]);
                            if ui.small_button(format!("x##del_run_{cat_key}_{}", run.id)) {
                                *delete_run_id = Some(run.id);
                            }
                            drop(_btn_color);
                            if ui.is_item_hovered() {
                                ui.tooltip_text("Delete this run");
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Format a duration as `MM:SS.d` with tenth-of-a-second precision.
fn format_duration(d: Duration) -> String {
    let hundos_total = d.as_millis() / 10;
    let hundos = hundos_total % 100;
    let secs_total = hundos_total / 100;
    let seconds = secs_total % 60;
    let minutes = secs_total / 60;
    format!("{minutes:02}:{seconds:02}.{hundos:02}")
}

fn format_duration_hrs(d: Duration) -> String {
    let hundos_total = d.as_millis() / 10;
    let hundos = hundos_total % 100;
    let secs_total = hundos_total / 100;
    let seconds = secs_total % 60;
    let minutes_total = secs_total / 60;
    let minutes = minutes_total % 60;
    let hours = minutes_total / 60;
    format!("{hours}:{minutes:02}:{seconds:02}.{hundos:02}")
}

fn format_system_date(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1024 + doe / 1461 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02} {hours:02}:{minutes:02}:{seconds:02}")
}

impl ImguiRenderLoop for ZoneTimer {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, _rc: &'a mut dyn RenderContext) {
        let fonts = ctx.fonts();
        fonts.add_font(&[FontSource::DefaultFontData { config: None }]);
        let id = fonts.add_font(&[FontSource::TtfData {
            data: TIMER_FONT_TTF,
            size_pixels: TIMER_FONT_PX,
            config: Some(FontConfig {
                oversample_h: 3,
                ..FontConfig::default()
            }),
        }]);
        self.big_font = Some(SyncFont(id));

        let run_log = match hudhook::util::get_dll_path() {
            Some(path) => RunLog::open(path.parent().unwrap().join("kaos_zone_timer.db")),
            None => RunLog::in_memory(),
        };

        match run_log {
            Ok(log) => {
                self.run_log = Some(log);
                self.refresh_records();
            }
            Err(e) => tracing::error!("failed to open run log: {e}"),
        }

        // TIPS tracker: does nothing until a token is entered and Start run is pressed.
        let cfg_path = hudhook::util::get_dll_path().map(|p| p.parent().unwrap().join("tips_tracker.cfg"));
        let uplink = uplink::Uplink::new(cfg_path);
        self.tips_token_input = uplink.config().token;
        self.uplink = Some(uplink);
    }

    fn render(&mut self, ui: &mut imgui::Ui) {
        if let Some(map_data) = gw::MissionMapData::current() {
            if map_data.is_loaded() {
                if !self.state.last_load_state {
                    let cur_map_id = map_data.map_id;
                    self.state.mapid_before_run = self.state.mapid_during_run;
                    self.state.mapid_during_run = self.state.mapid_now;
                    self.state.mapid_now = cur_map_id;
                    if let Some(up) = &self.uplink {
                        up.push_zone(cur_map_id, map_data.is_explorable != 0);
                    }
                    if !self.elapsed().is_zero() {
                        self.record_run(
                            self.state.mapid_before_run,
                            self.state.mapid_during_run,
                            self.state.mapid_now,
                            self.elapsed(),
                        );
                        self.reset();
                    }
                }
                if map_data.is_explorable != 0 {
                    if !self.is_running() {
                        if let Some(controlled_char) = gw::CharControlledCharacterData::current() {
                            if controlled_char.is_moving() {
                                self.start();
                            }
                        }
                    }
                }
            } else {
                if self.is_running() {
                    self.pause();
                }
            }
            self.state.last_load_state = map_data.is_loaded();
            if map_data.is_loaded() {
                self.sample_position();
            }
        }

        // The edit chord unlocks dragging and the context menu.
        let edit_modifier_keys = ui.io().key_ctrl && ui.io().key_shift;
        let edit = edit_modifier_keys || self.in_full_edit_mode;
        let entered_edit = edit && !self.was_edit;
        let left_edit = !edit && self.was_edit;
        self.was_edit = edit;

        let overlay_flags = WindowFlags::NO_RESIZE | WindowFlags::NO_MOVE | WindowFlags::NO_INPUTS | WindowFlags::NO_COLLAPSE;

        let mut flags = WindowFlags::NO_SCROLLBAR
            | WindowFlags::NO_COLLAPSE
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::ALWAYS_AUTO_RESIZE;
        if !edit {
            flags |= overlay_flags | WindowFlags::NO_TITLE_BAR;
        }

        let pos_cond = if edit { Condition::Appearing } else { Condition::Always };

        if self.show_log {
            let mut show_log = self.show_log;
            ui.window("Session Log##session_log")
                //.opened(&mut show_log)
                .size_constraints([640.0, 400.0], [f32::MAX, f32::MAX])
                .flags(if edit { WindowFlags::NO_COLLAPSE } else { overlay_flags })
                .build(|| {
                    self.render_session_table(ui);
                });
            self.show_log = show_log;
        }

        let mut timer_win = ui
            .window(format!("KAOS' Zone Timer (r{}:{})##zone_timer", version::GIT_REVISION, version::GIT_HASH))
            .position(self.pos, pos_cond)
            .focused(entered_edit)
            .flags(flags)
            .build(|| {
            let color = if self.is_running() {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [1.0, 0.75, 0.2, 1.0] // amber when paused
            };
            let font = self.big_font.as_ref().map(|f| ui.push_font(f.0));
            ui.text_colored(color, self.format_time());
            if let Some(font) = font {
                font.pop();
            }

            match self.pinned_target {
                PinnedTarget::None => {}
                PinnedTarget::Route (rec) => {
                     ui.text_colored([0.2, 0.9, 0.35, 1.0], format!("PB: {}", format_duration(rec.pb)));
                }
            }

            if self.show_inst_timer {
                if let Some(instance_time) = gw::get_instance_uptime() {
                    ui.text(format_duration_hrs(Duration::from_millis(*instance_time as u64)));
                }
            }

            if edit {
                self.pos = ui.window_pos();
            }


            if edit {
                if let Some(_menu) = ui.begin_popup_context_window() {

                    if ui.menu_item_config("Show Instance Time").selected(self.show_inst_timer).build() {
                        self.show_inst_timer = !self.show_inst_timer;
                    }

                    if let PinnedTarget::Route(_) = self.pinned_target {
                        if ui.menu_item_config("Remove Pinned PB Record").build() {
                            self.pinned_target = PinnedTarget::None;
                        }
                    }
                    
                    ui.separator();

                    if ui.menu_item_config("Session Log").selected(self.show_log).build() {
                        self.show_log = !self.show_log;
                    }
                    if ui.menu_item_config("Personal Records").selected(self.show_records).build() {
                        self.show_records = !self.show_records;
                    }
                    if ui.menu_item_config("TIPS Tracker").selected(self.show_tips).build() {
                        // Closed -> open and editable. Open but locked -> editable. Editable -> close.
                        if self.show_tips && self.tips_edit {
                            self.show_tips = false;
                            self.tips_edit = false;
                        } else {
                            self.show_tips = true;
                            self.tips_edit = true;
                        }
                    }

                    ui.separator();

                    if ui.menu_item_config("Exit Timer").build() {
                        self.confirm_exit = true;
                    }
                }
            }

            if std::mem::take(&mut self.confirm_exit) {
                ui.open_popup("Confirm Timer Exit");
            }

            if let Some(_exitpopup) = ui.begin_modal_popup("Confirm Timer Exit") {
                ui.text("Are you sure you want to exit the timer?");

                if ui.button("Yes") {
                    hudhook::eject();
                }
                ui.same_line();
                if ui.button("No") {
                    ui.close_current_popup();
                }
            }
        });

        // Trigger cache refresh when records window is freshly opened
        let records_opened = self.show_records && !self.was_records_open;
        self.was_records_open = self.show_records;
        if records_opened {
            self.refresh_records();
        }

        // Separate Personal Records window opened via the right-click menu.
        if self.show_records {
            let mut opened = self.show_records;
            let mut rec_flags = WindowFlags::NO_SAVED_SETTINGS;
            if !edit {
                rec_flags |= WindowFlags::NO_MOVE | WindowFlags::NO_RESIZE;
            }
            ui.window("Personal Records##records_window")
                //.opened(&mut opened)
                .size([840.0, 480.0], Condition::FirstUseEver)
                .flags(if edit { WindowFlags::empty() } else { overlay_flags })
                .build(|| {
                    self.render_records_table(ui, edit);
                });
            self.show_records = opened;
        }

        // TIPS live-tracking panel, opened via the right-click menu.
        if !self.show_tips {
            self.tips_edit = false;
        }
        if self.show_tips {
            let tips_interactive = edit || self.tips_edit;
            ui.window("TIPS Tracker##tips_window")
                .size([470.0, 430.0], Condition::FirstUseEver)
                .flags(if tips_interactive { WindowFlags::empty() } else { overlay_flags })
                .build(|| {
                    self.render_tips(ui, tips_interactive);
                });
        }
        let left_tips_edit = self.was_tips_edit && !self.tips_edit;
        self.was_tips_edit = self.tips_edit;

        if std::mem::take(&mut self.confirm_delete_route) {
            ui.open_popup("Delete Route History?");
        }

        if let Some(_deletepopup) = ui.begin_modal_popup("Delete Route History?") {
            if let Some((from, run, to)) = self.pending_delete_route {
                let from_name = map_names::map_name_by_id(from);
                let run_name = map_names::map_name_by_id(run);
                let to_name = map_names::map_name_by_id(to);
                ui.text(format!(
                    "Delete all recorded runs for route:\n\n  {} -> {} -> {}\n\nThis cannot be undone.",
                    from_name, run_name, to_name
                ));
                ui.spacing();

                if ui.button("Delete") {
                    if let Some(log) = &self.run_log {
                        let _ = log.delete_route(from, run, to);
                    }
                    // if self.pinned_target == (PinnedTarget::Route { from, run, to }) {
                    //     self.pinned_target = PinnedTarget::None;
                    // }
                    self.refresh_records();
                    self.pending_delete_route = None;
                    ui.close_current_popup();
                }
                ui.same_line();
                if ui.button("Cancel") {
                    self.pending_delete_route = None;
                    ui.close_current_popup();
                }
            } else {
                ui.close_current_popup();
            }
        }

        if (left_edit && !self.tips_edit) || left_tips_edit {
            unsafe { imgui::sys::igSetWindowFocus_Nil() };
        }
    }

    fn message_filter(&self, io: &imgui::Io) -> hudhook::MessageFilter {
        let quick_edit_keys_down = io.key_ctrl && io.key_shift;
        if quick_edit_keys_down || self.tips_edit {
            let mut filter = hudhook::MessageFilter::empty();
            if io.want_capture_keyboard { filter |= hudhook::MessageFilter::InputKeyboard; }
            if io.want_capture_mouse { filter |= hudhook::MessageFilter::InputMouse; }
            filter
        }
        else if self.in_full_edit_mode {
            hudhook::MessageFilter::InputAll | hudhook::MessageFilter::WindowAll
        }
        else {
            hudhook::MessageFilter::empty()
        }
    }
}

use hudhook::hooks::dx9::ImguiDx9Hooks;
hudhook::hudhook!(ImguiDx9Hooks, ZoneTimer::default());