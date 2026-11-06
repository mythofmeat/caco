//! The help dialog: which version this is, every key there is, and how to write a filter.

use crate::dialogs::Dialog;
use crate::theme;

/// As wide as the longest set of keys, so that every section's descriptions line up with the
/// next: each section is a grid of its own, and they used to be as wide as their own keys.
const KEYS_WIDTH: f32 = 150.0;

/// What a filter can say. Everything the query parser takes is in `CLAUDE.md`, and this is
/// what most people will want of it.
const FILTERS: &[(&str, &str)] = &[
    ("road", "Finds it in titles, authors and descriptions"),
    ("author:skillsaw", "By who made it"),
    ("year:2016", "By when it came out"),
    ("status:completed", "Also unplayed, in-progress, abandoned"),
    ("tag:caco*", "By tag, with * for anything"),
    ("cacoward:2023", "Won something that year, or winner, or *"),
    ("retrievable:lost", "Not here, and caco can't fetch it"),
    ("dup:yes", "Another WAD plays the same file"),
    ("^status:abandoned", "Anything but this"),
    ("tag:a , tag:b", "Either of these"),
];

/// Render the help dialog. Returns `true` if the user closed it.
pub(super) fn render_help_dialog(ctx: &egui::Context) -> bool {
    let mut closed = false;
    Dialog::new("Help", [520.0, 680.0])
        .about(format!(
            "caco {}, a Doom WAD library manager",
            crate::VERSION
        ))
        .show(ctx, |dialog| {
            dialog
                .scroll(|ui| {
                    shortcut_section(
                        ui,
                        "Library",
                        &[
                            (&["Enter", "P"], "Play the selected WAD"),
                            (&["\u{2191}\u{2193}", "J K"], "Move up and down"),
                            (&["\u{2190}\u{2192}", "H L"], "Move across (in the grid)"),
                            (&["PgUp", "PgDn"], "Move by a page"),
                            (&["Home", "G G"], "Go to the first WAD"),
                            (&["End", "Shift+G"], "Go to the last WAD"),
                            (&["I"], "Show or hide details"),
                            (&["E"], "Edit"),
                            (&["S"], "Sessions"),
                            (&["M"], "Map stats"),
                            (&["F"], "Saves and demos"),
                            (&["Delete", "D"], "Delete"),
                            (&["Esc"], "Clear the filter, then the selection"),
                        ],
                    );
                    shortcut_section(
                        ui,
                        "Cacowards",
                        &[
                            (&["\u{2190}\u{2192}", "H L"], "Go between years"),
                            (&["\u{2191}\u{2193}", "J K"], "Go through the entries"),
                            (&["Enter"], "Do what the entry's button says"),
                        ],
                    );
                    shortcut_section(
                        ui,
                        "Import",
                        &[
                            (&["1\u{2013}5"], "Switch source"),
                            (&["\u{2191}\u{2193}", "J K"], "Go through the results"),
                            (&["Enter"], "Import the selected result"),
                        ],
                    );
                    shortcut_section(
                        ui,
                        "Dialogs",
                        &[
                            (&["Esc"], "Close or cancel"),
                            (&["Ctrl+Enter"], "Press the main button"),
                        ],
                    );
                    shortcut_section(
                        ui,
                        "Anywhere",
                        &[
                            (&["Ctrl+1\u{2013}4"], "Library, Import, Cacowards, Stats"),
                            (&["/", "Ctrl+F"], "Search what is on the screen"),
                            (&["?", "F1"], "This help"),
                            (&["F5"], "Refresh the library"),
                            (&["Ctrl+R"], "Read settings from disk again"),
                            (&["Ctrl+Q"], "Quit"),
                        ],
                    );
                    filter_section(ui);
                })
                .buttons(|row| {
                    closed = row.close();
                })
        });
    closed || ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

fn shortcut_section(ui: &mut egui::Ui, title: &str, shortcuts: &[(&[&str], &str)]) {
    heading(ui, title);
    grid(title).show(ui, |ui| {
        for (keys, what) in shortcuts {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (index, key) in keys.iter().enumerate() {
                    if index > 0 {
                        ui.colored_label(theme::TEXT_MUTED, egui::RichText::new("or").size(11.0));
                    }
                    theme::key_cap(ui, &key.replace("Ctrl", theme::COMMAND_KEY));
                }
            });
            ui.colored_label(theme::TEXT_SECONDARY, *what);
            ui.end_row();
        }
    });
}

fn filter_section(ui: &mut egui::Ui) {
    heading(ui, "Writing a Filter");
    ui.colored_label(
        theme::TEXT_SECONDARY,
        "The box at the top of the library takes any of these, and several at once.",
    );
    ui.add_space(2.0);
    grid("filters").show(ui, |ui| {
        for (example, what) in FILTERS {
            ui.label(
                egui::RichText::new(*example)
                    .monospace()
                    .size(11.5)
                    .color(theme::TEXT_PRIMARY),
            );
            ui.colored_label(theme::TEXT_SECONDARY, *what);
            ui.end_row();
        }
    });
}

fn heading(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    theme::section_heading(ui, title);
    ui.add_space(2.0);
}

fn grid(id: &str) -> egui::Grid {
    egui::Grid::new(("help", id))
        .num_columns(2)
        .min_col_width(KEYS_WIDTH)
        .striped(false)
        .spacing([12.0, 4.0])
}
