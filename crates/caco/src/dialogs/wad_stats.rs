//! Map-stats & completions manager dialog.
//!
//! Two-pane layout: left pane lists the live snapshot + historical completions;
//! right pane renders the selected entry's per-map stats table. Users can add,
//! edit (notes + date), or delete completion rows, and import/export/clear the
//! stats snapshot attached to any entry.

use chrono::Utc;
use egui_extras::Column;
use rusqlite::Connection;

use crate::dialogs::{Dialog, FooterRow, Primary, count, dialog_table, form_label, scroll_body};
use crate::theme;
use crate::workers::{FileDialogReceiver, FileDialogRequest, spawn_file_dialog};

type StatsData = caco_core::wad_stats::WadStats;
type AnalysisData = caco_core::wad_analysis::WadAnalysis;
use caco_core::wad_analysis::MapClassification;

/// A stats-carrying entry shown in the left pane.
#[derive(Clone)]
enum EntryKind {
    /// The live `wad.stats_snapshot`; not tied to a completion row.
    Live,
    /// A historical `wad_completions` row.
    Completion(i64),
}

#[derive(Clone)]
struct Entry {
    kind: EntryKind,
    /// Primary label ("Current (live)" or formatted date).
    label: String,
    /// For completions: raw stored `completed_at`. For live: empty.
    raw_date: String,
    /// For completions: notes. For live: empty.
    notes: String,
    /// Parsed stats (if any).
    stats: Option<StatsData>,
}

/// Buffer used while inline-editing (or drafting) a completion's notes + date.
///
/// `completion_id == None` is a draft for a not-yet-inserted completion —
/// Cancel discards it without touching the DB, Save performs the insert.
struct EditBuffer {
    completion_id: Option<i64>,
    date: String,
    notes: String,
}

/// State for the WAD stats dialog.
pub struct WadStatsDialogState {
    wad_id: i64,
    wad_title: String,
    entries: Vec<Entry>,
    selected_index: usize,
    /// Cached `WadAnalysis` for this WAD, when one is present and fresh.
    /// When `None`, the per-map table falls back to "played maps from stats"
    /// behaviour and shows a banner explaining the gap. Re-loaded by
    /// `reload_entries` so background re-analysis is reflected on the next
    /// dialog interaction.
    analysis: Option<AnalysisData>,
    /// Inline editor, if active.
    edit: Option<EditBuffer>,
    /// Second click of the Delete button triggers deletion; first click arms
    /// it. Holds the completion id awaiting confirmation.
    confirm_delete: Option<i64>,
    pending_import: Option<(PendingImportTarget, FileDialogReceiver)>,
    pending_export: Option<(StatsData, FileDialogReceiver)>,
    error: Option<String>,
}

/// Which entry a pending file-picker import should apply to. Held outside the
/// receiver so the target can't drift if the user changes the selection while
/// the picker is open.
#[derive(Clone, Copy)]
enum PendingImportTarget {
    Live,
    Completion(i64),
}

/// Result of showing the WAD stats dialog.
pub enum WadStatsResult {
    Open,
    Closed,
    /// A DB write happened — caller should reload library data. The dialog
    /// stays open; users typically want to keep managing entries.
    Modified,
}

impl WadStatsDialogState {
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let wad = caco_core::db::wads::get_wad(conn, wad_id, false).ok()??;
        let mut state = Self {
            wad_id,
            wad_title: wad.title.clone(),
            entries: Vec::new(),
            selected_index: 0,
            analysis: None,
            edit: None,
            confirm_delete: None,
            pending_import: None,
            pending_export: None,
            error: None,
        };
        state.reload_entries(conn);
        Some(state)
    }

    /// Rebuild `entries` from DB. Keeps selection on the same entry where
    /// possible (live stays live; completions match by id).
    fn reload_entries(&mut self, conn: &Connection) {
        let prev_key = self
            .entries
            .get(self.selected_index)
            .map(|e| match &e.kind {
                EntryKind::Live => (true, 0i64),
                EntryKind::Completion(id) => (false, *id),
            });

        self.entries.clear();
        self.analysis = caco_core::db::get_analysis(conn, self.wad_id)
            .ok()
            .flatten();

        if let Ok(Some(wad)) = caco_core::db::wads::get_wad(conn, self.wad_id, false) {
            let stats = wad
                .stats_snapshot
                .as_deref()
                .and_then(|s| caco_core::wad_stats::stats_from_json(s).ok());
            self.entries.push(Entry {
                kind: EntryKind::Live,
                label: "Current playthrough".to_string(),
                raw_date: String::new(),
                notes: String::new(),
                stats,
            });
        }

        if let Ok(completions) = caco_core::db::sessions::get_wad_completions(conn, self.wad_id) {
            for comp in completions {
                let stats = comp
                    .stats_snapshot
                    .as_deref()
                    .and_then(|s| caco_core::wad_stats::stats_from_json(s).ok());
                let date_short = comp
                    .completed_at
                    .get(..10)
                    .unwrap_or(&comp.completed_at)
                    .to_string();
                self.entries.push(Entry {
                    kind: EntryKind::Completion(comp.id),
                    label: format!("Completion {date_short}"),
                    raw_date: comp.completed_at,
                    notes: comp.notes.unwrap_or_default(),
                    stats,
                });
            }
        }

        self.selected_index = prev_key
            .and_then(|(is_live, id)| {
                self.entries.iter().position(|e| match &e.kind {
                    EntryKind::Live => is_live,
                    EntryKind::Completion(cid) => !is_live && *cid == id,
                })
            })
            .unwrap_or(0);
    }

    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> WadStatsResult {
        let mut result = WadStatsResult::Open;
        let mut closed = false;

        // Drain file pickers from previous frames.
        if self.drain_import_picker(conn) {
            result = WadStatsResult::Modified;
        }
        self.drain_export_picker();

        Dialog::new("Map Stats", [960.0, 620.0])
            .about(&self.wad_title)
            .show(ctx, |dialog| {
                dialog
                    .fill(|ui, height| {
                        let left_width = (ui.available_width() * 0.32).clamp(220.0, 300.0);
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(left_width);
                                ui.set_height(height);
                                self.render_left_pane(ui, height);
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.set_height(height);
                                self.render_right_pane(ui, height);
                            });
                        });
                    })
                    .above(|ui| self.render_notes(ui))
                    .buttons(|row| {
                        if self.render_footer(ctx, row, conn, &mut result) {
                            closed = true;
                        }
                    })
            });

        if closed {
            return WadStatsResult::Closed;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            // Backing out of something being asked or edited is not leaving.
            if self.confirm_delete.is_some() {
                self.confirm_delete = None;
            } else if self.edit.is_some() {
                self.edit = None;
            } else {
                return WadStatsResult::Closed;
            }
        }

        result
    }

    fn render_left_pane(&mut self, ui: &mut egui::Ui, height: f32) {
        ui.colored_label(
            theme::TEXT_SECONDARY,
            count(self.completion_count(), "time finished", "times finished"),
        );
        ui.add_space(4.0);
        scroll_body(ui, height - 24.0, |ui| {
            for idx in 0..self.entries.len() {
                self.render_entry_row(ui, idx);
            }
        });
    }

    fn render_entry_row(&mut self, ui: &mut egui::Ui, idx: usize) {
        let is_selected = idx == self.selected_index;
        let entry = self.entries[idx].clone();

        let bg = if is_selected {
            theme::BG_SELECTED
        } else {
            theme::BG_MEDIUM
        };

        let frame = egui::Frame::new()
            .fill(bg)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .corner_radius(4);

        let resp = frame
            .show(ui, |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        match &entry.kind {
                            EntryKind::Live => {
                                ui.colored_label(
                                    theme::status_color(caco_core::db::Status::InProgress),
                                    egui::RichText::new(&entry.label).strong(),
                                );
                            }
                            EntryKind::Completion(_) => {
                                ui.label(
                                    egui::RichText::new(display_date(&entry.raw_date)).strong(),
                                );
                            }
                        }

                        if !entry.notes.is_empty() {
                            ui.colored_label(
                                theme::TEXT_SECONDARY,
                                egui::RichText::new(format!("\u{201c}{}\u{201d}", entry.notes))
                                    .small(),
                            );
                        }

                        let (word, color) = if entry.stats.is_some() {
                            ("Has map stats", theme::TEXT_SECONDARY)
                        } else {
                            ("No map stats", theme::TEXT_MUTED)
                        };
                        ui.colored_label(color, egui::RichText::new(word).small());
                    },
                );
            })
            .response
            .interact(egui::Sense::click());

        if resp.clicked() && idx != self.selected_index {
            self.selected_index = idx;
            self.edit = None;
            self.confirm_delete = None;
        }

        ui.add_space(4.0);
    }

    fn render_right_pane(&mut self, ui: &mut egui::Ui, height: f32) {
        if self.entries.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "Nothing yet. Add a completion to record a time you finished this WAD.",
            );
            return;
        }

        // During an add-draft, suppress the stats table: no entry has been created yet, so
        // showing the previously-selected entry's stats would be misleading.
        let is_draft = matches!(
            &self.edit,
            Some(EditBuffer {
                completion_id: None,
                ..
            })
        );

        if self.edit.is_some() {
            self.render_edit_panel(ui);
            ui.add_space(8.0);
        } else {
            self.render_selected_header(ui);
            ui.add_space(6.0);
        }

        if is_draft {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                egui::RichText::new("You can import map stats for it once it is saved.").small(),
            );
            return;
        }

        self.render_stats_table(ui, height);
    }

    fn render_selected_header(&self, ui: &mut egui::Ui) {
        let Some(entry) = self.entries.get(self.selected_index) else {
            return;
        };
        ui.horizontal(|ui| match &entry.kind {
            EntryKind::Live => {
                ui.colored_label(
                    theme::status_color(caco_core::db::Status::InProgress),
                    egui::RichText::new(&entry.label).strong(),
                );
            }
            EntryKind::Completion(_) => {
                ui.label(egui::RichText::new(display_date(&entry.raw_date)).strong());
                if !entry.notes.is_empty() {
                    ui.colored_label(theme::TEXT_SECONDARY, format!("\u{00b7} {}", entry.notes));
                }
            }
        });
    }

    fn render_edit_panel(&mut self, ui: &mut egui::Ui) {
        let Some(buf) = self.edit.as_mut() else {
            return;
        };
        let header = if buf.completion_id.is_some() {
            "Editing completion"
        } else {
            "Adding completion"
        };
        egui::Frame::new()
            .fill(theme::BG_MEDIUM)
            .stroke(egui::Stroke::new(1.0_f32, theme::CONTROL_BORDER))
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                theme::section_heading(ui, header);
                ui.add_space(4.0);
                egui::Grid::new("wad_stats_edit_grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        let label = form_label(ui, "Date");
                        ui.text_edit_singleline(&mut buf.date).labelled_by(label.id);
                        ui.end_row();
                        let label = form_label(ui, "Notes");
                        ui.text_edit_singleline(&mut buf.notes)
                            .labelled_by(label.id);
                        ui.end_row();
                    });
            });
    }

    /// What has gone wrong, and what is being asked.
    fn render_notes(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(theme::COLOR_ERROR, error);
        }
        if self.confirm_delete.is_some() {
            ui.colored_label(
                theme::COLOR_ERROR,
                "Delete this completion and its map stats?",
            );
        }
    }

    /// What can be done to the selected entry, of which nothing is offered that couldn't be
    /// done to it. Returns true if Close was pressed.
    fn render_footer(
        &mut self,
        ctx: &egui::Context,
        row: &mut FooterRow<'_>,
        conn: &Connection,
        result: &mut WadStatsResult,
    ) -> bool {
        if self.edit.is_some() {
            let ended = row.end(Some("Cancel"), Some(Primary::new("Save")));
            if ended.dismissed {
                self.edit = None;
            }
            if ended.primary {
                self.handle_save(conn, result);
            }
            return false;
        }
        if let Some(id) = self.confirm_delete {
            let ended = row.end(Some("Cancel"), Some(Primary::danger("Delete")));
            if ended.dismissed {
                self.confirm_delete = None;
            }
            if ended.primary {
                self.handle_delete(conn, id, result);
            }
            return false;
        }

        let completion = match self.entries.get(self.selected_index).map(|e| &e.kind) {
            Some(EntryKind::Completion(id)) => Some(*id),
            _ => None,
        };
        let selected_stats = self
            .entries
            .get(self.selected_index)
            .and_then(|e| e.stats.clone());

        row.left(|ui| {
            if ui.button("Add Completion").clicked() {
                self.handle_add();
            }
            if let Some(id) = completion {
                if ui.button("Edit").clicked() {
                    self.begin_edit();
                }
                if ui.button("Delete").clicked() {
                    self.confirm_delete = Some(id);
                }
            }
            if ui
                .add_enabled(
                    self.pending_import.is_none(),
                    egui::Button::new("Import Stats\u{2026}"),
                )
                .on_hover_text("From a stats.txt or levelstat.txt")
                .clicked()
                && let Some(target) = self.current_import_target()
            {
                self.pending_import = Some((target, spawn_import_picker(ctx)));
            }
            if let Some(stats) = &selected_stats {
                if ui
                    .add_enabled(
                        self.pending_export.is_none(),
                        egui::Button::new("Export Stats\u{2026}"),
                    )
                    .clicked()
                {
                    let rx = spawn_export_picker(ctx, stats);
                    self.pending_export = Some((stats.clone(), rx));
                }
                if completion.is_some() && ui.button("Clear Stats").clicked() {
                    self.handle_clear(conn, result);
                }
            }
        });
        row.close()
    }

    fn render_stats_table(&self, ui: &mut egui::Ui, height: f32) {
        let Some(entry) = self.entries.get(self.selected_index) else {
            return;
        };
        let Some(stats) = &entry.stats else {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No map stats for this one yet. They appear once you've finished a map, or you \
                 can import some.",
            );
            return;
        };

        // Single source of truth for "what counts as a map":
        //   - With analysis: every classified map (Required + OptionalSecret +
        //     OptionalCredits), with stats overlaid by lump name. Unplayed
        //     maps render with "—" placeholders so the user sees what they
        //     still owe.
        //   - Without analysis: fall back to "maps the player has actually
        //     exited" + a banner explaining the gap.
        let rows = build_table_rows(self.analysis.as_ref(), stats);
        let has_analysis = self.analysis.is_some();
        if !has_analysis {
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new(
                    "caco hasn't worked out this WAD's list of maps yet, so this only shows the \
                     ones you've finished. Open this again in a moment for the rest.",
                )
                .small(),
            );
            ui.add_space(4.0);
        }

        let text_height = ui.text_style_height(&egui::TextStyle::Body);
        let row_height = text_height + 6.0;

        // Less the line above it, the header row, and the summary below.
        let table = dialog_table(ui, height - row_height * 2.0 - 40.0)
            // What fits beside the list of playthroughs in the smallest window, with the last
            // taking whatever a bigger one has.
            .column(Column::exact(56.0).clip(true))
            .column(Column::exact(60.0).clip(true))
            .column(Column::exact(36.0).clip(true))
            .column(Column::exact(48.0).clip(true))
            .column(Column::exact(58.0).clip(true))
            .column(Column::exact(48.0).clip(true))
            .column(Column::remainder().at_least(40.0).clip(true));

        let rows_ref = &rows;
        let stats_format = stats.format.clone();
        table
            .header(row_height + 2.0, |mut header| {
                for label in ["Map", "Kind", "Skill", "Time", "Kills", "Items", "Secrets"] {
                    header.col(|ui| {
                        ui.label(theme::column_header(label));
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, rows_ref.len(), |mut row| {
                    let r = &rows_ref[row.index()];
                    row.col(|ui| {
                        ui.label(&r.lump);
                    });
                    row.col(|ui| {
                        ui.colored_label(class_color(r.class), class_label(r.class));
                    });
                    match r.stats {
                        Some(m) => {
                            row.col(|ui| {
                                ui.label(caco_core::wad_stats::skill_name(m.best_skill));
                            });
                            row.col(|ui| {
                                ui.label(format_map_time(m, &stats_format));
                            });
                            row.col(|ui| {
                                ui.label(ratio(m.kills, m.total_kills));
                            });
                            row.col(|ui| {
                                ui.label(ratio(m.items, m.total_items));
                            });
                            row.col(|ui| {
                                ui.label(ratio(m.secrets, m.total_secrets));
                            });
                        }
                        None => {
                            for _ in 0..5 {
                                row.col(|ui| {
                                    ui.colored_label(theme::TEXT_MUTED, "\u{2014}");
                                });
                            }
                        }
                    }
                });
            });

        ui.add_space(4.0);
        ui.colored_label(
            theme::TEXT_SECONDARY,
            egui::RichText::new(format_summary(self.analysis.as_ref(), stats, &rows)).small(),
        );
    }

    fn completion_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::Completion(_)))
            .count()
    }

    fn current_import_target(&self) -> Option<PendingImportTarget> {
        self.entries.get(self.selected_index).map(|e| match e.kind {
            EntryKind::Live => PendingImportTarget::Live,
            EntryKind::Completion(id) => PendingImportTarget::Completion(id),
        })
    }

    fn begin_edit(&mut self) {
        let Some(entry) = self.entries.get(self.selected_index) else {
            return;
        };
        if let EntryKind::Completion(id) = entry.kind {
            self.edit = Some(EditBuffer {
                completion_id: Some(id),
                date: entry.raw_date.clone(),
                notes: entry.notes.clone(),
            });
            self.confirm_delete = None;
            self.error = None;
        }
    }

    /// Open an edit draft for a brand-new completion. No DB write happens
    /// until Save; Cancel discards without side effects.
    fn handle_add(&mut self) {
        self.edit = Some(EditBuffer {
            completion_id: None,
            date: Utc::now().to_rfc3339(),
            notes: String::new(),
        });
        self.confirm_delete = None;
        self.error = None;
    }

    fn handle_save(&mut self, conn: &Connection, result: &mut WadStatsResult) {
        let Some(buf) = self.edit.take() else {
            return;
        };
        let date = buf.date.trim();
        if date.is_empty() {
            self.error = Some("Date cannot be empty.".to_string());
            self.edit = Some(buf);
            return;
        }
        let notes_opt = if buf.notes.trim().is_empty() {
            None
        } else {
            Some(buf.notes.as_str())
        };
        match buf.completion_id {
            Some(id) => match caco_core::db::sessions::update_wad_completion(
                conn,
                id,
                None,
                Some(notes_opt),
                Some(date),
            ) {
                Ok(_) => {
                    self.reload_entries(conn);
                    *result = WadStatsResult::Modified;
                }
                Err(e) => {
                    self.error = Some(format!("Save failed: {e}"));
                    self.edit = Some(buf);
                }
            },
            None => {
                match caco_core::db::sessions::add_wad_completion(
                    conn,
                    self.wad_id,
                    None,
                    notes_opt,
                    Some(date),
                ) {
                    Ok(new_id) => {
                        self.reload_entries(conn);
                        if let Some(idx) = self.entries.iter().position(
                            |e| matches!(e.kind, EntryKind::Completion(id) if id == new_id),
                        ) {
                            self.selected_index = idx;
                        }
                        *result = WadStatsResult::Modified;
                    }
                    Err(e) => {
                        self.error = Some(format!("Add failed: {e}"));
                        self.edit = Some(buf);
                    }
                }
            }
        }
    }

    fn handle_delete(&mut self, conn: &Connection, id: i64, result: &mut WadStatsResult) {
        match caco_core::db::sessions::delete_wad_completion(conn, id) {
            Ok(_) => {
                self.confirm_delete = None;
                self.reload_entries(conn);
                *result = WadStatsResult::Modified;
            }
            Err(e) => self.error = Some(format!("Delete failed: {e}")),
        }
    }

    fn handle_clear(&mut self, conn: &Connection, result: &mut WadStatsResult) {
        let Some(entry) = self.entries.get(self.selected_index) else {
            return;
        };
        let EntryKind::Completion(id) = entry.kind else {
            return;
        };
        match caco_core::db::sessions::update_wad_completion(conn, id, Some(None), None, None) {
            Ok(_) => {
                self.reload_entries(conn);
                *result = WadStatsResult::Modified;
            }
            Err(e) => self.error = Some(format!("Clear failed: {e}")),
        }
    }

    /// Returns `true` if DB state changed.
    fn drain_import_picker(&mut self, conn: &Connection) -> bool {
        let Some((target, rx)) = &self.pending_import else {
            return false;
        };
        let picked = match rx.try_recv() {
            Ok(p) => p,
            Err(_) => return false,
        };
        let target = *target;
        self.pending_import = None;

        let Some(path) = picked else {
            return false;
        };

        let stats = match caco_core::wad_stats::parse_stats_file(&path) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(format!("Parse failed: {e}"));
                return false;
            }
        };
        let json = match caco_core::wad_stats::stats_to_json(&stats) {
            Ok(j) => j,
            Err(e) => {
                self.error = Some(format!("Serialize failed: {e}"));
                return false;
            }
        };

        let ok = match target {
            PendingImportTarget::Live => {
                let update =
                    caco_core::db::wads::WadUpdate::new().set_text("stats_snapshot", Some(json));
                caco_core::db::wads::update_wad(conn, self.wad_id, &update).is_ok()
            }
            PendingImportTarget::Completion(id) => caco_core::db::sessions::update_wad_completion(
                conn,
                id,
                Some(Some(&json)),
                None,
                None,
            )
            .is_ok(),
        };

        if ok {
            self.reload_entries(conn);
        } else {
            self.error = Some("Import failed writing to DB.".to_string());
        }
        ok
    }

    fn drain_export_picker(&mut self) {
        let Some((_, rx)) = &self.pending_export else {
            return;
        };
        let picked = match rx.try_recv() {
            Ok(p) => p,
            Err(_) => return,
        };
        let Some((stats, _)) = self.pending_export.take() else {
            return;
        };
        if let Some(path) = picked {
            let text = caco_core::wad_stats::format_stats(&stats);
            if let Err(e) = std::fs::write(&path, text) {
                self.error = Some(format!("Export failed: {e}"));
            }
        }
    }
}

fn spawn_import_picker(ctx: &egui::Context) -> FileDialogReceiver {
    let req = FileDialogRequest::open()
        .add_filter("Stats files", &["txt"])
        .set_directory(dirs::home_dir().unwrap_or_default());
    spawn_file_dialog(Some(ctx.clone()), req)
}

fn spawn_export_picker(ctx: &egui::Context, stats: &StatsData) -> FileDialogReceiver {
    let default_name = if stats.format == "levelstat_txt" {
        "levelstat.txt"
    } else {
        "stats.txt"
    };
    let req = FileDialogRequest::save()
        .add_filter("Stats files", &["txt"])
        .set_directory(dirs::home_dir().unwrap_or_default())
        .set_file_name(default_name);
    spawn_file_dialog(Some(ctx.clone()), req)
}

fn ratio(value: i32, total: i32) -> String {
    if total >= 0 {
        format!("{value}/{total}")
    } else {
        value.to_string()
    }
}

fn format_map_time(m: &caco_core::wad_stats::MapStats, format: &str) -> String {
    if format == "stats_txt" {
        caco_core::wad_stats::format_time_tics(m.best_time)
    } else {
        caco_core::wad_stats::format_time_secs(m.time_secs)
    }
}

/// "2026-03-15 13:00", in this machine's time. As it is stored if it can't be read, since
/// a date is a text field here.
fn display_date(raw: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| raw.to_string())
}

// ---------------------------------------------------------------------------
// Per-map table model
//
// `RowClass::Played` is used in the no-analysis fallback so the dialog can
// still render meaningfully when a fresh `WadAnalysis` isn't cached yet (and
// the background re-analysis worker hasn't caught up). Once the analysis
// arrives, every row carries the classifier's verdict instead.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum RowClass {
    Required,
    Secret,
    Terminal,
    /// Fallback when no `WadAnalysis` exists yet — we can't classify, so
    /// just label the row as a "played" map.
    Played,
}

struct MapRow<'a> {
    lump: String,
    class: RowClass,
    stats: Option<&'a caco_core::wad_stats::MapStats>,
}

fn build_table_rows<'a>(analysis: Option<&AnalysisData>, stats: &'a StatsData) -> Vec<MapRow<'a>> {
    let stats_by_lump: std::collections::HashMap<&str, &caco_core::wad_stats::MapStats> =
        stats.maps.iter().map(|m| (m.lump.as_str(), m)).collect();

    match analysis {
        Some(a) => a
            .maps
            .iter()
            .filter_map(|m| {
                let class = match m.classification {
                    MapClassification::Required => RowClass::Required,
                    MapClassification::OptionalSecret => RowClass::Secret,
                    MapClassification::OptionalCredits => RowClass::Terminal,
                    MapClassification::Unreachable => return None,
                };
                Some(MapRow {
                    lump: m.lump.clone(),
                    class,
                    stats: stats_by_lump.get(m.lump.as_str()).copied(),
                })
            })
            .collect(),
        None => stats
            .played_maps()
            .into_iter()
            .map(|m| MapRow {
                lump: m.lump.clone(),
                class: RowClass::Played,
                stats: Some(m),
            })
            .collect(),
    }
}

fn class_label(c: RowClass) -> &'static str {
    match c {
        RowClass::Required => "Required",
        RowClass::Secret => "Secret",
        RowClass::Terminal => "Credits",
        RowClass::Played => "Played",
    }
}

fn class_color(c: RowClass) -> egui::Color32 {
    match c {
        RowClass::Required => theme::TEXT_PRIMARY,
        RowClass::Secret => theme::GOLD,
        RowClass::Terminal => theme::TEXT_SECONDARY,
        RowClass::Played => theme::TEXT_SECONDARY,
    }
}

fn format_summary(
    analysis: Option<&AnalysisData>,
    stats: &StatsData,
    rows: &[MapRow<'_>],
) -> String {
    let exited: std::collections::HashSet<&str> = stats
        .maps
        .iter()
        .filter(|m| m.total_exits >= 1)
        .map(|m| m.lump.as_str())
        .collect();

    match analysis {
        Some(_) => {
            let mut req_total = 0usize;
            let mut req_done = 0usize;
            let mut sec_total = 0usize;
            let mut sec_done = 0usize;
            let mut term_total = 0usize;
            let mut term_done = 0usize;
            for r in rows {
                let done = exited.contains(r.lump.as_str()) as usize;
                match r.class {
                    RowClass::Required => {
                        req_total += 1;
                        req_done += done;
                    }
                    RowClass::Secret => {
                        sec_total += 1;
                        sec_done += done;
                    }
                    RowClass::Terminal => {
                        term_total += 1;
                        term_done += done;
                    }
                    RowClass::Played => {}
                }
            }
            let mut summary = format!("Required maps: {req_done} of {req_total}");
            if sec_total > 0 {
                summary.push_str(&format!("  \u{00b7}  Secret: {sec_done} of {sec_total}"));
            }
            if term_total > 0 {
                summary.push_str(&format!("  \u{00b7}  Credits: {term_done} of {term_total}"));
            }
            summary.push_str(&format!(
                "  \u{00b7}  Total time: {}",
                stats.total_time_display()
            ));
            summary
        }
        None => format!(
            "Maps finished: {}  \u{00b7}  Total time: {}",
            stats.played_maps().len(),
            stats.total_time_display(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_that_cannot_be_read_is_shown_as_it_is() {
        assert_eq!(display_date("yesterday"), "yesterday");
        assert_eq!(display_date("2026-06-24 06:30:53"), "2026-06-24 06:30:53");
        assert!(display_date("2026-06-24T12:30:53+00:00").starts_with("2026-06-2"));
    }
}
