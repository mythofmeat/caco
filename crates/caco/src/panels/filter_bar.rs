use std::time::Instant;

use crate::state::AppState;
use crate::theme;

/// ID source for the filter TextEdit (shared with app.rs for Ctrl+F focus).
pub const FILTER_ID_SOURCE: &str = "filter_input";

const MIN_WIDTH: f32 = 90.0;
const MAX_WIDTH: f32 = 320.0;
const CLEAR_BUTTON_WIDTH: f32 = 24.0;

/// Render the filter/search bar.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    // Whatever the rest of the bar leaves, within reason. It is added last in a right-to-left
    // row, so what is available is what is left, and leaving a little keeps it off the title.
    let clear_room = if state.filter.input.is_empty() {
        0.0
    } else {
        CLEAR_BUTTON_WIDTH
    };
    let width = (ui.available_width() - clear_room - 12.0).clamp(MIN_WIDTH, MAX_WIDTH);
    let response = ui
        .add(
            egui::TextEdit::singleline(&mut state.filter.input)
                .hint_text("Filter WADs...")
                .text_color(theme::TEXT_PRIMARY)
                .desired_width(width)
                .id(egui::Id::unique(FILTER_ID_SOURCE)),
        )
        .accessible_name("Filter WADs")
        .on_hover_text(
            "Words look in titles, authors and descriptions. Also status:completed, tag:caco*, \
         author:skillsaw, year:2016, cacoward:2023 and ^status:abandoned.",
        );

    if response.changed() {
        state.filter.mark_changed(Instant::now());
    }

    // Clear button (only shown when there's text)
    if !state.filter.input.is_empty() {
        let clear = ui.add(
            egui::Button::new(egui::RichText::new("\u{00d7}").color(theme::TEXT_SECONDARY))
                .frame(false),
        );
        if clear.on_hover_text("Clear filter").clicked() {
            state.filter.clear(Instant::now());
        }
    }

    // Escape while focused: clear filter text
    if response.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Escape))
        && !state.filter.input.is_empty()
    {
        state.filter.clear(Instant::now());
    }
}
