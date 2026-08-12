use std::time::Instant;

use crate::state::AppState;
use crate::theme;

/// ID source for the filter TextEdit (shared with app.rs for Ctrl+F focus).
pub const FILTER_ID_SOURCE: &str = "filter_input";

/// Query the at-risk chip applies. Matches `WadRecord::is_lost`.
const LOST_QUERY: &str = "retrievable:lost";

/// Render the filter/search bar.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let response = ui.add(
        egui::TextEdit::singleline(&mut state.filter.input)
            .hint_text("Filter WADs...")
            .text_color(theme::TEXT_PRIMARY)
            .desired_width(250.0)
            .id(egui::Id::new(FILTER_ID_SOURCE)),
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

    // At-risk chip: manual-only WADs with no local copy. Hidden when there
    // are none, so a healthy library carries no permanent warning — and one
    // click away when there are, which is the point of tracking it at all.
    if state.lost_count > 0 && state.filter.input != LOST_QUERY {
        let chip = ui.add(
            egui::Button::new(
                egui::RichText::new(format!(
                    "{}{} at risk",
                    theme::LOST_MARKER,
                    state.lost_count
                ))
                .color(theme::COLOR_WARNING)
                .size(11.0),
            )
            .frame(false),
        );
        if chip
            .on_hover_text(
                "WADs with no local copy that caco cannot re-download. Click to show them.",
            )
            .clicked()
        {
            state.filter.input = LOST_QUERY.to_string();
            state.filter.mark_changed(Instant::now());
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
