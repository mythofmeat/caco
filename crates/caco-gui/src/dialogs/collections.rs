use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use caco_core::db::collections::{
    self, CollectionRecord,
};

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

        egui::Window::new("Collections")
            .collapsible(false)
            .resizable(true)
            .default_size([600.0, 420.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Table of collections
                self.render_table(ui);

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Add/Edit form (shown when in edit mode)
                match &self.edit_mode {
                    EditMode::Add | EditMode::Edit(_) => {
                        self.render_form(ui, conn);
                    }
                    EditMode::None => {}
                }

                // Status text
                if let Some((text, is_error)) = &self.status_text {
                    let color = if *is_error {
                        theme::COLOR_ERROR
                    } else {
                        theme::COLOR_SUCCESS
                    };
                    ui.colored_label(color, text.as_str());
                    ui.add_space(4.0);
                }

                // Button row
                ui.horizontal(|ui| {
                    let is_editing = !matches!(self.edit_mode, EditMode::None);
                    let has_selection =
                        self.selected.is_some() && !self.collections.is_empty();

                    if ui
                        .add_enabled(!is_editing, egui::Button::new("Add"))
                        .clicked()
                    {
                        self.edit_mode = EditMode::Add;
                        self.form_name.clear();
                        self.form_query.clear();
                        self.form_sort.clear();
                        self.form_desc = true;
                        self.status_text = None;
                    }

                    if ui
                        .add_enabled(has_selection && !is_editing, egui::Button::new("Edit"))
                        .clicked()
                        && let Some(idx) = self.selected
                    {
                        let coll = &self.collections[idx];
                        self.form_name = coll.name.clone();
                        self.form_query = coll.query.clone();
                        self.form_sort = coll.sort_by.clone().unwrap_or_default();
                        self.form_desc = coll.sort_desc;
                        self.edit_mode = EditMode::Edit(coll.name.clone());
                        self.status_text = None;
                    }

                    if ui
                        .add_enabled(has_selection && !is_editing, egui::Button::new("Delete"))
                        .clicked()
                    {
                        self.do_delete(conn);
                    }

                    if ui
                        .add_enabled(has_selection && !is_editing, egui::Button::new("Load"))
                        .on_hover_text("Apply this collection's query to the library filter")
                        .clicked()
                        && let Some(idx) = self.selected
                    {
                        let query = self.collections[idx].query.clone();
                        result = CollectionsResult::LoadQuery(query);
                    }

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.button("Close").clicked() {
                                result = CollectionsResult::Closed;
                            }
                        },
                    );
                });
            });

        // Escape closes (unless editing form)
        if matches!(self.edit_mode, EditMode::None)
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            return CollectionsResult::Closed;
        }

        result
    }

    fn render_table(&mut self, ui: &mut egui::Ui) {
        if self.collections.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "No collections. Create one with Add.");
            return;
        }

        let text_height = ui.text_style_height(&egui::TextStyle::Body);
        let row_height = text_height + 6.0;

        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .sense(egui::Sense::click())
            .column(Column::initial(120.0).at_least(60.0)) // Name
            .column(Column::remainder().at_least(200.0)) // Query
            .column(Column::initial(80.0).at_least(50.0)); // Sort

        table
            .header(row_height + 2.0, |mut header| {
                for label in ["Name", "Query", "Sort"] {
                    header.col(|ui| {
                        ui.strong(label);
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
                        let sort_display = match &coll.sort_by {
                            Some(s) if coll.sort_desc => format!("{s}-"),
                            Some(s) => format!("{s}+"),
                            None => String::new(),
                        };
                        ui.label(sort_display);
                    });

                    if row.response().clicked() {
                        self.selected = Some(idx);
                    }
                });
            });
    }

    fn render_form(&mut self, ui: &mut egui::Ui, conn: &Connection) {
        let is_add = matches!(self.edit_mode, EditMode::Add);
        let title = if is_add {
            "New Collection"
        } else {
            "Edit Collection"
        };

        ui.strong(title);
        ui.add_space(4.0);

        egui::Grid::new("collection_form")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label("Name:");
                ui.add_enabled(
                    is_add,
                    egui::TextEdit::singleline(&mut self.form_name)
                        .desired_width(ui.available_width() - 40.0)
                        .hint_text("Collection name"),
                );
                ui.end_row();

                ui.label("Query:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.form_query)
                        .desired_width(ui.available_width() - 40.0)
                        .hint_text("e.g. tag:cacoward status:playing"),
                );
                ui.end_row();

                ui.label("Sort:");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.form_sort)
                            .desired_width(100.0)
                            .hint_text("e.g. playtime"),
                    );
                    ui.checkbox(&mut self.form_desc, "Descending");
                });
                ui.end_row();
            });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                self.do_save(conn);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.edit_mode = EditMode::None;
            }
        });

        ui.add_space(4.0);
        ui.separator();
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
                        self.status_text =
                            Some((format!("Created collection '{name}'"), false));
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
