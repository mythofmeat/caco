//! Settings dialog — edit the caco config and persist it to config.toml.
//!
//! The dialog edits a snapshot of the current [`Config`]. Saving writes the
//! whole struct back via `save_config` and swaps the live snapshot with
//! `reload_config`, so changes apply immediately without a restart (except
//! `db_path`, which is only read at startup). Sections not shown here
//! (tui/list, sourceport preferences, IWAD priority) are preserved verbatim.

use std::collections::HashMap;

use caco_core::config::{self, Config, MirrorConfig};
use caco_core::sourceports;
use caco_sources::idgames::MIRRORS;
use rusqlite::Connection;

use crate::theme;

/// State for the settings dialog.
pub struct SettingsDialogState {
    // Sourceports
    sourceport: String,
    zdoom_sourceport: String,
    /// Shell-style quoted argument string (parsed with shlex).
    sourceport_args: String,
    /// Per-sourceport launch args: (executable basename, shlex arg string).
    executable_args: Vec<(String, String)>,
    /// Entry field for adding a sourceport to the per-sourceport args list.
    new_sourceport_name: String,
    detected_sourceports: Vec<String>,

    // Behavior
    iwad: String,
    link_mode: String,
    companion_orphan_cleanup: String,
    download_mirror: i64,
    manage_data_dirs: bool,
    auto_stats: bool,
    auto_detect_iwad: bool,
    auto_detect_complevel: bool,
    auto_doomwiki_enrich: bool,

    // Cache
    cache_auto_clean: bool,
    cache_max_size_gb: f64,
    cache_max_age_days: i64,
    sourceport_update_check_days: i64,

    // Mirrors
    mirrors: Vec<MirrorRow>,
    /// How many files each mirror's index listed, the last time it was read.
    mirror_file_counts: HashMap<String, i64>,

    // Paths
    cache_dir: String,
    data_dir: String,
    iwad_dir: String,
    profile_dir: String,
    db_path: String,

    error: Option<String>,
}

/// One row of the Mirrors section: text until Save makes a [`MirrorConfig`] of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MirrorRow {
    name: String,
    /// A URL or a path. One field, since which it is shows in how it starts.
    location: String,
    token_env: String,
    write: bool,
}

impl MirrorRow {
    fn from_config(mirror: &MirrorConfig) -> Self {
        Self {
            name: mirror.name.clone(),
            location: mirror
                .url
                .clone()
                .or_else(|| mirror.path.clone())
                .unwrap_or_default(),
            token_env: mirror.token_env.clone().unwrap_or_default(),
            write: mirror.write,
        }
    }

    fn to_config(&self) -> MirrorConfig {
        let location = self.location.trim();
        let filled = |text: &str| (!text.is_empty()).then(|| text.to_string());
        let (url, path) = if location.contains("://") {
            (filled(location), None)
        } else {
            (None, filled(location))
        };
        MirrorConfig {
            name: self.name.trim().to_string(),
            url,
            path,
            token_env: filled(self.token_env.trim()),
            write: self.write,
        }
    }
}

/// Result of showing the settings dialog.
pub enum SettingsResult {
    Open,
    Closed,
    Saved,
}

impl SettingsDialogState {
    /// Create the dialog from the current config snapshot.
    pub fn new(conn: &Connection) -> Self {
        let cfg = config::load_config();
        let mut detected_sourceports: Vec<String> = sourceports::detect_sourceports()
            .into_iter()
            .map(|(exe, _path, _family)| exe.to_string())
            .collect();
        detected_sourceports.dedup();

        let mut executable_args: Vec<(String, String)> = cfg
            .executable_args
            .iter()
            .map(|(sourceport, args)| (sourceport.clone(), join_args(args)))
            .collect();
        executable_args.sort_by(|a, b| a.0.cmp(&b.0));

        Self {
            sourceport: cfg.sourceport.clone(),
            zdoom_sourceport: cfg.zdoom_sourceport.clone(),
            sourceport_args: join_args(&cfg.sourceport_args),
            executable_args,
            new_sourceport_name: String::new(),
            detected_sourceports,
            iwad: cfg.iwad.clone(),
            link_mode: cfg.link_mode.clone(),
            companion_orphan_cleanup: cfg.companion_orphan_cleanup.clone(),
            download_mirror: cfg.download_mirror,
            manage_data_dirs: cfg.manage_data_dirs,
            auto_stats: cfg.auto_stats,
            auto_detect_iwad: cfg.auto_detect_iwad,
            auto_detect_complevel: cfg.auto_detect_complevel,
            auto_doomwiki_enrich: cfg.auto_doomwiki_enrich,
            cache_auto_clean: cfg.cache_auto_clean,
            cache_max_size_gb: cfg.cache_max_size_gb,
            cache_max_age_days: cfg.cache_max_age_days,
            sourceport_update_check_days: cfg.sourceport_update_check_days,
            mirrors: cfg.mirrors.iter().map(MirrorRow::from_config).collect(),
            mirror_file_counts: caco_core::db::mirrors::file_counts(conn).unwrap_or_default(),
            cache_dir: cfg.cache_dir.clone(),
            data_dir: cfg.data_dir.clone(),
            iwad_dir: cfg.iwad_dir.clone(),
            profile_dir: cfg.profile_dir.clone(),
            db_path: cfg.db_path.clone(),
            error: None,
        }
    }

    /// Build a full [`Config`] from the edited fields, preserving sections
    /// this dialog doesn't expose. Fails when an args string has unbalanced
    /// quotes or a mirror could not be used.
    fn to_config(&self) -> Result<Config, String> {
        let mut cfg = (*config::load_config()).clone();
        cfg.sourceport = self.sourceport.trim().to_string();
        cfg.zdoom_sourceport = self.zdoom_sourceport.trim().to_string();
        cfg.sourceport_args =
            parse_args(&self.sourceport_args).ok_or("Global launch args: unbalanced quotes")?;
        cfg.executable_args = self
            .executable_args
            .iter()
            .filter(|(sourceport, _)| !sourceport.trim().is_empty())
            .map(|(sourceport, args)| {
                let parsed = parse_args(args)
                    .ok_or_else(|| format!("Launch args for {sourceport}: unbalanced quotes"))?;
                Ok((sourceport.trim().to_string(), parsed))
            })
            .filter(|r| !matches!(r, Ok((_, args)) if args.is_empty()))
            .collect::<Result<_, String>>()?;
        cfg.iwad = self.iwad.trim().to_string();
        cfg.link_mode = self.link_mode.clone();
        cfg.companion_orphan_cleanup = self.companion_orphan_cleanup.clone();
        cfg.download_mirror = self.download_mirror;
        cfg.manage_data_dirs = self.manage_data_dirs;
        cfg.auto_stats = self.auto_stats;
        cfg.auto_detect_iwad = self.auto_detect_iwad;
        cfg.auto_detect_complevel = self.auto_detect_complevel;
        cfg.auto_doomwiki_enrich = self.auto_doomwiki_enrich;
        cfg.cache_auto_clean = self.cache_auto_clean;
        cfg.cache_max_size_gb = self.cache_max_size_gb.max(0.0);
        cfg.cache_max_age_days = self.cache_max_age_days.max(0);
        cfg.sourceport_update_check_days = self.sourceport_update_check_days.max(0);
        cfg.mirrors = self.mirrors.iter().map(MirrorRow::to_config).collect();
        config::check_mirrors(&cfg.mirrors)?;
        cfg.cache_dir = self.cache_dir.trim().to_string();
        cfg.data_dir = self.data_dir.trim().to_string();
        cfg.iwad_dir = self.iwad_dir.trim().to_string();
        cfg.profile_dir = self.profile_dir.trim().to_string();
        cfg.db_path = self.db_path.trim().to_string();
        Ok(cfg)
    }

    fn save(&mut self) -> bool {
        let cfg = match self.to_config() {
            Ok(cfg) => cfg,
            Err(e) => {
                self.error = Some(e);
                return false;
            }
        };
        match config::save_config(&cfg) {
            Ok(()) => {
                config::reload_config();
                true
            }
            Err(e) => {
                self.error = Some(format!("Failed to save config: {e}"));
                false
            }
        }
    }

    /// Render the settings dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context) -> SettingsResult {
        let mut result = SettingsResult::Open;

        crate::dialogs::modal_window(ctx, "settings_dialog", [560.0, 580.0])
            .title_bar(false)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(0x1a, 0x14, 0x10))
                    .corner_radius(16)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_MED))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 32,
                        spread: 8,
                        color: egui::Color32::from_black_alpha(128),
                    }),
            )
            .show(ctx, |ui| {
                // ── Header ──
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(20, 14))
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.colored_label(
                                theme::TEXT_PRIMARY,
                                egui::RichText::new("Settings").size(16.0).strong(),
                            );
                            ui.colored_label(
                                theme::TEXT_SECONDARY,
                                // Resolved, not spelled: the config lives in
                                // the data dir (and moves with CACO_HOME or
                                // CACO_CONFIG), never in ~/.config.
                                egui::RichText::new(format!(
                                    "Saved to {}",
                                    caco_core::config::config_file().display()
                                ))
                                .size(11.0),
                            );
                        });
                    });

                // ── Error banner ──
                if let Some(ref err) = self.error {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(0x2a, 0x0d, 0x0d))
                        .inner_margin(egui::Margin::symmetric(20, 6))
                        .show(ui, |ui| {
                            ui.colored_label(theme::COLOR_ERROR, err);
                        });
                }

                // ── Body ──
                // Height comes from what is left after the header and the
                // Save/Cancel row, not from a constant: 440 plus that chrome
                // is taller than the 800x400 minimum window, which put Save
                // below the bottom edge.
                let body_height = crate::dialogs::modal_body_height(ctx, 150.0);
                egui::ScrollArea::vertical()
                    .max_height(body_height)
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(20, 16))
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 8.0;
                                self.section_sourceports(ui);
                                self.section_behavior(ui);
                                self.section_cache(ui);
                                self.section_mirrors(ui);
                                self.section_paths(ui);
                            });
                    });

                // ── Footer ──
                ui.add_space(4.0);
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(20, 10))
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Save")
                                            .color(egui::Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(theme::TEXT_ACCENT)
                                    .corner_radius(8),
                                )
                                .clicked()
                                && self.save()
                            {
                                result = SettingsResult::Saved;
                            }
                            if ui.button("Cancel").clicked() {
                                result = SettingsResult::Closed;
                            }
                        });
                    });
            });

        // Escape closes without saving
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return SettingsResult::Closed;
        }

        result
    }

    fn section_header(ui: &mut egui::Ui, title: &str) {
        ui.add_space(10.0);
        ui.colored_label(
            theme::TEXT_ACCENT,
            egui::RichText::new(title.to_ascii_uppercase())
                .size(11.0)
                .strong(),
        );
        ui.separator();
        ui.add_space(2.0);
    }

    fn section_sourceports(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Sourceports");

        ui.columns(2, |cols| {
            let label = form_label(&mut cols[0], "Default sourceport");
            cols[0].horizontal(|ui| {
                let combo_w = 110.0;
                let field_w =
                    (ui.available_width() - combo_w - ui.spacing().item_spacing.x).max(80.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.sourceport)
                        .desired_width(field_w)
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id);
                if !self.detected_sourceports.is_empty() {
                    egui::ComboBox::from_id_salt("detected_sourceports")
                        .selected_text("detected")
                        .width(combo_w)
                        .show_ui(ui, |ui| {
                            for sourceport in &self.detected_sourceports {
                                if ui
                                    .selectable_label(self.sourceport == *sourceport, sourceport)
                                    .clicked()
                                {
                                    self.sourceport = sourceport.clone();
                                }
                            }
                        })
                        .response
                        .accessible_name("Detected sourceports");
                }
            });
            let label = form_label(&mut cols[1], "ZDoom-family sourceport");
            cols[1]
                .add(
                    egui::TextEdit::singleline(&mut self.zdoom_sourceport)
                        .desired_width(f32::INFINITY)
                        .text_color(theme::TEXT_PRIMARY),
                )
                .labelled_by(label.id)
                .on_hover_text("Used for WADs that require a zdoom-family sourceport");
        });

        let label = form_label(ui, "Global launch args (every sourceport)");
        ui.add(
            egui::TextEdit::singleline(&mut self.sourceport_args)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY)
                .hint_text("e.g. -nomusic — quote args containing spaces"),
        )
        .labelled_by(label.id);

        self.subsection_port_args(ui);
    }

    /// Per-sourceport launch args list: one row per sourceport, plus an add row.
    fn subsection_port_args(&mut self, ui: &mut egui::Ui) {
        form_label(ui, "Per-sourceport launch args");

        let mut remove: Option<usize> = None;
        for (idx, (sourceport, args)) in self.executable_args.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let label = ui.add_sized(
                    [130.0, 18.0],
                    egui::Label::new(
                        egui::RichText::new(sourceport.as_str())
                            .size(12.0)
                            .color(theme::TEXT_PRIMARY),
                    )
                    .truncate(),
                );
                let btn_w = 24.0;
                let field_w =
                    (ui.available_width() - btn_w - ui.spacing().item_spacing.x).max(80.0);
                ui.add(
                    egui::TextEdit::singleline(args)
                        .desired_width(field_w)
                        .text_color(theme::TEXT_PRIMARY)
                        .hint_text("e.g. -geometry 1920x1200"),
                )
                .labelled_by(label.id);
                if ui
                    .add(egui::Button::new("\u{2715}").frame(false))
                    .on_hover_text("Remove")
                    .clicked()
                {
                    remove = Some(idx);
                }
            });
        }
        if let Some(idx) = remove {
            self.executable_args.remove(idx);
        }

        // Add row: pick a detected sourceport not yet listed, or type a name
        ui.horizontal(|ui| {
            let listed: Vec<&str> = self
                .executable_args
                .iter()
                .map(|(p, _)| p.as_str())
                .collect();
            egui::ComboBox::from_id_salt("add_port_args")
                .selected_text(if self.new_sourceport_name.is_empty() {
                    "add sourceport…"
                } else {
                    self.new_sourceport_name.as_str()
                })
                .width(130.0)
                .show_ui(ui, |ui| {
                    for sourceport in &self.detected_sourceports {
                        if !listed.contains(&sourceport.as_str())
                            && ui
                                .selectable_label(
                                    self.new_sourceport_name == *sourceport,
                                    sourceport,
                                )
                                .clicked()
                        {
                            self.new_sourceport_name = sourceport.clone();
                        }
                    }
                })
                .response
                .accessible_name("Add a detected sourceport");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_sourceport_name)
                    .desired_width(140.0)
                    .text_color(theme::TEXT_PRIMARY)
                    .hint_text("or type a name"),
            )
            .accessible_name("Sourceport to add");
            let name = self.new_sourceport_name.trim().to_string();
            if ui
                .add_enabled(
                    !name.is_empty() && !listed.contains(&name.as_str()),
                    egui::Button::new("Add"),
                )
                .clicked()
            {
                self.executable_args.push((name, String::new()));
                self.new_sourceport_name.clear();
            }
        });
    }

    fn section_behavior(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Behavior");

        ui.columns(2, |cols| {
            let label = form_label(&mut cols[0], "Default IWAD");
            cols[0]
                .add(
                    egui::TextEdit::singleline(&mut self.iwad)
                        .desired_width(f32::INFINITY)
                        .text_color(theme::TEXT_PRIMARY)
                        .hint_text("empty = auto-detect"),
                )
                .labelled_by(label.id);
            let label = form_label(&mut cols[1], "idgames mirror");
            egui::ComboBox::from_id_salt("download_mirror")
                .selected_text(mirror_label(self.download_mirror))
                .width(cols[1].available_width())
                .show_ui(&mut cols[1], |ui| {
                    for (i, _) in MIRRORS.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.download_mirror,
                            i as i64,
                            mirror_label(i as i64),
                        );
                    }
                })
                .response
                .labelled_by(label.id);
        });

        ui.columns(2, |cols| {
            let label = form_label(&mut cols[0], "Import link mode");
            egui::ComboBox::from_id_salt("link_mode")
                .selected_text(&self.link_mode)
                .width(cols[0].available_width())
                .show_ui(&mut cols[0], |ui| {
                    for mode in ["move", "copy"] {
                        ui.selectable_value(&mut self.link_mode, mode.to_string(), mode);
                    }
                })
                .response
                .labelled_by(label.id);
            let label = form_label(&mut cols[1], "Orphaned companions");
            egui::ComboBox::from_id_salt("companion_orphan_cleanup")
                .selected_text(&self.companion_orphan_cleanup)
                .width(cols[1].available_width())
                .show_ui(&mut cols[1], |ui| {
                    for mode in ["ask", "delete", "keep"] {
                        ui.selectable_value(
                            &mut self.companion_orphan_cleanup,
                            mode.to_string(),
                            mode,
                        );
                    }
                })
                .response
                .labelled_by(label.id);
        });

        ui.add_space(4.0);
        ui.checkbox(
            &mut self.manage_data_dirs,
            "Manage per-WAD data directories (saves, stats, configs)",
        );
        ui.checkbox(&mut self.auto_stats, "Track per-map stats automatically");
        ui.checkbox(&mut self.auto_detect_iwad, "Auto-detect required IWAD");
        ui.checkbox(&mut self.auto_detect_complevel, "Auto-detect complevel");
        ui.checkbox(
            &mut self.auto_doomwiki_enrich,
            "Auto-enrich imports with Doom Wiki metadata",
        );

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let label = form_label(ui, "Check built sourceports for updates every (days)");
            ui.add(
                egui::DragValue::new(&mut self.sourceport_update_check_days)
                    .speed(1)
                    .range(0..=i64::MAX),
            )
            .labelled_by(label.id)
            .on_hover_text("One `git ls-remote` per built sourceport at startup. 0 = never check.");
        });
    }

    fn section_cache(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "WAD cache");
        ui.checkbox(&mut self.cache_auto_clean, "Auto-clean cache after play");
        ui.horizontal(|ui| {
            let label = form_label(ui, "Max size (GB)");
            ui.add(
                egui::DragValue::new(&mut self.cache_max_size_gb)
                    .speed(0.5)
                    .range(0.0..=f64::MAX),
            )
            .labelled_by(label.id)
            .on_hover_text("0 = unlimited");
            ui.add_space(16.0);
            let label = form_label(ui, "Max age (days)");
            ui.add(
                egui::DragValue::new(&mut self.cache_max_age_days)
                    .speed(1)
                    .range(0..=i64::MAX),
            )
            .labelled_by(label.id)
            .on_hover_text("0 = unlimited");
        });
    }

    fn section_mirrors(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Mirrors");
        ui.colored_label(
            theme::TEXT_SECONDARY,
            egui::RichText::new(
                "Your own store of WAD files: a directory or an HTTP file server. caco looks \
                 here first for a WAD that is not on this machine, and offers to upload what \
                 it could not otherwise get back.",
            )
            .size(12.0),
        );

        let mut remove: Option<usize> = None;
        for (idx, row) in self.mirrors.iter_mut().enumerate() {
            egui::Frame::new()
                .fill(theme::BG_MEDIUM)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let name_w = 120.0;
                    let spacing = ui.spacing().item_spacing.x;
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(name_w);
                            let label = form_label(ui, "Name");
                            ui.add(
                                egui::TextEdit::singleline(&mut row.name)
                                    .desired_width(name_w)
                                    .text_color(theme::TEXT_PRIMARY),
                            )
                            .labelled_by(label.id);
                        });
                        ui.vertical(|ui| {
                            let location_w = (ui.available_width() - spacing).max(120.0);
                            ui.set_width(location_w);
                            let label = form_label(ui, "URL or path");
                            ui.add(
                                egui::TextEdit::singleline(&mut row.location)
                                    .desired_width(location_w)
                                    .text_color(theme::TEXT_PRIMARY)
                                    .hint_text("https://files.example.lan/doom or /mnt/nas/doom"),
                            )
                            .labelled_by(label.id);
                        });
                    });
                    ui.horizontal(|ui| {
                        let label = form_label(ui, "Token variable");
                        ui.add(
                            egui::TextEdit::singleline(&mut row.token_env)
                                .desired_width(170.0)
                                .text_color(theme::TEXT_PRIMARY)
                                .hint_text("optional"),
                        )
                        .labelled_by(label.id)
                        .on_hover_text(
                            "The environment variable holding a bearer token. The token itself \
                             stays out of this file, which is meant to travel between machines.",
                        );
                        ui.checkbox(&mut row.write, "caco may upload");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Remove").clicked() {
                                remove = Some(idx);
                            }
                        });
                    });
                    if let Some(count) = self.mirror_file_counts.get(row.name.trim()) {
                        ui.colored_label(
                            theme::TEXT_MUTED,
                            egui::RichText::new(format!(
                                "{count} file{} as of the last time its index was read",
                                if *count == 1 { "" } else { "s" }
                            ))
                            .size(11.0),
                        );
                    }
                });
        }
        if let Some(idx) = remove {
            self.mirrors.remove(idx);
        }

        if ui.button("Add Mirror").clicked() {
            self.mirrors.push(MirrorRow::default());
        }
    }

    fn section_paths(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Paths");
        for (text, value) in [
            ("WAD cache dir", &mut self.cache_dir),
            ("Data dir", &mut self.data_dir),
            ("IWAD dir", &mut self.iwad_dir),
            ("Sourceport config dir", &mut self.profile_dir),
        ] {
            let label = form_label(ui, text);
            ui.add(
                egui::TextEdit::singleline(value)
                    .desired_width(f32::INFINITY)
                    .text_color(theme::TEXT_PRIMARY),
            )
            .labelled_by(label.id);
        }

        let label = form_label(ui, "Database path");
        ui.add(
            egui::TextEdit::singleline(&mut self.db_path)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY),
        )
        .labelled_by(label.id)
        .on_hover_text("Takes effect after restarting caco");
    }
}

/// A field's visible label. Returned so the field can name itself after it
/// with `labelled_by`: without that, seeing the two side by side is the only
/// thing connecting them.
fn form_label(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.colored_label(
        theme::TEXT_SECONDARY,
        egui::RichText::new(label).size(12.0).strong(),
    )
}

/// Join an args vec into a shell-style string (quoting where needed).
fn join_args(args: &[String]) -> String {
    shlex::try_join(args.iter().map(String::as_str)).unwrap_or_else(|_| args.join(" "))
}

/// Parse a shell-style args string. `None` on unbalanced quotes.
fn parse_args(text: &str) -> Option<Vec<String>> {
    shlex::split(text.trim())
}

fn mirror_label(index: i64) -> String {
    let i = (index.max(0) as usize) % MIRRORS.len();
    // Show just the host for readability
    MIRRORS[i]
        .trim_start_matches("https://")
        .trim_end_matches("/pub/idgames/")
        .trim_end_matches("/files/idgames/")
        .trim_end_matches("/idgames/")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_mirror_row_round_trips_through_the_config() {
        for mirror in [
            MirrorConfig {
                name: "home".to_string(),
                url: Some("https://files.example.lan/doom".to_string()),
                token_env: Some("CACO_MIRROR_TOKEN".to_string()),
                write: true,
                ..Default::default()
            },
            MirrorConfig {
                name: "nas".to_string(),
                path: Some("/mnt/nas/doom".to_string()),
                ..Default::default()
            },
        ] {
            assert_eq!(MirrorRow::from_config(&mirror).to_config(), mirror);
        }
    }

    #[test]
    fn test_a_mirror_row_works_out_which_kind_of_location_it_has() {
        let row = |location: &str| MirrorRow {
            name: " home ".to_string(),
            location: location.to_string(),
            token_env: "  ".to_string(),
            write: false,
        };

        let url = row(" http://nas.lan/doom ").to_config();
        assert_eq!(url.name, "home");
        assert_eq!(url.url.as_deref(), Some("http://nas.lan/doom"));
        assert_eq!(url.path, None);
        assert_eq!(url.token_env, None);

        let path = row("~/doom-mirror").to_config();
        assert_eq!(path.url, None);
        assert_eq!(path.path.as_deref(), Some("~/doom-mirror"));

        let blank = row("  ").to_config();
        assert_eq!((blank.url, blank.path), (None, None));

        // Taken for a URL, so that the check can say what is wrong with it.
        assert_eq!(
            row("ftp://nas.lan/doom").to_config().url.as_deref(),
            Some("ftp://nas.lan/doom")
        );
    }
}
