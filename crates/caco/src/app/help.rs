//! The help dialog: which version this is, and the keyboard shortcuts.

use crate::dialogs::Dialog;

/// Render the help dialog: which version this is, and the keyboard shortcuts. Returns `true`
/// if the user closed it.
pub(super) fn render_help_dialog(ctx: &egui::Context) -> bool {
    let mut closed = false;
    Dialog::new("Help", [440.0, 640.0])
        .about(format!(
            "caco {}, a Doom WAD library manager",
            crate::VERSION
        ))
        .show(ctx, |dialog| {
            dialog
                .scroll(|ui| {
                    shortcut_section(
                        ui,
                        "Library Navigation",
                        &[
                            ("\u{2191}\u{2193} / j k", "Navigate up/down"),
                            ("\u{2190}\u{2192} / h l", "Navigate left/right (grid)"),
                            ("Home / g g", "Jump to first WAD"),
                            ("End / Shift+G", "Jump to last WAD"),
                            ("Ctrl+F", "Focus search/filter"),
                        ],
                    );
                    shortcut_section(
                        ui,
                        "Library Actions",
                        &[
                            ("Enter / p", "Play selected WAD"),
                            ("e", "Edit selected WAD"),
                            ("d", "Delete selected WAD"),
                            ("s", "View sessions"),
                            ("m", "Map stats"),
                            ("f", "Saves & demos"),
                        ],
                    );
                    shortcut_section(ui, "Import", &[("1\u{2013}5", "Switch source")]);
                    shortcut_section(ui, "Dialogs", &[("Escape", "Close / cancel")]);
                    shortcut_section(
                        ui,
                        "Global",
                        &[
                            ("F1", "This help"),
                            ("Ctrl+Q", "Quit"),
                            ("F5", "Refresh library"),
                            ("Ctrl+R", "Reload settings from disk"),
                        ],
                    );
                })
                .buttons(|row| {
                    closed = row.close();
                })
        });
    closed || ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

fn shortcut_section(ui: &mut egui::Ui, title: &str, shortcuts: &[(&str, &str)]) {
    ui.add_space(4.0);
    ui.strong(title);
    ui.add_space(2.0);
    egui::Grid::new(title)
        .num_columns(2)
        .striped(false)
        .spacing([40.0, 4.0])
        .show(ui, |ui| {
            for (key, desc) in shortcuts {
                ui.label(egui::RichText::new(*key).monospace());
                ui.label(*desc);
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.separator();
}
