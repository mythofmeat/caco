use std::time::Instant;

use crate::state::AppState;
use crate::theme;

/// ID source for the filter TextEdit (shared with app.rs for Ctrl+F focus).
pub const FILTER_ID_SOURCE: &str = "filter_input";

/// Render the filter/search bar.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let response = ui.add(
        egui::TextEdit::singleline(&mut state.filter_text)
            .hint_text("Filter WADs...")
            .text_color(theme::TEXT_PRIMARY)
            .desired_width(250.0)
            .id(egui::Id::new(FILTER_ID_SOURCE)),
    );

    if response.changed() {
        state.filter_changed_at = Some(Instant::now());
    }

    // Clear button (only shown when there's text)
    if !state.filter_text.is_empty() {
        let clear = ui.add(
            egui::Button::new(
                egui::RichText::new("\u{00d7}").color(theme::TEXT_SECONDARY),
            )
            .frame(false),
        );
        if clear.on_hover_text("Clear filter").clicked() {
            state.filter_text.clear();
            state.filter_changed_at = Some(Instant::now());
        }
    }

    // Escape while focused: clear filter text
    if response.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Escape))
        && !state.filter_text.is_empty()
    {
        state.filter_text.clear();
        state.filter_changed_at = Some(Instant::now());
    }
}
