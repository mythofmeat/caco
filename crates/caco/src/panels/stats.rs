//! Library statistics — a browsing surface, not a command.
//!
//! This was a 500pt modal. Nothing in it is an action: you open it to read
//! numbers and then close it, which is what Library and Cacowards are for, so
//! it lives beside them as a view instead of blocking the app behind a window.
//!
//! Being a page rather than a 500pt column also means the content can use the
//! width: headline totals as a tile row, then the two short breakdowns beside
//! the activity list rather than stacked a scroll apart.

use caco_core::db::Status;
use caco_core::db::sessions::StatsSnapshot;

use crate::state::AppState;
use crate::theme;

/// Width below which the two-column body stacks instead.
const NARROW: f32 = 720.0;

/// Fixed columns for the By Status rows, so the bars share a baseline.
const LABEL_COLUMN: f32 = 88.0;
const COUNT_COLUMN: f32 = 40.0;

pub fn render(ui: &mut egui::Ui, state: &AppState) {
    let Some(snap) = &state.stats.snapshot else {
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.colored_label(theme::TEXT_SECONDARY, "No statistics available.");
        });
        return;
    };

    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(24, 20))
        .show(ui, |ui| {
            headline_tiles(ui, snap);
            ui.add_space(20.0);

            if ui.available_width() < NARROW {
                by_status(ui, snap);
                ui.add_space(16.0);
                completion(ui, snap);
                ui.add_space(16.0);
                activity(ui, snap);
            } else {
                // Halve the row, less the gap, so neither column claims the
                // slack — an unbalanced split reads as a bug at wide sizes.
                let column = (ui.available_width() - 24.0) / 2.0;
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(column);
                        by_status(ui, snap);
                        ui.add_space(16.0);
                        completion(ui, snap);
                    });
                    ui.add_space(24.0);
                    ui.vertical(|ui| {
                        ui.set_width(column);
                        activity(ui, snap);
                    });
                });
            }
        });
}

/// The four numbers worth seeing without reading: totals, as a tile row.
fn headline_tiles(ui: &mut egui::Ui, snap: &StatsSnapshot) {
    let played = if snap.total_wads > 0 {
        format!(
            "{:.0}%",
            snap.wads_with_sessions as f64 / snap.total_wads as f64 * 100.0
        )
    } else {
        "—".to_string()
    };

    let tiles = [
        (
            caco_core::player::format_duration(snap.total_playtime),
            "TOTAL PLAYTIME",
        ),
        (snap.total_wads.to_string(), "WADS"),
        (snap.total_sessions.to_string(), "SESSIONS"),
        (played, "LIBRARY PLAYED"),
    ];

    ui.horizontal_wrapped(|ui| {
        for (value, label) in tiles {
            tile(ui, &value, label);
        }
    });
}

fn tile(ui: &mut egui::Ui, value: &str, label: &str) {
    egui::Frame::new()
        .fill(theme::BG_MEDIUM)
        .corner_radius(8)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
        .inner_margin(egui::Margin::symmetric(18, 12))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_min_width(120.0);
                ui.colored_label(
                    theme::TEXT_PRIMARY,
                    egui::RichText::new(value).size(22.0).strong(),
                );
                ui.colored_label(theme::TEXT_MUTED, egui::RichText::new(label).size(10.0));
            });
        });
}

fn by_status(ui: &mut egui::Ui, snap: &StatsSnapshot) {
    section_header(ui, "By Status");
    // Bars rather than bare counts: the interesting question here is the
    // shape of the library, and four numbers in a column do not answer it.
    let max = Status::ALL
        .iter()
        .filter_map(|s| snap.wads_by_status.get(s.as_str()).copied())
        .max()
        .unwrap_or(0);

    for &status in Status::ALL {
        let count = snap
            .wads_by_status
            .get(status.as_str())
            .copied()
            .unwrap_or(0);
        if count == 0 {
            continue;
        }
        // Label, bar and count each own a column. Painting the label *on* the
        // bar puts it half on the fill and half off whenever the bar is short,
        // and the near-full bar then hides its own count against the fill.
        ui.horizontal(|ui| {
            ui.set_min_height(20.0);

            let (label_rect, _) =
                ui.allocate_exact_size(egui::vec2(LABEL_COLUMN, 16.0), egui::Sense::hover());
            ui.painter().text(
                egui::pos2(label_rect.min.x, label_rect.center().y),
                egui::Align2::LEFT_CENTER,
                theme::status_display(status),
                egui::FontId::proportional(11.0),
                theme::status_color(status),
            );

            let (count_rect, _) =
                ui.allocate_exact_size(egui::vec2(COUNT_COLUMN, 16.0), egui::Sense::hover());
            let bar_width = (ui.available_width().min(260.0) - 8.0).max(40.0);
            let (bar_rect, _) =
                ui.allocate_exact_size(egui::vec2(bar_width, 12.0), egui::Sense::hover());

            let frac = if max > 0 {
                count as f32 / max as f32
            } else {
                0.0
            };
            let painter = ui.painter();
            painter.text(
                egui::pos2(count_rect.max.x - 8.0, count_rect.center().y),
                egui::Align2::RIGHT_CENTER,
                count.to_string(),
                egui::FontId::proportional(11.0),
                theme::TEXT_PRIMARY,
            );
            painter.rect_filled(bar_rect, 3.0, theme::BG_MEDIUM);
            painter.rect_filled(
                egui::Rect::from_min_size(
                    bar_rect.min,
                    egui::vec2((bar_rect.width() * frac).max(2.0), bar_rect.height()),
                ),
                3.0,
                theme::status_color(status),
            );
        });
    }
}

fn completion(ui: &mut egui::Ui, snap: &StatsSnapshot) {
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
}

fn activity(ui: &mut egui::Ui, snap: &StatsSnapshot) {
    if snap.activity.is_empty() {
        return;
    }
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

fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.colored_label(
        theme::TEXT_ACCENT,
        egui::RichText::new(title).size(13.0).strong(),
    );
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(4.0);
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT_SECONDARY, format!("{label}:"));
        ui.colored_label(theme::TEXT_PRIMARY, value);
    });
}
