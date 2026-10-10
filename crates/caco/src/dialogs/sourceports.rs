//! Build and manage sourceports from source.
//!
//! The GUI half of `caco_core::sourceports`. Sourceports no distro packages —
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
//!
//! The log is not part of the scrolling body. It is reserved out of the
//! window's height and pinned above the button row, because the output is the
//! reason the dialog is open and anything that has to be scrolled into view
//! during a four-minute compile may as well not be shown.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use caco_core::sourceports::{self, DoctorReport, PackageCheck, SourceportPaths, SourceportStatus};
use caco_core::utils::format_size;

use crate::theme;

/// A build the dialog is asking the app to start.
pub struct SourceportBuildRequest {
    pub sourceport: String,
    pub git_ref: String,
    pub clean: bool,
    pub cancel: Arc<AtomicBool>,
}

pub struct SourceportVersionsRequest {
    pub sourceport: String,
    pub repo: String,
}

pub enum SourceportsResult {
    Open,
    Closed,
    Start(SourceportBuildRequest),
    CheckVersions(SourceportVersionsRequest),
}

/// Cap on retained log lines.
///
/// uzdoom is 1636 targets and ninja prints per target; keeping every line
/// would grow the dialog's memory for the whole session with output nobody
/// scrolls back to. A failure is always in the tail.
const LOG_LIMIT: usize = 2000;

/// Height of the Close row pinned below everything else.
const FOOTER: f32 = 36.0;

/// Height of the one-line "building X: step" / outcome row above the log.
const STATUS_ROW: f32 = 24.0;

/// Separator and spacing between the body and the log pane.
const LOG_CHROME: f32 = 16.0;

/// How much of the window the build log may claim.
///
/// Proportional rather than fixed: at the 800x400 minimum window a fixed pane
/// large enough to be useful leaves nothing for the sourceport list, and at 1200x800
/// a pane small enough to fit there wastes the room that makes a compile
/// readable. The clamp keeps both ends sane.
fn log_pane_height(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().height() * 0.35).clamp(90.0, 300.0)
}

/// Continue from the active install when the dialog opens. The recipe is only
/// the fallback for a sourceport that has not been built yet.
fn initial_selected_ref(sourceport: &SourceportStatus) -> String {
    sourceport
        .installed
        .as_ref()
        .map(|installed| installed.manifest.git_ref.clone())
        .unwrap_or_else(|| sourceport.recipe.git_ref.clone())
}

pub struct SourceportsDialogState {
    sourceports: Vec<SourceportStatus>,
    selected: Option<String>,
    /// Build ref selected per sourceport. Kept in dialog state only: choosing a
    /// one-off older version must not rewrite a user's recipe.
    selected_refs: BTreeMap<String, String>,
    /// Remote refs are intentionally absent until "Check versions" is used.
    remote_refs: BTreeMap<String, sourceports::RemoteRefs>,
    checking_versions: Option<String>,
    version_error: Option<(String, String)>,
    /// Cached pre-flight for the selected sourceport. Re-run on selection change
    /// rather than per frame — it shells out to the package manager.
    doctor: Option<DoctorReport>,
    clean: bool,
    /// Name of the sourceport being built, if any.
    running: Option<String>,
    step: Option<String>,
    log: Vec<String>,
    outcome: Option<Result<String, String>>,
    cancel: Arc<AtomicBool>,
    error: Option<String>,
    /// Set when a build or removal changed what a launch would resolve to.
    pub modified: bool,
}

impl SourceportsDialogState {
    pub fn new() -> Self {
        let mut state = Self {
            sourceports: Vec::new(),
            selected: None,
            selected_refs: BTreeMap::new(),
            remote_refs: BTreeMap::new(),
            checking_versions: None,
            version_error: None,
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
        match sourceports::status(&SourceportPaths::from_config()) {
            Ok(sourceports) => {
                self.sourceports = sourceports;
                for sourceport in &self.sourceports {
                    self.selected_refs
                        .entry(sourceport.recipe.name.clone())
                        .or_insert_with(|| initial_selected_ref(sourceport));
                }
                self.error = None;
            }
            Err(e) => {
                self.sourceports = Vec::new();
                self.error = Some(e.to_string());
            }
        }
        // Keep the selection if it still exists, otherwise take the first.
        let still_there = self
            .selected
            .as_ref()
            .is_some_and(|name| self.sourceports.iter().any(|p| &p.recipe.name == name));
        if !still_there {
            self.selected = self.sourceports.first().map(|p| p.recipe.name.clone());
        }
        self.refresh_doctor();
    }

    fn refresh_doctor(&mut self) {
        self.doctor = self.selected_port().map(|p| sourceports::doctor(&p.recipe));
    }

    fn selected_port(&self) -> Option<&SourceportStatus> {
        let name = self.selected.as_ref()?;
        self.sourceports.iter().find(|p| &p.recipe.name == name)
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

    /// Called when the on-demand remote ref lookup finishes.
    pub fn versions_loaded(
        &mut self,
        sourceport: String,
        outcome: Result<sourceports::RemoteRefs, String>,
    ) {
        if self.checking_versions.as_deref() == Some(&sourceport) {
            self.checking_versions = None;
        }
        match outcome {
            Ok(refs) => {
                self.remote_refs.insert(sourceport.clone(), refs);
                if self
                    .version_error
                    .as_ref()
                    .is_some_and(|(name, _)| name == &sourceport)
                {
                    self.version_error = None;
                }
            }
            Err(error) => self.version_error = Some((sourceport, error)),
        }
    }

    fn check_versions(&mut self, sourceport: String, repo: String) -> SourceportVersionsRequest {
        self.checking_versions = Some(sourceport.clone());
        self.version_error = None;
        SourceportVersionsRequest { sourceport, repo }
    }

    fn start(&mut self, sourceport: String, git_ref: String) -> SourceportBuildRequest {
        // A fresh flag per run: reusing a cancelled one would abort instantly.
        self.cancel = Arc::new(AtomicBool::new(false));
        self.running = Some(sourceport.clone());
        self.log.clear();
        self.outcome = None;
        self.step = None;
        SourceportBuildRequest {
            sourceport,
            git_ref,
            clean: self.clean,
            cancel: Arc::clone(&self.cancel),
        }
    }

    // -- rendering ---------------------------------------------------------

    pub fn render(&mut self, ctx: &egui::Context) -> SourceportsResult {
        let mut result = SourceportsResult::Open;

        // What the log costs this frame, taken out of the body's budget rather
        // than rendered after it inside the same scroll area. A compile is the
        // whole reason this dialog is open, and output that has to be scrolled
        // into view is output nobody watches — so the sourceport list and the details
        // shrink and the log stays on screen from the first line to the last.
        let showing_status = self.running.is_some() || self.outcome.is_some();
        let log_height = if self.log.is_empty() {
            0.0
        } else {
            log_pane_height(ctx)
        };
        let reserved = FOOTER
            + if showing_status { STATUS_ROW } else { 0.0 }
            + if log_height > 0.0 {
                log_height + LOG_CHROME
            } else {
                0.0
            };
        let body_height = crate::dialogs::modal_body_height(ctx, reserved);

        crate::dialogs::modal_window(ctx, "Sourceports", [820.0, 620.0]).show(ctx, |ui| {
            crate::dialogs::scroll_body(ui, body_height, |ui| {
                if let Some(error) = &self.error {
                    ui.colored_label(theme::COLOR_ERROR, error);
                    ui.add_space(6.0);
                }

                // Dropped once there is output to read. It explains what the
                // dialog is for, which stops being the question the moment a
                // build is underway, and at the 800x400 minimum window it is
                // two lines the sourceport list needs more.
                if self.log.is_empty() {
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        "Builds a sourceport from source into caco's own prefix. Nothing is \
                         installed system-wide, and the recipe travels with your library so \
                         another machine can rebuild it.",
                    );
                    ui.add_space(8.0);
                }

                ui.horizontal_top(|ui| {
                    self.render_list(ui);
                    ui.separator();
                    ui.vertical(|ui| self.render_details(ui, &mut result));
                });
            });

            self.render_log(ui, log_height);

            ui.add_space(6.0);
            if ui
                .add_enabled(self.running.is_none(), egui::Button::new("Close"))
                .clicked()
            {
                result = SourceportsResult::Closed;
            }
        });

        // Escape must not close mid-build: the log would be lost with nowhere
        // to report a failure.
        if self.running.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return SourceportsResult::Closed;
        }

        result
    }

    fn render_list(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.set_min_width(200.0);
            ui.set_max_width(200.0);
            let entries: Vec<(String, String, egui::Color32)> = self
                .sourceports
                .iter()
                .map(|p| {
                    let selected_ref = self
                        .selected_refs
                        .get(&p.recipe.name)
                        .map(String::as_str)
                        .unwrap_or(&p.recipe.git_ref);
                    let newer_release = p.installed.as_ref().is_some_and(|installed| {
                        self.remote_refs
                            .get(&p.recipe.name)
                            .and_then(|refs| refs.newer_release_than(&installed.manifest.git_ref))
                            .is_some()
                    });
                    // A rebuild can switch the selected ref, advance to a new
                    // release tag, or pick up commits on the current branch.
                    let (label, color) = match &p.installed {
                        None => ("not built", theme::TEXT_MUTED),
                        Some(_) if p.selected_ref_changed(selected_ref) => {
                            ("version selected", theme::COLOR_WARNING)
                        }
                        Some(_) if newer_release || p.update_available() => {
                            ("update available", theme::COLOR_WARNING)
                        }
                        Some(_) => ("installed", theme::COLOR_SUCCESS),
                    };
                    (p.recipe.name.clone(), label.to_string(), color)
                })
                .collect();

            for (name, label, color) in entries {
                let selected = self.selected.as_deref() == Some(name.as_str());
                let response = ui.add(egui::Button::selectable(
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

    fn render_details(&mut self, ui: &mut egui::Ui, result: &mut SourceportsResult) {
        let Some(sourceport) = self.selected_port().cloned() else {
            ui.colored_label(theme::TEXT_SECONDARY, "No recipes.");
            return;
        };

        let name = sourceport.recipe.name.clone();
        let recipe_ref = sourceport.recipe.git_ref.clone();
        let selected_ref = self
            .selected_refs
            .get(&name)
            .cloned()
            .unwrap_or_else(|| recipe_ref.clone());
        let running_this = self.running.as_deref() == Some(name.as_str());
        let busy = self.running.is_some();

        ui.horizontal(|ui| {
            ui.strong(&name);
            ui.colored_label(
                theme::TEXT_MUTED,
                format!("{} @ {}", sourceport.recipe.repo, selected_ref),
            );
        });
        ui.add_space(4.0);

        match &sourceport.installed {
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
                if sourceport.selected_ref_changed(&selected_ref) {
                    ui.colored_label(
                        theme::COLOR_WARNING,
                        format!(
                            "Selected version is '{}' — rebuild to switch.",
                            selected_ref
                        ),
                    );
                } else if sourceport.update_available() {
                    let remote = sourceport
                        .remote_commit
                        .as_deref()
                        .unwrap_or_default()
                        .chars()
                        .take(9)
                        .collect::<String>();
                    ui.colored_label(
                        theme::COLOR_WARNING,
                        format!("Upstream is at {remote} — rebuild to update."),
                    );
                }
            }
            None => {
                ui.colored_label(theme::TEXT_SECONDARY, "Not built yet.");
            }
        }

        ui.add_space(8.0);
        self.render_version_picker(ui, &sourceport, result);
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
                let git_ref = self.selected_refs.get(&name).cloned().unwrap_or(recipe_ref);
                *result = SourceportsResult::Start(self.start(name.clone(), git_ref));
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
                    .on_hover_text("Sets this sourceport as the one caco launches WADs with")
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
                    match sourceports::remove_installed(&installed.prefix) {
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

    fn render_version_picker(
        &mut self,
        ui: &mut egui::Ui,
        sourceport: &SourceportStatus,
        result: &mut SourceportsResult,
    ) {
        let name = &sourceport.recipe.name;
        let recipe_ref = &sourceport.recipe.git_ref;
        let mut selected = self
            .selected_refs
            .get(name)
            .cloned()
            .unwrap_or_else(|| recipe_ref.clone());
        let refs = self.remote_refs.get(name).cloned();
        let checking = self.checking_versions.as_deref() == Some(name);
        let has_refs = refs.is_some();
        let mut check_clicked = false;

        ui.horizontal(|ui| {
            ui.label("Build from");
            if let Some(refs) = &refs {
                egui::ComboBox::from_id_salt(("sourceport-build-ref", name))
                    .selected_text(selected.as_str())
                    .width(210.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut selected,
                            recipe_ref.clone(),
                            format!("Recipe default — {recipe_ref}"),
                        );
                        if let Some(latest) = refs.latest_release() {
                            ui.selectable_value(
                                &mut selected,
                                latest.to_string(),
                                format!("Latest release — {latest}"),
                            );
                        }
                        if !refs.releases.is_empty() {
                            ui.separator();
                            ui.label(egui::RichText::new("Releases").small().strong());
                            for release in &refs.releases {
                                ui.selectable_value(&mut selected, release.clone(), release);
                            }
                        }
                        if !refs.branches.is_empty() {
                            ui.separator();
                            ui.label(egui::RichText::new("Branches").small().strong());
                            for branch in &refs.branches {
                                ui.selectable_value(&mut selected, branch.clone(), branch);
                            }
                        }
                    });
            } else {
                ui.monospace(selected.as_str());
            }

            if checking {
                ui.spinner();
                ui.colored_label(theme::TEXT_MUTED, "Checking…");
            } else if ui
                .small_button(if has_refs {
                    "Refresh"
                } else {
                    "Check versions"
                })
                .on_hover_text("Ask the git remote for release tags and branches")
                .clicked()
            {
                check_clicked = true;
            }
        });
        self.selected_refs.insert(name.clone(), selected);
        if check_clicked {
            *result = SourceportsResult::CheckVersions(
                self.check_versions(name.clone(), sourceport.recipe.repo.clone()),
            );
        }

        if let Some(refs) = &refs {
            match refs.latest_release() {
                Some(latest) => {
                    let newer_release = sourceport.installed.as_ref().is_some_and(|installed| {
                        refs.newer_release_than(&installed.manifest.git_ref)
                            .is_some()
                    });
                    ui.colored_label(
                        if newer_release {
                            theme::COLOR_WARNING
                        } else {
                            theme::TEXT_MUTED
                        },
                        if newer_release {
                            format!("New release available: {latest}")
                        } else {
                            format!("Latest release tag: {latest}")
                        },
                    );
                }
                None => {
                    ui.colored_label(
                        theme::TEXT_MUTED,
                        "This remote does not advertise release tags.",
                    );
                }
            }
        }
        if let Some((error_port, error)) = &self.version_error
            && error_port == name
        {
            ui.colored_label(
                theme::COLOR_ERROR,
                format!("Could not check versions: {error}"),
            );
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

    /// Status line plus the build log, pinned below the scrolling body.
    ///
    /// `height` is handed in rather than asked for: whoever sized the body
    /// already subtracted this pane from the window, and the two numbers have
    /// to be the same one or the window grows past what it reserved.
    fn render_log(&self, ui: &mut egui::Ui, height: f32) {
        if let Some(sourceport) = &self.running {
            ui.horizontal(|ui| {
                ui.spinner();
                match &self.step {
                    Some(step) => {
                        ui.colored_label(theme::TEXT_SECONDARY, format!("{sourceport}: {step}"))
                    }
                    None => {
                        ui.colored_label(theme::TEXT_SECONDARY, format!("{sourceport}: starting"))
                    }
                };
            });
        } else if let Some(outcome) = &self.outcome {
            match outcome {
                Ok(msg) => ui.colored_label(theme::COLOR_SUCCESS, msg),
                Err(msg) => ui.colored_label(theme::COLOR_ERROR, msg),
            };
        }

        if height <= 0.0 {
            return;
        }
        ui.add_space(4.0);
        ui.separator();
        // `auto_shrink` off so the pane is its reserved size from the first
        // line onward: a log that grows the window as output arrives makes
        // every control below it move while the user is reading.
        egui::ScrollArea::vertical()
            .id_salt("sourceport-build-log")
            .max_height(height)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.log {
                    ui.label(egui::RichText::new(line).monospace().size(10.0));
                }
            });
    }
}

impl Default for SourceportsDialogState {
    fn default() -> Self {
        Self::new()
    }
}

/// Point `config.sourceport` at a freshly built sourceport.
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use caco_core::sourceports::{InstalledSourceport, SourceportManifest};

    use super::*;

    fn sourceport_status(installed_ref: Option<&str>) -> SourceportStatus {
        let recipe = sourceports::builtin_recipes()
            .into_iter()
            .find(|recipe| recipe.name == "nyan-doom")
            .unwrap();
        let installed = installed_ref.map(|git_ref| InstalledSourceport {
            manifest: SourceportManifest {
                name: recipe.name.clone(),
                repo: recipe.repo.clone(),
                git_ref: git_ref.to_string(),
                commit: "deadbeef".into(),
                binary: recipe.binary.clone(),
                built_at: "2026-01-01T00:00:00+00:00".into(),
            },
            prefix: PathBuf::from("/unused"),
        });
        SourceportStatus {
            recipe,
            installed,
            remote_commit: None,
        }
    }

    #[test]
    fn an_installed_release_stays_selected_instead_of_the_recipe_branch() {
        let status = sourceport_status(Some("v1.2.3"));
        assert_eq!(status.recipe.git_ref, "master");
        assert_eq!(initial_selected_ref(&status), "v1.2.3");
    }

    #[test]
    fn an_unbuilt_sourceport_starts_from_the_recipe() {
        let status = sourceport_status(None);
        assert_eq!(initial_selected_ref(&status), "master");
    }
}
