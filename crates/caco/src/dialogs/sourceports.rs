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
//! The log is not part of the scrolling body. It is pinned above the button row, because
//! the output is the reason the dialog is open and anything that has to be scrolled into
//! view during a four-minute compile may as well not be shown.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use caco_core::sourceports::{
    self, DoctorReport, PackageCheck, RemoteVersions, SourceportPaths, SourceportStatus, Update,
    Version,
};
use caco_core::utils::format_size;

use crate::dialogs::{Dialog, FooterRow, Primary};
use crate::theme;

/// A build the dialog is asking the app to start.
pub struct SourceportBuildRequest {
    pub sourceport: String,
    pub version: Version,
    pub clean: bool,
    pub cancel: Arc<AtomicBool>,
}

/// Asking a sourceport's repository which versions it has, which the dialog does the first
/// time it shows one. By name, as a build is, and the worker reads the recipe.
pub struct SourceportVersionsRequest {
    pub sourceport: String,
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

/// How much of the window the build log may claim.
///
/// Proportional rather than fixed: at the 800x400 minimum window a fixed pane
/// large enough to be useful leaves nothing for the sourceport list, and at 1200x800
/// a pane small enough to fit there wastes the room that makes a compile
/// readable. The clamp keeps both ends sane.
fn log_pane_height(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().height() * 0.35).clamp(90.0, 300.0)
}

/// What the spinner beside the version picker is called, while the repository is asked.
pub const CHECKING_VERSIONS: &str = "Checking which versions there are";

/// Continue from the active install when the dialog opens. The recipe is only
/// the fallback for a sourceport that has not been built yet.
fn initial_version(sourceport: &SourceportStatus) -> Version {
    sourceport
        .installed_version()
        .unwrap_or_else(|| sourceport.recipe.default_version())
}

/// What a version is called in the picker.
fn version_label(version: &Version, versions: Option<&RemoteVersions>) -> String {
    let development = versions.and_then(RemoteVersions::development);
    match version {
        Version::LatestRelease => match versions.and_then(RemoteVersions::latest_release) {
            Some(tag) => format!("Latest release ({tag})"),
            None => "Latest release".to_string(),
        },
        Version::Named(name) if development == Some(name.as_str()) => {
            format!("{name} (development)")
        }
        Version::Named(name) => name.clone(),
    }
}

/// What the picker offers: the latest release and the development branch, then whatever is
/// built or chosen already if it is neither, so that it can be gone back to; and the older
/// releases, newest first, apart from those. Nothing else: a repository's other branches are
/// its developers' work in progress, and its other tags are candidates, CI's, and history.
fn picker_options(
    choice: &Version,
    installed: Option<&Version>,
    versions: Option<&RemoteVersions>,
) -> (Vec<Version>, Vec<Version>) {
    let older: Vec<Version> = versions
        .map(|versions| {
            versions
                .older_releases()
                .iter()
                .map(|tag| Version::Named(tag.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut main = Vec::new();
    if versions.is_none_or(|versions| versions.latest_release().is_some()) {
        main.push(Version::LatestRelease);
    }
    if let Some(development) = versions.and_then(RemoteVersions::development) {
        main.push(Version::Named(development.to_string()));
    }
    for already in installed.into_iter().chain([choice]) {
        if !main.contains(already) && !older.contains(already) {
            main.push(already.clone());
        }
    }
    (main, older)
}

/// What to say when the version picked is not the one built.
fn switch_message(choice: &Version, versions: Option<&RemoteVersions>) -> String {
    let to = match choice {
        Version::LatestRelease => versions
            .and_then(RemoteVersions::latest_release)
            .map_or_else(|| "the latest release".to_string(), str::to_string),
        Version::Named(name) => name.clone(),
    };
    format!("Rebuild to switch to {to}.")
}

/// What to say when rebuilding would bring something new.
fn update_message(update: &Update, built_from: &str) -> String {
    match update {
        Update::Release(tag) => format!("{tag} is out. Rebuild to update."),
        Update::Commits => format!("There are new changes on {built_from}. Rebuild to get them."),
    }
}

pub struct SourceportsDialogState {
    sourceports: Vec<SourceportStatus>,
    selected: Option<String>,
    /// Version picked per sourceport. Kept in dialog state only: picking an
    /// older version must not rewrite anybody's recipe.
    choices: BTreeMap<String, Version>,
    /// What each sourceport's repository has, asked the first time the
    /// sourceport is shown, since until then "Latest release" has no number.
    versions: BTreeMap<String, RemoteVersions>,
    checking_versions: BTreeSet<String>,
    /// Why asking failed, until the user asks again.
    version_errors: BTreeMap<String, String>,
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
        let mut state = Self::empty();
        state.reload();
        state
    }

    /// Nothing loaded: what the tests start from, since [`Self::reload`] reads the real
    /// sourceport directories.
    fn empty() -> Self {
        Self {
            sourceports: Vec::new(),
            selected: None,
            choices: BTreeMap::new(),
            versions: BTreeMap::new(),
            checking_versions: BTreeSet::new(),
            version_errors: BTreeMap::new(),
            doctor: None,
            clean: false,
            running: None,
            step: None,
            log: Vec::new(),
            outcome: None,
            cancel: Arc::new(AtomicBool::new(false)),
            error: None,
            modified: false,
        }
    }

    fn reload(&mut self) {
        match sourceports::status(&SourceportPaths::from_config()) {
            Ok(sourceports) => {
                self.sourceports = sourceports;
                for sourceport in &self.sourceports {
                    self.choices
                        .entry(sourceport.recipe.name.clone())
                        .or_insert_with(|| initial_version(sourceport));
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

    /// Called when asking a repository which versions it has finishes.
    pub fn versions_loaded(&mut self, sourceport: String, outcome: Result<RemoteVersions, String>) {
        self.checking_versions.remove(&sourceport);
        match outcome {
            Ok(versions) => {
                // A project that has never made a release is built from its development
                // branch, and the picker should say so rather than offer a release that
                // is not there.
                if let Some(choice) = self.choices.get_mut(&sourceport)
                    && *choice == Version::LatestRelease
                    && versions.latest_release().is_none()
                    && let Some(development) = versions.development()
                {
                    *choice = Version::Named(development.to_string());
                }
                self.version_errors.remove(&sourceport);
                self.versions.insert(sourceport, versions);
            }
            Err(error) => {
                self.version_errors.insert(sourceport, error);
            }
        }
    }

    /// Ask about the sourceport on show, once, if nobody has yet and the last try did
    /// not fail: a failure waits for Try Again, rather than asking every frame.
    fn next_version_check(&mut self) -> Option<SourceportVersionsRequest> {
        let sourceport = self.selected_port()?.recipe.name.clone();
        if self.versions.contains_key(&sourceport)
            || self.checking_versions.contains(&sourceport)
            || self.version_errors.contains_key(&sourceport)
        {
            return None;
        }
        self.checking_versions.insert(sourceport.clone());
        Some(SourceportVersionsRequest { sourceport })
    }

    fn choice(&self, sourceport: &SourceportStatus) -> Version {
        self.choices
            .get(&sourceport.recipe.name)
            .cloned()
            .unwrap_or_else(|| initial_version(sourceport))
    }

    fn start(&mut self, sourceport: String, version: Version) -> SourceportBuildRequest {
        // A fresh flag per run: reusing a cancelled one would abort instantly.
        self.cancel = Arc::new(AtomicBool::new(false));
        self.running = Some(sourceport.clone());
        self.log.clear();
        self.outcome = None;
        self.step = None;
        SourceportBuildRequest {
            sourceport,
            version,
            clean: self.clean,
            cancel: Arc::clone(&self.cancel),
        }
    }

    // -- rendering ---------------------------------------------------------

    pub fn render(&mut self, ctx: &egui::Context) -> SourceportsResult {
        let mut result = SourceportsResult::Open;

        // The log's height, taken out of the body's by the footer it is pinned into. A
        // compile is the whole reason this dialog is open, so the sourceport list and the
        // details shrink and the log stays on screen from the first line to the last.
        let log_height = if self.log.is_empty() {
            0.0
        } else {
            log_pane_height(ctx)
        };

        Dialog::new("Sourceports", [820.0, 620.0]).show(ctx, |dialog| {
            dialog
                .scroll(|ui| {
                    if let Some(error) = &self.error {
                        ui.colored_label(theme::COLOR_ERROR, error);
                        ui.add_space(6.0);
                    }

                    // Dropped once there is output to read. It explains what the dialog is
                    // for, which stops being the question the moment a build is underway, and
                    // at the 800x400 minimum window it is two lines the list needs more.
                    if self.log.is_empty() {
                        ui.colored_label(
                            theme::TEXT_SECONDARY,
                            "caco can build these from their source code, for when your system \
                             doesn't package them. They stay inside caco, so nothing else on \
                             this machine changes.",
                        );
                        ui.add_space(8.0);
                    }

                    ui.horizontal_top(|ui| {
                        self.render_list(ui);
                        ui.separator();
                        ui.vertical(|ui| self.render_details(ui));
                    });
                })
                .above(|ui| self.render_log(ui, log_height))
                .buttons(|row| {
                    if self.render_footer(row, &mut result) {
                        result = SourceportsResult::Closed;
                    }
                })
        });

        // Escape must not close mid-build: the log would be lost with nowhere
        // to report a failure.
        if self.running.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return SourceportsResult::Closed;
        }

        // Last, so that it never takes the place of what was pressed this frame: only the
        // request that is sent is marked as asked.
        if matches!(result, SourceportsResult::Open)
            && let Some(request) = self.next_version_check()
        {
            result = SourceportsResult::CheckVersions(request);
        }

        result
    }

    /// What can be done with the selected sourceport. During a build there is one thing to do
    /// about it, and no way out. Returns true if Close was pressed.
    fn render_footer(&mut self, row: &mut FooterRow<'_>, result: &mut SourceportsResult) -> bool {
        if self.running.is_some() {
            let ended = row.end(Some("Stop Build"), None);
            if ended.dismissed {
                self.cancel.store(true, Ordering::Relaxed);
            }
            return false;
        }

        let Some(sourceport) = self.selected_port().cloned() else {
            return row.close();
        };
        let name = sourceport.recipe.name.clone();
        let installed = sourceport.installed.clone();

        if let Some(installed) = &installed {
            row.left(|ui| {
                if ui
                    .button("Use as Default")
                    .on_hover_text("caco launches WADs with this one, unless a WAD says otherwise")
                    .clicked()
                {
                    match set_default_sourceport(&name) {
                        Ok(()) => {
                            self.modified = true;
                            self.outcome =
                                Some(Ok(format!("{name} is now the default sourceport.")));
                        }
                        Err(e) => self.outcome = Some(Err(e)),
                    }
                }
                if ui
                    .button("Remove")
                    .on_hover_text("Deletes this build. You can build it again later.")
                    .clicked()
                {
                    match sourceports::remove_installed(&installed.prefix) {
                        Ok(()) => {
                            self.modified = true;
                            self.outcome = Some(Ok(format!("Removed {name}.")));
                            self.reload();
                        }
                        Err(e) => self.outcome = Some(Err(e.to_string())),
                    }
                }
            });
        }

        let verb = if installed.is_some() {
            "Rebuild"
        } else {
            "Build"
        };
        let can_build = self.doctor.as_ref().is_none_or(DoctorReport::can_build);
        let ended = row.end(
            Some("Close"),
            Some(Primary::new(verb).when(can_build, "Install the missing build tools first")),
        );
        if ended.primary {
            let version = self.choice(&sourceport);
            *result = SourceportsResult::Start(self.start(name, version));
        }
        ended.dismissed
    }

    fn render_list(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.set_min_width(200.0);
            ui.set_max_width(200.0);
            let entries: Vec<(String, String, egui::Color32)> = self
                .sourceports
                .iter()
                .map(|p| {
                    let versions = self.versions.get(&p.recipe.name);
                    // A rebuild can switch to the version picked, or bring a newer release or
                    // new commits to the one built.
                    let (label, color) = match &p.installed {
                        None => ("not built", theme::TEXT_MUTED),
                        Some(_) if p.switches_version(&self.choice(p)) => {
                            ("version picked", theme::COLOR_WARNING)
                        }
                        Some(_) if p.update(versions).is_some() => {
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
                    ui.colored_label(color, egui::RichText::new(label).small());
                });
                if response.clicked() {
                    self.select(&name);
                }
                ui.add_space(4.0);
            }
        });
    }

    fn render_details(&mut self, ui: &mut egui::Ui) {
        let Some(sourceport) = self.selected_port().cloned() else {
            ui.colored_label(theme::TEXT_SECONDARY, "No sourceports to build.");
            return;
        };

        let name = sourceport.recipe.name.clone();
        let choice = self.choice(&sourceport);
        let versions = self.versions.get(&name);
        let busy = self.running.is_some();

        ui.horizontal(|ui| {
            ui.strong(&name);
            ui.colored_label(theme::TEXT_MUTED, &sourceport.recipe.repo);
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
                if sourceport.switches_version(&choice) {
                    ui.colored_label(theme::COLOR_WARNING, switch_message(&choice, versions));
                } else if let Some(update) = sourceport.update(versions) {
                    ui.colored_label(
                        theme::COLOR_WARNING,
                        update_message(&update, &installed.manifest.git_ref),
                    );
                }
            }
            None => {
                ui.colored_label(theme::TEXT_SECONDARY, "Not built yet.");
            }
        }

        ui.add_space(8.0);
        self.render_version_picker(ui, &sourceport);
        ui.add_space(8.0);
        self.render_doctor(ui);
        ui.add_space(8.0);
        ui.add_enabled(
            !busy,
            egui::Checkbox::new(&mut self.clean, "Build from scratch"),
        )
        .on_hover_text("Slower, but it can fix a build that keeps failing");
    }

    fn render_version_picker(&mut self, ui: &mut egui::Ui, sourceport: &SourceportStatus) {
        let name = &sourceport.recipe.name;
        let mut choice = self.choice(sourceport);
        let installed = sourceport.installed_version();
        let versions = self.versions.get(name);
        let (main, older) = picker_options(&choice, installed.as_ref(), versions);

        ui.horizontal(|ui| {
            let label = ui.label("Version");
            egui::ComboBox::from_id_salt(("sourceport-version", name))
                .selected_text(version_label(&choice, versions))
                .width(230.0)
                .show_ui(ui, |ui| {
                    for version in &main {
                        let text = version_label(version, versions);
                        ui.selectable_value(&mut choice, version.clone(), text);
                    }
                    if !older.is_empty() {
                        ui.separator();
                        theme::section_heading(ui, "Older releases");
                        for version in &older {
                            let text = version_label(version, versions);
                            ui.selectable_value(&mut choice, version.clone(), text);
                        }
                    }
                })
                .response
                .labelled_by(label.id);
            if self.checking_versions.contains(name) {
                // Named, so that a screen reader hears why the picker has no numbers yet.
                ui.spinner().widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::accesskit::Role::ProgressIndicator,
                        true,
                        CHECKING_VERSIONS,
                    )
                });
            }
        });

        if let Some(error) = self.version_errors.get(name) {
            let mut try_again = false;
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    theme::COLOR_ERROR,
                    format!("Couldn't check which versions there are: {error}"),
                );
                try_again = ui.small_button("Try Again").clicked();
            });
            if try_again {
                self.version_errors.remove(name);
            }
        }
        self.choices.insert(name.clone(), choice);
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
                    "Not installed: {}. Some may be optional.",
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
            ui.colored_label(theme::COLOR_SUCCESS, "Everything it needs is installed.");
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
                    ui.label(egui::RichText::new(line).monospace().size(11.0));
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

    const UZDOOM: &str = "\
ref: refs/heads/trunk\tHEAD
1111111111111111111111111111111111111111\trefs/heads/trunk
4444444444444444444444444444444444444444\trefs/heads/4.14
5005005005005005005005005005005005005005\trefs/tags/5.0.0
5025025025025025025025025025025025025025\trefs/tags/5.0.2
5035035035035035035035035035035035035035\trefs/tags/5.0.3
9999999999999999999999999999999999999999\trefs/tags/x-testing
";

    fn uzdoom_versions() -> RemoteVersions {
        RemoteVersions::parse(UZDOOM, None)
    }

    fn sourceport_status(installed: Option<(&str, Option<bool>)>) -> SourceportStatus {
        let recipe = sourceports::builtin_recipes()
            .into_iter()
            .find(|recipe| recipe.name == "uzdoom")
            .unwrap();
        let installed = installed.map(|(git_ref, follows_releases)| InstalledSourceport {
            manifest: SourceportManifest {
                name: recipe.name.clone(),
                repo: recipe.repo.clone(),
                git_ref: git_ref.to_string(),
                commit: "deadbeef".into(),
                follows_releases,
                binary: recipe.binary.clone(),
                built_at: "2026-01-01T00:00:00+00:00".into(),
            },
            prefix: PathBuf::from("/unused"),
        });
        SourceportStatus {
            recipe,
            installed,
            last_check: None,
        }
    }

    fn dialog_showing(sourceport: SourceportStatus) -> SourceportsDialogState {
        let mut dialog = SourceportsDialogState::empty();
        dialog.selected = Some(sourceport.recipe.name.clone());
        dialog.sourceports = vec![sourceport];
        dialog
    }

    #[test]
    fn an_unbuilt_sourceport_starts_from_the_latest_release() {
        assert_eq!(
            initial_version(&sourceport_status(None)),
            Version::LatestRelease
        );
    }

    #[test]
    fn the_dialog_picks_up_where_the_build_left_off() {
        let older = sourceport_status(Some(("5.0.2", Some(false))));
        assert_eq!(initial_version(&older), Version::Named("5.0.2".into()));
        let trunk = sourceport_status(Some(("trunk", None)));
        assert_eq!(initial_version(&trunk), Version::Named("trunk".into()));
        // Built before caco wrote down what a build follows, from a release.
        let release = sourceport_status(Some(("5.0.3", None)));
        assert_eq!(initial_version(&release), Version::LatestRelease);
    }

    #[test]
    fn the_picker_offers_releases_and_the_development_branch_and_nothing_else() {
        let versions = uzdoom_versions();
        let (main, older) = picker_options(&Version::LatestRelease, None, Some(&versions));
        assert_eq!(
            main,
            [Version::LatestRelease, Version::Named("trunk".into())]
        );
        assert_eq!(
            older,
            [
                Version::Named("5.0.2".into()),
                Version::Named("5.0.0".into())
            ]
        );
        let labels: Vec<String> = main
            .iter()
            .chain(&older)
            .map(|version| version_label(version, Some(&versions)))
            .collect();
        assert_eq!(
            labels,
            [
                "Latest release (5.0.3)",
                "trunk (development)",
                "5.0.2",
                "5.0.0"
            ]
        );
    }

    #[test]
    fn what_is_built_stays_in_the_picker_when_it_is_none_of_those() {
        let versions = uzdoom_versions();
        let built = Version::Named("4.14".into());
        let (main, _) = picker_options(&Version::LatestRelease, Some(&built), Some(&versions));
        assert!(main.contains(&built), "nothing to go back to: {main:?}");
    }

    #[test]
    fn before_the_repository_answers_the_picker_has_the_latest_release() {
        let (main, older) = picker_options(&Version::LatestRelease, None, None);
        assert_eq!(main, [Version::LatestRelease]);
        assert!(older.is_empty());
        assert_eq!(
            version_label(&Version::LatestRelease, None),
            "Latest release"
        );
    }

    #[test]
    fn a_project_without_releases_is_shown_as_its_development_branch() {
        let mut dialog = dialog_showing(sourceport_status(None));
        dialog
            .choices
            .insert("uzdoom".into(), Version::LatestRelease);
        let versions =
            RemoteVersions::parse("ref: refs/heads/main\tHEAD\nabc\trefs/heads/main\n", None);
        dialog.versions_loaded("uzdoom".into(), Ok(versions.clone()));
        assert_eq!(dialog.choices["uzdoom"], Version::Named("main".into()));
        let (main, _) = picker_options(&dialog.choices["uzdoom"], None, Some(&versions));
        assert_eq!(main, [Version::Named("main".into())]);
    }

    #[test]
    fn the_repository_is_asked_once_and_again_only_after_try_again() {
        let mut dialog = dialog_showing(sourceport_status(None));
        assert!(dialog.next_version_check().is_some());
        assert!(dialog.next_version_check().is_none(), "asked while asking");

        dialog.versions_loaded("uzdoom".into(), Err("offline".into()));
        assert!(dialog.next_version_check().is_none(), "asked every frame");
        dialog.version_errors.remove("uzdoom");
        assert!(dialog.next_version_check().is_some());

        dialog.versions_loaded("uzdoom".into(), Ok(uzdoom_versions()));
        assert!(dialog.next_version_check().is_none());
    }

    #[test]
    fn the_messages_name_what_a_rebuild_would_bring() {
        let versions = uzdoom_versions();
        assert_eq!(
            switch_message(&Version::LatestRelease, Some(&versions)),
            "Rebuild to switch to 5.0.3."
        );
        assert_eq!(
            switch_message(&Version::LatestRelease, None),
            "Rebuild to switch to the latest release."
        );
        assert_eq!(
            update_message(&Update::Release("5.0.4".into()), "5.0.3"),
            "5.0.4 is out. Rebuild to update."
        );
        assert_eq!(
            update_message(&Update::Commits, "trunk"),
            "There are new changes on trunk. Rebuild to get them."
        );
    }
}
