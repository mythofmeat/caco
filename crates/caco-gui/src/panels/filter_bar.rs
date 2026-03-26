use std::time::Instant;

use crate::state::AppState;
use crate::theme;

/// Render the filter/search bar.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let response = ui.add(
        egui::TextEdit::singleline(&mut state.filter_text)
            .hint_text("Filter WADs...")
            .text_color(theme::TEXT_PRIMARY)
            .desired_width(250.0),
    );

    if response.changed() {
        state.filter_changed_at = Some(Instant::now());
    }
}
