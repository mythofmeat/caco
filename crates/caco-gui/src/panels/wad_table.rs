use egui_extras::{Column, TableBuilder};

use crate::state::{ActionRequest, AppState};
use crate::theme;

/// Render the WAD library table with virtual scrolling.
/// Returns an action request if a keyboard shortcut was pressed.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) -> Option<ActionRequest> {
    let mut action = None;

    // Handle keyboard shortcuts when no dialog is open and no text input is focused
    if !state.has_dialog() && !ui.ctx().wants_keyboard_input() {
        if ui.input(|i| i.key_pressed(egui::Key::J)) {
            state.select_next();
        }
        if ui.input(|i| i.key_pressed(egui::Key::K)) {
            state.select_prev();
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
                let stats = state.stats_map.get(&wad.id);

                // ID
                row.col(|ui| {
                    if ui
                        .selectable_label(is_selected, wad.id.to_string())
                        .clicked()
                    {
                        state.selected_row = idx;
                        state.selected_wad_id = Some(wad.id);
                    }
                });

                // Title
                row.col(|ui| {
                    if ui
                        .selectable_label(is_selected, &wad.title)
                        .clicked()
                    {
                        state.selected_row = idx;
                        state.selected_wad_id = Some(wad.id);
                    }
                });

                // Author
                row.col(|ui| {
                    let author = wad.author.as_deref().unwrap_or("");
                    if ui
                        .selectable_label(is_selected, author)
                        .clicked()
                    {
                        state.selected_row = idx;
                        state.selected_wad_id = Some(wad.id);
                    }
                });

                // Status (colored)
                row.col(|ui| {
                    let label = theme::status_display(&wad.status);
                    let color = theme::status_color(&wad.status);
                    let response = ui.selectable_label(is_selected, "");
                    let rect = response.rect;
                    ui.painter().text(
                        rect.left_center() + egui::vec2(4.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::default(),
                        color,
                    );
                    if response.clicked() {
                        state.selected_row = idx;
                        state.selected_wad_id = Some(wad.id);
                    }
                });

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
                        .and_then(|ts| ts.get(..10))
                        .unwrap_or("");
                    ui.colored_label(theme::TEXT_SECONDARY, last_played);
                });
            });
        });

    action
}
