use crate::import::state::{FormKind, FormState};
use crate::theme;

// ---------------------------------------------------------------------------
// Actions returned to the caller
// ---------------------------------------------------------------------------

pub enum FormPanelAction {
    Submit(Vec<(String, String)>),
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

pub fn render(ui: &mut egui::Ui, state: &mut FormState) -> Option<FormPanelAction> {
    let mut action = None;
    let is_local = state.kind == FormKind::Local;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        for field in &mut state.fields {
            let is_path_field = is_local && field.name == "path";
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&field.display_label)
                        .color(theme::TEXT_SECONDARY)
                        .monospace(),
                );
                let text_width = if is_path_field { 330.0 } else { 400.0 };
                ui.add(egui::TextEdit::singleline(&mut field.value).desired_width(text_width));
                if is_path_field && ui.button("Browse…").clicked() {
                    let mut dialog =
                        rfd::FileDialog::new().add_filter("WAD/ZIP files", &["wad", "zip"]);
                    if let Some(dir) = dirs::home_dir() {
                        dialog = dialog.set_directory(dir);
                    }
                    if let Some(path) = dialog.pick_file() {
                        field.value = path.display().to_string();
                    }
                }
            });
        }

        ui.add_space(8.0);

        // Status text
        if !state.status_text.is_empty() {
            ui.colored_label(theme::COLOR_ERROR, &state.status_text);
        }

        // Submit button
        let enabled = !state.is_submitting;
        let label = if state.is_submitting {
            "Importing..."
        } else {
            "Import"
        };
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            match state.validate() {
                Ok(()) => {
                    state.status_text.clear();
                    state.is_submitting = true;
                    action = Some(FormPanelAction::Submit(state.collect_values()));
                }
                Err(msg) => {
                    state.status_text = msg;
                }
            }
        }
    });

    action
}
