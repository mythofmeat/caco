use caco_core::companion_service::{self, OrphanPolicy};
use caco_core::complevel::parse_complevel;
use caco_core::db::companions;
use caco_core::db::models::Status;
use caco_core::db::wads::{self, WadUpdate};
use rusqlite::Connection;

use crate::dialogs::wad_file::WadFileSection;
use crate::dialogs::{Dialog, Primary, form_label, tab_bar};
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
    /// Answer to the "this file will be orphaned" prompt.
    ResolveOrphan(OrphanPolicy),
    CancelOrphan,
}

/// A removal held back because the managed file would be left with no owner
/// and the configured policy is `ask`. Without this the GUI silently kept
/// every orphan, since it had no way to put the question to the user.
struct PendingOrphan {
    companion_id: i64,
    filename: String,
}

// ---------------------------------------------------------------------------
// EditDialogState
// ---------------------------------------------------------------------------

pub struct EditDialogState {
    wad_id: i64,
    active_tab: EditTab,

    // Metadata tab
    title_field: String,
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
    sourceport_family: String,
    iwad: String,
    complevel: String,
    config: String,
    args: String,
    version: String,

    // Sources tab
    source_type_display: String,
    source_url: String,
    idgames_id: String,
    file: WadFileSection,
    /// A mirror the File section just asked to upload to, for `render` to pass on.
    upload_requested: Option<String>,

    // Companions tab
    companions: Vec<CompanionEntry>,
    companions_modified: bool,
    /// Async file-picker for the "Add companion" button, polled each frame.
    pending_companion_pick: Option<crate::workers::FileDialogReceiver>,
    /// Removal awaiting the user's answer on what to do with the managed file.
    pending_orphan: Option<PendingOrphan>,

    // Tag add state
    adding_tag: bool,
    new_tag_text: String,

    pub error_message: Option<String>,
}

/// Result of showing the edit dialog.
pub enum EditResult {
    Saved,
    Cancelled,
    /// Companions were modified but form was cancelled — triggers reload without notification.
    Modified,
    /// Upload the WAD's file to this mirror. The dialog stays open.
    UploadToMirror {
        wad_id: i64,
        mirror: String,
    },
    /// Ask whether to delete this WAD. Goes to the question every delete goes through.
    Delete(i64),
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

            title_field: wad.title.clone(),
            author: wad.author.as_deref().unwrap_or("").to_string(),
            year: wad.year.map(|y| y.to_string()).unwrap_or_default(),
            status: wad.status.as_str().to_string(),
            rating: wad.rating.map(|r| r.to_string()).unwrap_or_default(),
            tags: wad.tags.join(", "),
            notes: wad.notes.as_deref().unwrap_or("").to_string(),
            description: wad.description.as_deref().unwrap_or("").to_string(),
            original_tags,

            sourceport: wad.custom_sourceport.as_deref().unwrap_or("").to_string(),
            sourceport_family: wad
                .required_sourceport_family
                .as_deref()
                .unwrap_or("")
                .to_string(),
            iwad: wad.custom_iwad.as_deref().unwrap_or("").to_string(),
            complevel: wad.complevel.map(|c| c.to_string()).unwrap_or_default(),
            config: wad.custom_config.as_deref().unwrap_or("").to_string(),
            args: wad.custom_args.as_deref().unwrap_or("").to_string(),
            version: wad.version.as_deref().unwrap_or("").to_string(),

            source_type_display: wad.source_type.as_str().to_string(),
            source_url: wad.source_url.as_deref().unwrap_or("").to_string(),
            idgames_id: wad.idgames_id.as_deref().unwrap_or("").to_string(),
            file: WadFileSection::load(conn, &wad),
            upload_requested: None,

            companions,
            companions_modified: false,
            pending_companion_pick: None,
            pending_orphan: None,

            adding_tag: false,
            new_tag_text: String::new(),

            error_message: None,
        })
    }

    /// Render the edit dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> EditResult {
        // Pick up the result of an async companion file-picker if it's ready.
        if let Some(rx) = &self.pending_companion_pick
            && let Ok(picked) = rx.try_recv()
        {
            self.pending_companion_pick = None;
            if let Some(path) = picked {
                match companion_service::register_companion(conn, self.wad_id, &path) {
                    Ok(_) => {
                        self.companions_modified = true;
                        self.reload_companions(conn);
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to add companion: {e}"));
                    }
                }
            }
        }

        let mut result = EditResult::Open;
        let mut companion_action: Option<CompanionAction> = None;
        let dismissed = if self.companions_modified {
            EditResult::Modified
        } else {
            EditResult::Cancelled
        };

        let about = match (self.author.is_empty(), self.year.is_empty()) {
            (false, false) => format!("{} \u{00b7} {}", self.author, self.year),
            (false, true) => self.author.clone(),
            (true, false) => self.year.clone(),
            (true, true) => "No author or year yet".to_string(),
        };
        // Keyed, because its title is what is being edited.
        Dialog::new(self.title_field.clone(), [600.0, 620.0])
            .keyed("Edit WAD")
            .about(about)
            .show(ctx, |dialog| {
                dialog
                    .pinned(|ui| {
                        tab_bar(
                            ui,
                            &mut self.active_tab,
                            &[
                                (EditTab::Metadata, "Details"),
                                (EditTab::Sourceport, "Sourceport"),
                                (EditTab::Sources, "Sources"),
                                (EditTab::Companions, "Companions"),
                            ],
                        );
                    })
                    .scroll(|ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        match self.active_tab {
                            EditTab::Metadata => self.render_metadata_tab(ui),
                            EditTab::Sourceport => self.render_sourceport_tab(ui),
                            EditTab::Sources => self.render_sources_tab(ui),
                            EditTab::Companions => {
                                companion_action = self.render_companions_tab(ui);
                            }
                        }
                    })
                    .above(|ui| {
                        if let Some(err) = &self.error_message {
                            ui.colored_label(theme::COLOR_ERROR, err);
                        }
                    })
                    .buttons(|row| {
                        row.left(|ui| {
                            if ui
                                .button(egui::RichText::new("Delete WAD").color(theme::COLOR_ERROR))
                                .clicked()
                            {
                                result = EditResult::Delete(self.wad_id);
                            }
                        });
                        let ended = row.end(Some("Cancel"), Some(Primary::new("Save Changes")));
                        if ended.dismissed {
                            result = dismissed;
                        }
                        if ended.primary && self.save(conn).is_ok() {
                            result = EditResult::Saved;
                        }
                    })
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
            self.process_companion_action(action, conn, ctx);
        }

        // Dialog state rather than an assignment in the tab, which renders inside the
        // window's closure. The save and cancel buttons win if they were pressed in the
        // same frame, which only a test could manage.
        if let (EditResult::Open, Some(mirror)) = (&result, self.upload_requested.take()) {
            return EditResult::UploadToMirror {
                wad_id: self.wad_id,
                mirror,
            };
        }

        result
    }

    // -----------------------------------------------------------------------
    // Tab renderers
    // -----------------------------------------------------------------------

    fn render_metadata_tab(&mut self, ui: &mut egui::Ui) {
        let label = form_label(ui, "Title");
        ui.add(
            egui::TextEdit::singleline(&mut self.title_field)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY),
        )
        .labelled_by(label.id);

        ui.columns(2, |cols| {
            let label = form_label(&mut cols[0], "Author");
            cols[0]
                .add(
                    egui::TextEdit::singleline(&mut self.author)
                        .desired_width(f32::INFINITY)
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
            let label = form_label(&mut cols[1], "Year");
            cols[1]
                .add(
                    egui::TextEdit::singleline(&mut self.year)
                        .desired_width(f32::INFINITY)
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
        });

        // Status picker (clickable pills — mutually exclusive)
        form_label(ui, "Status");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for &status in Status::ALL {
                let style = theme::status_style(status);
                let is_selected = self.status == status.as_str();

                let stroke = if is_selected {
                    egui::Stroke::new(1.5_f32, style.color)
                } else {
                    egui::Stroke::NONE
                };

                let response = egui::Frame::new()
                    .fill(style.fill)
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(12, 4))
                    .stroke(stroke)
                    .show(ui, |ui| {
                        ui.colored_label(
                            style.color,
                            egui::RichText::new(style.word).size(12.0).strong(),
                        );
                    })
                    .response;

                if response.interact(egui::Sense::click()).clicked() {
                    self.status = status.as_str().to_string();
                }
            }
        });

        // Rating (clickable stars)
        form_label(ui, "Rating");
        ui.horizontal(|ui| {
            let current_rating: i32 = self.rating.parse().unwrap_or(0);
            for i in 1..=5 {
                let is_filled = i <= current_rating;
                let star = if is_filled { "\u{2605}" } else { "\u{2606}" };
                let color = if is_filled {
                    theme::TEXT_ACCENT
                } else {
                    theme::BORDER_MED
                };
                let response = ui.add(
                    egui::Label::new(egui::RichText::new(star).color(color).size(22.0))
                        .sense(egui::Sense::click()),
                );
                if response.clicked() {
                    self.rating = if i == current_rating {
                        String::new()
                    } else {
                        i.to_string()
                    };
                }
            }
            if current_rating > 0
                && ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("clear")
                                .size(11.0)
                                .color(theme::TEXT_MUTED),
                        )
                        .frame(false),
                    )
                    .clicked()
            {
                self.rating.clear();
            }
        });

        // Tags (pill editor)
        form_label(ui, "Tags");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.spacing_mut().item_spacing.y = 4.0;

            let mut tag_list: Vec<String> = self
                .tags
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            let mut remove_idx = None;

            for (i, tag) in tag_list.iter().enumerate() {
                egui::Frame::new()
                    .fill(theme::BG_LIGHT)
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.colored_label(theme::TEXT_TAG, egui::RichText::new(tag).size(12.0));
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("\u{00d7}")
                                            .size(11.0)
                                            .color(theme::TEXT_SECONDARY),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                remove_idx = Some(i);
                            }
                        });
                    });
            }

            if let Some(idx) = remove_idx {
                tag_list.remove(idx);
                self.tags = tag_list.join(", ");
            }

            // Add tag button / inline input
            if self.adding_tag {
                let response = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.new_tag_text)
                            .desired_width(80.0)
                            .hint_text("tag name")
                            .text_color(theme::TEXT_PRIMARY),
                    )
                    .accessible_name("New tag");
                if response.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !self.new_tag_text.trim().is_empty()
                    {
                        let new_tag = self.new_tag_text.trim().to_lowercase();
                        if !tag_list.contains(&new_tag) {
                            tag_list.push(new_tag);
                            self.tags = tag_list.join(", ");
                        }
                    }
                    self.adding_tag = false;
                    self.new_tag_text.clear();
                } else {
                    // Request focus on first frame
                    response.request_focus();
                }
            } else if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("+ add tag")
                            .size(12.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_MED))
                    .corner_radius(8),
                )
                .clicked()
            {
                self.adding_tag = true;
            }
        });

        let label = form_label(ui, "Notes");
        ui.add(
            egui::TextEdit::singleline(&mut self.notes)
                .desired_width(f32::INFINITY)
                .hint_text("Personal notes...")
                .text_color(theme::TEXT_PRIMARY),
        )
        .labelled_by(label.id);

        let label = form_label(ui, "Description");
        ui.add(
            egui::TextEdit::multiline(&mut self.description)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .text_color(theme::TEXT_PRIMARY),
        )
        .labelled_by(label.id);
    }

    fn render_sourceport_tab(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |cols| {
            let label = form_label(&mut cols[0], "Sourceport");
            cols[0]
                .add(
                    egui::TextEdit::singleline(&mut self.sourceport)
                        .desired_width(f32::INFINITY)
                        .hint_text("the default one")
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
            let label = form_label(&mut cols[1], "Needs a sourceport like");
            cols[1]
                .add(
                    egui::TextEdit::singleline(&mut self.sourceport_family)
                        .desired_width(f32::INFINITY)
                        .hint_text("any")
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
        });

        ui.columns(2, |cols| {
            let label = form_label(&mut cols[0], "IWAD");
            cols[0]
                .add(
                    egui::TextEdit::singleline(&mut self.iwad)
                        .desired_width(f32::INFINITY)
                        .hint_text("worked out from the file")
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
            cols[1].add_space(0.0);
        });

        ui.columns(3, |cols| {
            let label = form_label(&mut cols[0], "Complevel");
            cols[0]
                .add(
                    egui::TextEdit::singleline(&mut self.complevel)
                        .desired_width(f32::INFINITY)
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
            let label = form_label(&mut cols[1], "Profile");
            cols[1]
                .add(
                    egui::TextEdit::singleline(&mut self.config)
                        .desired_width(f32::INFINITY)
                        .hint_text("none")
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
            let label = form_label(&mut cols[2], "Version");
            cols[2]
                .add(
                    egui::TextEdit::singleline(&mut self.version)
                        .desired_width(f32::INFINITY)
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
        });

        let label = form_label(ui, "Launch arguments (JSON)");
        ui.add(
            egui::TextEdit::singleline(&mut self.args)
                .desired_width(f32::INFINITY)
                .hint_text("[\"--arg1\", \"value\"]")
                .text_color(theme::TEXT_PRIMARY),
        )
        .labelled_by(label.id);
    }

    fn render_sources_tab(&mut self, ui: &mut egui::Ui) {
        let label = form_label(ui, "Source Type");
        ui.add_enabled(
            false,
            egui::TextEdit::singleline(&mut self.source_type_display).desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);

        let label = form_label(ui, "Source URL");
        ui.add(
            egui::TextEdit::singleline(&mut self.source_url)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY),
        )
        .labelled_by(label.id);

        let label = form_label(ui, "idgames ID");
        ui.add(
            egui::TextEdit::singleline(&mut self.idgames_id)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY),
        )
        .labelled_by(label.id);

        ui.add_space(6.0);
        form_label(ui, "File");
        self.upload_requested = self.file.render(ui);
    }

    fn render_companions_tab(&mut self, ui: &mut egui::Ui) -> Option<CompanionAction> {
        let mut action = None;

        if let Some(pending) = &self.pending_orphan {
            ui.colored_label(
                theme::COLOR_ERROR,
                format!("No other WAD uses '{}'.", pending.filename),
            );
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "Delete caco's copy of it too, or keep it for later?",
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Remove & Delete File").clicked() {
                    action = Some(CompanionAction::ResolveOrphan(OrphanPolicy::Delete));
                }
                if ui.button("Remove & Keep File").clicked() {
                    action = Some(CompanionAction::ResolveOrphan(OrphanPolicy::Keep));
                }
                if ui.button("Cancel").clicked() {
                    action = Some(CompanionAction::CancelOrphan);
                }
            });
            return action;
        }

        if self.companions.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No companion files for this WAD. A companion is a file that loads with it, such \
                 as a DeHackEd patch or a music pack.",
            );
            ui.add_space(8.0);
        } else {
            for entry in &mut self.companions {
                let cid = entry.companion_id;

                egui::Frame::new()
                    .fill(theme::BG_MEDIUM)
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(theme::TEXT_PRIMARY, &entry.filename);
                            ui.colored_label(theme::TEXT_MUTED, format_size(entry.size));

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Remove button
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("\u{00d7}")
                                                    .color(theme::TEXT_MUTED),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_text("Remove from this WAD")
                                        .clicked()
                                    {
                                        action = Some(CompanionAction::Remove(cid));
                                    }

                                    // Toggle
                                    if ui
                                        .checkbox(&mut entry.enabled, "")
                                        .on_hover_text("Load it with this WAD")
                                        .changed()
                                    {
                                        action = Some(CompanionAction::Toggle(cid, entry.enabled));
                                    }
                                },
                            );
                        });
                    });
            }

            ui.add_space(8.0);
        }

        if ui
            .add(
                egui::Button::new("+ Add Companion")
                    .fill(theme::BG_LIGHT)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_MED)),
            )
            .clicked()
        {
            action = Some(CompanionAction::Add);
        }

        action
    }

    // -----------------------------------------------------------------------
    // Companion action processing
    // -----------------------------------------------------------------------

    fn process_companion_action(
        &mut self,
        action: CompanionAction,
        conn: &Connection,
        ctx: &egui::Context,
    ) {
        match action {
            CompanionAction::Add => {
                // Skip if a picker is already in flight.
                if self.pending_companion_pick.is_some() {
                    return;
                }
                let req = crate::workers::FileDialogRequest::open()
                    .add_filter("Companion Files", &["deh", "bex", "wad", "pk3", "zip"])
                    .set_directory(dirs::home_dir().unwrap_or_default());
                self.pending_companion_pick =
                    Some(crate::workers::spawn_file_dialog(Some(ctx.clone()), req));
            }
            CompanionAction::Remove(companion_id) => {
                // `None` means the managed file would be orphaned and the
                // config says to ask — stage the question instead of guessing.
                match companion_service::plan_unregister(conn, self.wad_id, companion_id) {
                    Ok(Some(policy)) => self.remove_companion(conn, companion_id, policy),
                    Ok(None) => {
                        let filename = self
                            .companions
                            .iter()
                            .find(|c| c.companion_id == companion_id)
                            .map(|c| c.filename.clone())
                            .unwrap_or_default();
                        self.pending_orphan = Some(PendingOrphan {
                            companion_id,
                            filename,
                        });
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to remove companion: {e}"));
                    }
                }
            }
            CompanionAction::ResolveOrphan(policy) => {
                if let Some(pending) = self.pending_orphan.take() {
                    self.remove_companion(conn, pending.companion_id, policy);
                }
            }
            CompanionAction::CancelOrphan => {
                self.pending_orphan = None;
            }
            CompanionAction::Toggle(companion_id, enabled) => {
                match companions::set_companion_enabled(conn, self.wad_id, companion_id, enabled) {
                    Ok(_) => {
                        self.companions_modified = true;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to toggle companion: {e}"));
                    }
                }
            }
        }
    }

    fn remove_companion(&mut self, conn: &Connection, companion_id: i64, policy: OrphanPolicy) {
        match companion_service::unregister_companion(conn, self.wad_id, companion_id, policy) {
            Ok(_) => {
                self.companions_modified = true;
                self.reload_companions(conn);
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to remove companion: {e}"));
            }
        }
    }

    fn reload_companions(&mut self, conn: &Connection) {
        self.pending_orphan = None;
        let records = companions::get_companions_for_wad(conn, self.wad_id).unwrap_or_default();
        self.companions = records.iter().map(CompanionEntry::from_record).collect();
    }

    // -----------------------------------------------------------------------
    // Save (persists Metadata + Sourceport + Sources tabs)
    // -----------------------------------------------------------------------

    fn save(&mut self, conn: &Connection) -> Result<(), ()> {
        self.error_message = None;

        // Validate title
        if self.title_field.is_empty() {
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

        let status = Status::parse(&self.status).unwrap_or(Status::Unplayed);

        let rating: Option<i64> = if self.rating.is_empty() {
            None
        } else {
            self.rating.parse().ok()
        };

        let custom_args = if self.args.trim().is_empty() {
            None
        } else {
            match caco_core::player::normalize_custom_args(&self.args) {
                Ok(json) => Some(json),
                Err(e) => {
                    self.error_message = Some(format!("Invalid args: {e}"));
                    return Err(());
                }
            }
        };

        let update = WadUpdate::new()
            .set_text("title", Some(self.title_field.clone()))
            .set_text("author", opt_str(&self.author))
            .set_int("year", year)
            .set_status(status)
            .set_int("rating", rating)
            .set_text("notes", opt_str(&self.notes))
            .set_text("description", opt_str(&self.description))
            // Sourceport tab fields
            .set_text("custom_iwad", opt_str(&self.iwad))
            .set_text("custom_sourceport", opt_str(&self.sourceport))
            .set_text(
                "required_sourceport_family",
                opt_str(&self.sourceport_family),
            )
            .set_int("complevel", complevel)
            .set_text("custom_config", opt_str(&self.config))
            .set_text("custom_args", custom_args)
            .set_text("version", opt_str(&self.version))
            // Sources tab fields
            .set_text("source_url", opt_str(&self.source_url))
            .set_text("idgames_id", opt_str(&self.idgames_id));

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
