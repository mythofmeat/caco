//! Keyboard-shortcut help dialog and "About" dialog.

/// Render the keyboard-shortcut help dialog. Returns `true` if the user closed it.
pub(super) fn render_help_dialog(ctx: &egui::Context) -> bool {
    let mut closed = false;
    crate::dialogs::modal_window(ctx, "Keyboard Shortcuts", [400.0, 620.0]).show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height() - 30.0)
            .show(ui, |ui| {
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
                shortcut_section(
                    ui,
                    "Dialogs",
                    &[
                        ("Escape", "Close / cancel"),
                        ("Enter", "Confirm / default action"),
                    ],
                );
                shortcut_section(
                    ui,
                    "Global",
                    &[
                        ("Ctrl+Q", "Quit"),
                        ("F5", "Refresh library"),
                        ("Ctrl+R", "Reload config from disk"),
                    ],
                );
            });
        ui.add_space(8.0);
        if ui.button("Close").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            closed = true;
        }
    });
    closed
}

fn shortcut_section(ui: &mut egui::Ui, title: &str, shortcuts: &[(&str, &str)]) {
    ui.add_space(4.0);
    ui.strong(title);
    ui.add_space(2.0);
    egui::Grid::new(title)
        .num_columns(2)
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

/// Render the "About Caco" dialog. Returns `true` if the user closed it.
pub(super) fn render_about_dialog(ctx: &egui::Context) -> bool {
    let mut closed = false;
    crate::dialogs::modal_window(ctx, "About Caco", [320.0, 200.0]).show(ctx, |ui| {
        ui.heading("Caco");
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.add_space(4.0);
        ui.label("A Doom WAD library manager");
        ui.add_space(8.0);
        if ui.button("Close").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            closed = true;
        }
    });
    closed
}
