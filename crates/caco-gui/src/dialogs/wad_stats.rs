//! Map-stats & completions manager dialog.
//!
//! Two-pane layout: left pane lists the live snapshot + historical completions;
//! right pane renders the selected entry's per-map stats table. Users can add,
//! edit (notes + date), or delete completion rows, and import/export/clear the
//! stats snapshot attached to any entry.

use chrono::Utc;
use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use crate::theme;
use crate::workers::{FileDialogReceiver, FileDialogRequest, spawn_file_dialog};

type StatsData = caco_core::wad_stats::WadStats;

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

        if let Ok(Some(wad)) = caco_core::db::wads::get_wad(conn, self.wad_id, false) {
            let stats = wad
                .stats_snapshot
                .as_deref()
                .and_then(|s| caco_core::wad_stats::stats_from_json(s).ok());
            self.entries.push(Entry {
                kind: EntryKind::Live,
                label: "Current (live)".to_string(),
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

        // Drain file pickers from previous frames.
        if self.drain_import_picker(conn) {
            result = WadStatsResult::Modified;
        }
        self.drain_export_picker();

        let mut close_requested = false;

        // Fully fixed geometry. `default_size` + `resizable(true)` interacts
        // badly with our anchor-centered layout: egui's Resize grows
        // `desired_size` to match `last_content_size` every frame and never
        // shrinks, so any transient oversize gets locked in and subsequent
        // user drags fight the anchor. Fixed sizing gives a predictable
        // dialog that sits inside the viewport on any display. Widths and
        // pane heights are also constants so nothing depends on
        // `ui.available_*`, which returns unbounded values during egui's
        // sizing pass.
        const LEFT_PANE_W: f32 = 300.0;
        const RIGHT_PANE_W: f32 = 620.0;
        const DIALOG_W: f32 = 960.0;
        let screen_h = ctx.screen_rect().height();
        let dialog_h = 580.0f32.min((screen_h - 60.0).max(400.0));
        let pane_h = (dialog_h - 90.0).max(240.0);

        egui::Window::new(format!("Map Stats \u{2014} {}", self.wad_title))
            .id(egui::Id::new("wad_stats_dialog"))
            .collapsible(false)
            .fixed_size([DIALOG_W, dialog_h])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if let Some(err) = self.error.clone() {
                    ui.horizontal(|ui| {
                        ui.colored_label(theme::COLOR_ERROR, &err);
                        if ui.small_button("\u{2715}").clicked() {
                            self.error = None;
                        }
                    });
                    ui.add_space(4.0);
                }

                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(LEFT_PANE_W, pane_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            self.render_left_pane(ui, conn, &mut result);
                        },
                    );

                    ui.separator();

                    ui.allocate_ui_with_layout(
                        egui::vec2(RIGHT_PANE_W, pane_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            self.render_right_pane(ui, ctx, conn, &mut result);
                        },
                    );
                });

                ui.add_space(6.0);
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            });

        if close_requested || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return WadStatsResult::Closed;
        }

        result
    }

    fn render_left_pane(
        &mut self,
        ui: &mut egui::Ui,
        conn: &Connection,
        result: &mut WadStatsResult,
    ) {
        theme::section_label(ui, &format!("Completions · {}", self.completion_count()));

        egui::ScrollArea::vertical()
            .id_salt("completions_list")
            .max_height(ui.available_height() - 96.0)
            .show(ui, |ui| {
                for idx in 0..self.entries.len() {
                    self.render_entry_row(ui, idx);
                }
            });

        ui.add_space(8.0);

        // Snapshot selection-dependent data up-front so the button row can
        // freely borrow `self` mutably.
        let (is_completion, del_id) = match self.entries.get(self.selected_index).map(|e| &e.kind) {
            Some(EntryKind::Completion(id)) => (true, Some(*id)),
            _ => (false, None),
        };
        let editing = self.edit.is_some();
        let pending = self.confirm_delete;
        let armed = pending.is_some() && pending == del_id;

        ui.horizontal(|ui| {
            if ui.button("+ Add beaten").clicked() {
                self.handle_add();
            }
            if ui
                .add_enabled(is_completion && !editing, egui::Button::new("Edit"))
                .clicked()
            {
                self.begin_edit();
            }
            let label = if armed { "Confirm?" } else { "Delete" };
            let btn = egui::Button::new(egui::RichText::new(label).color(if armed {
                theme::COLOR_ERROR
            } else {
                theme::TEXT_PRIMARY
            }));
            if ui.add_enabled(del_id.is_some() && !editing, btn).clicked()
                && let Some(id) = del_id
            {
                if armed {
                    self.handle_delete(conn, id, result);
                } else {
                    self.confirm_delete = Some(id);
                }
            }
        });
        ui.colored_label(
            theme::TEXT_MUTED,
            egui::RichText::new("Actions target the selected entry.").small(),
        );
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
                                    theme::COLOR_SUCCESS,
                                    egui::RichText::new(&entry.label).strong(),
                                );
                            }
                            EntryKind::Completion(_) => {
                                let date_display = if entry.raw_date.len() >= 16 {
                                    &entry.raw_date[..16]
                                } else {
                                    &entry.raw_date
                                };
                                ui.label(egui::RichText::new(date_display).strong());
                            }
                        }

                        let note_text = if entry.notes.is_empty() {
                            "\u{2014}".to_string()
                        } else {
                            format!("\u{201c}{}\u{201d}", entry.notes)
                        };
                        if !matches!(entry.kind, EntryKind::Live) {
                            ui.colored_label(
                                theme::TEXT_SECONDARY,
                                egui::RichText::new(note_text).small(),
                            );
                        }

                        ui.horizontal(|ui| {
                            if entry.stats.is_some() {
                                ui.colored_label(theme::TEXT_ACCENT, "\u{25cf}");
                                ui.colored_label(
                                    theme::TEXT_SECONDARY,
                                    egui::RichText::new("stats").small(),
                                );
                            } else {
                                ui.colored_label(theme::TEXT_MUTED, "\u{00b7}");
                                ui.colored_label(
                                    theme::TEXT_MUTED,
                                    egui::RichText::new("no stats").small(),
                                );
                            }
                        });
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

    fn render_right_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        conn: &Connection,
        result: &mut WadStatsResult,
    ) {
        if self.entries.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No stats available. Use \u{201c}+ Add beaten\u{201d} to record a completion.",
            );
            return;
        }

        // During an add-draft, suppress the stats table and per-entry
        // actions — no entry has been created yet, so showing the
        // previously-selected entry's stats would be misleading.
        let is_draft = matches!(
            &self.edit,
            Some(EditBuffer {
                completion_id: None,
                ..
            })
        );

        if self.edit.is_some() {
            self.render_edit_panel(ui, conn, result);
            ui.add_space(8.0);
        } else {
            self.render_selected_header(ui);
            ui.add_space(6.0);
        }

        if is_draft {
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new(
                    "Stats can be attached after saving — use Import for the new entry.",
                )
                .small(),
            );
            return;
        }

        self.render_stats_table(ui);

        ui.add_space(8.0);
        ui.separator();

        // Per-entry stats actions.
        ui.horizontal(|ui| {
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new("Stats for selected").small().strong(),
            );
            ui.add_space(8.0);

            let import_busy = self.pending_import.is_some();
            if ui
                .add_enabled(!import_busy, egui::Button::new("Import\u{2026}"))
                .clicked()
                && let Some(target) = self.current_import_target()
            {
                self.pending_import = Some((target, spawn_import_picker(ctx)));
            }

            let selected_stats = self
                .entries
                .get(self.selected_index)
                .and_then(|e| e.stats.clone());
            let export_busy = self.pending_export.is_some();
            if ui
                .add_enabled(
                    selected_stats.is_some() && !export_busy,
                    egui::Button::new("Export\u{2026}"),
                )
                .clicked()
                && let Some(stats) = selected_stats
            {
                let rx = spawn_export_picker(ctx, &stats);
                self.pending_export = Some((stats, rx));
            }

            let can_clear = matches!(
                self.entries.get(self.selected_index).map(|e| &e.kind),
                Some(EntryKind::Completion(_))
            ) && self
                .entries
                .get(self.selected_index)
                .and_then(|e| e.stats.as_ref())
                .is_some();
            if ui
                .add_enabled(can_clear, egui::Button::new("Clear"))
                .clicked()
            {
                self.handle_clear(conn, result);
            }
        });
    }

    fn render_selected_header(&self, ui: &mut egui::Ui) {
        let Some(entry) = self.entries.get(self.selected_index) else {
            return;
        };
        ui.horizontal(|ui| match &entry.kind {
            EntryKind::Live => {
                ui.colored_label(
                    theme::COLOR_SUCCESS,
                    egui::RichText::new(&entry.label).strong(),
                );
                if let Some(s) = &entry.stats {
                    ui.add_space(8.0);
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        egui::RichText::new(format_name(&s.format)).small(),
                    );
                }
            }
            EntryKind::Completion(_) => {
                ui.label(egui::RichText::new(&entry.raw_date).strong());
                if !entry.notes.is_empty() {
                    ui.colored_label(theme::TEXT_SECONDARY, format!("\u{00b7} {}", entry.notes));
                }
                if let Some(s) = &entry.stats {
                    ui.add_space(8.0);
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        egui::RichText::new(format_name(&s.format)).small(),
                    );
                }
            }
        });
    }

    fn render_edit_panel(
        &mut self,
        ui: &mut egui::Ui,
        conn: &Connection,
        result: &mut WadStatsResult,
    ) {
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
            .stroke(egui::Stroke::new(1.0, theme::TEXT_ACCENT))
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.colored_label(
                    theme::TEXT_ACCENT,
                    egui::RichText::new(header).small().strong(),
                );
                ui.add_space(4.0);
                egui::Grid::new("wad_stats_edit_grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.colored_label(theme::TEXT_MUTED, "Date");
                        ui.text_edit_singleline(&mut buf.date);
                        ui.end_row();
                        ui.colored_label(theme::TEXT_MUTED, "Notes");
                        ui.text_edit_singleline(&mut buf.notes);
                        ui.end_row();
                    });
            });

        ui.add_space(6.0);
        let save_clicked;
        let cancel_clicked;
        {
            let resp = ui.horizontal(|ui| {
                let save = ui.button("Save").clicked();
                let cancel = ui.button("Cancel").clicked();
                (save, cancel)
            });
            (save_clicked, cancel_clicked) = resp.inner;
        }

        if cancel_clicked {
            self.edit = None;
        } else if save_clicked {
            self.handle_save(conn, result);
        }
    }

    fn render_stats_table(&self, ui: &mut egui::Ui) {
        let Some(entry) = self.entries.get(self.selected_index) else {
            return;
        };
        let Some(stats) = &entry.stats else {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    "No stats attached to this entry yet.",
                );
                ui.colored_label(
                    theme::TEXT_MUTED,
                    egui::RichText::new("Import a stats.txt / levelstat.txt file below.").small(),
                );
            });
            return;
        };

        let played = stats.played_maps();

        let text_height = ui.text_style_height(&egui::TextStyle::Body);
        let row_height = text_height + 6.0;

        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(70.0).at_least(50.0))
            .column(Column::initial(50.0).at_least(40.0))
            .column(Column::initial(70.0).at_least(50.0))
            .column(Column::initial(80.0).at_least(60.0))
            .column(Column::initial(80.0).at_least(60.0))
            .column(Column::remainder().at_least(60.0));

        table
            .header(row_height + 2.0, |mut header| {
                for label in ["Map", "Skill", "Time", "Kills", "Items", "Secrets"] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, played.len(), |mut row| {
                    let m = played[row.index()];
                    row.col(|ui| {
                        ui.label(&m.lump);
                    });
                    row.col(|ui| {
                        ui.label(caco_core::wad_stats::skill_name(m.best_skill));
                    });
                    row.col(|ui| {
                        ui.label(format_map_time(m, &stats.format));
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
                });
            });

        ui.add_space(4.0);
        ui.colored_label(
            theme::TEXT_SECONDARY,
            egui::RichText::new(format!(
                "Format: {}  |  Maps played: {}  |  Total time: {}",
                format_name(&stats.format),
                played.len(),
                stats.total_time_display(),
            ))
            .small(),
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

fn format_name(format: &str) -> &str {
    match format {
        "stats_txt" => "stats.txt",
        "levelstat_txt" => "levelstat.txt",
        _ => format,
    }
}
