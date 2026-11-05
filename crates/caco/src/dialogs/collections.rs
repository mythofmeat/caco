use egui_extras::Column;
use rusqlite::Connection;

use caco_core::db::collections::{self, CollectionRecord};

use crate::dialogs::{Dialog, Primary, dialog_table, form_label};
use crate::state::SORT_FIELDS;
use crate::theme;

/// What the caller should do when the dialog closes.
pub enum CollectionsResult {
    Open,
    Closed,
    /// Load this query into the filter bar.
    LoadQuery(String),
}

/// Inline editing mode.
enum EditMode {
    None,
    /// Adding a new collection.
    Add,
    /// Editing an existing collection (by name).
    Edit(String),
}

pub struct CollectionsDialogState {
    collections: Vec<CollectionRecord>,
    selected: Option<usize>,
    edit_mode: EditMode,
    // Form fields
    form_name: String,
    form_query: String,
    form_sort: String,
    form_desc: bool,
    status_text: Option<(String, bool)>,
    pub modified: bool,
}

impl CollectionsDialogState {
    pub fn new(conn: &Connection) -> Self {
        let mut state = Self {
            collections: Vec::new(),
            selected: None,
            edit_mode: EditMode::None,
            form_name: String::new(),
            form_query: String::new(),
            form_sort: String::new(),
            form_desc: true,
            status_text: None,
            modified: false,
        };
        state.load(conn);
        state
    }

    /// Open the dialog with a specific collection pre-selected for editing.
    pub fn new_editing(conn: &Connection, name: &str) -> Self {
        let mut state = Self::new(conn);
        if let Some(idx) = state.collections.iter().position(|c| c.name == name) {
            state.selected = Some(idx);
            let coll = &state.collections[idx];
            state.form_name = coll.name.clone();
            state.form_query = coll.query.clone();
            state.form_sort = coll.sort_by.clone().unwrap_or_default();
            state.form_desc = coll.sort_desc;
            state.edit_mode = EditMode::Edit(coll.name.clone());
        }
        state
    }

    fn load(&mut self, conn: &Connection) {
        self.collections = collections::get_all_collections(conn).unwrap_or_default();
        if self.selected.is_none() && !self.collections.is_empty() {
            self.selected = Some(0);
        }
        if let Some(idx) = self.selected
            && idx >= self.collections.len()
        {
            self.selected = if self.collections.is_empty() {
                None
            } else {
                Some(self.collections.len() - 1)
            };
        }
    }

    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> CollectionsResult {
        let mut result = CollectionsResult::Open;
        let is_editing = !matches!(self.edit_mode, EditMode::None);

        Dialog::new("Collections", [640.0, 460.0]).show(ctx, |dialog| {
            dialog
                .fill(|ui, height| self.render_table(ui, height))
                .above(|ui| {
                    if is_editing {
                        self.render_form(ui);
                    }
                    if let Some((text, is_error)) = &self.status_text {
                        let color = if *is_error {
                            theme::COLOR_ERROR
                        } else {
                            theme::TEXT_SECONDARY
                        };
                        ui.colored_label(color, text.as_str());
                    }
                })
                .buttons(|row| {
                    if is_editing {
                        let ended = row.end(Some("Cancel"), Some(Primary::new("Save")));
                        if ended.dismissed {
                            self.edit_mode = EditMode::None;
                        }
                        if ended.primary {
                            self.do_save(conn);
                        }
                        return;
                    }
                    row.left(|ui| {
                        if ui.button("Add").clicked() {
                            self.edit_mode = EditMode::Add;
                            self.form_name.clear();
                            self.form_query.clear();
                            self.form_sort.clear();
                            self.form_desc = true;
                            self.status_text = None;
                        }
                        if let Some(idx) = self.selected.filter(|_| !self.collections.is_empty()) {
                            if ui.button("Edit").clicked() {
                                let coll = &self.collections[idx];
                                self.form_name = coll.name.clone();
                                self.form_query = coll.query.clone();
                                self.form_sort = coll.sort_by.clone().unwrap_or_default();
                                self.form_desc = coll.sort_desc;
                                self.edit_mode = EditMode::Edit(coll.name.clone());
                                self.status_text = None;
                            }
                            if ui.button("Delete").clicked() {
                                self.do_delete(conn);
                            }
                            if ui
                                .button("Show in Library")
                                .on_hover_text("Filter the library by this collection's query")
                                .clicked()
                            {
                                let query = self.collections[idx].query.clone();
                                result = CollectionsResult::LoadQuery(query);
                            }
                        }
                    });
                    if row.close() {
                        result = CollectionsResult::Closed;
                    }
                })
        });

        // Escape backs out of the form before it closes the dialog.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if is_editing {
                self.edit_mode = EditMode::None;
            } else {
                return CollectionsResult::Closed;
            }
        }

        result
    }

    fn render_table(&mut self, ui: &mut egui::Ui, height: f32) {
        if self.collections.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "A collection is a saved search you can pick from the sidebar. Add one to start.",
            );
            return;
        }

        let text_height = ui.text_style_height(&egui::TextStyle::Body);
        let row_height = text_height + 6.0;

        let table = dialog_table(ui, height - row_height)
            .sense(egui::Sense::click())
            .column(Column::initial(140.0).at_least(60.0).clip(true)) // Name
            .column(Column::remainder().at_least(160.0).clip(true)) // Query
            .column(Column::initial(120.0).at_least(60.0)); // Sort

        table
            .header(row_height + 2.0, |mut header| {
                for label in ["Name", "Query", "Sort"] {
                    header.col(|ui| {
                        ui.label(theme::column_header(label));
                    });
                }
            })
            .body(|body| {
                let count = self.collections.len();
                body.rows(row_height, count, |mut row| {
                    let idx = row.index();
                    let is_selected = self.selected == Some(idx);
                    row.set_selected(is_selected);

                    let coll = &self.collections[idx];

                    row.col(|ui| {
                        ui.label(&coll.name);
                    });
                    row.col(|ui| {
                        ui.colored_label(theme::TEXT_SECONDARY, &coll.query);
                    });
                    row.col(|ui| {
                        ui.label(sort_label(coll));
                    });

                    if row.response().clicked() {
                        self.selected = Some(idx);
                    }
                });
            });
    }

    fn render_form(&mut self, ui: &mut egui::Ui) {
        let is_add = matches!(self.edit_mode, EditMode::Add);

        egui::Grid::new("collection_form")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                let label = form_label(ui, "Name");
                ui.add_enabled(
                    is_add,
                    egui::TextEdit::singleline(&mut self.form_name)
                        .desired_width(ui.available_width() - 40.0),
                )
                .labelled_by(label.id);
                ui.end_row();

                let label = form_label(ui, "Query");
                ui.add(
                    egui::TextEdit::singleline(&mut self.form_query)
                        .desired_width(ui.available_width() - 40.0)
                        .hint_text("e.g. tag:cacoward status:unplayed"),
                )
                .labelled_by(label.id);
                ui.end_row();

                let label = form_label(ui, "Sort by");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("collection_sort")
                        .selected_text(sort_field_label(&self.form_sort))
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.form_sort,
                                String::new(),
                                sort_field_label(""),
                            );
                            for (key, field) in SORT_FIELDS {
                                ui.selectable_value(
                                    &mut self.form_sort,
                                    (*key).to_string(),
                                    *field,
                                );
                            }
                        })
                        .response
                        .labelled_by(label.id);
                    ui.checkbox(&mut self.form_desc, "Descending");
                });
                ui.end_row();
            });
        ui.add_space(4.0);
    }

    fn do_save(&mut self, conn: &Connection) {
        let name = self.form_name.trim();
        let query = self.form_query.trim();

        if name.is_empty() || query.is_empty() {
            self.status_text = Some(("Name and query are required".to_string(), true));
            return;
        }

        let sort_by = if self.form_sort.trim().is_empty() {
            None
        } else {
            Some(self.form_sort.trim())
        };

        match &self.edit_mode {
            EditMode::Add => {
                match collections::create_collection(conn, name, query, sort_by, self.form_desc) {
                    Ok(_) => {
                        self.status_text = Some((format!("Created collection '{name}'"), false));
                        self.edit_mode = EditMode::None;
                        self.modified = true;
                        self.load(conn);
                    }
                    Err(e) => {
                        self.status_text = Some((format!("Error: {e}"), true));
                    }
                }
            }
            EditMode::Edit(original_name) => {
                match collections::update_collection(
                    conn,
                    original_name,
                    Some(query),
                    Some(sort_by),
                    Some(self.form_desc),
                ) {
                    Ok(true) => {
                        self.status_text =
                            Some((format!("Updated collection '{original_name}'"), false));
                        self.edit_mode = EditMode::None;
                        self.modified = true;
                        self.load(conn);
                    }
                    Ok(false) => {
                        self.status_text = Some(("No changes to save".to_string(), true));
                    }
                    Err(e) => {
                        self.status_text = Some((format!("Error: {e}"), true));
                    }
                }
            }
            EditMode::None => {}
        }
    }

    fn do_delete(&mut self, conn: &Connection) {
        if let Some(idx) = self.selected
            && let Some(coll) = self.collections.get(idx)
        {
            let name = coll.name.clone();
            match collections::delete_collection(conn, &name) {
                Ok(true) => {
                    self.status_text = Some((format!("Deleted '{name}'"), false));
                    self.modified = true;
                    self.load(conn);
                }
                Ok(false) => {
                    self.status_text = Some(("Collection not found".to_string(), true));
                }
                Err(e) => {
                    self.status_text = Some((format!("Error: {e}"), true));
                }
            }
        }
    }
}

/// A sort field as the library's sort control names it, and a key it has never heard of as
/// it is, since a collection can hold anything.
fn sort_field_label(key: &str) -> &str {
    if key.is_empty() {
        return "Whatever it is now";
    }
    SORT_FIELDS
        .iter()
        .find(|(field, _)| *field == key)
        .map_or(key, |(_, label)| label)
}

/// "Playtime \u{2193}", for the table.
fn sort_label(collection: &CollectionRecord) -> String {
    let Some(key) = &collection.sort_by else {
        return String::new();
    };
    let arrow = if collection.sort_desc {
        "\u{2193}"
    } else {
        "\u{2191}"
    };
    format!("{} {arrow}", sort_field_label(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sort_reads_as_the_library_names_it() {
        assert_eq!(sort_field_label("playtime"), "Playtime");
        assert_eq!(sort_field_label("something-old"), "something-old");
        let collection = CollectionRecord {
            id: 1,
            name: "n".to_string(),
            query: "q".to_string(),
            sort_by: Some("playtime".to_string()),
            sort_desc: true,
            created_at: String::new(),
        };
        assert_eq!(sort_label(&collection), "Playtime \u{2193}");
    }
}
