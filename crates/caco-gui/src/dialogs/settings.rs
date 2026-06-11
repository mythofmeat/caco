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

use crate::theme;

/// State for the settings dialog.
pub struct SettingsDialogState {
    // Sourceports
    sourceport: String,
    zdoom_sourceport: String,
    /// One argument per line.
    sourceport_args: String,
    detected_ports: Vec<String>,

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

    // Paths
    cache_dir: String,
    data_dir: String,
    iwad_dir: String,
    sourceport_dir: String,
    db_path: String,

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
        let mut detected_ports: Vec<String> = sourceports::detect_sourceports()
            .into_iter()
            .map(|(exe, _path, _family)| exe.to_string())
            .collect();
        detected_ports.dedup();

        Self {
            sourceport: cfg.sourceport.clone(),
            zdoom_sourceport: cfg.zdoom_sourceport.clone(),
            sourceport_args: cfg.sourceport_args.join("\n"),
            detected_ports,
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
            cache_dir: cfg.cache_dir.clone(),
            data_dir: cfg.data_dir.clone(),
            iwad_dir: cfg.iwad_dir.clone(),
            sourceport_dir: cfg.sourceport_dir.clone(),
            db_path: cfg.db_path.clone(),
            error: None,
        }
    }

    /// Build a full [`Config`] from the edited fields, preserving sections
    /// this dialog doesn't expose.
    fn to_config(&self) -> Config {
        let mut cfg = (*config::load_config()).clone();
        cfg.sourceport = self.sourceport.trim().to_string();
        cfg.zdoom_sourceport = self.zdoom_sourceport.trim().to_string();
        cfg.sourceport_args = self
            .sourceport_args
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
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
        cfg.cache_dir = self.cache_dir.trim().to_string();
        cfg.data_dir = self.data_dir.trim().to_string();
        cfg.iwad_dir = self.iwad_dir.trim().to_string();
        cfg.sourceport_dir = self.sourceport_dir.trim().to_string();
        cfg.db_path = self.db_path.trim().to_string();
        cfg
    }

    fn save(&mut self) -> bool {
        match config::save_config(&self.to_config()) {
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

        egui::Window::new("Settings")
            .collapsible(false)
            .resizable(true)
            .default_size([560.0, 580.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| {
                        self.section_sourceports(ui);
                        self.section_behavior(ui);
                        self.section_cache(ui);
                        self.section_paths(ui);
                    });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                if let Some(ref err) = self.error {
                    ui.colored_label(theme::COLOR_ERROR, err);
                    ui.add_space(4.0);
                }

                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() && self.save() {
                        result = SettingsResult::Saved;
                    }
                    if ui.button("Cancel").clicked() {
                        result = SettingsResult::Closed;
                    }
                });
            });

        // Escape closes without saving
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return SettingsResult::Closed;
        }

        result
    }

    fn section_header(ui: &mut egui::Ui, title: &str) {
        ui.add_space(8.0);
        ui.colored_label(theme::TEXT_SECONDARY, egui::RichText::new(title).strong());
        ui.separator();
    }

    fn section_sourceports(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Sourceports");
        egui::Grid::new("settings_sourceports")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Default sourceport");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.sourceport).desired_width(180.0));
                    if !self.detected_ports.is_empty() {
                        egui::ComboBox::from_id_salt("detected_ports")
                            .selected_text("detected")
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for port in &self.detected_ports {
                                    if ui
                                        .selectable_label(self.sourceport == *port, port)
                                        .clicked()
                                    {
                                        self.sourceport = port.clone();
                                    }
                                }
                            });
                    }
                });
                ui.end_row();

                ui.label("ZDoom-family port");
                ui.add(egui::TextEdit::singleline(&mut self.zdoom_sourceport).desired_width(180.0))
                    .on_hover_text("Used for WADs that require a zdoom-family sourceport");
                ui.end_row();

                ui.label("Extra launch args");
                ui.add(
                    egui::TextEdit::multiline(&mut self.sourceport_args)
                        .desired_width(280.0)
                        .desired_rows(2)
                        .hint_text("one argument per line"),
                );
                ui.end_row();
            });
    }

    fn section_behavior(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Behavior");
        egui::Grid::new("settings_behavior")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Default IWAD");
                ui.add(
                    egui::TextEdit::singleline(&mut self.iwad)
                        .desired_width(180.0)
                        .hint_text("empty = auto-detect"),
                );
                ui.end_row();

                ui.label("Import link mode");
                egui::ComboBox::from_id_salt("link_mode")
                    .selected_text(&self.link_mode)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for mode in ["move", "copy"] {
                            ui.selectable_value(&mut self.link_mode, mode.to_string(), mode);
                        }
                    });
                ui.end_row();

                ui.label("Orphaned companions");
                egui::ComboBox::from_id_salt("companion_orphan_cleanup")
                    .selected_text(&self.companion_orphan_cleanup)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for mode in ["ask", "delete", "keep"] {
                            ui.selectable_value(
                                &mut self.companion_orphan_cleanup,
                                mode.to_string(),
                                mode,
                            );
                        }
                    });
                ui.end_row();

                ui.label("idgames mirror");
                egui::ComboBox::from_id_salt("download_mirror")
                    .selected_text(mirror_label(self.download_mirror))
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        for (i, _) in MIRRORS.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.download_mirror,
                                i as i64,
                                mirror_label(i as i64),
                            );
                        }
                    });
                ui.end_row();
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
    }

    fn section_cache(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "WAD cache");
        ui.checkbox(&mut self.cache_auto_clean, "Auto-clean cache after play");
        egui::Grid::new("settings_cache")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Max cache size (GB)");
                ui.add(
                    egui::DragValue::new(&mut self.cache_max_size_gb)
                        .speed(0.5)
                        .range(0.0..=f64::MAX),
                )
                .on_hover_text("0 = unlimited");
                ui.end_row();

                ui.label("Max cache age (days)");
                ui.add(
                    egui::DragValue::new(&mut self.cache_max_age_days)
                        .speed(1)
                        .range(0..=i64::MAX),
                )
                .on_hover_text("0 = unlimited");
                ui.end_row();
            });
    }

    fn section_paths(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Paths");
        egui::Grid::new("settings_paths")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for (label, value) in [
                    ("WAD cache dir", &mut self.cache_dir),
                    ("Data dir", &mut self.data_dir),
                    ("IWAD dir", &mut self.iwad_dir),
                    ("Sourceport config dir", &mut self.sourceport_dir),
                ] {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(value).desired_width(320.0));
                    ui.end_row();
                }

                ui.label("Database path");
                ui.add(egui::TextEdit::singleline(&mut self.db_path).desired_width(320.0))
                    .on_hover_text("Takes effect after restarting caco");
                ui.end_row();
            });
    }
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
