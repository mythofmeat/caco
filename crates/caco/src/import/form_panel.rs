use crate::import::state::{FormKind, FormState};
use crate::theme;
use crate::workers::{FileDialogRequest, spawn_file_dialog};

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

    // Drain any pending Browse… picker. We hold the path temporarily so we
    // can write it to the field during the borrow below.
    let picked_path: Option<String> = if let Some(rx) = &state.pending_browse {
        match rx.try_recv() {
            Ok(result) => {
                state.pending_browse = None;
                result.map(|p| p.display().to_string())
            }
            Err(_) => None,
        }
    } else {
        None
    };

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        for field in &mut state.fields {
            let is_path_field = is_local && field.name == "path";
            if is_path_field && let Some(ref p) = picked_path {
                field.value = p.clone();
            }
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&field.display_label)
                        .color(theme::TEXT_SECONDARY)
                        .monospace(),
                );
                let text_width = if is_path_field { 330.0 } else { 400.0 };
                ui.add(egui::TextEdit::singleline(&mut field.value).desired_width(text_width));
                if is_path_field {
                    let browse_busy = state.pending_browse.is_some();
                    if ui
                        .add_enabled(!browse_busy, egui::Button::new("Browse\u{2026}"))
                        .clicked()
                    {
                        let mut req =
                            FileDialogRequest::open().add_filter("WAD/ZIP files", &["wad", "zip"]);
                        if let Some(dir) = dirs::home_dir() {
                            req = req.set_directory(dir);
                        }
                        state.pending_browse = Some(spawn_file_dialog(Some(ui.ctx().clone()), req));
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
