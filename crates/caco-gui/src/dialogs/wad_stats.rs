use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use crate::theme;

// Use a type alias to disambiguate from db::sessions::WadStats
type StatsData = caco_core::wad_stats::WadStats;

/// A stats entry: either the current live snapshot or a historical completion.
struct StatsEntry {
    label: String,
    stats: StatsData,
}

/// State for the WAD map stats dialog.
pub struct WadStatsDialogState {
    wad_title: String,
    wad_id: i64,
    entries: Vec<StatsEntry>,
    selected_index: usize,
}

/// Result of showing the WAD stats dialog.
pub enum WadStatsResult {
    Open,
    Closed,
    /// Stats were imported — caller should reload
    Modified,
}

impl WadStatsDialogState {
    /// Create a new WAD stats dialog, loading stats from DB.
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let wad = caco_core::db::wads::get_wad(conn, wad_id, false).ok()??;

        let mut entries = Vec::new();

        // Current (live) stats from wad.stats_snapshot
        if let Some(ref snapshot_json) = wad.stats_snapshot
            && let Ok(stats) = caco_core::wad_stats::stats_from_json(snapshot_json)
        {
            entries.push(StatsEntry {
                label: "Current (live)".to_string(),
                stats,
            });
        }

        // Historical completions
        if let Ok(completions) = caco_core::db::sessions::get_wad_completions(conn, wad_id) {
            for comp in completions {
                if let Some(ref snapshot_json) = comp.stats_snapshot
                    && let Ok(stats) = caco_core::wad_stats::stats_from_json(snapshot_json)
                {
                    let date = comp.completed_at.get(..10).unwrap_or(&comp.completed_at);
                    entries.push(StatsEntry {
                        label: format!("Completion {date}"),
                        stats,
                    });
                }
            }
        }

        Some(Self {
            wad_title: wad.title,
            wad_id,
            entries,
            selected_index: 0,
        })
    }

    /// Render the WAD stats dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> WadStatsResult {
        let mut result = WadStatsResult::Open;

        egui::Window::new(format!("Map Stats \u{2014} {}", self.wad_title))
            .collapsible(false)
            .resizable(true)
            .default_size([650.0, 500.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if self.entries.is_empty() {
                    ui.colored_label(theme::TEXT_SECONDARY, "No stats available");
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        if ui.button("Import").clicked() && self.do_import(conn) {
                            result = WadStatsResult::Modified;
                        }
                        if ui.button("Close").clicked() {
                            result = WadStatsResult::Closed;
                        }
                    });
                    return;
                }

                // Dropdown selector
                ui.horizontal(|ui| {
                    ui.label("View:");
                    let current_label = self
                        .entries
                        .get(self.selected_index)
                        .map(|e| e.label.as_str())
                        .unwrap_or("\u{2014}");
                    egui::ComboBox::from_id_salt("stats_selector")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for (i, entry) in self.entries.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_index, i, &entry.label);
                            }
                        });
                });

                ui.add_space(4.0);

                let stats = &self.entries[self.selected_index].stats;
                let played = stats.played_maps();

                // Stats table
                let text_height = ui.text_style_height(&egui::TextStyle::Body);
                let row_height = text_height + 6.0;

                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::initial(70.0).at_least(50.0)) // Map
                    .column(Column::initial(50.0).at_least(40.0)) // Skill
                    .column(Column::initial(70.0).at_least(50.0)) // Time
                    .column(Column::initial(80.0).at_least(60.0)) // Kills
                    .column(Column::initial(80.0).at_least(60.0)) // Items
                    .column(Column::remainder().at_least(60.0)); // Secrets

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

                // Summary row
                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        format!(
                            "Format: {}  |  Maps played: {}  |  Total time: {}",
                            format_name(&stats.format),
                            played.len(),
                            stats.total_time_display(),
                        ),
                    );
                });

                ui.add_space(8.0);

                // Clone stats for export (avoids borrow conflict with &mut self)
                let export_stats = stats.clone();

                // Action buttons
                ui.horizontal(|ui| {
                    if ui.button("Import").clicked() && self.do_import(conn) {
                        result = WadStatsResult::Modified;
                    }
                    if ui.button("Export").clicked() {
                        do_export(&export_stats);
                    }
                    if ui.button("Close").clicked() {
                        result = WadStatsResult::Closed;
                    }
                });
            });

        // Escape closes
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return WadStatsResult::Closed;
        }

        result
    }

    /// Import stats from a file picker.
    fn do_import(&mut self, conn: &Connection) -> bool {
        let dir = dirs::home_dir().unwrap_or_default();
        let path = rfd::FileDialog::new()
            .add_filter("Stats files", &["txt"])
            .set_directory(&dir)
            .pick_file();

        let Some(path) = path else { return false };

        let stats = match caco_core::wad_stats::parse_stats_file(&path) {
            Ok(s) => s,
            Err(_) => return false,
        };

        let json = match caco_core::wad_stats::stats_to_json(&stats) {
            Ok(j) => j,
            Err(_) => return false,
        };

        // Attach to most recent completion
        let completions =
            caco_core::db::sessions::get_wad_completions(conn, self.wad_id).unwrap_or_default();

        if let Some(latest) = completions.first() {
            let _ =
                caco_core::db::sessions::update_wad_completion(conn, latest.id, Some(&json), None);
        }

        // Also update the WAD's stats_snapshot
        if let Ok(update) =
            caco_core::db::wads::WadUpdate::new().set_text("stats_snapshot", Some(json))
        {
            let _ = caco_core::db::wads::update_wad(conn, self.wad_id, &update);
        }

        // Rebuild entries from DB
        self.reload_entries(conn);
        true
    }

    /// Reload entries from the database.
    fn reload_entries(&mut self, conn: &Connection) {
        self.entries.clear();

        if let Ok(Some(wad)) = caco_core::db::wads::get_wad(conn, self.wad_id, false)
            && let Some(ref sj) = wad.stats_snapshot
            && let Ok(s) = caco_core::wad_stats::stats_from_json(sj)
        {
            self.entries.push(StatsEntry {
                label: "Current (live)".to_string(),
                stats: s,
            });
        }

        if let Ok(comps) = caco_core::db::sessions::get_wad_completions(conn, self.wad_id) {
            for comp in comps {
                if let Some(ref sj) = comp.stats_snapshot
                    && let Ok(s) = caco_core::wad_stats::stats_from_json(sj)
                {
                    let d = comp.completed_at.get(..10).unwrap_or(&comp.completed_at);
                    self.entries.push(StatsEntry {
                        label: format!("Completion {d}"),
                        stats: s,
                    });
                }
            }
        }

        self.selected_index = 0;
    }
}

/// Export stats to a file via save-file picker.
fn do_export(stats: &StatsData) {
    let dir = dirs::home_dir().unwrap_or_default();
    let default_name = if stats.format == "levelstat_txt" {
        "levelstat.txt"
    } else {
        "stats.txt"
    };
    let path = rfd::FileDialog::new()
        .add_filter("Stats files", &["txt"])
        .set_directory(&dir)
        .set_file_name(default_name)
        .save_file();

    if let Some(path) = path {
        let text = caco_core::wad_stats::format_stats(stats);
        let _ = std::fs::write(path, text);
    }
}

/// Format a ratio like "45/50" or just the value if total is unknown.
fn ratio(value: i32, total: i32) -> String {
    if total >= 0 {
        format!("{value}/{total}")
    } else {
        value.to_string()
    }
}

/// Format map time depending on stats format.
fn format_map_time(m: &caco_core::wad_stats::MapStats, format: &str) -> String {
    if format == "stats_txt" {
        caco_core::wad_stats::format_time_tics(m.best_time)
    } else {
        caco_core::wad_stats::format_time_secs(m.time_secs)
    }
}

/// Human-readable format name.
fn format_name(format: &str) -> &str {
    match format {
        "stats_txt" => "stats.txt",
        "levelstat_txt" => "levelstat.txt",
        _ => format,
    }
}
