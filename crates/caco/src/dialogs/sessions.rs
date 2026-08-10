use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use crate::theme;

/// State for the session history dialog.
pub struct SessionsDialogState {
    wad_title: String,
    sessions: Vec<SessionRow>,
}

struct SessionRow {
    date: String,
    time: String,
    duration: String,
    sourceport: String,
    maps: String,
    status_text: String,
    crashed: bool,
}

/// Result of showing the sessions dialog.
pub enum SessionsResult {
    Open,
    Closed,
}

impl SessionsDialogState {
    /// Create a new sessions dialog, loading session history from the DB.
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let wad = caco_core::db::wads::get_wad(conn, wad_id, false).ok()??;
        let sessions = caco_core::db::sessions::get_sessions(conn, wad_id).ok()?;

        let rows: Vec<SessionRow> = sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                let (date, time) = format_session_date_time(&s.started_at);

                // Duration
                let duration = match s.duration_seconds {
                    Some(d) if d > 0 => caco_core::player::format_duration(d),
                    _ => "\u{2014}".to_string(),
                };

                // Sourceport
                let sourceport = s.sourceport.clone().unwrap_or_default();

                // Maps played (from stats_before/stats_after delta). For older
                // rows recorded without stats_before, fall back to the next
                // older session's after snapshot because sessions are loaded
                // newest-first.
                let fallback_before = sessions
                    .get(idx + 1)
                    .and_then(|prev| prev.stats_after.as_deref());
                let maps = compute_maps_played(
                    s.stats_before.as_deref(),
                    fallback_before,
                    s.stats_after.as_deref(),
                );

                // Crash status
                let (status_text, crashed) = match s.exit_code {
                    Some(0) => ("OK".to_string(), false),
                    Some(code) => (format!("Crash ({code})"), true),
                    None => ("\u{2014}".to_string(), false),
                };

                SessionRow {
                    date,
                    time,
                    duration,
                    sourceport,
                    maps,
                    status_text,
                    crashed,
                }
            })
            .collect();

        Some(Self {
            wad_title: wad.title,
            sessions: rows,
        })
    }

    /// Render the sessions dialog. Returns the dialog result.
    pub fn render(&self, ctx: &egui::Context) -> SessionsResult {
        // Use egui's built-in title-bar close button via `.open(&mut bool)`
        // instead of adding a footer with a custom Close button — a custom
        // footer requires reserving space from `ui.available_*`, which
        // interacts badly with the Window's Resize widget and produces either
        // dead space or runaway growth.
        let mut open = true;

        egui::Window::new(format!("Sessions \u{2014} {}", self.wad_title))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([700.0, 450.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if self.sessions.is_empty() {
                    ui.colored_label(theme::TEXT_SECONDARY, "No play sessions recorded.");
                    return;
                }

                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    format!(
                        "{} session{}",
                        self.sessions.len(),
                        if self.sessions.len() == 1 { "" } else { "s" }
                    ),
                );
                ui.add_space(4.0);

                let text_height = ui.text_style_height(&egui::TextStyle::Body);
                let row_height = text_height + 6.0;

                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::initial(90.0).at_least(70.0)) // Date
                    .column(Column::initial(55.0).at_least(45.0)) // Time
                    .column(Column::initial(75.0).at_least(50.0)) // Duration
                    .column(Column::initial(100.0).at_least(60.0)) // Sourceport
                    .column(Column::remainder().at_least(100.0)) // Maps
                    .column(Column::initial(80.0).at_least(50.0)); // Status

                table
                    .header(row_height + 2.0, |mut header| {
                        for label in ["Date", "Time", "Duration", "Sourceport", "Maps", "Status"] {
                            header.col(|ui| {
                                ui.strong(label);
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(row_height, self.sessions.len(), |mut row| {
                            let s = &self.sessions[row.index()];
                            row.col(|ui| {
                                ui.label(&s.date);
                            });
                            row.col(|ui| {
                                ui.label(&s.time);
                            });
                            row.col(|ui| {
                                ui.label(&s.duration);
                            });
                            row.col(|ui| {
                                ui.label(&s.sourceport);
                            });
                            row.col(|ui| {
                                ui.colored_label(theme::TEXT_SECONDARY, &s.maps);
                            });
                            row.col(|ui| {
                                let color = if s.crashed {
                                    theme::COLOR_ERROR
                                } else if s.status_text == "OK" {
                                    theme::COLOR_SUCCESS
                                } else {
                                    theme::TEXT_SECONDARY
                                };
                                ui.colored_label(color, &s.status_text);
                            });
                        });
                    });
            });

        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return SessionsResult::Closed;
        }

        SessionsResult::Open
    }
}

fn format_session_date_time(ts: &str) -> (String, String) {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        let local = dt.with_timezone(&chrono::Local);
        return (
            local.format("%Y-%m-%d").to_string(),
            local.format("%H:%M").to_string(),
        );
    }

    match ts.split_once('T') {
        Some((d, t)) => (d.to_string(), t.get(..5).unwrap_or(t).to_string()),
        None => (ts.to_string(), String::new()),
    }
}

/// Compute a short "maps played" summary from stats_before/stats_after JSON.
fn compute_maps_played(
    before: Option<&str>,
    fallback_before: Option<&str>,
    after: Option<&str>,
) -> String {
    let Some(after_str) = after else {
        return "\u{2014}".to_string();
    };

    let after_stats = match caco_core::wad_stats::stats_from_json(after_str) {
        Ok(s) => s,
        Err(_) => return "\u{2014}".to_string(),
    };

    let before_stats = before
        .or(fallback_before)
        .and_then(|b| caco_core::wad_stats::stats_from_json(b).ok());
    let delta = caco_core::wad_stats::compute_stats_delta(before_stats.as_ref(), &after_stats);

    if delta.maps_played.is_empty() {
        return "\u{2014}".to_string();
    }

    let maps = &delta.maps_played;
    if maps.len() <= 3 {
        maps.join(", ")
    } else {
        format!("{}, {} + {} more", maps[0], maps[1], maps.len() - 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_json(maps: &[(&str, i32)]) -> String {
        let maps: Vec<caco_core::wad_stats::MapStats> = maps
            .iter()
            .map(|(lump, exits)| caco_core::wad_stats::MapStats {
                lump: (*lump).to_string(),
                total_exits: *exits,
                best_skill: if *exits > 0 { 4 } else { 0 },
                best_time: if *exits > 0 { 3500 } else { -1 },
                time_secs: if *exits > 0 { 100.0 } else { -1.0 },
                kills: 0,
                total_kills: -1,
                items: 0,
                total_items: -1,
                secrets: 0,
                total_secrets: -1,
                episode: 0,
                map_num: 0,
                best_max_time: -1,
                best_nm_time: -1,
                cumulative_kills: 0,
                total_time_secs: -1.0,
            })
            .collect();
        let stats = caco_core::wad_stats::WadStats {
            format: "stats_txt".to_string(),
            maps,
            version: 1,
            header_total_kills: 0,
        };
        caco_core::wad_stats::stats_to_json(&stats).unwrap()
    }

    #[test]
    fn maps_played_uses_fallback_before_when_session_before_missing() {
        let before = stats_json(&[("MAP01", 1), ("MAP02", 1), ("MAP03", 1), ("MAP04", 1)]);
        let after = stats_json(&[
            ("MAP01", 1),
            ("MAP02", 1),
            ("MAP03", 1),
            ("MAP04", 1),
            ("MAP05", 1),
        ]);

        assert_eq!(
            compute_maps_played(None, Some(&before), Some(&after)),
            "MAP05"
        );
    }

    #[test]
    fn maps_played_still_summarizes_first_cumulative_session_without_before() {
        let after = stats_json(&[
            ("MAP01", 1),
            ("MAP02", 1),
            ("MAP03", 1),
            ("MAP04", 1),
            ("MAP05", 1),
        ]);

        assert_eq!(
            compute_maps_played(None, None, Some(&after)),
            "MAP01, MAP02 + 3 more"
        );
    }
}
