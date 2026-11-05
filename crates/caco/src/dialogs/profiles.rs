//! Sourceport config profile management: the contents are edited in place, and the
//! operations themselves live in `caco_core::profiles`.

use rusqlite::Connection;

use caco_core::profiles::{self, Profile};

use crate::dialogs::{Dialog, Primary, StatusLine, count, form_label, scroll_body};
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
            Some((sourceport, name)) => self
                .profiles
                .iter()
                .position(|p| p.sourceport == sourceport && p.name == name),
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
        self.status = Some(StatusLine::error(text));
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine::info(text));
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
        let (sourceport, name) = (p.sourceport.clone(), p.name.clone());
        match profiles::write(&sourceport, &name, &self.buffer) {
            Ok(()) => {
                self.dirty = false;
                self.set_info(format!("Saved {sourceport}/{name}."));
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

        let sourceport = match self.target_sourceport() {
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

        match profiles::create(&sourceport, &name, source.as_deref()) {
            Ok(_) => {
                self.new_name.clear();
                self.set_info(format!("Created {sourceport}/{name}."));
                self.reload(Some((sourceport, name)));
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
            .map(|w| w.title)
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
        let (sourceport, name) = (p.sourceport.clone(), p.name.clone());
        match profiles::remove(&sourceport, &name) {
            Ok(()) => {
                self.set_info(format!("Deleted {sourceport}/{name}."));
                self.reload(None);
            }
            Err(e) => self.set_error(e.to_string()),
        }
    }

    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> ProfilesResult {
        let mut result = ProfilesResult::Open;

        Dialog::new("Profiles", [780.0, 520.0])
            .about("Sourceport settings you can give each WAD")
            .show(ctx, |dialog| {
                dialog
                    .fill(|ui, height| {
                        ui.horizontal_top(|ui| {
                            self.render_list(ui, height);
                            ui.separator();
                            self.render_editor(ui, height);
                        });
                    })
                    .above(|ui| {
                        if let Some((idx, referencing)) = self.pending_delete.clone() {
                            self.render_delete_question(ui, idx, &referencing);
                        } else {
                            self.render_create_row(ui);
                        }
                        if let Some(status) = &self.status {
                            status.show(ui);
                        }
                    })
                    .buttons(|row| {
                        if self.pending_delete.is_some() {
                            let ended = row.end(Some("Cancel"), Some(Primary::danger("Delete")));
                            if ended.dismissed {
                                self.pending_delete = None;
                            }
                            if ended.primary {
                                self.confirm_delete();
                            }
                            return;
                        }
                        row.left(|ui| {
                            if self.dirty {
                                if ui.button("Save").clicked() {
                                    self.save_buffer();
                                }
                                if ui.button("Revert").clicked() {
                                    self.load_buffer();
                                }
                            }
                            if let Some(idx) = self.selected
                                && ui.button("Delete").clicked()
                            {
                                self.stage_delete(conn, idx);
                            }
                        });
                        if row.close() {
                            result = ProfilesResult::Closed;
                        }
                    })
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

    fn render_list(&mut self, ui: &mut egui::Ui, height: f32) {
        ui.vertical(|ui| {
            ui.set_width(220.0);
            ui.set_height(height);

            if self.profiles.is_empty() {
                ui.colored_label(theme::TEXT_SECONDARY, "No profiles yet.");
                return;
            }

            let mut clicked = None;
            scroll_body(ui, height, |ui| {
                for (idx, profile) in self.profiles.iter().enumerate() {
                    let label = format!("{}/{}", profile.sourceport, profile.name);
                    if ui
                        .selectable_label(self.selected == Some(idx), label)
                        .clicked()
                    {
                        clicked = Some(idx);
                    }
                }
            });
            if let Some(idx) = clicked
                && self.selected != Some(idx)
            {
                self.selected = Some(idx);
                self.load_buffer();
            }
        });
    }

    fn render_editor(&mut self, ui: &mut egui::Ui, height: f32) {
        ui.vertical(|ui| {
            ui.set_height(height);
            let Some(p) = self.selected.and_then(|i| self.profiles.get(i)) else {
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    "A profile is a sourceport's config file, kept by caco, which a WAD can use \
                     instead of the sourceport's own. Create one below.",
                );
                return;
            };
            ui.horizontal(|ui| {
                ui.strong(format!("{}/{}", p.sourceport, p.name));
                if self.dirty {
                    ui.colored_label(theme::TEXT_SECONDARY, "(not saved)");
                }
            });
            ui.add_space(2.0);

            scroll_body(ui, height - 24.0, |ui| {
                let response = ui
                    .add(
                        egui::TextEdit::multiline(&mut self.buffer)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(18),
                    )
                    .accessible_name("Profile contents");
                if response.changed() {
                    self.dirty = true;
                }
            });
        });
    }

    fn render_delete_question(&mut self, ui: &mut egui::Ui, idx: usize, referencing: &[String]) {
        let label = self
            .profiles
            .get(idx)
            .map(|p| format!("{}/{}", p.sourceport, p.name))
            .unwrap_or_default();

        ui.colored_label(theme::COLOR_ERROR, format!("Delete {label}?"));
        if referencing.is_empty() {
            return;
        }
        ui.colored_label(
            theme::TEXT_SECONDARY,
            format!(
                "{} still using it:",
                count(referencing.len(), "WAD is", "WADs are")
            ),
        );
        for entry in referencing.iter().take(5) {
            ui.colored_label(theme::TEXT_SECONDARY, format!("  {entry}"));
        }
        if referencing.len() > 5 {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!("  \u{2026} and {} more", referencing.len() - 5),
            );
        }
    }

    /// Making a new profile, from nothing or from the one selected.
    fn render_create_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = form_label(ui, "New profile");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .desired_width(160.0)
                    .hint_text("name"),
            )
            .labelled_by(label.id);

            if ui.button("Create").clicked() {
                self.create(false);
            }
            if self.selected.is_some()
                && ui
                    .button("Duplicate")
                    .on_hover_text("A copy of the selected profile, under this name")
                    .clicked()
            {
                self.create(true);
            }
        });
    }
}

impl Default for ProfilesDialogState {
    fn default() -> Self {
        Self::new()
    }
}
