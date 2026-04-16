use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use caco_core::config;
use caco_core::db::id24::{self, Id24Record};
use caco_core::db::iwads::{self, IwadRecord};
use caco_core::resource_service;

use crate::theme;

/// Active sub-tab within the Resources dialog.
#[derive(Clone, Copy, PartialEq)]
enum ResourceTab {
    Iwad,
    Id24,
}

/// State for the IWAD/id24 resources dialog.
pub struct ResourcesDialogState {
    active_tab: ResourceTab,
    iwads: Vec<IwadRecord>,
    id24s: Vec<Id24Record>,
    selected_iwad: Option<usize>,
    selected_id24: Option<usize>,
    preferred_iwads: HashSet<String>,
    import_path: String,
    status_text: Option<(String, bool)>, // (message, is_error)
    pub modified: bool,
}

/// Result of showing the resources dialog.
pub enum ResourcesResult {
    Open,
    Closed,
}

impl ResourcesDialogState {
    /// Create a new resources dialog, loading IWADs and id24 WADs from the DB.
    pub fn new(conn: &Connection) -> Self {
        let mut state = Self {
            active_tab: ResourceTab::Iwad,
            iwads: Vec::new(),
            id24s: Vec::new(),
            selected_iwad: None,
            selected_id24: None,
            preferred_iwads: HashSet::new(),
            import_path: String::new(),
            status_text: None,
            modified: false,
        };
        state.load(conn);
        state
    }

    fn load(&mut self, conn: &Connection) {
        self.iwads = iwads::get_all_iwads(conn).unwrap_or_default();
        self.id24s = id24::get_all_id24(conn).unwrap_or_default();

        // Compute preferred IWAD variants
        let cfg = config::load_config();
        self.preferred_iwads = self
            .iwads
            .iter()
            .filter_map(|iwad| {
                let priority = iwads::get_iwad_priority(&iwad.family, Some(&cfg.iwad_priority));
                if priority
                    .first()
                    .map(|v| v == &iwad.variant)
                    .unwrap_or(false)
                {
                    Some(format!("{}/{}", iwad.family, iwad.variant))
                } else {
                    None
                }
            })
            .collect();

        // Auto-select first row if available
        if self.selected_iwad.is_none() && !self.iwads.is_empty() {
            self.selected_iwad = Some(0);
        }
        if self.selected_id24.is_none() && !self.id24s.is_empty() {
            self.selected_id24 = Some(0);
        }

        // Clamp selections
        if let Some(idx) = self.selected_iwad
            && idx >= self.iwads.len()
        {
            self.selected_iwad = if self.iwads.is_empty() {
                None
            } else {
                Some(self.iwads.len() - 1)
            };
        }
        if let Some(idx) = self.selected_id24
            && idx >= self.id24s.len()
        {
            self.selected_id24 = if self.id24s.is_empty() {
                None
            } else {
                Some(self.id24s.len() - 1)
            };
        }
    }

    /// Render the resources dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> ResourcesResult {
        let mut result = ResourcesResult::Open;

        egui::Window::new("Resources")
            .collapsible(false)
            .resizable(true)
            .default_size([750.0, 500.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Tab bar
                ui.horizontal(|ui| {
                    let iwad_active = self.active_tab == ResourceTab::Iwad;
                    let iwad_text = egui::RichText::new("IWAD");
                    let iwad_text = if iwad_active {
                        iwad_text.strong().color(theme::TEXT_ACCENT)
                    } else {
                        iwad_text.color(theme::TEXT_SECONDARY)
                    };
                    if ui.selectable_label(iwad_active, iwad_text).clicked() {
                        self.active_tab = ResourceTab::Iwad;
                    }

                    let id24_active = self.active_tab == ResourceTab::Id24;
                    let id24_text = egui::RichText::new("id24");
                    let id24_text = if id24_active {
                        id24_text.strong().color(theme::TEXT_ACCENT)
                    } else {
                        id24_text.color(theme::TEXT_SECONDARY)
                    };
                    if ui.selectable_label(id24_active, id24_text).clicked() {
                        self.active_tab = ResourceTab::Id24;
                    }
                });

                ui.add_space(4.0);

                // Table
                match self.active_tab {
                    ResourceTab::Iwad => self.render_iwad_table(ui),
                    ResourceTab::Id24 => self.render_id24_table(ui),
                }

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Import row
                ui.horizontal(|ui| {
                    ui.label("Import path:");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.import_path)
                            .desired_width(ui.available_width() - 120.0)
                            .hint_text("Path to IWAD or id24 WAD file..."),
                    );
                    let enter_pressed =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Browse…").clicked() {
                        let mut dialog = rfd::FileDialog::new().add_filter("WAD files", &["wad"]);
                        if let Some(dir) = dirs::home_dir() {
                            dialog = dialog.set_directory(dir);
                        }
                        if let Some(path) = dialog.pick_file() {
                            self.import_path = path.display().to_string();
                        }
                    }
                    if ui.button("Add").clicked() || enter_pressed {
                        self.do_import(conn);
                    }
                });

                // Status text
                if let Some((text, is_error)) = &self.status_text {
                    let color = if *is_error {
                        theme::COLOR_ERROR
                    } else {
                        theme::COLOR_SUCCESS
                    };
                    ui.colored_label(color, text.as_str());
                }

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Button row
                ui.horizontal(|ui| {
                    let has_selection = match self.active_tab {
                        ResourceTab::Iwad => self.selected_iwad.is_some() && !self.iwads.is_empty(),
                        ResourceTab::Id24 => self.selected_id24.is_some() && !self.id24s.is_empty(),
                    };

                    if ui
                        .add_enabled(has_selection, egui::Button::new("Delete Selected"))
                        .clicked()
                    {
                        self.do_delete(conn);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            result = ResourcesResult::Closed;
                        }
                    });
                });
            });

        // Escape closes
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return ResourcesResult::Closed;
        }

        result
    }

    fn render_iwad_table(&mut self, ui: &mut egui::Ui) {
        if self.iwads.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "No IWADs registered.");
            return;
        }

        let text_height = ui.text_style_height(&egui::TextStyle::Body);
        let row_height = text_height + 6.0;

        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .sense(egui::Sense::click())
            .column(Column::initial(100.0).at_least(60.0)) // Family
            .column(Column::initial(80.0).at_least(50.0)) // Variant
            .column(Column::initial(200.0).at_least(100.0)) // Title
            .column(Column::remainder().at_least(150.0)); // Path

        table
            .header(row_height + 2.0, |mut header| {
                for label in ["Family", "Variant", "Title", "Path"] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                let count = self.iwads.len();
                body.rows(row_height, count, |mut row| {
                    let idx = row.index();
                    let is_selected = self.selected_iwad == Some(idx);
                    row.set_selected(is_selected);

                    let iwad = &self.iwads[idx];
                    let key = format!("{}/{}", iwad.family, iwad.variant);
                    let is_preferred = self.preferred_iwads.contains(&key);

                    row.col(|ui| {
                        ui.label(&iwad.family);
                    });
                    row.col(|ui| {
                        let variant_display = if is_preferred {
                            format!("{} *", iwad.variant)
                        } else {
                            iwad.variant.clone()
                        };
                        ui.label(variant_display);
                    });
                    row.col(|ui| {
                        ui.label(iwad.title.as_deref().unwrap_or(""));
                    });
                    row.col(|ui| {
                        ui.colored_label(theme::TEXT_SECONDARY, &iwad.path);
                    });

                    if row.response().clicked() {
                        self.selected_iwad = Some(idx);
                    }
                });
            });
    }

    fn render_id24_table(&mut self, ui: &mut egui::Ui) {
        if self.id24s.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "No id24 WADs registered.");
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
            .column(Column::initial(80.0).at_least(50.0)) // Version
            .column(Column::initial(200.0).at_least(100.0)) // Title
            .column(Column::remainder().at_least(150.0)); // Path

        table
            .header(row_height + 2.0, |mut header| {
                for label in ["Name", "Version", "Title", "Path"] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                let count = self.id24s.len();
                body.rows(row_height, count, |mut row| {
                    let idx = row.index();
                    let is_selected = self.selected_id24 == Some(idx);
                    row.set_selected(is_selected);

                    let record = &self.id24s[idx];

                    row.col(|ui| {
                        ui.label(&record.name);
                    });
                    row.col(|ui| {
                        ui.label(record.version.as_deref().unwrap_or(""));
                    });
                    row.col(|ui| {
                        ui.label(record.title.as_deref().unwrap_or(""));
                    });
                    row.col(|ui| {
                        ui.colored_label(theme::TEXT_SECONDARY, &record.path);
                    });

                    if row.response().clicked() {
                        self.selected_id24 = Some(idx);
                    }
                });
            });
    }

    fn do_import(&mut self, conn: &Connection) {
        let path_str = self.import_path.trim().to_string();
        if path_str.is_empty() {
            return;
        }

        let path = PathBuf::from(&path_str);

        // Try IWAD first
        match resource_service::register_iwad(conn, &path) {
            Ok(Some((name, _, title))) => {
                self.import_path.clear();
                self.status_text = Some((format!("Registered IWAD: {title} ({name})"), false));
                self.active_tab = ResourceTab::Iwad;
                self.modified = true;
                self.load(conn);
                return;
            }
            Ok(None) => {} // Not an IWAD, try id24
            Err(e) => {
                self.status_text = Some((format!("Error: {e}"), true));
                return;
            }
        }

        // Try id24
        match resource_service::register_id24(conn, &path) {
            Ok(Some((name, _, title))) => {
                self.import_path.clear();
                self.status_text = Some((format!("Registered id24: {title} ({name})"), false));
                self.active_tab = ResourceTab::Id24;
                self.modified = true;
                self.load(conn);
            }
            Ok(None) => {
                self.status_text =
                    Some(("File not recognized as IWAD or id24 WAD".to_string(), true));
            }
            Err(e) => {
                self.status_text = Some((format!("Error: {e}"), true));
            }
        }
    }

    fn do_delete(&mut self, conn: &Connection) {
        match self.active_tab {
            ResourceTab::Iwad => {
                if let Some(idx) = self.selected_iwad
                    && let Some(iwad) = self.iwads.get(idx)
                {
                    let paths =
                        iwads::remove_iwad_with_paths(conn, &iwad.family, Some(&iwad.variant))
                            .unwrap_or_default();
                    let iwad_dir = config::get_iwad_dir();
                    for p in &paths {
                        if PathBuf::from(p).starts_with(&iwad_dir) {
                            let _ = fs::remove_file(p);
                        }
                    }
                    self.status_text = Some(("IWAD removed".to_string(), false));
                    self.modified = true;
                    self.load(conn);
                }
            }
            ResourceTab::Id24 => {
                if let Some(idx) = self.selected_id24
                    && let Some(record) = self.id24s.get(idx)
                {
                    let paths =
                        id24::remove_id24_with_paths(conn, &record.name).unwrap_or_default();
                    let id24_dir = config::get_id24_dir();
                    for p in &paths {
                        if PathBuf::from(p).starts_with(&id24_dir) {
                            let _ = fs::remove_file(p);
                        }
                    }
                    self.status_text = Some(("id24 WAD removed".to_string(), false));
                    self.modified = true;
                    self.load(conn);
                }
            }
        }
    }
}
