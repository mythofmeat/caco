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
    Open,
}

impl EditDialogState {
    /// Get the WAD title for breadcrumb display.
    pub fn title(&self) -> &str {
        &self.title_field
    }

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
            iwad: wad.custom_iwad.as_deref().unwrap_or("").to_string(),
            complevel: wad.complevel.map(|c| c.to_string()).unwrap_or_default(),
            config: wad.custom_config.as_deref().unwrap_or("").to_string(),
            args: wad.custom_args.as_deref().unwrap_or("").to_string(),
            version: wad.version.as_deref().unwrap_or("").to_string(),

            source_type_display: wad.source_type.as_str().to_string(),
            source_url: wad.source_url.as_deref().unwrap_or("").to_string(),
            idgames_id: wad.idgames_id.as_deref().unwrap_or("").to_string(),

            companions,
            companions_modified: false,

            adding_tag: false,
            new_tag_text: String::new(),

            error_message: None,
        })
    }

    /// Render the edit dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> EditResult {
        let mut result = EditResult::Open;
        let mut companion_action: Option<CompanionAction> = None;

        egui::Window::new("edit_wad_dialog")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size([560.0, 580.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(0x1a, 0x14, 0x10))
                    .corner_radius(16)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER_MED))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 32,
                        spread: 8,
                        color: egui::Color32::from_black_alpha(128),
                    }),
            )
            .show(ctx, |ui| {
                // ── Header with thumbnail + title ──
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(20, 14))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Thumbnail placeholder
                            let (c1, _c2, ci) = theme::thumb_colors(self.wad_id);
                            let (thumb_rect, _) = ui
                                .allocate_exact_size(egui::vec2(56.0, 42.0), egui::Sense::hover());
                            ui.painter().rect_filled(thumb_rect, 6.0, c1);
                            let initials: String = self
                                .title_field
                                .chars()
                                .filter(|c| c.is_alphanumeric())
                                .take(2)
                                .flat_map(|c| c.to_uppercase())
                                .collect();
                            ui.painter().text(
                                thumb_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &initials,
                                egui::FontId::proportional(16.0),
                                ci,
                            );

                            ui.add_space(12.0);

                            ui.vertical(|ui| {
                                ui.colored_label(
                                    theme::TEXT_PRIMARY,
                                    egui::RichText::new(&self.title_field).size(16.0).strong(),
                                );
                                let meta = format!(
                                    "{}{}{}",
                                    &self.author,
                                    if !self.year.is_empty() {
                                        " \u{00b7} "
                                    } else {
                                        ""
                                    },
                                    &self.year
                                );
                                if !meta.trim().is_empty() {
                                    ui.colored_label(
                                        theme::TEXT_SECONDARY,
                                        egui::RichText::new(meta).size(12.0),
                                    );
                                }
                            });

                            // Close button (right aligned)
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("\u{00d7}")
                                                    .size(18.0)
                                                    .color(theme::TEXT_SECONDARY),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        result = if self.companions_modified {
                                            EditResult::Modified
                                        } else {
                                            EditResult::Cancelled
                                        };
                                    }
                                },
                            );
                        });
                    });

                // ── Tab bar ──
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(0x16, 0x12, 0x0e))
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 20,
                        top: 0,
                        bottom: 0,
                    })
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            render_edit_tab(
                                ui,
                                "Metadata",
                                EditTab::Metadata,
                                &mut self.active_tab,
                            );
                            render_edit_tab(
                                ui,
                                "Sourceport",
                                EditTab::Sourceport,
                                &mut self.active_tab,
                            );
                            render_edit_tab(ui, "Sources", EditTab::Sources, &mut self.active_tab);
                            render_edit_tab(
                                ui,
                                "Companions",
                                EditTab::Companions,
                                &mut self.active_tab,
                            );
                        });
                    });

                // ── Error banner ──
                if let Some(err) = &self.error_message {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(0x2a, 0x0d, 0x0d))
                        .inner_margin(egui::Margin::symmetric(20, 6))
                        .show(ui, |ui| {
                            ui.colored_label(theme::COLOR_ERROR, err);
                        });
                }

                // ── Tab content ──
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(20, 16))
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 8.0;

                                match self.active_tab {
                                    EditTab::Metadata => self.render_metadata_tab(ui),
                                    EditTab::Sourceport => self.render_sourceport_tab(ui),
                                    EditTab::Sources => self.render_sources_tab(ui),
                                    EditTab::Companions => {
                                        companion_action = self.render_companions_tab(ui);
                                    }
                                }
                            });
                    });

                // ── Footer ──
                ui.add_space(4.0);
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(20, 10))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Delete button (left)
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Delete WAD").color(theme::COLOR_ERROR),
                                    )
                                    .fill(egui::Color32::from_rgb(0x2a, 0x0d, 0x0d))
                                    .stroke(egui::Stroke::new(
                                        1.0,
                                        egui::Color32::from_rgb(0x3a, 0x16, 0x16),
                                    )),
                                )
                                .clicked()
                            {
                                // TODO: confirm first, for now just close
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Save button
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("Save Changes")
                                                    .color(egui::Color32::WHITE)
                                                    .strong(),
                                            )
                                            .fill(theme::TEXT_ACCENT)
                                            .corner_radius(8),
                                        )
                                        .clicked()
                                        && self.save(conn).is_ok()
                                    {
                                        result = EditResult::Saved;
                                    }

                                    // Cancel button
                                    if ui
                                        .add(
                                            egui::Button::new("Cancel")
                                                .fill(theme::BG_LIGHT)
                                                .stroke(egui::Stroke::new(1.0, theme::BORDER_MED)),
                                        )
                                        .clicked()
                                    {
                                        result = if self.companions_modified {
                                            EditResult::Modified
                                        } else {
                                            EditResult::Cancelled
                                        };
                                    }
                                },
                            );
                        });
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
        form_label(ui, "Title");
        ui.add(
            egui::TextEdit::singleline(&mut self.title_field)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY),
        );

        ui.columns(2, |cols| {
            form_label(&mut cols[0], "Author");
            cols[0].add(
                egui::TextEdit::singleline(&mut self.author)
                    .desired_width(f32::INFINITY)
                    .text_color(theme::TEXT_PRIMARY),
            );
            form_label(&mut cols[1], "Year");
            cols[1].add(
                egui::TextEdit::singleline(&mut self.year)
                    .desired_width(f32::INFINITY)
                    .text_color(theme::TEXT_PRIMARY),
            );
        });

        // Status picker (clickable pills — mutually exclusive)
        form_label(ui, "Status");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for &status in Status::ALL {
                let color = theme::status_color(status);
                let bg = theme::status_bg(status);
                let is_selected = self.status == status.as_str();

                let stroke = if is_selected {
                    egui::Stroke::new(1.5, color)
                } else {
                    egui::Stroke::NONE
                };

                let response = egui::Frame::new()
                    .fill(bg)
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(12, 4))
                    .stroke(stroke)
                    .show(ui, |ui| {
                        ui.colored_label(
                            color,
                            egui::RichText::new(theme::status_display(status))
                                .size(12.0)
                                .strong(),
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
                    .fill(egui::Color32::from_rgb(0x26, 0x1c, 0x14))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.colored_label(
                                egui::Color32::from_rgb(0xcc, 0x77, 0x44),
                                egui::RichText::new(tag).size(12.0),
                            );
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
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.new_tag_text)
                        .desired_width(80.0)
                        .hint_text("tag name")
                        .text_color(theme::TEXT_PRIMARY),
                );
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
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_premultiplied(0x3a, 0x2e, 0x24, 128),
                    ))
                    .corner_radius(8),
                )
                .clicked()
            {
                self.adding_tag = true;
            }
        });

        form_label(ui, "Notes");
        ui.add(
            egui::TextEdit::singleline(&mut self.notes)
                .desired_width(f32::INFINITY)
                .hint_text("Personal notes...")
                .text_color(theme::TEXT_PRIMARY),
        );

        form_label(ui, "Description");
        ui.add(
            egui::TextEdit::multiline(&mut self.description)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .text_color(theme::TEXT_PRIMARY),
        );
    }

    fn render_sourceport_tab(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |cols| {
            form_label(&mut cols[0], "Sourceport");
            cols[0].add(
                egui::TextEdit::singleline(&mut self.sourceport)
                    .desired_width(f32::INFINITY)
                    .hint_text("default")
                    .text_color(theme::TEXT_PRIMARY),
            );
            form_label(&mut cols[1], "IWAD");
            cols[1].add(
                egui::TextEdit::singleline(&mut self.iwad)
                    .desired_width(f32::INFINITY)
                    .hint_text("auto-detect")
                    .text_color(theme::TEXT_PRIMARY),
            );
        });

        ui.columns(3, |cols| {
            form_label(&mut cols[0], "Complevel");
            cols[0].add(
                egui::TextEdit::singleline(&mut self.complevel)
                    .desired_width(f32::INFINITY)
                    .text_color(theme::TEXT_PRIMARY),
            );
            form_label(&mut cols[1], "Config Profile");
            cols[1].add(
                egui::TextEdit::singleline(&mut self.config)
                    .desired_width(f32::INFINITY)
                    .hint_text("default")
                    .text_color(theme::TEXT_PRIMARY),
            );
            form_label(&mut cols[2], "Version");
            cols[2].add(
                egui::TextEdit::singleline(&mut self.version)
                    .desired_width(f32::INFINITY)
                    .text_color(theme::TEXT_PRIMARY),
            );
        });

        form_label(ui, "Custom Args (JSON)");
        ui.add(
            egui::TextEdit::singleline(&mut self.args)
                .desired_width(f32::INFINITY)
                .hint_text("[\"--arg1\", \"value\"]")
                .text_color(theme::TEXT_PRIMARY),
        );
    }

    fn render_sources_tab(&mut self, ui: &mut egui::Ui) {
        form_label(ui, "Source Type");
        ui.add_enabled(
            false,
            egui::TextEdit::singleline(&mut self.source_type_display).desired_width(f32::INFINITY),
        );

        form_label(ui, "Source URL");
        ui.add(
            egui::TextEdit::singleline(&mut self.source_url)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY),
        );

        form_label(ui, "idgames ID");
        ui.add(
            egui::TextEdit::singleline(&mut self.idgames_id)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY),
        );
    }

    fn render_companions_tab(&mut self, ui: &mut egui::Ui) -> Option<CompanionAction> {
        let mut action = None;

        if self.companions.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No companion files linked to this WAD.",
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
                                        .clicked()
                                    {
                                        action = Some(CompanionAction::Remove(cid));
                                    }

                                    // Toggle
                                    if ui.checkbox(&mut entry.enabled, "").changed() {
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
                    .stroke(egui::Stroke::new(1.0, theme::BORDER_MED)),
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
                            self.error_message = Some(format!("Failed to add companion: {e}"));
                        }
                    }
                }
            }
            CompanionAction::Remove(companion_id) => {
                match companion_service::unregister_companion(conn, self.wad_id, companion_id, None)
                {
                    Ok(_) => {
                        self.companions_modified = true;
                        self.reload_companions(conn);
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to remove companion: {e}"));
                    }
                }
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

    fn reload_companions(&mut self, conn: &Connection) {
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

fn form_label(ui: &mut egui::Ui, label: &str) {
    ui.colored_label(
        theme::TEXT_SECONDARY,
        egui::RichText::new(label).size(12.0).strong(),
    );
}

fn render_edit_tab(ui: &mut egui::Ui, label: &str, tab: EditTab, active: &mut EditTab) {
    let is_active = *active == tab;
    let text_color = if is_active {
        theme::TEXT_ACCENT
    } else {
        theme::TEXT_SECONDARY
    };

    let response = ui.add(
        egui::Button::new(egui::RichText::new(label).size(13.0).color(text_color)).frame(false),
    );

    if is_active {
        let rect = response.rect;
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(2.0, theme::TEXT_ACCENT),
        );
    }

    if response.clicked() {
        *active = tab;
    }
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
