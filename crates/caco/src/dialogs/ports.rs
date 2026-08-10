//! Build and manage sourceports from source.
//!
//! The GUI half of `caco_core::ports`. Ports that no distro packages —
//! nyan-doom, uzdoom — are cloned, compiled and installed into a caco-owned
//! prefix, and `resolve_sourceport` finds them without anything being
//! installed system-wide.
//!
//! A compile is minutes long and produces thousands of lines, so the work
//! runs on a worker thread and this dialog holds only the request, the live
//! log and the outcome — the same split as `dialogs/enrich.rs`. What it adds
//! is the pre-flight check: `doctor` answers "will this even build here?"
//! against the host package manager before the user spends four minutes
//! finding out that it will not.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use caco_core::ports::{self, DoctorReport, PackageCheck, PortPaths, PortStatus};
use caco_core::utils::format_size;

use crate::theme;

/// A build the dialog is asking the app to start.
pub struct PortBuildRequest {
    pub port: String,
    pub clean: bool,
    pub cancel: Arc<AtomicBool>,
}

pub enum PortsResult {
    Open,
    Closed,
    Start(PortBuildRequest),
}

/// Cap on retained log lines.
///
/// uzdoom is 1636 targets and ninja prints per target; keeping every line
/// would grow the dialog's memory for the whole session with output nobody
/// scrolls back to. A failure is always in the tail.
const LOG_LIMIT: usize = 2000;

pub struct PortsDialogState {
    ports: Vec<PortStatus>,
    selected: Option<String>,
    /// Cached pre-flight for the selected port. Re-run on selection change
    /// rather than per frame — it shells out to pacman.
    doctor: Option<DoctorReport>,
    clean: bool,
    /// Name of the port being built, if any.
    running: Option<String>,
    step: Option<String>,
    log: Vec<String>,
    outcome: Option<Result<String, String>>,
    cancel: Arc<AtomicBool>,
    error: Option<String>,
    /// Set when a build or removal changed what a launch would resolve to.
    pub modified: bool,
}

impl PortsDialogState {
    pub fn new() -> Self {
        let mut state = Self {
            ports: Vec::new(),
            selected: None,
            doctor: None,
            clean: false,
            running: None,
            step: None,
            log: Vec::new(),
            outcome: None,
            cancel: Arc::new(AtomicBool::new(false)),
            error: None,
            modified: false,
        };
        state.reload();
        state
    }

    fn reload(&mut self) {
        match ports::status(&PortPaths::from_config()) {
            Ok(ports) => {
                self.ports = ports;
                self.error = None;
            }
            Err(e) => {
                self.ports = Vec::new();
                self.error = Some(e.to_string());
            }
        }
        // Keep the selection if it still exists, otherwise take the first.
        let still_there = self
            .selected
            .as_ref()
            .is_some_and(|name| self.ports.iter().any(|p| &p.recipe.name == name));
        if !still_there {
            self.selected = self.ports.first().map(|p| p.recipe.name.clone());
        }
        self.refresh_doctor();
    }

    fn refresh_doctor(&mut self) {
        self.doctor = self.selected_port().map(|p| ports::doctor(&p.recipe));
    }

    fn selected_port(&self) -> Option<&PortStatus> {
        let name = self.selected.as_ref()?;
        self.ports.iter().find(|p| &p.recipe.name == name)
    }

    fn select(&mut self, name: &str) {
        if self.selected.as_deref() == Some(name) {
            return;
        }
        self.selected = Some(name.to_string());
        self.refresh_doctor();
    }

    // -- worker callbacks --------------------------------------------------

    pub fn set_step(&mut self, step: String) {
        self.push_log(format!("== {step}"));
        self.step = Some(step);
    }

    pub fn push_log(&mut self, line: String) {
        if self.log.len() >= LOG_LIMIT {
            self.log.remove(0);
        }
        self.log.push(line);
    }

    /// Called by the app when the build finishes, succeeds or fails.
    pub fn finish(&mut self, outcome: Result<String, String>) {
        self.running = None;
        self.step = None;
        if outcome.is_ok() {
            self.modified = true;
        }
        self.outcome = Some(outcome);
        self.reload();
    }

    fn start(&mut self, port: String) -> PortBuildRequest {
        // A fresh flag per run: reusing a cancelled one would abort instantly.
        self.cancel = Arc::new(AtomicBool::new(false));
        self.running = Some(port.clone());
        self.log.clear();
        self.outcome = None;
        self.step = None;
        PortBuildRequest {
            port,
            clean: self.clean,
            cancel: Arc::clone(&self.cancel),
        }
    }

    // -- rendering ---------------------------------------------------------

    pub fn render(&mut self, ctx: &egui::Context) -> PortsResult {
        let mut result = PortsResult::Open;

        egui::Window::new("Sourceports")
            .collapsible(false)
            .resizable(true)
            .default_size([820.0, 620.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if let Some(error) = &self.error {
                    ui.colored_label(theme::COLOR_ERROR, error);
                    ui.add_space(6.0);
                }

                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    "Builds a sourceport from source into caco's own prefix. Nothing is \
                     installed system-wide, and the recipe travels with your library so \
                     another machine can rebuild it.",
                );
                ui.add_space(8.0);

                ui.horizontal_top(|ui| {
                    self.render_list(ui);
                    ui.separator();
                    ui.vertical(|ui| self.render_details(ui, &mut result));
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
                self.render_log(ui);

                ui.add_space(6.0);
                if ui
                    .add_enabled(self.running.is_none(), egui::Button::new("Close"))
                    .clicked()
                {
                    result = PortsResult::Closed;
                }
            });

        // Escape must not close mid-build: the log would be lost with nowhere
        // to report a failure.
        if self.running.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return PortsResult::Closed;
        }

        result
    }

    fn render_list(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.set_min_width(200.0);
            ui.set_max_width(200.0);
            let entries: Vec<(String, String, egui::Color32)> = self
                .ports
                .iter()
                .map(|p| {
                    let (label, color) = match (&p.installed, p.ref_changed) {
                        (Some(_), true) => ("update available", theme::COLOR_WARNING),
                        (Some(_), false) => ("installed", theme::COLOR_SUCCESS),
                        (None, _) => ("not built", theme::TEXT_MUTED),
                    };
                    (p.recipe.name.clone(), label.to_string(), color)
                })
                .collect();

            for (name, label, color) in entries {
                let selected = self.selected.as_deref() == Some(name.as_str());
                let response = ui.add(egui::SelectableLabel::new(
                    selected,
                    egui::RichText::new(&name).strong(),
                ));
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.colored_label(color, egui::RichText::new(label).size(10.0));
                });
                if response.clicked() {
                    self.select(&name);
                }
                ui.add_space(4.0);
            }
        });
    }

    fn render_details(&mut self, ui: &mut egui::Ui, result: &mut PortsResult) {
        let Some(port) = self.selected_port() else {
            ui.colored_label(theme::TEXT_SECONDARY, "No recipes.");
            return;
        };

        let name = port.recipe.name.clone();
        let running_this = self.running.as_deref() == Some(name.as_str());
        let busy = self.running.is_some();

        ui.horizontal(|ui| {
            ui.strong(&name);
            ui.colored_label(
                theme::TEXT_MUTED,
                format!("{} @ {}", port.recipe.repo, port.recipe.git_ref),
            );
        });
        ui.add_space(4.0);

        match &port.installed {
            Some(installed) => {
                let commit = installed
                    .manifest
                    .commit
                    .chars()
                    .take(9)
                    .collect::<String>();
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    format!(
                        "Built {} from {} ({}), {}",
                        installed
                            .manifest
                            .built_at
                            .chars()
                            .take(10)
                            .collect::<String>(),
                        installed.manifest.git_ref,
                        commit,
                        format_size(installed.size_bytes()),
                    ),
                );
                ui.colored_label(
                    theme::TEXT_MUTED,
                    installed.binary_path().display().to_string(),
                );
                if port.ref_changed {
                    ui.colored_label(
                        theme::COLOR_WARNING,
                        format!(
                            "Recipe now asks for '{}' — rebuild to switch.",
                            port.recipe.git_ref
                        ),
                    );
                }
            }
            None => {
                ui.colored_label(theme::TEXT_SECONDARY, "Not built yet.");
            }
        }

        ui.add_space(8.0);
        self.render_doctor(ui);
        ui.add_space(8.0);

        let can_build = self.doctor.as_ref().is_none_or(DoctorReport::can_build);
        let installed = self.selected_port().and_then(|p| p.installed.clone());

        ui.horizontal(|ui| {
            let verb = if installed.is_some() {
                "Rebuild"
            } else {
                "Build"
            };
            if ui
                .add_enabled(!busy && can_build, egui::Button::new(verb))
                .on_disabled_hover_text(if can_build {
                    "A build is already running"
                } else {
                    "Install the missing build tools first"
                })
                .clicked()
            {
                *result = PortsResult::Start(self.start(name.clone()));
            }
            if ui
                .add_enabled(running_this, egui::Button::new("Cancel"))
                .on_hover_text("Stops the running command; nothing already installed is removed")
                .clicked()
            {
                self.cancel.store(true, Ordering::Relaxed);
            }
            ui.add_enabled(!busy, egui::Checkbox::new(&mut self.clean, "Clean"))
                .on_hover_text(
                    "Discard the checkout and build tree first — slower, always correct",
                );
        });

        if let Some(installed) = installed {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("Use as default sourceport"))
                    .on_hover_text("Sets this port as the one caco launches WADs with")
                    .clicked()
                {
                    match set_default_sourceport(&name) {
                        Ok(()) => {
                            self.modified = true;
                            self.outcome =
                                Some(Ok(format!("{name} is now the default sourceport")));
                        }
                        Err(e) => self.outcome = Some(Err(e)),
                    }
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Remove"))
                    .on_hover_text("Deletes the install prefix. The recipe stays.")
                    .clicked()
                {
                    match ports::remove_installed(&installed.prefix) {
                        Ok(()) => {
                            self.modified = true;
                            self.outcome = Some(Ok(format!("Removed {name}")));
                            self.reload();
                        }
                        Err(e) => self.outcome = Some(Err(e.to_string())),
                    }
                }
            });
        }
    }

    fn render_doctor(&self, ui: &mut egui::Ui) {
        let Some(report) = &self.doctor else {
            return;
        };

        if !report.missing_tools.is_empty() {
            ui.colored_label(
                theme::COLOR_ERROR,
                format!("Missing build tools: {}", report.missing_tools.join(", ")),
            );
        }
        if !report.missing_packages.is_empty() {
            // Not an error: caco cannot tell an optional dependency from a
            // required one, so cmake stays the authority on what a build needs.
            ui.colored_label(
                theme::COLOR_WARNING,
                format!(
                    "Not installed: {} — some may be optional.",
                    report.missing_packages.join(", ")
                ),
            );
        }
        if let PackageCheck::Unsupported(reason) = &report.package_check {
            ui.colored_label(theme::TEXT_MUTED, reason);
        }
        if let Some(hint) = report.install_hint() {
            ui.horizontal(|ui| {
                ui.colored_label(theme::TEXT_SECONDARY, &hint);
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(hint.clone());
                }
            });
        }
        if report.missing_tools.is_empty() && report.missing_packages.is_empty() {
            ui.colored_label(theme::COLOR_SUCCESS, "All dependencies present.");
        }
    }

    fn render_log(&self, ui: &mut egui::Ui) {
        if let Some(port) = &self.running {
            ui.horizontal(|ui| {
                ui.spinner();
                match &self.step {
                    Some(step) => {
                        ui.colored_label(theme::TEXT_SECONDARY, format!("{port}: {step}"))
                    }
                    None => ui.colored_label(theme::TEXT_SECONDARY, format!("{port}: starting")),
                };
            });
        } else if let Some(outcome) = &self.outcome {
            match outcome {
                Ok(msg) => ui.colored_label(theme::COLOR_SUCCESS, msg),
                Err(msg) => ui.colored_label(theme::COLOR_ERROR, msg),
            };
        }

        if self.log.is_empty() {
            return;
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.log {
                    ui.label(egui::RichText::new(line).monospace().size(10.0));
                }
            });
    }
}

impl Default for PortsDialogState {
    fn default() -> Self {
        Self::new()
    }
}

/// Point `config.sourceport` at a freshly built port.
///
/// Written through the same load/save path the settings dialog uses, so the
/// keys it does not touch survive, and reloaded so the next launch sees it
/// without a restart.
fn set_default_sourceport(name: &str) -> Result<(), String> {
    let mut config = (*caco_core::config::load_config()).clone();
    config.sourceport = name.to_string();
    caco_core::config::save_config(&config).map_err(|e| e.to_string())?;
    caco_core::config::reload_config();
    Ok(())
}
