use egui_extras::{Column, TableBuilder};

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

    let table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(available.y)
        .column(Column::exact(50.0)) // ID
        .column(Column::remainder().at_least(150.0)) // Title
        .column(Column::initial(150.0).at_least(80.0)) // Author
        .column(Column::initial(100.0).at_least(70.0)) // Status
        .column(Column::initial(80.0).at_least(60.0)) // Rating
        .column(Column::initial(90.0).at_least(60.0)) // Playtime
        .column(Column::initial(110.0).at_least(80.0)); // Last Played

    table
        .header(row_height + 2.0, |mut header| {
            let headers = ["ID", "Title", "Author", "Status", "Rating", "Playtime", "Last Played"];
            for label in headers {
                header.col(|ui| {
                    ui.strong(label);
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

                // Macro to handle click selection + context menu on each cell response
                macro_rules! cell_interact {
                    ($r:expr) => {
                        if $r.clicked() || $r.secondary_clicked() {
                            state.selected_row = idx;
                            state.selected_wad_id = Some(wad_id);
                        }
                        if let Some(a) = super::wad_context_menu(&$r, wad_id) {
                            action = Some(a);
                        }
                    };
                }

                // ID
                row.col(|ui| {
                    let r = ui.selectable_label(is_selected, wad_id.to_string());
                    cell_interact!(r);
                });

                // Title
                row.col(|ui| {
                    let r = ui.selectable_label(is_selected, &wad.title);
                    cell_interact!(r);
                });

                // Author
                row.col(|ui| {
                    let author = wad.author.as_deref().unwrap_or("");
                    let r = ui.selectable_label(is_selected, author);
                    cell_interact!(r);
                });

                // Status (colored)
                row.col(|ui| {
                    let label = theme::status_display(&wad.status);
                    let color = theme::status_color(&wad.status);
                    let r = ui.selectable_label(is_selected, "");
                    ui.painter().text(
                        r.rect.left_center() + egui::vec2(4.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::default(),
                        color,
                    );
                    cell_interact!(r);
                });

                // Rating
                row.col(|ui| {
                    let stars = theme::rating_stars(wad.rating);
                    let r = if !stars.is_empty() {
                        ui.colored_label(theme::TEXT_ACCENT, stars)
                    } else {
                        ui.selectable_label(is_selected, "")
                    };
                    cell_interact!(r);
                });

                // Playtime
                row.col(|ui| {
                    let playtime_str = match stats {
                        Some(s) if s.playtime > 0 => {
                            caco_core::player::format_duration(s.playtime)
                        }
                        _ => String::new(),
                    };
                    let r = ui.label(playtime_str);
                    cell_interact!(r);
                });

                // Last Played
                row.col(|ui| {
                    let last_played = stats
                        .and_then(|s| s.last_played.as_deref())
                        .and_then(|ts| ts.get(..10))
                        .unwrap_or("");
                    let r = ui.colored_label(theme::TEXT_SECONDARY, last_played);
                    cell_interact!(r);
                });
            });
        });

    action
}
