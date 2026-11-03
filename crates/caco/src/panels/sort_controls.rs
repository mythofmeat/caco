use crate::state::{AppState, SORT_FIELDS};
use crate::theme;

/// Render the sort controls, which read `Sort by [field] [direction]`.
///
/// The top bar lays these out from its right edge, so they are added in
/// reverse: the last one added ends up leftmost.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let (arrow, tooltip) = if state.sort_desc {
        ("\u{2193}", "Sort descending")
    } else {
        ("\u{2191}", "Sort ascending")
    };
    if ui.button(arrow).on_hover_text(tooltip).clicked() {
        state.sort_desc = !state.sort_desc;
        state.needs_reload = true;
    }

    let current_label = SORT_FIELDS[state.sort_field_index].1;
    let combo = egui::ComboBox::from_id_salt("sort_field")
        .selected_text(current_label)
        .width(100.0)
        .show_ui(ui, |ui| {
            for (i, (_key, label)) in SORT_FIELDS.iter().enumerate() {
                if ui
                    .selectable_value(&mut state.sort_field_index, i, *label)
                    .changed()
                {
                    state.needs_reload = true;
                }
            }
        });

    let label = ui.colored_label(theme::TEXT_SECONDARY, "Sort by");
    combo.response.labelled_by(label.id);
}
