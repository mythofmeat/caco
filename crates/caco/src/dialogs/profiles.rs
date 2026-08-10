//! Sourceport config profile management.
//!
//! The GUI answer to `caco profile`. Where the CLI shells out to `$EDITOR`,
//! this edits the profile contents in place — the operations themselves live in
//! `caco_core::profiles`, so both frontends share one implementation.

use rusqlite::Connection;

use caco_core::profiles::{self, Profile};

use crate::theme;

/// State for the profile management dialog.
pub struct ProfilesDialogState {
    profiles: Vec<Profile>,
    selected: Option<usize>,
    /// Contents of the selected profile, edited in place.
    buffer: String,
    /// Whether `buffer` has diverged from what is on disk.
    dirty: bool,
    /// Name typed into the create/duplicate field.
    new_name: String,
    /// Index awaiting delete confirmation, with the WADs still referencing it.
    pending_delete: Option<(usize, Vec<String>)>,
    status: Option<StatusLine>,
}

struct StatusLine {
    text: String,
    is_error: bool,
}

pub enum ProfilesResult {
    Open,
    Closed,
}

impl ProfilesDialogState {
    pub fn new() -> Self {
        let mut state = Self {
            profiles: Vec::new(),
            selected: None,
            buffer: String::new(),
            dirty: false,
            new_name: String::new(),
            pending_delete: None,
            status: None,
        };
        state.reload(None);
        state
    }

    /// Reload the profile list, keeping `keep_selected` selected if it survives.
    fn reload(&mut self, keep_selected: Option<(String, String)>) {
        self.profiles = profiles::list(None);
        self.pending_delete = None;

        self.selected = match keep_selected {
            Some((port, name)) => self
                .profiles
                .iter()
                .position(|p| p.sourceport == port && p.name == name),
            None => None,
        };
        if self.selected.is_none() && !self.profiles.is_empty() {
            self.selected = Some(0);
        }
        self.load_buffer();
    }

    /// Read the selected profile's contents into the edit buffer.
    fn load_buffer(&mut self) {
        self.dirty = false;
        self.buffer = match self.selected.and_then(|i| self.profiles.get(i)) {
            Some(p) => profiles::read(&p.sourceport, &p.name).unwrap_or_default(),
            None => String::new(),
        };
    }

    fn set_error(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine {
            text: text.into(),
            is_error: true,
        });
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine {
            text: text.into(),
            is_error: false,
        });
    }

    /// Sourceport to create new profiles under: the selected one, else default.
    fn target_sourceport(&self) -> Result<String, String> {
        if let Some(p) = self.selected.and_then(|i| self.profiles.get(i)) {
            return Ok(p.sourceport.clone());
        }
        profiles::resolve_sourceport(None).map_err(|e| e.to_string())
    }

    fn save_buffer(&mut self) {
        let Some(p) = self.selected.and_then(|i| self.profiles.get(i)) else {
            return;
        };
        let (port, name) = (p.sourceport.clone(), p.name.clone());
        match profiles::write(&port, &name, &self.buffer) {
            Ok(()) => {
                self.dirty = false;
                self.set_info(format!("Saved {port}/{name}."));
            }
            Err(e) => self.set_error(e.to_string()),
        }
    }

    fn create(&mut self, from_selected: bool) {
        let name = self.new_name.trim().to_string();
        if name.is_empty() {
            self.set_error("Enter a name first.");
            return;
        }

        let port = match self.target_sourceport() {
            Ok(p) => p,
            Err(e) => {
                self.set_error(e);
                return;
            }
        };

        let source = if from_selected {
            self.selected
                .and_then(|i| self.profiles.get(i))
                .map(|p| p.name.clone())
        } else {
            None
        };

        match profiles::create(&port, &name, source.as_deref()) {
            Ok(_) => {
                self.new_name.clear();
                self.set_info(format!("Created {port}/{name}."));
                self.reload(Some((port, name)));
            }
            Err(e) => self.set_error(e.to_string()),
        }
    }

    /// Stage a delete, collecting the WADs that still reference the profile so
    /// the confirmation can show what will be affected.
    fn stage_delete(&mut self, conn: &Connection, idx: usize) {
        let Some(p) = self.profiles.get(idx) else {
            return;
        };
        let referencing = profiles::referencing_wads(conn, &p.name)
            .unwrap_or_default()
            .into_iter()
            .map(|w| format!("{}: {}", w.id, w.title))
            .collect();
        self.pending_delete = Some((idx, referencing));
    }

    fn confirm_delete(&mut self) {
        let Some((idx, _)) = self.pending_delete.take() else {
            return;
        };
        let Some(p) = self.profiles.get(idx) else {
            return;
        };
        let (port, name) = (p.sourceport.clone(), p.name.clone());
        match profiles::remove(&port, &name) {
            Ok(()) => {
                self.set_info(format!("Deleted {port}/{name}."));
                self.reload(None);
            }
            Err(e) => self.set_error(e.to_string()),
        }
    }

    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> ProfilesResult {
        let mut result = ProfilesResult::Open;

        egui::Window::new("Sourceport Profiles")
            .collapsible(false)
            .resizable(true)
            .default_size([760.0, 480.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    self.render_list(ui);
                    ui.separator();
                    self.render_editor(ui);
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                if let Some((idx, referencing)) = self.pending_delete.clone() {
                    self.render_delete_confirmation(ui, idx, &referencing);
                } else {
                    self.render_actions(ui, conn, &mut result);
                }

                if let Some(status) = &self.status {
                    ui.add_space(4.0);
                    let color = if status.is_error {
                        theme::COLOR_ERROR
                    } else {
                        theme::TEXT_SECONDARY
                    };
                    ui.colored_label(color, &status.text);
                }
            });

        // Escape closes, unless it is dismissing a pending confirmation first.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.pending_delete.is_some() {
                self.pending_delete = None;
            } else {
                return ProfilesResult::Closed;
            }
        }

        result
    }

    fn render_list(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.set_min_width(220.0);
            ui.set_max_width(220.0);
            ui.strong("Profiles");
            ui.add_space(2.0);

            if self.profiles.is_empty() {
                ui.colored_label(theme::TEXT_SECONDARY, "No profiles yet.");
                return;
            }

            egui::ScrollArea::vertical()
                .max_height(340.0)
                .show(ui, |ui| {
                    let mut clicked = None;
                    for (idx, profile) in self.profiles.iter().enumerate() {
                        let label = format!("{}/{}", profile.sourceport, profile.name);
                        if ui
                            .selectable_label(self.selected == Some(idx), label)
                            .clicked()
                        {
                            clicked = Some(idx);
                        }
                    }
                    if let Some(idx) = clicked
                        && self.selected != Some(idx)
                    {
                        self.selected = Some(idx);
                        self.load_buffer();
                    }
                });
        });
    }

    fn render_editor(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            match self.selected.and_then(|i| self.profiles.get(i)) {
                Some(p) => {
                    ui.horizontal(|ui| {
                        ui.strong(format!("{}/{}", p.sourceport, p.name));
                        if self.dirty {
                            ui.colored_label(theme::TEXT_SECONDARY, "(unsaved)");
                        }
                    });
                    ui.colored_label(theme::TEXT_SECONDARY, p.path.display().to_string());
                }
                None => {
                    ui.strong("No profile selected");
                    ui.colored_label(theme::TEXT_SECONDARY, "Create one below.");
                }
            }
            ui.add_space(2.0);

            let enabled = self.selected.is_some();
            egui::ScrollArea::vertical()
                .max_height(340.0)
                .show(ui, |ui| {
                    let response = ui.add_enabled(
                        enabled,
                        egui::TextEdit::multiline(&mut self.buffer)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(18),
                    );
                    if response.changed() {
                        self.dirty = true;
                    }
                });
        });
    }

    fn render_delete_confirmation(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        referencing: &[String],
    ) {
        let label = self
            .profiles
            .get(idx)
            .map(|p| format!("{}/{}", p.sourceport, p.name))
            .unwrap_or_default();

        ui.colored_label(theme::COLOR_ERROR, format!("Delete {label}?"));
        if !referencing.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!(
                    "{} WAD{} still reference this profile:",
                    referencing.len(),
                    if referencing.len() == 1 { "" } else { "s" }
                ),
            );
            for entry in referencing.iter().take(5) {
                ui.colored_label(theme::TEXT_SECONDARY, format!("  {entry}"));
            }
            if referencing.len() > 5 {
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    format!("  … and {} more", referencing.len() - 5),
                );
            }
        }

        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                self.confirm_delete();
            }
            if ui.button("Cancel").clicked() {
                self.pending_delete = None;
            }
        });
    }

    fn render_actions(
        &mut self,
        ui: &mut egui::Ui,
        conn: &Connection,
        result: &mut ProfilesResult,
    ) {
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .desired_width(140.0)
                    .hint_text("new profile"),
            );

            if ui.button("Create").clicked() {
                self.create(false);
            }

            let has_selection = self.selected.is_some();
            if ui
                .add_enabled(has_selection, egui::Button::new("Duplicate"))
                .on_hover_text("Create a copy of the selected profile under the new name")
                .clicked()
            {
                self.create(true);
            }
        });

        ui.add_space(2.0);

        ui.horizontal(|ui| {
            let has_selection = self.selected.is_some();

            if ui
                .add_enabled(has_selection && self.dirty, egui::Button::new("Save"))
                .clicked()
            {
                self.save_buffer();
            }

            if ui
                .add_enabled(has_selection && self.dirty, egui::Button::new("Revert"))
                .clicked()
            {
                self.load_buffer();
            }

            if ui
                .add_enabled(has_selection, egui::Button::new("Delete"))
                .clicked()
                && let Some(idx) = self.selected
            {
                self.stage_delete(conn, idx);
            }

            if ui.button("Close").clicked() {
                *result = ProfilesResult::Closed;
            }
        });
    }
}

impl Default for ProfilesDialogState {
    fn default() -> Self {
        Self::new()
    }
}
