use caco_core::companion_service;
use caco_core::complevel::parse_complevel;
use caco_core::db::companions;
use caco_core::db::models::Status;
use caco_core::db::wads::{self, WadUpdate};
use rusqlite::Connection;

use crate::theme;

// ---------------------------------------------------------------------------
// Tab enum
// ---------------------------------------------------------------------------

#[derive(PartialEq, Eq, Clone, Copy)]
enum EditTab {
    Metadata,
    Sourceport,
    Sources,
    Companions,
}

// ---------------------------------------------------------------------------
// Companion entry (UI state)
// ---------------------------------------------------------------------------

struct CompanionEntry {
    companion_id: i64,
    filename: String,
    size: i64,
    enabled: bool,
}

impl CompanionEntry {
    fn from_record(r: &companions::WadCompanionRecord) -> Self {
        Self {
            companion_id: r.companion_id,
            filename: r.filename.clone(),
            size: r.size,
            enabled: r.enabled,
        }
    }
}

// Deferred companion action (processed after render)
enum CompanionAction {
    Add,
    Remove(i64),
    Toggle(i64, bool),
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const STATUSES: &[&str] = &[
    "to-play", "backlog", "playing", "finished", "abandoned", "awaiting-update",
];

const RATINGS: &[&str] = &["", "1", "2", "3", "4", "5"];

// ---------------------------------------------------------------------------
// EditDialogState
// ---------------------------------------------------------------------------

pub struct EditDialogState {
    wad_id: i64,
    active_tab: EditTab,

    // Metadata tab
    title: String,
    author: String,
    year: String,
    status: String,
    rating: String,
    tags: String,
    notes: String,
    description: String,
    original_tags: Vec<String>,

    // Sourceport tab
    sourceport: String,
    iwad: String,
    complevel: String,
    config: String,
    args: String,
    version: String,

    // Sources tab
    source_type_display: String,
    source_url: String,
    idgames_id: String,

    // Companions tab
    companions: Vec<CompanionEntry>,
    companions_modified: bool,

    pub error_message: Option<String>,
}

/// Result of showing the edit dialog.
pub enum EditResult {
    Saved,
    Cancelled,
    /// Companions were modified but form was cancelled — triggers reload without notification.
    Modified,
    Open,
}

impl EditDialogState {
    /// Create a new edit dialog, loading current WAD values from the DB.
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let mut wad = caco_core::db::wads::get_wad(conn, wad_id, true).ok()??;
        let _ = caco_core::db::connection::attach_tags(conn, &mut wad);

        let original_tags = wad.tags.clone();

        let companion_records =
            companions::get_companions_for_wad(conn, wad_id).unwrap_or_default();
        let companions = companion_records
            .iter()
            .map(CompanionEntry::from_record)
            .collect();

        Some(Self {
            wad_id,
            active_tab: EditTab::Metadata,

            title: wad.title.clone(),
            author: wad.author.as_deref().unwrap_or("").to_string(),
            year: wad.year.map(|y| y.to_string()).unwrap_or_default(),
            status: wad.status.clone(),
            rating: wad.rating.map(|r| r.to_string()).unwrap_or_default(),
            tags: wad.tags.join(", "),
            notes: wad.notes.as_deref().unwrap_or("").to_string(),
            description: wad.description.as_deref().unwrap_or("").to_string(),
            original_tags,

            sourceport: wad.custom_sourceport.as_deref().unwrap_or("").to_string(),
            iwad: wad.custom_iwad.as_deref().unwrap_or("").to_string(),
            complevel: wad.complevel.map(|c| c.to_string()).unwrap_or_default(),
            config: wad.custom_config.as_deref().unwrap_or("").to_string(),
            args: wad.custom_args.as_deref().unwrap_or("").to_string(),
            version: wad.version.as_deref().unwrap_or("").to_string(),

            source_type_display: wad.source_type.clone(),
            source_url: wad.source_url.as_deref().unwrap_or("").to_string(),
            idgames_id: wad.idgames_id.as_deref().unwrap_or("").to_string(),

            companions,
            companions_modified: false,

            error_message: None,
        })
    }

    /// Render the edit dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> EditResult {
        let mut result = EditResult::Open;
        let mut companion_action: Option<CompanionAction> = None;

        egui::Window::new("Edit WAD")
            .collapsible(false)
            .resizable(false)
            .fixed_size([480.0, 540.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Error banner
                if let Some(err) = &self.error_message {
                    ui.colored_label(theme::COLOR_ERROR, err);
                    ui.add_space(4.0);
                }

                // Tab bar
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.active_tab, EditTab::Metadata, "Metadata");
                    ui.selectable_value(
                        &mut self.active_tab,
                        EditTab::Sourceport,
                        "Sourceport",
                    );
                    ui.selectable_value(&mut self.active_tab, EditTab::Sources, "Sources");
                    ui.selectable_value(
                        &mut self.active_tab,
                        EditTab::Companions,
                        "Companions",
                    );
                });
                ui.separator();

                // Tab content
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;

                        match self.active_tab {
                            EditTab::Metadata => self.render_metadata_tab(ui),
                            EditTab::Sourceport => self.render_sourceport_tab(ui),
                            EditTab::Sources => self.render_sources_tab(ui),
                            EditTab::Companions => {
                                companion_action = self.render_companions_tab(ui);
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
                        result = if self.companions_modified {
                            EditResult::Modified
                        } else {
                            EditResult::Cancelled
                        };
                    }
                });
            });

        // Escape closes
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return if self.companions_modified {
                EditResult::Modified
            } else {
                EditResult::Cancelled
            };
        }

        // Process deferred companion actions
        if let Some(action) = companion_action {
            self.process_companion_action(action, conn);
        }

        result
    }

    // -----------------------------------------------------------------------
    // Tab renderers
    // -----------------------------------------------------------------------

    fn render_metadata_tab(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "Title");
        ui.add(egui::TextEdit::singleline(&mut self.title).desired_width(f32::INFINITY));

        field_label(ui, "Author");
        ui.add(egui::TextEdit::singleline(&mut self.author).desired_width(f32::INFINITY));

        field_label(ui, "Year");
        ui.add(egui::TextEdit::singleline(&mut self.year).desired_width(f32::INFINITY));

        field_label(ui, "Status");
        egui::ComboBox::from_id_salt("edit_status_combo")
            .selected_text(theme::status_display(&self.status))
            .width(200.0)
            .show_ui(ui, |ui| {
                for &s in STATUSES {
                    ui.selectable_value(
                        &mut self.status,
                        s.to_string(),
                        theme::status_display(s),
                    );
                }
            });

        field_label(ui, "Rating");
        let rating_display = if self.rating.is_empty() {
            "None".to_string()
        } else {
            theme::rating_stars(self.rating.parse().ok())
        };
        egui::ComboBox::from_id_salt("edit_rating_combo")
            .selected_text(rating_display)
            .width(200.0)
            .show_ui(ui, |ui| {
                for &r in RATINGS {
                    let label = if r.is_empty() {
                        "None".to_string()
                    } else {
                        theme::rating_stars(r.parse().ok())
                    };
                    ui.selectable_value(&mut self.rating, r.to_string(), label);
                }
            });

        field_label(ui, "Tags (comma-separated)");
        ui.add(egui::TextEdit::singleline(&mut self.tags).desired_width(f32::INFINITY));

        field_label(ui, "Notes");
        ui.add(egui::TextEdit::singleline(&mut self.notes).desired_width(f32::INFINITY));

        field_label(ui, "Description");
        ui.add(
            egui::TextEdit::multiline(&mut self.description)
                .desired_width(f32::INFINITY)
                .desired_rows(4),
        );
    }

    fn render_sourceport_tab(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "Sourceport");
        ui.add(
            egui::TextEdit::singleline(&mut self.sourceport).desired_width(f32::INFINITY),
        );

        field_label(ui, "IWAD");
        ui.add(egui::TextEdit::singleline(&mut self.iwad).desired_width(f32::INFINITY));

        field_label(ui, "Complevel");
        ui.add(
            egui::TextEdit::singleline(&mut self.complevel).desired_width(f32::INFINITY),
        );

        field_label(ui, "Config Profile");
        ui.add(egui::TextEdit::singleline(&mut self.config).desired_width(f32::INFINITY));

        field_label(ui, "Custom Args (JSON)");
        ui.add(egui::TextEdit::singleline(&mut self.args).desired_width(f32::INFINITY));

        field_label(ui, "Version");
        ui.add(egui::TextEdit::singleline(&mut self.version).desired_width(f32::INFINITY));
    }

    fn render_sources_tab(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "Source Type");
        ui.add_enabled(
            false,
            egui::TextEdit::singleline(&mut self.source_type_display)
                .desired_width(f32::INFINITY),
        );

        field_label(ui, "Source URL");
        ui.add(
            egui::TextEdit::singleline(&mut self.source_url).desired_width(f32::INFINITY),
        );

        field_label(ui, "idgames ID");
        ui.add(
            egui::TextEdit::singleline(&mut self.idgames_id).desired_width(f32::INFINITY),
        );
    }

    fn render_companions_tab(&mut self, ui: &mut egui::Ui) -> Option<CompanionAction> {
        let mut action = None;

        if self.companions.is_empty() {
            ui.label("No companion files linked to this WAD.");
            ui.add_space(8.0);
        } else {
            egui::Grid::new("edit_companions_grid")
                .num_columns(4)
                .spacing([8.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    // Header
                    ui.strong("Filename");
                    ui.strong("Size");
                    ui.strong("Enabled");
                    ui.strong("");
                    ui.end_row();

                    for entry in &mut self.companions {
                        let cid = entry.companion_id;

                        ui.label(&entry.filename);
                        ui.label(format_size(entry.size));

                        if ui.checkbox(&mut entry.enabled, "").changed() {
                            action = Some(CompanionAction::Toggle(cid, entry.enabled));
                        }

                        if ui.small_button("Remove").clicked() {
                            action = Some(CompanionAction::Remove(cid));
                        }

                        ui.end_row();
                    }
                });

            ui.add_space(8.0);
        }

        if ui.button("Add Companion...").clicked() {
            action = Some(CompanionAction::Add);
        }

        action
    }

    // -----------------------------------------------------------------------
    // Companion action processing
    // -----------------------------------------------------------------------

    fn process_companion_action(&mut self, action: CompanionAction, conn: &Connection) {
        match action {
            CompanionAction::Add => {
                let start_dir = dirs::home_dir().unwrap_or_default();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Companion Files", &["deh", "bex", "wad", "pk3", "zip"])
                    .set_directory(start_dir)
                    .pick_file()
                {
                    match companion_service::register_companion(conn, self.wad_id, &path) {
                        Ok(_) => {
                            self.companions_modified = true;
                            self.reload_companions(conn);
                        }
                        Err(e) => {
                            self.error_message =
                                Some(format!("Failed to add companion: {e}"));
                        }
                    }
                }
            }
            CompanionAction::Remove(companion_id) => {
                match companion_service::unregister_companion(
                    conn,
                    self.wad_id,
                    companion_id,
                    None,
                ) {
                    Ok(_) => {
                        self.companions_modified = true;
                        self.reload_companions(conn);
                    }
                    Err(e) => {
                        self.error_message =
                            Some(format!("Failed to remove companion: {e}"));
                    }
                }
            }
            CompanionAction::Toggle(companion_id, enabled) => {
                match companions::set_companion_enabled(
                    conn,
                    self.wad_id,
                    companion_id,
                    enabled,
                ) {
                    Ok(_) => {
                        self.companions_modified = true;
                    }
                    Err(e) => {
                        self.error_message =
                            Some(format!("Failed to toggle companion: {e}"));
                    }
                }
            }
        }
    }

    fn reload_companions(&mut self, conn: &Connection) {
        let records =
            companions::get_companions_for_wad(conn, self.wad_id).unwrap_or_default();
        self.companions = records.iter().map(CompanionEntry::from_record).collect();
    }

    // -----------------------------------------------------------------------
    // Save (persists Metadata + Sourceport + Sources tabs)
    // -----------------------------------------------------------------------

    fn save(&mut self, conn: &Connection) -> Result<(), ()> {
        self.error_message = None;

        // Validate title
        if self.title.is_empty() {
            self.error_message = Some("Title is required".to_string());
            return Err(());
        }

        // Validate year
        let year: Option<i64> = if self.year.is_empty() {
            None
        } else {
            match self.year.parse::<i64>() {
                Ok(y) if (1993..=2100).contains(&y) => Some(y),
                _ => {
                    self.error_message = Some("Year must be 1993-2100".to_string());
                    return Err(());
                }
            }
        };

        // Validate complevel
        let complevel: Option<i64> = if self.complevel.is_empty() {
            None
        } else {
            match parse_complevel(&self.complevel) {
                Some(c) => Some(c as i64),
                None => {
                    self.error_message = Some("Invalid complevel".to_string());
                    return Err(());
                }
            }
        };

        // Build WadUpdate
        let status = Status::parse(&self.status);

        let mut update = WadUpdate::new();
        update = update.set_text("title", Some(self.title.clone())).unwrap();
        update = update
            .set_text("author", opt_str(&self.author))
            .unwrap();
        update = update.set_int("year", year).unwrap();

        if let Some(s) = status {
            update = update.set_status(s).unwrap();
        }

        let rating: Option<i64> = if self.rating.is_empty() {
            None
        } else {
            self.rating.parse().ok()
        };
        update = update.set_int("rating", rating).unwrap();

        update = update.set_text("notes", opt_str(&self.notes)).unwrap();
        update = update
            .set_text("description", opt_str(&self.description))
            .unwrap();

        // Sourceport tab fields
        update = update
            .set_text("custom_iwad", opt_str(&self.iwad))
            .unwrap();
        update = update
            .set_text("custom_sourceport", opt_str(&self.sourceport))
            .unwrap();
        update = update.set_int("complevel", complevel).unwrap();
        update = update
            .set_text("custom_config", opt_str(&self.config))
            .unwrap();
        update = update
            .set_text("custom_args", opt_str(&self.args))
            .unwrap();
        update = update
            .set_text("version", opt_str(&self.version))
            .unwrap();

        // Sources tab fields
        update = update
            .set_text("source_url", opt_str(&self.source_url))
            .unwrap();
        update = update
            .set_text("idgames_id", opt_str(&self.idgames_id))
            .unwrap();

        if let Err(e) = wads::update_wad(conn, self.wad_id, &update) {
            self.error_message = Some(format!("Save failed: {e}"));
            return Err(());
        }

        // Handle tags delta
        let new_tags: Vec<String> = self
            .tags
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn field_label(ui: &mut egui::Ui, label: &str) {
    ui.colored_label(
        theme::TEXT_SECONDARY,
        egui::RichText::new(label).strong(),
    );
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn opt_str(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
