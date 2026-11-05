use egui_extras::{Column, TableBuilder};

use crate::relative_time;
use crate::state::{ActionRequest, AppState};
use crate::theme;

const STATUS_COLUMN: f32 = 84.0;
const RATING_COLUMN: f32 = 80.0;
const PLAYTIME_COLUMN: f32 = 70.0;
const LAST_PLAYED_COLUMN: f32 = 88.0;

/// The same as the banner and the cards have.
const MARGIN: f32 = 20.0;

/// Below this the rating goes, to leave the title something to work with.
const NARROW: f32 = 640.0;

/// Render the WAD library table with virtual scrolling.
/// Returns an action request if a keyboard shortcut was pressed.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) -> Option<ActionRequest> {
    let mut action = None;

    // Handle keyboard shortcuts when no dialog is open and no text input is focused
    if !state.has_dialog() && !ui.ctx().egui_wants_keyboard_input() {
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

    let text_height = ui.text_style_height(&egui::TextStyle::Body);
    let row_height = text_height + 6.0;

    let mut table_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(
                ui.available_rect_before_wrap()
                    .shrink2(egui::vec2(MARGIN, 0.0)),
            )
            .layout(*ui.layout()),
    );
    let ui_outer = ui;
    let ui = &mut table_ui;

    // Widths worked out from what there is, every frame, with the title taking the rest. They
    // were a resizable table's before, which saved them from a frame when the panel had not
    // settled, so that Last Played ended up off the edge of the window.
    let width = ui.available_width();
    let (author, rating) = if width < NARROW {
        (0.22 * width, 0.0)
    } else {
        (0.2 * width, RATING_COLUMN)
    };

    let table = TableBuilder::new(ui)
        .striped(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        // The view scrolls as a whole, with the banner over it.
        .vscroll(false)
        .column(Column::remainder().at_least(120.0).clip(true)) // Title
        .column(Column::exact(author).clip(true)) // Author
        .column(Column::exact(STATUS_COLUMN)) // Status
        .column(Column::exact(rating)) // Rating
        .column(Column::exact(PLAYTIME_COLUMN)) // Playtime
        .column(Column::exact(LAST_PLAYED_COLUMN)); // Last Played

    table
        .header(row_height + 2.0, |mut header| {
            for label in [
                "Title",
                "Author",
                "Status",
                if rating > 0.0 { "Rating" } else { "" },
                "Playtime",
                "Last Played",
            ] {
                header.col(|ui| {
                    ui.colored_label(theme::TEXT_ACCENT, egui::RichText::new(label).strong());
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
                    ui.colored_label(
                        theme::status_color(wad.status),
                        theme::status_display(wad.status),
                    );
                });

                // Rating
                row.col(|ui| {
                    let stars = theme::rating_stars(wad.rating);
                    if rating > 0.0 && !stars.is_empty() {
                        ui.colored_label(theme::TEXT_ACCENT, stars);
                    }
                });

                // Playtime
                row.col(|ui| {
                    let playtime_str = match stats {
                        Some(s) if s.playtime > 0 => caco_core::player::format_duration(s.playtime),
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

                // Row-level click selection + context menu
                let response = row.response();
                if response.clicked() || response.secondary_clicked() {
                    state.selected_row = idx;
                    state.selected_wad_id = Some(wad_id);
                }
                if response.double_clicked() {
                    action = Some(ActionRequest::Play(wad_id));
                }
                let wad = &state.wads[idx];
                if let Some(a) =
                    super::wad_context_menu(&response, wad_id, wad.status, wad.is_lost())
                {
                    action = Some(a);
                }
            });
        });

    // The table was laid out in a child, which the view scrolling around it has to be told of.
    let used = table_ui.min_rect();
    ui_outer.advance_cursor_after_rect(used);

    action
}
