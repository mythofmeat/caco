use caco_core::db::sessions::StatsSnapshot;
use rusqlite::Connection;

use crate::theme;

/// State for the library statistics dialog.
pub struct StatsDialogState {
    snapshot: Option<StatsSnapshot>,
}

/// Result of showing the stats dialog.
pub enum StatsResult {
    Open,
    Closed,
}

impl StatsDialogState {
    /// Create a new stats dialog, loading the stats snapshot from the DB.
    pub fn new(conn: &Connection) -> Self {
        let snapshot = caco_core::db::sessions::get_stats_snapshot(conn, "month").ok();
        Self { snapshot }
    }

    /// Render the stats dialog. Returns the dialog result.
    pub fn render(&self, ctx: &egui::Context) -> StatsResult {
        let mut result = StatsResult::Open;

        egui::Window::new("Library Statistics")
            .collapsible(false)
            .resizable(true)
            .default_size([500.0, 550.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let Some(snap) = &self.snapshot else {
                    ui.colored_label(theme::TEXT_SECONDARY, "No statistics available.");
                    if ui.button("Close").clicked() {
                        result = StatsResult::Closed;
                    }
                    return;
                };

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;

                    // Overview section
                    section_header(ui, "Overview");
                    stat_row(ui, "Total WADs", &snap.total_wads.to_string());
                    stat_row(ui, "Total Sessions", &snap.total_sessions.to_string());
                    stat_row(ui, "Total Playtime", &caco_core::player::format_duration(snap.total_playtime));
                    if snap.total_wads > 0 {
                        stat_row(
                            ui,
                            "WADs Played",
                            &format!(
                                "{} / {} ({:.0}%)",
                                snap.wads_with_sessions,
                                snap.total_wads,
                                if snap.total_wads > 0 {
                                    snap.wads_with_sessions as f64 / snap.total_wads as f64 * 100.0
                                } else {
                                    0.0
                                }
                            ),
                        );
                    }

                    ui.add_space(8.0);

                    // By Status section
                    section_header(ui, "By Status");
                    for &status in theme::STATUSES {
                        let count = snap.wads_by_status
                            .iter()
                            .find(|(s, _)| s.as_str() == status)
                            .map(|(_, c)| *c)
                            .unwrap_or(0);
                        if count > 0 {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    theme::status_color(status),
                                    theme::status_display(status),
                                );
                                ui.colored_label(theme::TEXT_SECONDARY, format!("{count}"));
                            });
                        }
                    }

                    ui.add_space(8.0);

                    // Completion section
                    section_header(ui, "Completion");
                    stat_row(ui, "Completed WADs", &snap.completed_wads.to_string());
                    stat_row(ui, "Total Completions", &snap.total_completions.to_string());
                    if snap.total_wads > 0 {
                        stat_row(
                            ui,
                            "Completion Rate",
                            &format!("{:.1}%", snap.completion_rate * 100.0),
                        );
                    }

                    ui.add_space(8.0);

                    // Monthly Activity section
                    if !snap.activity.is_empty() {
                        section_header(ui, "Monthly Activity");
                        for period in &snap.activity {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    theme::TEXT_ACCENT,
                                    egui::RichText::new(&period.period).strong(),
                                );
                                ui.colored_label(
                                    theme::TEXT_SECONDARY,
                                    format!(
                                        "{} WAD{}, {} session{}, {}",
                                        period.wad_count,
                                        if period.wad_count == 1 { "" } else { "s" },
                                        period.session_count,
                                        if period.session_count == 1 { "" } else { "s" },
                                        caco_core::player::format_duration(period.total_playtime),
                                    ),
                                );
                            });
                        }
                    }
                });
            });

        // Escape closes
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return StatsResult::Closed;
        }

        result
    }
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.separator();
    ui.colored_label(
        theme::TEXT_ACCENT,
        egui::RichText::new(title).heading().strong(),
    );
    ui.add_space(2.0);
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT_SECONDARY, format!("{label}:"));
        ui.colored_label(theme::TEXT_PRIMARY, value);
    });
}
