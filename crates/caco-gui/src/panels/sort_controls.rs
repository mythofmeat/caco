use crate::state::{AppState, SORT_FIELDS};

/// Render sort field combo box and direction toggle.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let current_label = SORT_FIELDS[state.sort_field_index].1;

    egui::ComboBox::from_id_salt("sort_field")
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

    let (arrow, tooltip) = if state.sort_desc {
        ("\u{25bc}", "Sort descending")
    } else {
        ("\u{25b2}", "Sort ascending")
    };
    if ui.button(arrow).on_hover_text(tooltip).clicked() {
        state.sort_desc = !state.sort_desc;
        state.needs_reload = true;
    }
}
