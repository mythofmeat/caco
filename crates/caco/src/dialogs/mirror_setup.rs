//! The wizard for adding a mirror, or changing one: which kind, how to set that kind
//! up, and a test that it works before anything is saved.
//!
//! A mirror is meant to be something a person can set up without leaving caco, so this
//! says what to do at each step, in the words the other end uses: the Gitea steps were
//! read off a real Gitea's token page, and the rclone one was run. What caco can do by
//! itself it does when you finish, in `caco_sources::mirror::set_up_mirror`: create the
//! folder, and write an empty index.
//!
//! `mirrors.rs` owns the page this renders on, and the workers. This only holds the form
//! and says what it wants done.

use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

use caco_core::config::{MirrorConfig, SavedToken};
use caco_core::mirror_tokens::TokenFile;
use caco_sources::mirror::MirrorProbe;

use crate::theme;
use crate::workers::{FileDialogReceiver, FileDialogRequest, spawn_file_dialog};

/// What a mirror lives on. Not saved anywhere: it only decides what the wizard asks,
/// and it can be read back off a mirror's config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorKind {
    Folder,
    Gitea,
    Webdav,
    WebServer,
}

impl MirrorKind {
    pub const ALL: [MirrorKind; 4] = [
        MirrorKind::Folder,
        MirrorKind::Gitea,
        MirrorKind::Webdav,
        MirrorKind::WebServer,
    ];

    pub fn title(self) -> &'static str {
        match self {
            MirrorKind::Folder => "A Folder",
            MirrorKind::Gitea => "Gitea",
            MirrorKind::Webdav => "WebDAV",
            MirrorKind::WebServer => "A Web Server",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            MirrorKind::Folder => {
                "A folder on a NAS, an external drive, or anything else this machine can \
                 reach as a path. The simplest, if you have somewhere like that."
            }
            MirrorKind::Gitea => {
                "A package on a Gitea server. Reachable wherever the server is, and all it \
                 takes is a token."
            }
            MirrorKind::Webdav => {
                "A folder on a server that speaks WebDAV: Nextcloud, a Synology or QNAP NAS, \
                 or a server of your own."
            }
            MirrorKind::WebServer => {
                "Any server that can serve a folder. Read only: caco can download from it but \
                 not upload, so something else has to fill it."
            }
        }
    }

    fn default_name(self) -> &'static str {
        match self {
            MirrorKind::Folder => "folder",
            MirrorKind::Gitea => "gitea",
            MirrorKind::Webdav => "webdav",
            MirrorKind::WebServer => "web",
        }
    }

    fn can_write(self) -> bool {
        self != MirrorKind::WebServer
    }

    /// Which kind a saved mirror is, to ask the right questions when it is edited. A
    /// URL that could be either is WebDAV if it logs in or is written to, since a plain
    /// web server does neither.
    pub fn of(config: &MirrorConfig) -> Self {
        match config.url.as_deref() {
            None => MirrorKind::Folder,
            Some(url) if parse_gitea_url(url).is_some() => MirrorKind::Gitea,
            Some(_) if config.login().is_some() || config.write => MirrorKind::Webdav,
            Some(_) => MirrorKind::WebServer,
        }
    }
}

/// The root of a mirror in Gitea's generic package registry, which has `<package>` as
/// the mirror and each `<sha256>` as a version of it.
fn gitea_url(server: &str, owner: &str, package: &str) -> Option<String> {
    let (server, owner, package) = (server.trim(), owner.trim(), package.trim());
    if server.is_empty() || owner.is_empty() || package.is_empty() {
        return None;
    }
    let server = server.trim_end_matches('/');
    Some(format!("{server}/api/packages/{owner}/generic/{package}"))
}

/// The server, owner and package of a mirror on a Gitea registry, if it is one.
fn parse_gitea_url(url: &str) -> Option<(String, String, String)> {
    let (server, path) = url.split_once("/api/packages/")?;
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    match parts.as_slice() {
        [owner, "generic", package] => Some((
            server.to_string(),
            (*owner).to_string(),
            (*package).to_string(),
        )),
        _ => None,
    }
}

/// Gitea's page for making a token.
fn gitea_token_page(server: &str) -> String {
    format!(
        "{}/user/settings/applications",
        server.trim().trim_end_matches('/')
    )
}

/// What Gitea takes for a package name: seen to refuse anything else with a 400.
fn is_gitea_package_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
}

/// `base`, or `base-2` and so on if that is taken.
fn unused_name(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|name| name == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|name| !taken.contains(name))
        .expect("there is always another number")
}

/// How much room how a test went gets, above the buttons. It scrolls past that, which
/// the longest failures here do not need at any window size caco allows.
const CHECK_HEIGHT: f32 = 84.0;

/// What the person has typed: text until it is a [`MirrorConfig`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Form {
    name: String,
    folder: String,
    address: String,
    server: String,
    owner: String,
    package: String,
    username: String,
    /// Empty keeps whatever is saved, when editing.
    token: String,
    token_env: String,
    write: bool,
}

/// How a test or a set-up went.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Check {
    NotRun,
    Running,
    Passed(MirrorProbe),
    Failed(String),
    SettingUp,
    SetUpFailed(String),
}

/// What the wizard wants done with what it has.
#[derive(Debug, PartialEq, Eq)]
pub enum SetupAction {
    None,
    /// Leave without saving.
    Cancel,
    /// Try this mirror, on a worker.
    Probe(Box<MirrorConfig>),
    /// Make this mirror ready, on a worker, and save it if that works.
    SetUp(Box<MirrorConfig>),
    /// Save this mirror as it is, without being able to reach it.
    SaveAnyway(Box<MirrorConfig>),
}

pub struct MirrorSetup {
    /// `None` until a kind is picked.
    kind: Option<MirrorKind>,
    form: Form,
    /// The mirror being changed, by name, when this is not a new one.
    editing: Option<String>,
    /// The names of the other mirrors, which this one's cannot be.
    taken: Vec<String>,
    /// A token already saved for this mirror, which an empty field keeps.
    kept_token: Option<SavedToken>,
    forget_token: bool,
    token_file: PathBuf,
    check: Check,
    /// The config the last test or set-up was for. When the form no longer makes this
    /// the result says nothing about it, so a passed test cannot go stale.
    checked: Option<MirrorConfig>,
    picker: Option<FileDialogReceiver>,
}

impl MirrorSetup {
    /// A new mirror, starting from the choice of kind.
    pub fn new(taken: Vec<String>) -> Self {
        Self {
            kind: None,
            form: Form {
                package: "doom-mirror".to_string(),
                write: true,
                ..Default::default()
            },
            editing: None,
            taken,
            kept_token: None,
            forget_token: false,
            token_file: TokenFile::from_config().path().to_path_buf(),
            check: Check::NotRun,
            checked: None,
            picker: None,
        }
    }

    /// An existing mirror, at the page for its kind.
    pub fn editing(config: &MirrorConfig, taken: Vec<String>) -> Self {
        let kind = MirrorKind::of(config);
        let mut setup = Self::new(taken);
        setup.kind = Some(kind);
        setup.editing = Some(config.name.clone());
        setup.kept_token = config.saved_token.clone();
        setup.form.name = config.name.clone();
        setup.form.username = config.username.clone().unwrap_or_default();
        setup.form.token_env = config.token_env.clone().unwrap_or_default();
        setup.form.write = config.write;
        match (kind, config.url.as_deref()) {
            (MirrorKind::Gitea, Some(url)) => {
                if let Some((server, owner, package)) = parse_gitea_url(url) {
                    setup.form.server = server;
                    setup.form.owner = owner;
                    setup.form.package = package;
                }
            }
            (_, Some(url)) => setup.form.address = url.to_string(),
            (_, None) => setup.form.folder = config.path.clone().unwrap_or_default(),
        }
        setup
    }

    /// The window's title.
    pub fn title(&self) -> String {
        match &self.editing {
            Some(name) => format!("Edit {name}"),
            None => "Set Up a Mirror".to_string(),
        }
    }

    fn pick_kind(&mut self, kind: MirrorKind) {
        self.kind = Some(kind);
        self.form.name = unused_name(kind.default_name(), &self.taken);
        self.form.write = kind.can_write();
    }

    /// Whether Escape should go back a page rather than out of the wizard.
    pub fn go_back(&mut self) -> bool {
        if self.kind.is_some() && self.editing.is_none() {
            self.kind = None;
            return true;
        }
        false
    }

    /// The mirror the form describes.
    fn config(&self, kind: MirrorKind) -> MirrorConfig {
        let filled = |text: &str| {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        };
        let form = &self.form;
        let kept = (!self.forget_token)
            .then(|| self.kept_token.clone())
            .flatten();
        let mut config = MirrorConfig {
            name: form.name.trim().to_string(),
            token_env: filled(&form.token_env),
            write: form.write,
            saved_token: filled(&form.token).map(SavedToken).or(kept),
            ..Default::default()
        };
        match kind {
            MirrorKind::Folder => config.path = filled(&form.folder),
            MirrorKind::Gitea => {
                config.url = gitea_url(&form.server, &form.owner, &form.package);
            }
            MirrorKind::Webdav => {
                config.url = filled(&form.address);
                config.username = filled(&form.username);
            }
            MirrorKind::WebServer => config.url = filled(&form.address),
        }
        config
    }

    /// What stands in the way of trying this, in words, if anything does.
    fn problem(&self, kind: MirrorKind) -> Option<String> {
        let config = self.config(kind);
        if config.name.is_empty() {
            return Some("Give it a name.".to_string());
        }
        if self.taken.contains(&config.name) {
            return Some(format!("You already have a mirror called {}.", config.name));
        }
        match kind {
            MirrorKind::Folder if config.path.is_none() => {
                return Some("Choose a folder.".to_string());
            }
            MirrorKind::Gitea if config.url.is_none() => {
                return Some("Fill in the server, the owner and the package.".to_string());
            }
            MirrorKind::Gitea if !is_gitea_package_name(self.form.package.trim()) => {
                return Some(
                    "A package name can only have letters, digits, dots, hyphens, pluses and \
                     underscores."
                        .to_string(),
                );
            }
            MirrorKind::Webdav | MirrorKind::WebServer if config.url.is_none() => {
                return Some("Fill in its address.".to_string());
            }
            _ => {}
        }
        config.location().err().map(|problem| {
            let mut chars = problem.chars();
            match chars.next() {
                Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
                None => problem,
            }
        })
    }

    /// Called by the app when a test finishes.
    pub fn probe_finished(&mut self, outcome: Result<MirrorProbe, String>) {
        self.check = match outcome {
            Ok(probe) => Check::Passed(probe),
            Err(why) => Check::Failed(why),
        };
    }

    /// Called by the app when setting the mirror up failed. Success leaves this page.
    pub fn set_up_failed(&mut self, why: String) {
        self.check = Check::SetUpFailed(why);
    }

    /// What saving this does to the token file.
    pub fn token_changes(&self) -> TokenChanges {
        TokenChanges {
            renamed_from: self.editing.clone(),
            token: {
                let token = self.form.token.trim();
                (!token.is_empty()).then(|| token.to_string())
            },
            forget: self.forget_token,
        }
    }

    /// The name of the mirror this replaces, when it replaces one.
    pub fn replacing(&self) -> Option<&str> {
        self.editing.as_deref()
    }

    /// Whether a test or a set-up is running, which a change of page must wait for: the
    /// answer would otherwise land on a form that is no longer what it was asked about.
    pub fn is_busy(&self) -> bool {
        matches!(self.check, Check::Running | Check::SettingUp)
    }

    /// How much taller the footer is than its button row, to keep out of the body.
    pub fn footer_extra(&self) -> f32 {
        if self.kind.is_none() || self.check == Check::NotRun {
            0.0
        } else {
            CHECK_HEIGHT + 8.0
        }
    }

    pub fn render(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // The result only stands for what it was run on, whatever has been typed since.
        if let Some(kind) = self.kind
            && self.checked.as_ref() != Some(&self.config(kind))
        {
            self.check = Check::NotRun;
        }
        match self.kind {
            None => self.render_kinds(ui),
            Some(kind) => self.render_details(ctx, ui, kind),
        }
    }

    /// The page's footer, so that none of it is scrolled out of reach: the buttons, and
    /// above them how a test went. Below the fields it would be below the fold, and at
    /// the smallest window pressing Test It would seem to do nothing.
    pub fn render_footer(&mut self, ui: &mut egui::Ui) -> SetupAction {
        let Some(kind) = self.kind else {
            if ui.button("Cancel").clicked() {
                return SetupAction::Cancel;
            }
            return SetupAction::None;
        };

        if self.check != Check::NotRun {
            egui::ScrollArea::vertical()
                .id_salt("mirror_check")
                .max_height(CHECK_HEIGHT)
                .auto_shrink([false, false])
                .show(ui, |ui| self.render_check(ui, kind));
            ui.add_space(4.0);
        }

        let config = self.config(kind);
        let mut action = SetupAction::None;
        ui.horizontal(|ui| {
            let busy = self.is_busy();
            if self.editing.is_some() {
                if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() {
                    action = SetupAction::Cancel;
                }
            } else if ui.add_enabled(!busy, egui::Button::new("Back")).clicked() {
                self.kind = None;
                return;
            }

            let problem = self.problem(kind);
            let ready = matches!(&self.check, Check::Passed(probe) if probe.is_ok())
                || matches!(self.check, Check::SetUpFailed(_));

            if ready {
                if primary(ui, "Finish", problem.is_none()) {
                    self.checked = Some(config.clone());
                    self.check = Check::SettingUp;
                    action = SetupAction::SetUp(Box::new(config.clone()));
                }
            } else {
                let label = match self.check {
                    Check::NotRun | Check::Running | Check::SettingUp => "Test It",
                    _ => "Test Again",
                };
                if primary(ui, label, problem.is_none() && !busy) {
                    self.checked = Some(config.clone());
                    self.check = Check::Running;
                    action = SetupAction::Probe(Box::new(config.clone()));
                }
            }
            if busy {
                ui.spinner();
            }
            // A server that is down today can still be the right one, and a laptop
            // away from home should not have to wait to be told so.
            let failed = matches!(self.check, Check::Failed(_))
                || matches!(&self.check, Check::Passed(probe) if !probe.is_ok());
            if failed && ui.button("Add Anyway").clicked() {
                action = SetupAction::SaveAnyway(Box::new(config.clone()));
            }
            if let Some(problem) = &problem {
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    egui::RichText::new(problem).size(12.0),
                );
            }
        });
        action
    }

    fn render_kinds(&mut self, ui: &mut egui::Ui) {
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "A mirror is your own store of WAD files. caco looks there first for a WAD that \
             isn't on this machine, and offers to upload what it couldn't otherwise get back, \
             so any machine with your library can have them.",
        );
        ui.add_space(8.0);
        ui.strong("Where do you want to keep it?");
        ui.add_space(4.0);

        let mut picked = None;
        for kind in MirrorKind::ALL {
            ui.horizontal_top(|ui| {
                if ui
                    .add_sized([110.0, 28.0], egui::Button::new(kind.title()))
                    .clicked()
                {
                    picked = Some(kind);
                }
                note(ui, kind.summary());
            });
            ui.add_space(4.0);
        }
        if let Some(kind) = picked {
            self.pick_kind(kind);
        }

        ui.add_space(4.0);
        note(
            ui,
            "You can have more than one, and caco tries them in the order they are listed. \
             Keep one away from the disk your library is on: a mirror on the same disk goes \
             when the disk does.",
        );
    }

    fn render_details(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, kind: MirrorKind) {
        self.poll_picker();
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.strong(kind.title());
        match kind {
            MirrorKind::Folder => self.render_folder(ctx, ui),
            MirrorKind::Gitea => self.render_gitea(ui),
            MirrorKind::Webdav => self.render_webdav(ui),
            MirrorKind::WebServer => self.render_web_server(ui),
        }

        ui.add_space(8.0);
        ui.separator();
        field(ui, "Name", &mut self.form.name, "");
        note(
            ui,
            "What caco calls it, in this list and on its upload buttons.",
        );
        if kind.can_write() {
            ui.checkbox(&mut self.form.write, "caco may upload to it");
        }
    }

    fn render_folder(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        note(
            ui,
            "Any folder this machine can reach. Best on another disk, because a mirror on the \
             same disk as your library goes when the disk does.",
        );
        let label = form_label(ui, "Folder");
        ui.horizontal(|ui| {
            let browse_w = 80.0;
            let field_w =
                (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(120.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.form.folder)
                    .desired_width(field_w)
                    .text_color(theme::TEXT_PRIMARY)
                    .hint_text("/mnt/nas/doom-mirror"),
            )
            .labelled_by(label.id);
            if ui
                .add_enabled(self.picker.is_none(), egui::Button::new("Browse\u{2026}"))
                .clicked()
            {
                let start = dirs::home_dir().unwrap_or_default();
                let request = FileDialogRequest::folder().set_directory(start);
                self.picker = Some(spawn_file_dialog(Some(ctx.clone()), request));
            }
        });
        note(
            ui,
            "If it isn't there, caco will create it when you finish. If it should be there \
             already, check that its drive is mounted. Already a mirror on another machine? \
             Point this at the same place, and caco will read what's there.",
        );
    }

    fn render_gitea(&mut self, ui: &mut egui::Ui) {
        step(ui, "1. Where it goes");
        field(
            ui,
            "Server",
            &mut self.form.server,
            "https://gitea.example.com",
        );
        ui.columns(2, |cols| {
            field(
                &mut cols[0],
                "Owner",
                &mut self.form.owner,
                "you, or an organization",
            );
            field(
                &mut cols[1],
                "Package",
                &mut self.form.package,
                "doom-mirror",
            );
        });
        note(
            ui,
            "The package appears by itself, so nothing needs setting up on the server. Packages \
             are on by default in Gitea.",
        );

        step(ui, "2. Make a token for caco");
        let server = self.form.server.trim().to_string();
        if ui
            .add_enabled(
                !server.is_empty(),
                egui::Button::new("Open Gitea's Token Page"),
            )
            .clicked()
        {
            let _ = open::that(gitea_token_page(&server));
        }
        note(
            ui,
            "Under Generate New Token, give it a name and set package to Read and Write. Gitea \
             shows the token once, so copy it before leaving the page.",
        );

        step(ui, "3. Paste it here");
        self.render_token_field(ui, "Token");
        self.render_token_notes(ui);
    }

    fn render_webdav(&mut self, ui: &mut egui::Ui) {
        step(ui, "1. The address of a folder on the server");
        field(
            ui,
            "Address",
            &mut self.form.address,
            "https://cloud.example.com/remote.php/dav/files/you/doom-mirror",
        );
        note(
            ui,
            "caco creates the folder if it isn't there. On Nextcloud the address is the one \
             shown above, with your username. A Synology or QNAP NAS can turn WebDAV on from \
             its own settings.",
        );
        note(
            ui,
            "No server? This serves a folder from any machine with rclone installed:",
        );
        copyable(
            ui,
            "rclone serve webdav --addr :8080 --user doom --pass choose-one /path/to/doom-mirror",
        );
        note(ui, "The address is then http://that-machine:8080.");

        step(ui, "2. How to log in");
        ui.columns(2, |cols| {
            field(&mut cols[0], "Username", &mut self.form.username, "");
            self.render_token_field(&mut cols[1], "Password");
        });
        note(
            ui,
            "Leave both blank if the server has no login. On Nextcloud, make an app password \
             under Personal settings, Security, rather than using your own.",
        );
        self.render_token_notes(ui);
    }

    fn render_web_server(&mut self, ui: &mut egui::Ui) {
        note(
            ui,
            "For a mirror something else fills, such as a folder mirror on another machine that \
             a web server also shares. caco can download from it but not upload.",
        );
        step(ui, "1. Serve the mirror's folder");
        note(ui, "Any web server will do. For a quick one:");
        copyable(
            ui,
            "python3 -m http.server --directory /path/to/doom-mirror 8080",
        );

        step(ui, "2. Its address");
        field(
            ui,
            "Address",
            &mut self.form.address,
            "http://that-machine:8080",
        );
        note(
            ui,
            "Of the mirror's folder itself: index/index.json should be right under it.",
        );
    }

    /// The token or password field.
    fn render_token_field(&mut self, ui: &mut egui::Ui, label: &str) {
        let label = form_label(ui, label);
        ui.add(
            egui::TextEdit::singleline(&mut self.form.token)
                .password(true)
                .desired_width(f32::INFINITY)
                .text_color(theme::TEXT_PRIMARY)
                .hint_text(if self.kept_token.is_some() && !self.forget_token {
                    "saved; leave blank to keep it"
                } else {
                    ""
                }),
        )
        .labelled_by(label.id);
    }

    /// Where the token goes, how to avoid that, and how to take a saved one back. Not in
    /// a column with the field, because a long path cannot wrap in one and the text
    /// around it ends up justified a letter at a time.
    fn render_token_notes(&mut self, ui: &mut egui::Ui) {
        if self.kept_token.is_some()
            && !self.forget_token
            && ui.button("Forget the Saved One").clicked()
        {
            self.forget_token = true;
        }
        note(
            ui,
            &format!(
                "Saved in {}, which only you can read. It stays on this machine: your data \
                 directory, which you may copy or sync, never sees it. Another machine needs it \
                 pasted again.",
                self.token_file.display()
            ),
        );
        egui::CollapsingHeader::new(
            egui::RichText::new("Or read it from an environment variable")
                .size(12.0)
                .color(theme::TEXT_SECONDARY),
        )
        .default_open(!self.form.token_env.is_empty())
        .show(ui, |ui| {
            field(
                ui,
                "Variable",
                &mut self.form.token_env,
                "CACO_MIRROR_TOKEN",
            );
            note(
                ui,
                "Nothing is saved then. It has to be set wherever caco is started, which a \
                 desktop launcher may not see from a shell's rc file.",
            );
        });
    }

    fn render_check(&self, ui: &mut egui::Ui, kind: MirrorKind) {
        match &self.check {
            Check::NotRun => {}
            Check::Running => note(ui, "Trying it\u{2026}"),
            Check::SettingUp => note(ui, "Setting it up\u{2026}"),
            Check::Passed(probe) => render_probe(ui, probe, kind),
            Check::Failed(why) => {
                bad(ui, &format!("Could not use it: {why}"));
                if let Some(hint) = failure_hint(kind, why) {
                    note(ui, hint);
                }
            }
            Check::SetUpFailed(why) => {
                bad(ui, &format!("Could not set it up: {why}"));
                if let Some(hint) = failure_hint(kind, why) {
                    note(ui, hint);
                }
            }
        }
    }

    fn poll_picker(&mut self) {
        let Some(picker) = &self.picker else {
            return;
        };
        match picker.try_recv() {
            Ok(picked) => {
                self.picker = None;
                if let Some(path) = picked {
                    self.form.folder = path.display().to_string();
                }
            }
            Err(TryRecvError::Disconnected) => self.picker = None,
            Err(TryRecvError::Empty) => {}
        }
    }
}

/// What saving this mirror does to the token file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenChanges {
    pub renamed_from: Option<String>,
    pub token: Option<String>,
    pub forget: bool,
}

fn render_probe(ui: &mut egui::Ui, probe: &MirrorProbe, kind: MirrorKind) {
    if probe.folder_missing {
        good(
            ui,
            "This folder isn't there yet. caco will create it when you finish.",
        );
    } else {
        match probe.files {
            Some(0) => good(ui, "It answered. Its index lists no files yet."),
            Some(1) => good(ui, "It answered, and holds 1 file."),
            Some(files) => good(ui, &format!("It answered, and holds {files} files.")),
            None if kind.can_write() => {
                good(ui, "It answered. It's empty, so this will be a new mirror.");
            }
            None => bad(
                ui,
                "It answered, but there is no index/index.json there. Is this the address of \
                 the mirror's folder?",
            ),
        }
    }
    match &probe.can_write {
        Some(Ok(())) => good(ui, "caco can write to it."),
        Some(Err(why)) => {
            bad(ui, &format!("caco can't write to it: {why}"));
            if let Some(hint) = failure_hint(kind, why) {
                note(ui, hint);
            }
            note(
                ui,
                "If it was only ever going to be read, turn off \"caco may upload to it\".",
            );
        }
        None => {}
    }
}

/// What to try next, for the failures a person setting one up is likely to meet.
fn failure_hint(kind: MirrorKind, why: &str) -> Option<&'static str> {
    if why.contains("invalid peer certificate") {
        return Some(
            "The server's certificate isn't trusted here. A self-signed one has to be added to \
             this system's trust store, or use http:// on a network you trust.",
        );
    }
    if why.contains("could not connect") || why.contains("timed out") {
        return Some(
            "Check the address, and that the server is up and reachable from this machine.",
        );
    }
    let refused = why.contains("HTTP 401") || why.contains("HTTP 403");
    match kind {
        MirrorKind::Gitea if refused => Some(
            "Check that the whole token was pasted, and that its package permission is Read \
             and Write.",
        ),
        MirrorKind::Gitea if why.contains("HTTP 404") => {
            Some("Check the owner: Gitea answers 404 for a user it doesn't know.")
        }
        MirrorKind::Gitea if why.contains("HTTP 405") => {
            Some("Check the server's address: this doesn't look like its package registry.")
        }
        MirrorKind::Webdav if refused => Some("Check the username and password."),
        MirrorKind::Webdav if why.contains("HTTP 405") || why.contains("refused") => Some(
            "This server may not have WebDAV turned on at that address, or may only allow \
             reading.",
        ),
        MirrorKind::Folder if why.contains("denied") => {
            Some("This user can't write there. Check the folder's permissions.")
        }
        _ => None,
    }
}

fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(label).strong()),
    )
    .clicked()
}

fn step(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.colored_label(
        theme::TEXT_ACCENT,
        egui::RichText::new(text).size(12.0).strong(),
    );
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(12.0)
                .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
}

fn good(ui: &mut egui::Ui, text: &str) {
    ui.colored_label(theme::COLOR_SUCCESS, text);
}

fn bad(ui: &mut egui::Ui, text: &str) {
    ui.colored_label(theme::COLOR_ERROR, text);
}

fn form_label(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.colored_label(
        theme::TEXT_SECONDARY,
        egui::RichText::new(label).size(12.0).strong(),
    )
}

/// A labelled text field the width of its column, named after its label.
fn field(ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str) {
    let label = form_label(ui, label);
    ui.add(
        egui::TextEdit::singleline(text)
            .desired_width(f32::INFINITY)
            .text_color(theme::TEXT_PRIMARY)
            .hint_text(hint),
    )
    .labelled_by(label.id);
}

/// A command to run somewhere else, with a button to copy it.
fn copyable(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_top(|ui| {
        if ui.small_button("Copy").clicked() {
            ui.ctx().copy_text(text.to_string());
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .monospace()
                    .size(11.0)
                    .color(theme::TEXT_PRIMARY),
            )
            .wrap(),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(kind: MirrorKind) -> MirrorSetup {
        let mut setup = MirrorSetup::new(Vec::new());
        setup.pick_kind(kind);
        setup
    }

    #[test]
    fn test_a_folder_becomes_a_path() {
        let mut setup = at(MirrorKind::Folder);
        setup.form.folder = " /mnt/nas/doom ".to_string();
        let config = setup.config(MirrorKind::Folder);
        assert_eq!(config.name, "folder");
        assert_eq!(config.path.as_deref(), Some("/mnt/nas/doom"));
        assert_eq!(config.url, None);
        assert!(config.write);
        assert_eq!(setup.problem(MirrorKind::Folder), None);
    }

    /// The URL the registry wants, built from what a person knows.
    #[test]
    fn test_a_gitea_mirror_builds_its_registry_url() {
        let mut setup = at(MirrorKind::Gitea);
        setup.form.server = " https://gitea.example.com/ ".to_string();
        setup.form.owner = "me".to_string();
        let config = setup.config(MirrorKind::Gitea);
        assert_eq!(
            config.url.as_deref(),
            Some("https://gitea.example.com/api/packages/me/generic/doom-mirror")
        );
        assert_eq!(config.username, None, "Gitea takes a bearer token");
        assert_eq!(setup.problem(MirrorKind::Gitea), None);
    }

    #[test]
    fn test_gitea_urls_read_back_as_what_built_them() {
        let url = gitea_url("https://g.lan:3000/", "me", "doom-mirror").unwrap();
        assert_eq!(
            parse_gitea_url(&url),
            Some((
                "https://g.lan:3000".to_string(),
                "me".to_string(),
                "doom-mirror".to_string()
            ))
        );
        assert_eq!(
            parse_gitea_url("https://cloud/remote.php/dav/files/me/doom"),
            None
        );
        assert_eq!(parse_gitea_url("https://g/api/packages/me/npm/x"), None);
    }

    #[test]
    fn test_gitea_needs_all_three_and_a_name_it_takes() {
        let mut setup = at(MirrorKind::Gitea);
        setup.form.server = "https://g.lan".to_string();
        assert!(
            setup
                .problem(MirrorKind::Gitea)
                .unwrap()
                .contains("the owner")
        );

        setup.form.owner = "me".to_string();
        setup.form.package = "doom mirror".to_string();
        assert!(
            setup
                .problem(MirrorKind::Gitea)
                .unwrap()
                .contains("package name")
        );

        setup.form.package = "doom-mirror_2+x.y".to_string();
        assert_eq!(setup.problem(MirrorKind::Gitea), None);
    }

    #[test]
    fn test_the_token_page_is_on_their_server() {
        assert_eq!(
            gitea_token_page(" https://g.lan:3000/ "),
            "https://g.lan:3000/user/settings/applications"
        );
    }

    #[test]
    fn test_webdav_logs_in_and_a_web_server_cannot_be_written() {
        let mut setup = at(MirrorKind::Webdav);
        setup.form.address = "https://cloud/dav/doom".to_string();
        setup.form.username = "me".to_string();
        setup.form.token = "pw".to_string();
        let config = setup.config(MirrorKind::Webdav);
        assert_eq!(config.username.as_deref(), Some("me"));
        assert_eq!(config.token().as_deref(), Some("pw"));
        assert!(config.write);

        let mut setup = at(MirrorKind::WebServer);
        setup.form.address = "http://nas:8080".to_string();
        let config = setup.config(MirrorKind::WebServer);
        assert!(!config.write, "a plain web server takes no uploads");
    }

    #[test]
    fn test_a_new_mirror_gets_a_name_nothing_else_has() {
        let taken = vec!["gitea".to_string(), "gitea-2".to_string()];
        let mut setup = MirrorSetup::new(taken);
        setup.pick_kind(MirrorKind::Gitea);
        assert_eq!(setup.form.name, "gitea-3");
    }

    #[test]
    fn test_problems_are_said_in_words() {
        let mut setup = MirrorSetup::new(vec!["home".to_string()]);
        setup.pick_kind(MirrorKind::Folder);
        assert_eq!(
            setup.problem(MirrorKind::Folder).unwrap(),
            "Choose a folder."
        );

        setup.form.folder = "relative/doom".to_string();
        assert_eq!(
            setup.problem(MirrorKind::Folder).unwrap(),
            "Relative/doom is not an absolute path."
        );

        setup.form.folder = "/mnt/doom".to_string();
        setup.form.name = "home".to_string();
        assert!(
            setup
                .problem(MirrorKind::Folder)
                .unwrap()
                .contains("called home")
        );
        setup.form.name = "  ".to_string();
        assert_eq!(
            setup.problem(MirrorKind::Folder).unwrap(),
            "Give it a name."
        );

        let mut web = at(MirrorKind::WebServer);
        web.form.address = "nas:8080".to_string();
        assert!(
            web.problem(MirrorKind::WebServer)
                .unwrap()
                .contains("not an http")
        );
    }

    /// Every saved mirror reads back as the kind that made it, and editing it changes
    /// nothing until something is typed.
    #[test]
    fn test_editing_reads_a_mirror_back_as_it_was() {
        let saved = |config: MirrorConfig| MirrorConfig {
            name: "home".to_string(),
            ..config
        };
        let token = SavedToken("sekrit".to_string());
        for (config, kind) in [
            (
                saved(MirrorConfig {
                    path: Some("/mnt/nas".to_string()),
                    write: true,
                    ..Default::default()
                }),
                MirrorKind::Folder,
            ),
            (
                saved(MirrorConfig {
                    url: gitea_url("https://g.lan", "me", "doom"),
                    write: true,
                    saved_token: Some(token.clone()),
                    ..Default::default()
                }),
                MirrorKind::Gitea,
            ),
            (
                saved(MirrorConfig {
                    url: Some("https://cloud/dav/doom".to_string()),
                    username: Some("me".to_string()),
                    write: true,
                    saved_token: Some(token.clone()),
                    token_env: Some("TOKEN".to_string()),
                    ..Default::default()
                }),
                MirrorKind::Webdav,
            ),
            // Read only, and it still logs in, so it is not a plain web server.
            (
                saved(MirrorConfig {
                    url: Some("https://cloud/dav/doom".to_string()),
                    username: Some("me".to_string()),
                    ..Default::default()
                }),
                MirrorKind::Webdav,
            ),
            (
                saved(MirrorConfig {
                    url: Some("http://nas:8080".to_string()),
                    ..Default::default()
                }),
                MirrorKind::WebServer,
            ),
        ] {
            assert_eq!(MirrorKind::of(&config), kind, "{config:?}");
            let setup = MirrorSetup::editing(&config, Vec::new());
            assert_eq!(setup.config(kind), config, "{kind:?}");
            assert_eq!(setup.problem(kind), None, "{kind:?}");
        }
    }

    #[test]
    fn test_a_saved_token_stays_unless_replaced_or_forgotten() {
        let config = MirrorConfig {
            name: "home".to_string(),
            url: gitea_url("https://g.lan", "me", "doom"),
            saved_token: Some(SavedToken("old".to_string())),
            ..Default::default()
        };
        let mut setup = MirrorSetup::editing(&config, Vec::new());
        let token = |setup: &MirrorSetup| setup.config(MirrorKind::Gitea).token();
        assert_eq!(token(&setup).as_deref(), Some("old"));
        assert_eq!(setup.token_changes().token, None);

        setup.form.token = " new ".to_string();
        assert_eq!(token(&setup).as_deref(), Some("new"));
        assert_eq!(setup.token_changes().token.as_deref(), Some("new"));

        setup.form.token.clear();
        setup.forget_token = true;
        assert_eq!(token(&setup), None);
        assert!(setup.token_changes().forget);
    }

    #[test]
    fn test_the_name_of_an_edited_mirror_is_not_taken_by_itself() {
        let config = MirrorConfig {
            name: "home".to_string(),
            path: Some("/mnt/nas".to_string()),
            ..Default::default()
        };
        // `taken` is the other mirrors'.
        let setup = MirrorSetup::editing(&config, vec!["other".to_string()]);
        assert_eq!(setup.problem(MirrorKind::Folder), None);
        assert_eq!(setup.replacing(), Some("home"));
    }

    #[test]
    fn test_going_back_only_goes_somewhere_when_adding() {
        let mut adding = at(MirrorKind::Folder);
        assert!(adding.go_back());
        assert_eq!(adding.kind, None);
        assert!(!adding.go_back(), "nowhere left to go back to");

        let config = MirrorConfig {
            name: "home".to_string(),
            path: Some("/mnt/nas".to_string()),
            ..Default::default()
        };
        assert!(!MirrorSetup::editing(&config, Vec::new()).go_back());
    }

    #[test]
    fn test_failures_say_what_to_try_next() {
        use MirrorKind::{Folder, Gitea, Webdav};
        for (kind, why, says) in [
            (
                Gitea,
                "HTTP 401 Unauthorized: it refused the token or password",
                "package",
            ),
            (Gitea, "HTTP 404 Not Found", "owner"),
            (Gitea, "HTTP 405 Method Not Allowed", "registry"),
            (Webdav, "HTTP 401 Unauthorized", "username"),
            (Webdav, "it refused index/index.json", "WebDAV"),
            (
                Folder,
                "could not write: Permission denied (os error 13)",
                "permissions",
            ),
            (Gitea, "could not connect: tcp connect error", "reachable"),
            (
                Webdav,
                "could not connect: invalid peer certificate: UnknownIssuer",
                "certificate",
            ),
        ] {
            let hint = failure_hint(kind, why).unwrap_or_else(|| panic!("{why}"));
            assert!(hint.contains(says), "{why} -> {hint}");
        }
        assert_eq!(failure_hint(Folder, "something else"), None);
    }
}
