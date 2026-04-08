use egui_extras::{Column, TableBuilder};

use crate::relative_time;
use crate::state::{ActionRequest, AppState};
use crate::theme;

/// Render the WAD library table with virtual scrolling.
/// Returns an action request if a keyboard shortcut was pressed.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) -> Option<ActionRequest> {
    let mut action = None;

    // Handle keyboard shortcuts when no dialog is open and no text input is focused
    if !state.has_dialog() && !ui.ctx().wants_keyboard_input() {
        if ui.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown)) {
            state.select_next();
        }
        if ui.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp)) {
            state.select_prev();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Home)) {
            state.select_first();
        }
        if ui.input(|i| i.key_pressed(egui::Key::End)) {
            state.select_last();
        }
        if ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::G)) {
            state.select_last();
        } else if ui.input(|i| !i.modifiers.shift && i.key_pressed(egui::Key::G)) {
            state.handle_g_press();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.selected_wad_id = None;
        }

        action = super::handle_action_keys(ui, state.selected_wad_id);
    }

    let available = ui.available_size();
    let text_height = ui.text_style_height(&egui::TextStyle::Body);
    let row_height = text_height + 6.0;

    let compact = false;

    // Proportional column widths based on available space.
    let base = (available.x - 50.0).max(0.0); // subtract fixed ID column

    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(available.y)
        .column(Column::exact(50.0)) // ID
        .column(Column::remainder().at_least(150.0)); // Title

    if compact {
        table = table
            .column(Column::initial(base * 0.25).at_least(80.0)) // Author
            .column(Column::initial(base * 0.15).at_least(60.0)); // Status
    } else {
        table = table
            .column(Column::initial(base * 0.18).at_least(80.0)) // Author
            .column(Column::initial(base * 0.12).at_least(70.0)) // Status
            .column(Column::initial(base * 0.10).at_least(60.0)) // Rating
            .column(Column::initial(base * 0.10).at_least(60.0)) // Playtime
            .column(Column::initial(base * 0.12).at_least(80.0)); // Last Played
    }

    table
        .header(row_height + 2.0, |mut header| {
            let mut headers: Vec<&str> = vec!["ID", "Title", "Author", "Status"];
            if !compact {
                headers.extend(["Rating", "Playtime", "Last Played"]);
            }
            for label in headers {
                header.col(|ui| {
                    ui.colored_label(
                        theme::TEXT_ACCENT,
                        egui::RichText::new(label).strong(),
                    );
                });
            }
        })
        .body(|body| {
            let wad_count = state.wads.len();
            body.rows(row_height, wad_count, |mut row| {
                let idx = row.index();
                let is_selected = state.selected_row == idx;

                // Highlight selected row
                row.set_selected(is_selected);

                let wad = &state.wads[idx];
                let wad_id = wad.id;
                let stats = state.stats_map.get(&wad_id);

                // ID
                row.col(|ui| {
                    ui.label(wad_id.to_string());
                });

                // Title
                row.col(|ui| {
                    ui.label(&wad.title);
                });

                // Author
                row.col(|ui| {
                    ui.label(wad.author.as_deref().unwrap_or(""));
                });

                // Status (unified, colored)
                row.col(|ui| {
                    ui.colored_label(theme::status_color(&wad.status), theme::status_display(&wad.status));
                });

                if !compact {
                    // Rating
                    row.col(|ui| {
                        let stars = theme::rating_stars(wad.rating);
                        if !stars.is_empty() {
                            ui.colored_label(theme::TEXT_ACCENT, stars);
                        }
                    });

                    // Playtime
                    row.col(|ui| {
                        let playtime_str = match stats {
                            Some(s) if s.playtime > 0 => {
                                caco_core::player::format_duration(s.playtime)
                            }
                            _ => String::new(),
                        };
                        ui.label(playtime_str);
                    });

                    // Last Played
                    row.col(|ui| {
                        let last_played = stats
                            .and_then(|s| s.last_played.as_deref())
                            .and_then(relative_time::parse_timestamp)
                            .map(|dt| relative_time::relative_time(&dt))
                            .unwrap_or_default();
                        ui.colored_label(theme::TEXT_SECONDARY, last_played);
                    });
                }

                // Row-level click selection + context menu
                let response = row.response();
                if response.clicked() || response.secondary_clicked() {
                    state.selected_row = idx;
                    state.selected_wad_id = Some(wad_id);
                }
                if let Some(a) = super::wad_context_menu(&response, wad_id) {
                    action = Some(a);
                }
            });
        });

    action
}
