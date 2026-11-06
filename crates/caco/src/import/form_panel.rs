use crate::dialogs::form_label;
use crate::import::state::{FormField, FormKind, FormState};
use crate::theme;
use crate::workers::{FileDialogRequest, spawn_file_dialog};

/// How wide a form gets, however wide the window: a field half the width of a big screen is not
/// any easier to fill in.
const FORM_WIDTH: f32 = 560.0;

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
    // Drain any pending Browse… picker.
    if let Some(rx) = &state.pending_browse
        && let Ok(result) = rx.try_recv()
    {
        state.pending_browse = None;
        if let Some(path) = result {
            state.set_path(&path);
        }
    }

    let mut submit = false;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(FORM_WIDTH));
            ui.spacing_mut().item_spacing.y = 4.0;

            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(state.kind.describe()).color(theme::TEXT_SECONDARY),
                )
                .wrap(),
            );
            ui.add_space(8.0);

            let is_local = state.kind == FormKind::Local;
            let browse_busy = state.pending_browse.is_some();
            let mut browse = false;
            let mut rows = state.fields.iter_mut().peekable();
            while let Some(field) = rows.next() {
                // Two short ones share a row, which is the difference between all of this
                // fitting at 800x400 and not.
                if field.short
                    && let Some(next) = rows.next_if(|next| next.short)
                {
                    ui.columns(2, |columns| {
                        submit |= field_input(&mut columns[0], field);
                        submit |= field_input(&mut columns[1], next);
                    });
                } else if is_local && field.name == "path" {
                    let label = form_label(ui, field.label);
                    ui.horizontal(|ui| {
                        let browse_width = 80.0;
                        let width = ui.available_width() - browse_width - 8.0;
                        let response = ui
                            .add(
                                egui::TextEdit::singleline(&mut field.value)
                                    .desired_width(width)
                                    .hint_text(field.hint),
                            )
                            .labelled_by(label.id);
                        submit |=
                            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        browse = ui
                            .add_enabled(
                                !browse_busy,
                                egui::Button::new("Browse\u{2026}")
                                    .min_size(egui::vec2(browse_width, 0.0)),
                            )
                            .clicked();
                    });
                } else {
                    submit |= field_input(ui, field);
                }
                ui.add_space(4.0);
            }

            if browse {
                let mut req = FileDialogRequest::open()
                    .add_filter("WADs and archives", &["wad", "zip", "pk3", "pk7", "ipk3"]);
                if let Some(dir) = dirs::home_dir() {
                    req = req.set_directory(dir);
                }
                state.pending_browse = Some(spawn_file_dialog(Some(ui.ctx().clone()), req));
            }

            ui.add_space(4.0);

            // How it went
            if !state.status_text.is_empty() {
                ui.colored_label(theme::COLOR_ERROR, &state.status_text);
            }

            // Submit button
            if state.is_submitting {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.colored_label(theme::TEXT_SECONDARY, "Importing\u{2026}");
                });
            } else {
                submit |= ui.add(theme::primary_button("Import")).clicked();
            }
        });

    if !submit || state.is_submitting {
        return None;
    }
    match state.validate() {
        Ok(()) => {
            state.status_text.clear();
            state.is_submitting = true;
            Some(FormPanelAction::Submit(state.collect_values()))
        }
        Err(msg) => {
            state.status_text = msg;
            None
        }
    }
}

/// A labelled field the width of what it is in. True if Enter was pressed in it.
fn field_input(ui: &mut egui::Ui, field: &mut FormField) -> bool {
    let label = form_label(ui, field.label);
    let response = ui
        .add(
            egui::TextEdit::singleline(&mut field.value)
                .desired_width(f32::INFINITY)
                .hint_text(field.hint),
        )
        .labelled_by(label.id);
    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}
