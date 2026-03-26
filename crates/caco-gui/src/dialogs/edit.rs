use caco_core::complevel::parse_complevel;
use caco_core::db::models::Status;
use caco_core::db::wads::{self, WadUpdate};
use rusqlite::Connection;

use crate::theme;

// ---------------------------------------------------------------------------
// Field types
// ---------------------------------------------------------------------------

enum FieldKind {
    Text,
    StatusCombo,
    RatingCombo,
}

struct EditField {
    name: &'static str,
    label: &'static str,
    value: String,
    kind: FieldKind,
}

const STATUSES: &[&str] = &[
    "to-play", "backlog", "playing", "finished", "abandoned", "awaiting-update",
];

const RATINGS: &[&str] = &["", "1", "2", "3", "4", "5"];

// ---------------------------------------------------------------------------
// EditDialogState
// ---------------------------------------------------------------------------

pub struct EditDialogState {
    wad_id: i64,
    fields: Vec<EditField>,
    original_tags: Vec<String>,
    pub error_message: Option<String>,
}

/// Result of showing the edit dialog.
pub enum EditResult {
    Saved,
    Cancelled,
    Open,
}

impl EditDialogState {
    /// Create a new edit dialog, loading current WAD values from the DB.
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let mut wad = caco_core::db::wads::get_wad(conn, wad_id, true).ok()??;
        let _ = caco_core::db::connection::attach_tags(conn, &mut wad);

        let original_tags = wad.tags.clone();

        let fields = vec![
            text("title", "Title", &wad.title),
            text("author", "Author", wad.author.as_deref().unwrap_or("")),
            text("year", "Year", &wad.year.map(|y| y.to_string()).unwrap_or_default()),
            EditField {
                name: "status",
                label: "Status",
                value: wad.status.clone(),
                kind: FieldKind::StatusCombo,
            },
            EditField {
                name: "rating",
                label: "Rating",
                value: wad.rating.map(|r| r.to_string()).unwrap_or_default(),
                kind: FieldKind::RatingCombo,
            },
            text("tags", "Tags (comma-separated)", &wad.tags.join(", ")),
            text("notes", "Notes", wad.notes.as_deref().unwrap_or("")),
            text("description", "Description", wad.description.as_deref().unwrap_or("")),
            text("iwad", "IWAD", wad.custom_iwad.as_deref().unwrap_or("")),
            text("sourceport", "Sourceport", wad.custom_sourceport.as_deref().unwrap_or("")),
            text("complevel", "Complevel", &wad.complevel.map(|c| c.to_string()).unwrap_or_default()),
            text("config", "Config Profile", wad.custom_config.as_deref().unwrap_or("")),
            text("args", "Custom Args (JSON)", wad.custom_args.as_deref().unwrap_or("")),
            text("version", "Version", wad.version.as_deref().unwrap_or("")),
        ];

        Some(Self {
            wad_id,
            fields,
            original_tags,
            error_message: None,
        })
    }

    /// Render the edit dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> EditResult {
        let mut result = EditResult::Open;

        egui::Window::new("Edit WAD")
            .collapsible(false)
            .resizable(false)
            .fixed_size([460.0, 520.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Error banner
                if let Some(err) = &self.error_message {
                    ui.colored_label(crate::theme::COLOR_ERROR, err);
                    ui.add_space(4.0);
                }

                egui::ScrollArea::vertical()
                    .max_height(440.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;

                        for field in &mut self.fields {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    theme::TEXT_SECONDARY,
                                    egui::RichText::new(field.label).strong(),
                                );
                            });

                            match &field.kind {
                                FieldKind::Text => {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut field.value)
                                            .desired_width(f32::INFINITY),
                                    );
                                }
                                FieldKind::StatusCombo => {
                                    egui::ComboBox::from_id_salt("status_combo")
                                        .selected_text(theme::status_display(&field.value))
                                        .width(200.0)
                                        .show_ui(ui, |ui| {
                                            for &s in STATUSES {
                                                ui.selectable_value(
                                                    &mut field.value,
                                                    s.to_string(),
                                                    theme::status_display(s),
                                                );
                                            }
                                        });
                                }
                                FieldKind::RatingCombo => {
                                    let display = if field.value.is_empty() {
                                        "None".to_string()
                                    } else {
                                        theme::rating_stars(field.value.parse().ok())
                                    };
                                    egui::ComboBox::from_id_salt("rating_combo")
                                        .selected_text(display)
                                        .width(200.0)
                                        .show_ui(ui, |ui| {
                                            for &r in RATINGS {
                                                let label = if r.is_empty() {
                                                    "None".to_string()
                                                } else {
                                                    theme::rating_stars(r.parse().ok())
                                                };
                                                ui.selectable_value(
                                                    &mut field.value,
                                                    r.to_string(),
                                                    label,
                                                );
                                            }
                                        });
                                }
                            }
                        }
                    });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Buttons
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() && self.save(conn).is_ok() {
                        result = EditResult::Saved;
                    }
                    if ui.button("Cancel").clicked() {
                        result = EditResult::Cancelled;
                    }
                });
            });

        // Escape closes
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return EditResult::Cancelled;
        }

        result
    }

    fn get_value(&self, name: &str) -> String {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value.clone())
            .unwrap_or_default()
    }

    fn save(&mut self, conn: &Connection) -> Result<(), ()> {
        self.error_message = None;

        // Validate title
        let title = self.get_value("title");
        if title.is_empty() {
            self.error_message = Some("Title is required".to_string());
            return Err(());
        }

        // Validate year
        let year_str = self.get_value("year");
        let year: Option<i64> = if year_str.is_empty() {
            None
        } else {
            match year_str.parse::<i64>() {
                Ok(y) if (1993..=2100).contains(&y) => Some(y),
                _ => {
                    self.error_message = Some("Year must be 1993-2100".to_string());
                    return Err(());
                }
            }
        };

        // Validate complevel
        let complevel_str = self.get_value("complevel");
        let complevel: Option<i64> = if complevel_str.is_empty() {
            None
        } else {
            match parse_complevel(&complevel_str) {
                Some(c) => Some(c as i64),
                None => {
                    self.error_message = Some("Invalid complevel".to_string());
                    return Err(());
                }
            }
        };

        // Build WadUpdate
        let status_str = self.get_value("status");
        let status = Status::parse(&status_str);

        let mut update = WadUpdate::new();
        update = update.set_text("title", Some(title)).unwrap();
        update = update.set_text("author", opt_str(&self.get_value("author"))).unwrap();
        update = update.set_int("year", year).unwrap();

        if let Some(s) = status {
            update = update.set_status(s).unwrap();
        }

        let rating_str = self.get_value("rating");
        let rating: Option<i64> = if rating_str.is_empty() {
            None
        } else {
            rating_str.parse().ok()
        };
        update = update.set_int("rating", rating).unwrap();

        update = update.set_text("notes", opt_str(&self.get_value("notes"))).unwrap();
        update = update.set_text("description", opt_str(&self.get_value("description"))).unwrap();
        update = update.set_text("custom_iwad", opt_str(&self.get_value("iwad"))).unwrap();
        update = update.set_text("custom_sourceport", opt_str(&self.get_value("sourceport"))).unwrap();
        update = update.set_int("complevel", complevel).unwrap();
        update = update.set_text("custom_config", opt_str(&self.get_value("config"))).unwrap();
        update = update.set_text("custom_args", opt_str(&self.get_value("args"))).unwrap();
        update = update.set_text("version", opt_str(&self.get_value("version"))).unwrap();

        if let Err(e) = wads::update_wad(conn, self.wad_id, &update) {
            self.error_message = Some(format!("Save failed: {e}"));
            return Err(());
        }

        // Handle tags delta
        let new_tags: Vec<String> = self
            .get_value("tags")
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();

        for tag in &self.original_tags {
            if !new_tags.contains(tag) {
                let _ = wads::remove_tag(conn, self.wad_id, tag);
            }
        }
        for tag in &new_tags {
            if !self.original_tags.contains(tag) {
                let _ = wads::add_tag(conn, self.wad_id, tag);
            }
        }

        Ok(())
    }
}

fn text(name: &'static str, label: &'static str, value: &str) -> EditField {
    EditField {
        name,
        label,
        value: value.to_string(),
        kind: FieldKind::Text,
    }
}

fn opt_str(s: &str) -> Option<String> {
    if s.trim().is_empty() { None } else { Some(s.to_string()) }
}
