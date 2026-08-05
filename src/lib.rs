
mod gw;
mod run_log;
mod map_names;
mod categories;

pub mod version {
    include!(concat!(env!("OUT_DIR"), "/version.rs"));
}

use run_log::RunLog;


use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hudhook::ImguiRenderLoop;
use hudhook::RenderContext;
use hudhook::imgui::{
    self, Condition, FontConfig, FontId, FontSource, StyleColor, TableColumnFlags, TableColumnSetup,
    TableFlags, TreeNodeFlags, WindowFlags,
};

use run_log::{RouteRecord, RunEntry};

/// Target that can be pinned to display its PB on the main timer readout.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PinnedTarget {
    Route { from: u32, run: u32, to: u32 },
    Category(&'static str),
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
    /// Aggregated route records loaded from the database.
    records: Vec<RouteRecord>,
    /// Whether the personal-records window is open.
    show_records: bool,
    /// Set when "Exit Timer" is picked, to open the confirm popup next.
    confirm_exit: bool,
    /// Target route awaiting confirmation to delete (from, run, to).
    pending_delete_route: Option<(u32, u32, u32)>,
    /// Set when a route delete button is clicked, to open the confirm modal next.
    confirm_delete_route: bool,
    /// The currently pinned route or category whose PB is displayed under the timer.
    pinned_target: Option<PinnedTarget>,
    /// Edit-mode state from the previous frame, for detecting enter/leave.
    was_edit: bool,

    show_inst_timer: bool,

    state: ZoneTimerState,

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
            show_records: false,
            confirm_exit: false,
            pending_delete_route: None,
            confirm_delete_route: false,
            pinned_target: None,
            show_inst_timer: false,
            was_edit: false,
            state: ZoneTimerState{
                last_load_state: false,
                mapid_before_run: 0,
                mapid_during_run: 0,
                mapid_now: 0
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

    /// Log a completed run to the database and this session's list. Intended to
    /// be called by the (upcoming) map-change detection.
    #[allow(dead_code)]
    fn record_run(&mut self, from_map_id: u32, run_map_id: u32, to_map_id: u32, duration: Duration) {
        let Some(log) = &self.run_log else { return };
        match log.log_run(from_map_id, run_map_id, to_map_id, duration) {
            Ok(entry) => {
                self.session_runs.push(entry);
                self.refresh_records();
            }
            Err(e) => tracing::error!("failed to log run: {e}"),
        }
    }

    /// Refresh aggregated route records from the database into memory.
    fn refresh_records(&mut self) {
        if let Some(log) = &self.run_log {
            match log.records() {
                Ok(recs) => self.records = recs,
                Err(e) => tracing::error!("failed to load route records: {e}"),
            }
        }
    }

    /// Draw the table of personal records and route statistics, grouped by
    /// running categories with collapsible dropdowns and aggregate totals.
    fn render_records_table(&mut self, ui: &imgui::Ui, edit: bool) {
        if self.records.is_empty() {
            ui.text_disabled("No records logged in database.");
            return;
        }

        let mut delete_run_id: Option<i64> = None;

        for cat in categories::ALL_CATEGORIES {
            let summary = cat.summarize(&self.records);
            if !summary.records.is_empty() {
                let header_title = format!("{}##cat_{}", cat.name, cat.name);
                let is_open = ui.collapsing_header(header_title, TreeNodeFlags::empty());
                let rect_min = ui.item_rect_min();
                let rect_max = ui.item_rect_max();
                let title_size = ui.calc_text_size(cat.name);

                let mut x = rect_min[0] + title_size[0] + 32.0;
                let y = rect_min[1] + (rect_max[1] - rect_min[1] - title_size[1]) * 0.5;

                let draw_list = ui.get_window_draw_list();
                let text_gray = [0.6, 0.6, 0.6, 1.0];
                let text_green = [0.2, 0.9, 0.35, 1.0];

                draw_list.add_text([x, y], text_gray, "[PB: ");
                x += ui.calc_text_size("[PB: ")[0];

                let pb_str = format_duration(summary.total_pb);
                draw_list.add_text([x, y], text_green, &pb_str);
                x += ui.calc_text_size(&pb_str)[0];

                let rest = format!(" | Avg: {} | Zones: {}]", format_duration(summary.total_avg), summary.zone_count);
                draw_list.add_text([x, y], text_gray, &rest);

                if is_open {
                    if edit {
                        let is_cat_pinned = self.pinned_target == Some(PinnedTarget::Category(cat.name));
                        let pin_color = if is_cat_pinned {
                            [0.2, 0.9, 0.35, 1.0]
                        } else {
                            [0.7, 0.7, 0.7, 1.0]
                        };
                        let _pin_btn_color = ui.push_style_color(StyleColor::Text, pin_color);
                        let btn_label = if is_cat_pinned { "Unpin Category PB" } else { "Pin Category PB" };
                        if ui.small_button(format!("{btn_label}##pin_cat_{}", cat.name)) {
                            if is_cat_pinned {
                                self.pinned_target = None;
                            } else {
                                self.pinned_target = Some(PinnedTarget::Category(cat.name));
                            }
                        }
                        drop(_pin_btn_color);
                        if ui.is_item_hovered() {
                            if is_cat_pinned {
                                ui.tooltip_text("Unpin category total PB from main timer");
                            } else {
                                ui.tooltip_text("Pin category total PB under main timer");
                            }
                        }
                        ui.spacing();
                    }
                    self.render_records_subtable(ui, edit, &summary.records, cat.name, &mut delete_run_id);
                    ui.spacing();
                }
            }
        }

        let unmatched: Vec<RouteRecord> = self.records.iter()
            .copied()
            .filter(|rec| !categories::ALL_CATEGORIES.iter().any(|cat| cat.matches(rec.from_map_id, rec.run_map_id, rec.to_map_id)))
            .collect();

        if !unmatched.is_empty() {
            let header_title = "Other / Custom Runs##cat_other";
            let is_open = ui.collapsing_header(header_title, TreeNodeFlags::empty());
            let rect_min = ui.item_rect_min();
            let rect_max = ui.item_rect_max();
            let title_size = ui.calc_text_size("Other / Custom Runs");

            let x = rect_min[0] + title_size[0] + 32.0;
            let y = rect_min[1] + (rect_max[1] - rect_min[1] - title_size[1]) * 0.5;

            let draw_list = ui.get_window_draw_list();
            let text_gray = [0.6, 0.6, 0.6, 1.0];
            let label = format!("[Zones: {}]", unmatched.len());
            draw_list.add_text([x, y], text_gray, label);

            if is_open {
                self.render_records_subtable(ui, edit, &unmatched, "other", &mut delete_run_id);
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

    fn render_records_subtable(
        &mut self,
        ui: &imgui::Ui,
        edit: bool,
        records: &[RouteRecord],
        cat_key: &str,
        delete_run_id: &mut Option<i64>,
    ) {
        let time_width = ui.calc_text_size("00:00.00")[0] + 5.0;
        let runs_width = ui.calc_text_size("Runs")[0] + 10.0;
        let actions_width = if edit {
            ui.calc_text_size(" x ")[0] + ui.calc_text_size(" Pin ")[0] + 16.0
        } else {
            1.0
        };
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
                        self.pending_delete_route = Some((rec.from_map_id, rec.run_map_id, rec.to_map_id));
                        self.confirm_delete_route = true;
                    }
                    drop(_btn_color);
                    if ui.is_item_hovered() {
                        ui.tooltip_text("Delete all runs for this route");
                    }

                    ui.same_line();
                    let is_pinned = self.pinned_target == Some(PinnedTarget::Route {
                        from: rec.from_map_id,
                        run: rec.run_map_id,
                        to: rec.to_map_id,
                    });
                    let pin_color = if is_pinned {
                        [0.2, 0.9, 0.35, 1.0]
                    } else {
                        [0.7, 0.7, 0.7, 1.0]
                    };
                    let _pin_btn_color = ui.push_style_color(StyleColor::Text, pin_color);
                    if ui.small_button(format!("Pin##pin_route_{cat_key}_{}_{}_{}", rec.from_map_id, rec.run_map_id, rec.to_map_id)) {
                        if is_pinned {
                            self.pinned_target = None;
                        } else {
                            self.pinned_target = Some(PinnedTarget::Route {
                                from: rec.from_map_id,
                                run: rec.run_map_id,
                                to: rec.to_map_id,
                            });
                        }
                    }
                    drop(_pin_btn_color);
                    if ui.is_item_hovered() {
                        if is_pinned {
                            ui.tooltip_text("Unpin PB from timer");
                        } else {
                            ui.tooltip_text("Pin PB under timer");
                        }
                    }
                }

                if let Some(_token) = is_open {
                    if let Some(log) = &self.run_log {
                        if let Ok(runs) = log.runs_for_route(rec.from_map_id, rec.run_map_id, rec.to_map_id) {
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
    }

    /// Draw the table of runs recorded this session. Meant to be called inside
    /// the main window, under the session-log collapsing header.
    fn render_session_table(&self, ui: &imgui::Ui) {
        if self.session_runs.is_empty() {
            ui.text_disabled("No runs logged yet this session.");
            return;
        }

        // The time is always "MM:SS.hh", so pin that column to a constant width
        // (its text width plus a small pad) rather than letting it size to data.
        let time_width = ui.calc_text_size("00:00.00")[0] + 5.0;
        let table = ui.begin_table_header_with_flags(
            "session_runs",
            [
                // Under SIZING_FIXED_FIT these three size to their content.
                TableColumnSetup{
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: 160.0,
                    ..TableColumnSetup::new("From")
                },
                
                // Under SIZING_FIXED_FIT these three size to their content.
                TableColumnSetup{
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: 160.0,
                    ..TableColumnSetup::new("Through")
                },
                TableColumnSetup{
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
            // Fixed-fit columns give the table a definite width, which the
            // auto-resizing window needs (stretch columns have nothing to
            // stretch into).
            // TableFlags::ROW_BG | TableFlags::BORDERS |
            TableFlags::SIZING_FIXED_FIT,
        );
        if let Some(_table) = table {
            // Newest run first.
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

/// Display string for a map: its decoded name if available, otherwise the
/// numeric id as a fallback while the async decode is still in flight.
fn map_name_or_id(mapid: u32) -> String {
   // gw::mapdata::map_name(mapid).unwrap_or_else(|| mapid.to_string())
   mapid.to_string()
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
        // Add the normal-size default font first so it stays ImGui's global
        // default (font index 0) — everything except the timer uses this.
        fonts.add_font(&[FontSource::DefaultFontData { config: None }]);
        // The large font is a separate TTF entry we push only around the timer
        // text; rasterizing at the target size (vs. scaling the bitmap default)
        // is what gives smooth glyphs.
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
            None => RunLog::in_memory()
        };

        match run_log {
            Ok(log) => {
                if let Ok(recs) = log.records() {
                    self.records = recs;
                }
                self.run_log = Some(log);
            }
            Err(e) => tracing::error!("failed to open run log: {e}"),
        }
    }




    fn render(&mut self, ui: &mut imgui::Ui) {

        // timer logic

        // let map_data_ptr = gw::MissionMapData::current();
        

        if let Some(map_data) = gw::MissionMapData::current() {
            if map_data.is_loaded() {
                if self.state.last_load_state == false {
                    let cur_map_id = map_data.map_id;
                    self.state.mapid_before_run = self.state.mapid_during_run;
                    self.state.mapid_during_run = self.state.mapid_now;
                    self.state.mapid_now = cur_map_id;
                    if !self.elapsed().is_zero() {
                        self.record_run(
                            self.state.mapid_before_run, 
                            self.state.mapid_during_run,
                            self.state.mapid_now, 
                        self.elapsed());
                        self.reset();
                    }
                }
                if map_data.is_explorable != 0 {
                    if !self.is_running() {
                        if let Some(controlled_char) = gw::CharControlledCharacterData::current() {
                            if  controlled_char.is_moving() {
                                self.start();
                            }
                        }
                    }
                }
            }
            else {
                if self.is_running() {
                    self.pause();
                }
            }
            self.state.last_load_state = map_data.is_loaded();
        }



        // The edit chord unlocks dragging and the context menu.
        let edit = ui.io().key_ctrl && ui.io().key_shift;
        // Detect the frame edit mode turns on/off, to grab and release focus.
        let entered_edit = edit && !self.was_edit;
        let left_edit = !edit && self.was_edit;
        self.was_edit = edit;

        let mut flags = WindowFlags::NO_SCROLLBAR
            | WindowFlags::NO_COLLAPSE
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::ALWAYS_AUTO_RESIZE;
        if !edit {
            // Locked: can't be moved, and lets mouse input pass through to the game.
            flags |= WindowFlags::NO_RESIZE | WindowFlags::NO_MOVE | WindowFlags::NO_INPUTS;

            if !self.show_log {
                flags |= WindowFlags::NO_TITLE_BAR;
            }
        }

        // While locked, hard-pin to `self.pos` every frame. While editing, only
        // seed position on first appearance so imgui's own drag can take over;
        // we read the dragged position back into `self.pos` below.
        let pos_cond = if edit { Condition::Appearing } else { Condition::Always };

        let mut win = ui
            .window(format!("KAOS' Zone Timer (r{}:{})##zone_timer", version::GIT_REVISION,version::GIT_HASH))
            .position(self.pos, pos_cond)
            // Bring the window to the front on the frame we enter edit mode;
            // `.focused(false)` is a no-op, so this only fires on the transition.
            .focused(entered_edit)
            .flags(flags);
        // When the log is showing, hold the window to at least 640x400
        // so the list has room; otherwise it auto-resizes to hug the timer.
        if self.show_log {
            win = win.size_constraints([640.0, 400.0], [f32::MAX, f32::MAX]);
        }

        win.build(|| {
                let color = if self.is_running() {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [1.0, 0.75, 0.2, 1.0] // amber when paused
                };
                // Push the large font only for the time readout; the context
                // menu and everything else keep the default UI size.
                let font = self.big_font.as_ref().map(|f| ui.push_font(f.0));
                ui.text_colored(color, self.format_time());
                if let Some(font) = font {
                    font.pop();
                }

                if let Some(target) = &self.pinned_target {
                    match target {
                        PinnedTarget::Route { from, run, to } => {
                            if let Some(rec) = self.records.iter().find(|r| r.from_map_id == *from && r.run_map_id == *run && r.to_map_id == *to) {
                                ui.text_colored([0.2, 0.9, 0.35, 1.0], format!("PB: {}", format_duration(rec.pb)));
                            }
                        }
                        PinnedTarget::Category(cat_name) => {
                            if let Some(cat) = categories::ALL_CATEGORIES.iter().find(|c| c.name == *cat_name) {
                                let summary = cat.summarize(&self.records);
                                if summary.total_pb > Duration::ZERO {
                                    ui.text_colored([0.2, 0.9, 0.35, 1.0], format!("PB ({}): {}", cat.name, format_duration(summary.total_pb)));
                                }
                            }
                        }
                    }
                }

                if self.show_inst_timer {
                    if let Some(instance_time) = gw::get_instance_uptime() {
                        ui.text(format_duration_hrs(Duration::from_millis(*instance_time as u64)));
                    }
                }

                if edit {
                    // Capture the (possibly dragged) position so it survives the
                    // switch back to the locked, position-pinned state.
                    self.pos = ui.window_pos();
                }

                // Session-log panel. The collapsing header is only interactive
                // in edit mode (the window is click-through when locked). When
                // locked and collapsed, neither the header nor the table shows.
                if edit || self.show_log {
                    ui.separator();
                    // `show_log` is the source of truth: force the header's open
                    // state from it each frame. imgui's own collapse state lives
                    // in per-context storage that's wiped when the D3D9 device
                    // resets (e.g. on a map change), which would otherwise make
                    // the panel spuriously collapse. A user click still toggles:
                    // it flips the forced value, and we read the result back.
                    unsafe {
                        imgui::sys::igSetNextItemOpen(self.show_log, Condition::Always as i32);
                    }
                    // When locked, paint the header bar the window background
                    // color so it blends in instead of showing the accent color.
                    let header_colors = (!edit).then(|| {
                        let bg = ui.style_color(StyleColor::WindowBg);
                        [
                            ui.push_style_color(StyleColor::Header, bg),
                            ui.push_style_color(StyleColor::HeaderHovered, bg),
                            ui.push_style_color(StyleColor::HeaderActive, bg),
                        ]
                    });
                    self.show_log = ui.collapsing_header("Session Log", TreeNodeFlags::empty());
                    drop(header_colors);

                    if self.show_log {
                        self.render_session_table(ui);
                    }
                }

                if edit {
                    if let Some(_menu) = ui.begin_popup_context_window() {
                        if ui.menu_item_config("Personal Records").selected(self.show_records).build() {
                            self.show_records = !self.show_records;
                        }

                        if ui.menu_item_config("Show Instance Time").selected(self.show_inst_timer).build() {
                            self.show_inst_timer = !self.show_inst_timer;
                        }

                        if ui.menu_item_config("Clear Log").build() {
                            self.session_runs.clear();
                        }
                        // Only record the intent here. Opening the popup from
                        // inside the context menu would hash its id against the
                        // menu's id stack and nest it one popup level deep, so
                        // the `begin_modal_popup` below (main window, level 0)
                        // would never match it — and the menu item's own
                        // auto-close would tear the nested entry down anyway.
                        if ui.menu_item_config("Exit Timer").build() {
                            self.confirm_exit = true;
                        }
                    }
                }

                // Opened out here so the id and popup nesting level line up with
                // the `begin_modal_popup` call that follows.
                if std::mem::take(&mut self.confirm_exit) {
                    ui.open_popup("Confirm Timer Exit");
                }

                // Modal (not plain popup) so it survives the user releasing the
                // edit chord and grabs mouse capture while it's up.
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

        // Separate Personal Records window opened via the right-click menu.
        if self.show_records {
            let mut opened = self.show_records;
            let mut rec_flags = WindowFlags::NO_SAVED_SETTINGS;
            if !edit {
                rec_flags |= WindowFlags::NO_MOVE | WindowFlags::NO_RESIZE;
            }
            ui.window("Personal Records##records_window")
                .opened(&mut opened)
                .size([680.0, 400.0], Condition::FirstUseEver)
                .flags(rec_flags)
                .build(|| {
                    self.render_records_table(ui, edit);
                });
            self.show_records = opened;
        }

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
                    if self.pinned_target == Some(PinnedTarget::Route { from, run, to }) {
                        self.pinned_target = None;
                    }
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

        if left_edit {
            // Release focus back to the game when leaving edit mode.
            unsafe { imgui::sys::igSetWindowFocus_Nil() };
        }

    }

    fn message_filter(&self, io: &imgui::Io) -> hudhook::MessageFilter {
        // Only take input away from the game when ImGui actually wants it:
        //  - mouse: while the cursor is over one of our windows or dragging a
        //    widget (`want_capture_mouse`), so clicks on the overlay no longer
        //    leak through to the game underneath.
        //  - keyboard: only while an ImGui text field has focus
        //    (`want_capture_keyboard`), which today is basically never.
        //
        // We deliberately never block based on the held Ctrl+Shift edit chord.
        // Doing that ate the KEY-UP for whichever modifier was released first
        // (the cached filter was still "block all keyboard" at that instant),
        // leaving the game convinced the modifier was still held down. Letting
        // every modifier up/down reach the game keeps its key state in sync.
        let mut filter = hudhook::MessageFilter::empty();
        if io.want_capture_mouse {
            filter |= hudhook::MessageFilter::InputMouse;
        }
        if io.want_capture_keyboard {
            filter |= hudhook::MessageFilter::InputKeyboard;
        }
        filter
    }
}

use hudhook::hooks::dx9::ImguiDx9Hooks;
hudhook::hudhook!(ImguiDx9Hooks, ZoneTimer::default());