//! Settings dialog — edit the caco config and persist it to config.toml.
//!
//! The dialog edits a snapshot of the current [`Config`]. Saving writes the
//! whole struct back via `save_config` and swaps the live snapshot with
//! `reload_config`, so changes apply immediately without a restart (except
//! `db_path`, which is only read at startup). Sections not shown here
//! (tui/list, sourceport preferences, IWAD priority) are preserved verbatim.

use caco_core::config::{self, Config};
use caco_core::sourceports;
use caco_sources::idgames::MIRRORS;

use crate::dialogs::{Dialog, Primary, form_label};
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
    warn_newer_saves: bool,

    // Cache
    cache_auto_clean: bool,
    cache_max_size_gb: f64,
    cache_max_age_days: i64,
    sourceport_update_check_days: i64,
    idgames_index_check_days: i64,

    // Paths
    cache_dir: String,
    data_dir: String,
    iwad_dir: String,
    profile_dir: String,
    db_path: String,
    downloads_dir: String,
    /// What an empty `downloads_dir` means on this machine, to say so in the field.
    default_downloads_dir: String,
    sync_dir: String,
    /// What an empty `sync_dir` means for this library, to say so in the field.
    default_sync_dir: String,

    error: Option<String>,
}

/// Result of showing the settings dialog.
pub enum SettingsResult {
    Open,
    Closed,
    Saved,
}

impl Default for SettingsDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsDialogState {
    /// Create the dialog from the current config snapshot.
    pub fn new() -> Self {
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
            warn_newer_saves: cfg.warn_newer_saves,
            cache_auto_clean: cfg.cache_auto_clean,
            cache_max_size_gb: cfg.cache_max_size_gb,
            cache_max_age_days: cfg.cache_max_age_days,
            sourceport_update_check_days: cfg.sourceport_update_check_days,
            idgames_index_check_days: cfg.idgames_index_check_days,
            cache_dir: cfg.cache_dir.clone(),
            data_dir: cfg.data_dir.clone(),
            iwad_dir: cfg.iwad_dir.clone(),
            profile_dir: cfg.profile_dir.clone(),
            db_path: cfg.db_path.clone(),
            downloads_dir: cfg.downloads_dir.clone(),
            default_downloads_dir: config::default_downloads_dir().display().to_string(),
            sync_dir: cfg.sync_dir.clone(),
            default_sync_dir: config::get_db_path()
                .with_file_name("sync")
                .display()
                .to_string(),
            error: None,
        }
    }

    /// Build a full [`Config`] from the edited fields, preserving sections
    /// this dialog doesn't expose. Fails when an args string has unbalanced
    /// quotes.
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
        cfg.warn_newer_saves = self.warn_newer_saves;
        cfg.cache_auto_clean = self.cache_auto_clean;
        cfg.cache_max_size_gb = self.cache_max_size_gb.max(0.0);
        cfg.cache_max_age_days = self.cache_max_age_days.max(0);
        cfg.sourceport_update_check_days = self.sourceport_update_check_days.max(0);
        cfg.idgames_index_check_days = self.idgames_index_check_days.max(0);
        cfg.cache_dir = self.cache_dir.trim().to_string();
        cfg.data_dir = self.data_dir.trim().to_string();
        cfg.iwad_dir = self.iwad_dir.trim().to_string();
        cfg.profile_dir = self.profile_dir.trim().to_string();
        cfg.db_path = self.db_path.trim().to_string();
        cfg.downloads_dir = self.downloads_dir.trim().to_string();
        cfg.sync_dir = self.sync_dir.trim().to_string();
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

        Dialog::new("Settings", [600.0, 640.0]).show(ctx, |dialog| {
            dialog
                .scroll(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    self.section_sourceports(ui);
                    self.section_behavior(ui);
                    self.section_cache(ui);
                    self.section_paths(ui);
                })
                .above(|ui| {
                    if let Some(error) = &self.error {
                        ui.colored_label(theme::COLOR_ERROR, error);
                    }
                })
                .buttons(|row| {
                    let ended = row.end(Some("Cancel"), Some(Primary::new("Save")));
                    if ended.dismissed {
                        result = SettingsResult::Closed;
                    }
                    if ended.primary && self.save() {
                        result = SettingsResult::Saved;
                    }
                })
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
        Self::this_machine_note(ui);

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
                .on_hover_text("Used for WADs that need a ZDoom-family sourceport");
        });

        let label = form_label(ui, "Launch arguments for every sourceport");
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
        form_label(ui, "Launch arguments for one sourceport");

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
                    "add a sourceport\u{2026}"
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

    /// Said where it applies, since these are the settings a person would otherwise expect
    /// to come across with the rest. See `config::MACHINE_KEYS`.
    fn this_machine_note(ui: &mut egui::Ui) {
        ui.colored_label(
            theme::TEXT_MUTED,
            egui::RichText::new("For this machine only, and not synced with the rest.").small(),
        );
        ui.add_space(2.0);
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
            let label = form_label(&mut cols[0], "When linking a download");
            egui::ComboBox::from_id_salt("link_mode")
                .selected_text(&self.link_mode)
                .width(cols[0].available_width())
                .show_ui(&mut cols[0], |ui| {
                    for mode in ["move", "copy"] {
                        ui.selectable_value(&mut self.link_mode, mode.to_string(), mode);
                    }
                })
                .response
                .labelled_by(label.id)
                .on_hover_text(
                    "What happens to a file you link when Get finds it in your Downloads folder.",
                );
            let label = form_label(&mut cols[1], "When a companion file is left unused");
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
            "Keep each WAD's saves, stats and config in a folder of its own",
        );
        ui.checkbox(&mut self.auto_stats, "Track per-map stats automatically");
        ui.checkbox(
            &mut self.auto_detect_iwad,
            "Work out which IWAD a WAD needs from its file",
        );
        ui.checkbox(
            &mut self.auto_detect_complevel,
            "Work out a WAD's complevel from its file",
        );
        ui.checkbox(
            &mut self.auto_doomwiki_enrich,
            "Look up what I import on the Doom Wiki",
        );
        ui.checkbox(
            &mut self.warn_newer_saves,
            "Ask before playing a WAD another machine has newer saves for",
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
            .on_hover_text(
                "When caco starts, asks each sourceport you built whether there is a newer \
                 version. 0 means never.",
            );
        });
        ui.horizontal(|ui| {
            let label = form_label(ui, "Update the idgames index every (days)");
            ui.add(
                egui::DragValue::new(&mut self.idgames_index_check_days)
                    .speed(1)
                    .range(0..=i64::MAX),
            )
            .labelled_by(label.id)
            .on_hover_text(
                "When caco starts, asks a mirror whether idgames has anything new. 0 means never: \
                 search then goes by what caco already knows, and by idgames itself.",
            );
        });
    }

    fn section_cache(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Downloaded WADs");
        ui.checkbox(
            &mut self.cache_auto_clean,
            "Clear out old downloads after playing",
        );
        ui.horizontal(|ui| {
            let label = form_label(ui, "Keep up to (GB)");
            ui.add(
                egui::DragValue::new(&mut self.cache_max_size_gb)
                    .speed(0.5)
                    .range(0.0..=f64::MAX),
            )
            .labelled_by(label.id)
            .on_hover_text("0 means no limit");
            ui.add_space(16.0);
            let label = form_label(ui, "For up to (days)");
            ui.add(
                egui::DragValue::new(&mut self.cache_max_age_days)
                    .speed(1)
                    .range(0..=i64::MAX),
            )
            .labelled_by(label.id)
            .on_hover_text("0 means no limit");
        });
    }

    fn section_paths(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Paths");
        Self::this_machine_note(ui);
        for (text, value) in [
            ("Downloaded WADs folder", &mut self.cache_dir),
            ("Saves and stats folder", &mut self.data_dir),
            ("IWADs folder", &mut self.iwad_dir),
            ("Sourceport profiles folder", &mut self.profile_dir),
        ] {
            let label = form_label(ui, text);
            ui.add(
                egui::TextEdit::singleline(value)
                    .desired_width(f32::INFINITY)
                    .text_color(theme::TEXT_PRIMARY),
            )
            .labelled_by(label.id);
        }

        let label = form_label(ui, "Library file");
        ui.add(
            egui::TextEdit::singleline(&mut self.db_path)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY),
        )
        .labelled_by(label.id)
        .on_hover_text("Takes effect after restarting caco");

        // With a hint saying what empty means, because that is what most people want,
        // and a browser in a sandbox is the reason to change it.
        let label = form_label(ui, "Downloads folder");
        ui.add(
            egui::TextEdit::singleline(&mut self.downloads_dir)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY)
                .hint_text(&self.default_downloads_dir),
        )
        .labelled_by(label.id)
        .on_hover_text("Where Get looks for what your browser downloads");

        let label = form_label(ui, "Sync folder");
        ui.add(
            egui::TextEdit::singleline(&mut self.sync_dir)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY)
                .hint_text(&self.default_sync_dir),
        )
        .labelled_by(label.id)
        .on_hover_text("Where each machine keeps its log. See the Sync tool.");

        ui.add_space(4.0);
        ui.colored_label(
            theme::TEXT_MUTED,
            egui::RichText::new(format!(
                "Settings are saved in {}, and for this machine in {}.",
                config::config_file().display(),
                config::machine_config_file().display()
            ))
            .small(),
        );
    }
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
