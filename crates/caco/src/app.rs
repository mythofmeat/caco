use std::path::PathBuf;

use rusqlite::Connection;

use crate::dialogs::collections::CollectionsDialogState;
use crate::dialogs::delete::DeleteDialogState;
use crate::dialogs::edit::EditDialogState;
use crate::dialogs::file_changed::FileChangedDialogState;
use crate::dialogs::link::LinkDialogState;
use crate::dialogs::newer_saves::NewerSavesDialogState;
use crate::dialogs::resources::ResourcesDialogState;
use crate::dialogs::sessions::SessionsDialogState;
use crate::dialogs::wad_stats::WadStatsDialogState;
use crate::get::{GetOutcome, GetTarget, PageOpener};
use crate::import;
use crate::import::state::{FormKind, ImportOrigin, SearchSourceData};
use crate::message::{AppMessage, EnrichReport, NoticeAction, Notification};
use crate::panels;
use crate::persist;
use crate::state::{ActionRequest, ActiveDialog, AppState, PlayState, ViewLayout, ViewMode};
use crate::theme;
use crate::thumbnails::{ThumbnailHint, ThumbnailManager};
use crate::workers::{AnalysisJob, BackgroundChannel, spawn_reanalysis};

mod dialogs;
mod file_check;
mod get;
mod help;
mod hero;
mod idgames_index;
mod mirrors;
mod section_header;
mod sidebar;
mod status_bar;
mod sync;
mod topbar;

use dialogs::render_active_dialog;
use hero::render_now_playing_hero;
use section_header::render_section_header;
use sidebar::render_sidebar;
use status_bar::render_status_bar;
use topbar::render_topbar;

/// One line describing what enrichment found for a WAD.
fn describe_outcome(outcome: &caco_sources::enrich_service::EnrichOutcome) -> String {
    let mut parts = Vec::new();
    if let Some(cl) = outcome.complevel {
        parts.push(format!(
            "complevel {cl} ({})",
            caco_core::complevel::complevel_name(Some(cl))
        ));
    }
    if let Some(ref iwad) = outcome.iwad {
        parts.push(format!("IWAD {iwad}"));
    }
    if outcome.zdoom_required == Some(true) {
        parts.push("needs a ZDoom-family sourceport".to_string());
    }
    format!("{} → {}", outcome.title, parts.join(", "))
}

/// Which key goes to which view, with Ctrl or Cmd held, in the order the sidebar has them.
const VIEW_KEYS: [(egui::Key, ViewMode); 4] = [
    (egui::Key::Num1, ViewMode::Library),
    (egui::Key::Num2, ViewMode::Import),
    (egui::Key::Num3, ViewMode::Cacowards),
    (egui::Key::Num4, ViewMode::Stats),
];

/// Say what will happen to a file being dragged over the window. Nothing to go on otherwise, and
/// nothing to say with a dialog open, since a file dropped then goes nowhere.
fn render_drop_hint(ctx: &egui::Context, dialog_open: bool) {
    if dialog_open || ctx.input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("drop-hint"),
    ));
    painter.rect_filled(screen, 0.0, theme::SCRIM);
    painter.text(
        screen.center(),
        egui::Align2::CENTER_CENTER,
        "Drop to import",
        egui::FontId::proportional(20.0),
        theme::TEXT_PRIMARY,
    );
}

/// Whether `text` was typed this frame with nothing there to type it into, and if so take it,
/// or it would land in the field it is about to put the keyboard in. Goes by what was typed
/// rather than which key, since `?` and `/` are different keys on different keyboards.
fn typed_alone(ctx: &egui::Context, text: &str) -> bool {
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    ctx.input_mut(|i| {
        let typed = i
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Text(t) if t == text));
        if typed {
            i.events
                .retain(|event| !matches!(event, egui::Event::Text(t) if t == text));
        }
        typed
    })
}

pub struct CacoApp {
    conn: Connection,
    state: AppState,
    bg: BackgroundChannel,
    thumbnails: ThumbnailManager,
    sync: sync::SyncWorker,
}

impl CacoApp {
    pub fn new(conn: Connection, db_path: PathBuf, ctx: &egui::Context) -> Self {
        let mut bg = BackgroundChannel::new();
        bg.set_ctx(ctx.clone());
        let (sync, sync_inbox) = sync::worker();

        let mut app = Self {
            conn,
            state: AppState::new(db_path),
            bg,
            thumbnails: ThumbnailManager::new(),
            sync,
        };
        app.sync_cacowards();
        sync::spawn(sync_inbox, app.bg.sender(), app.state.db_path.clone());
        file_check::spawn(app.bg.sender(), app.state.db_path.clone());
        mirrors::spawn_sync(app.bg.sender(), app.state.db_path.clone());
        app.spawn_idgames_index(false);
        app.spawn_sourceport_update_check();
        app
    }

    /// Bring the local index of idgames up to date, on a worker. One at a time, since two
    /// would only be asking the same mirror for the same files.
    fn spawn_idgames_index(&mut self, asked_for: bool) {
        if self.state.import.index.is_running() {
            return;
        }
        let cancel = self.state.import.index.started();
        idgames_index::spawn(
            self.bg.sender(),
            self.state.db_path.clone(),
            asked_for,
            cancel,
        );
    }

    /// Bring the Cacowards table up to date with the dataset compiled into this
    /// binary and any user files beside it.
    ///
    /// Here rather than on a worker, because the Cacowards view must not open empty
    /// on a new library, and there is nothing here to wait for: a launch with
    /// nothing new reads a few files and a table without taking the write lock.
    ///
    /// A user file that does not parse holds the update back until it is fixed,
    /// since applying the rest would remove that file's rows.
    fn sync_cacowards(&mut self) {
        let warn = |e: &dyn std::fmt::Display| {
            Notification::warning(format!("Cacowards not updated: {e}"))
        };
        match caco_core::cacowards::load(&caco_core::config::cacoward_data_dir()) {
            Ok(dataset) => {
                if let Err(e) = caco_sources::cacoward_sync::sync_cacowards(&self.conn, &dataset) {
                    self.state.notification = Some(warn(&e));
                }
                self.state.cacowards.pages = dataset.pages;
            }
            Err(e) => {
                self.state.notification = Some(warn(&e));
                self.state.cacowards.pages = caco_core::cacowards::builtin().pages;
            }
        }
    }

    /// The handle background workers answer on.
    ///
    /// Public for `tests/screenshots.rs`, which has to reach states only a
    /// worker can produce — a build in flight, with a step and a log — and
    /// posting the messages a worker would post is the honest way to get there.
    /// Reaching into the dialog's private state instead would prove a shot
    /// renders and nothing about whether the app can get there.
    pub fn background_sender(&self) -> crate::workers::BackgroundSender {
        self.bg.sender()
    }

    /// Ask each built sourceport's remote whether its ref has moved.
    ///
    /// Fire-and-forget on a worker thread: one `git ls-remote` per installed
    /// sourceport, throttled to `sourceport_update_check_days` and answered from cache in
    /// between, so most launches do no network at all. A user with nothing
    /// built never sees a thread do any work, and an offline machine gets
    /// silence rather than a stall — the window is already up either way.
    fn spawn_sourceport_update_check(&self) {
        use caco_core::sourceports::{self, SourceportPaths};

        let interval = caco_core::config::load_config().sourceport_update_check_days;
        if interval <= 0 {
            return;
        }
        let sender = self.bg.sender();
        std::thread::spawn(move || {
            let paths = SourceportPaths::from_config();
            let behind: Vec<String> =
                sourceports::check_updates(&paths, interval, chrono::Local::now())
                    .into_iter()
                    .filter(sourceports::UpdateStatus::is_behind)
                    .map(|s| s.name)
                    .collect();
            if !behind.is_empty() {
                sender.send(AppMessage::SourceportUpdatesAvailable(behind));
            }
        });
    }

    /// Show a message as soon as the first frame renders.
    ///
    /// Startup work happens before the app exists, so anything it wants to
    /// tell the user has to be handed in rather than pushed.
    pub fn notify(&mut self, text: String) {
        self.state.notification = Some(Notification::info(text));
    }

    /// Dispatch an import action (from import view).
    fn dispatch_import_action(&mut self, action: import::ImportAction) {
        let sender = self.bg.sender();
        let db_path = self.state.db_path.clone();

        match action {
            import::ImportAction::Search(source, query) => {
                import::workers::spawn_search(sender, db_path, source, query);
            }
            import::ImportAction::Newest => {
                import::workers::spawn_newest(sender, db_path);
            }
            import::ImportAction::ShowWad(wad_id) => self.state.show_wad(wad_id),
            import::ImportAction::UpdateIndex => {
                self.spawn_idgames_index(true);
            }
            import::ImportAction::ImportSearchResult(source, entry) => {
                self.state.import.search_state_mut(source).importing = Some(entry.key());
                match entry.source_data {
                    SearchSourceData::Idgames { target, .. } => {
                        import::workers::spawn_import_idgames(sender, db_path, target);
                    }
                    SearchSourceData::Doomwiki { .. } => {
                        import::workers::spawn_import_doomwiki(sender, db_path, entry.title);
                    }
                }
            }
            import::ImportAction::ImportForm(kind, values) => {
                import::workers::spawn_import_form(sender, db_path, kind, values);
            }
        }
    }

    /// Spawn a background re-analysis pass for any visible WAD with a cached
    /// file but no fresh analysis.
    ///
    /// Bumping `ANALYSIS_VERSION` (or importing pre-analyzed WADs from a
    /// snapshot of the library) leaves rows that are silently filtered out
    /// of `state.analyses_map`. Without this pass the hero/grid/dialog would
    /// stay blank for those WADs until the user replayed them. The worker
    /// opens its own DB connection and posts results back via
    /// `AppMessage::AnalysesRefreshed`.
    fn kick_reanalysis(&self) {
        let jobs: Vec<AnalysisJob> = self
            .state
            .wads
            .iter()
            .filter(|w| !self.state.analyses_map.contains_key(&w.id))
            // No `exists()` here: this runs on the UI thread at every reload,
            // and a stat on a dead mount would freeze it. `cached_path` is
            // checked at startup and before each launch, and the worker skips
            // a file it cannot read.
            .filter_map(|w| {
                Some(AnalysisJob {
                    wad_id: w.id,
                    wad_path: PathBuf::from(w.cached_path.as_deref().filter(|p| !p.is_empty())?),
                })
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        spawn_reanalysis(self.bg.sender(), self.state.db_path.clone(), jobs);
    }

    /// Build a sourceport on a worker thread, streaming its output back to
    /// the dialog a line at a time.
    ///
    /// A compile is minutes long (uzdoom measured at 3m45s on 16 threads) and
    /// runs several child processes, so nothing about it can happen on the UI
    /// thread. Unlike enrichment this needs no database connection — sourceports
    /// live entirely on the filesystem.
    fn spawn_sourceport_build(
        &mut self,
        request: crate::dialogs::sourceports::SourceportBuildRequest,
    ) {
        use caco_core::sourceports::{self, BuildOptions, BuildProgress, SourceportPaths};
        use std::sync::atomic::Ordering;

        let sender = self.bg.sender();
        let crate::dialogs::sourceports::SourceportBuildRequest {
            sourceport,
            git_ref,
            clean,
            cancel,
        } = request;

        std::thread::spawn(move || {
            let paths = SourceportPaths::from_config();
            let mut recipe = match sourceports::find_recipe(&paths.recipe_dir, &sourceport) {
                Ok(r) => r,
                Err(e) => {
                    sender.send(AppMessage::SourceportBuildComplete(Err(e.to_string())));
                    return;
                }
            };
            recipe.git_ref = git_ref;

            let opts = BuildOptions { jobs: None, clean };
            let outcome = sourceports::build::install(
                &recipe,
                &paths,
                &opts,
                &mut |progress| match progress {
                    BuildProgress::Step(step) => {
                        sender.send(AppMessage::SourceportBuildStep(step.label().to_string()));
                    }
                    BuildProgress::Line(line) => {
                        sender.send(AppMessage::SourceportBuildLine(line.to_string()));
                    }
                },
                &|| cancel.load(Ordering::Relaxed),
            );

            let message = outcome
                .map(|installed| format!("Built {}.", installed.manifest.name))
                .map_err(|e| e.to_string());
            sender.send(AppMessage::SourceportBuildComplete(message));
        });
    }

    /// Discover selectable releases and branches without blocking the UI.
    fn spawn_sourceport_version_check(
        &self,
        request: crate::dialogs::sourceports::SourceportVersionsRequest,
    ) {
        let sender = self.bg.sender();
        std::thread::spawn(move || {
            let outcome =
                caco_core::sourceports::remote_refs(&request.repo).map_err(|e| e.to_string());
            sender.send(AppMessage::SourceportVersionsLoaded {
                sourceport: request.sourceport,
                outcome,
            });
        });
    }

    /// Launch a WAD on a worker thread: check its file is here, download it
    /// when it is not and caco can, then play it.
    ///
    /// The worker answers with what the UI does next. A file caco cannot
    /// fetch opens the link dialog, one that has changed since the WAD was
    /// last played asks before launching, and anything else is a toast.
    fn spawn_play(&mut self, wad_id: i64, opts: caco_core::player::PlayOptions) {
        if let PlayState::Playing { wad_title, .. } = &self.state.play_state {
            self.state.notification = Some(Notification::warning(format!(
                "{wad_title} is still running. Close it first."
            )));
            return;
        }

        // Get title for status bar display
        let title = self
            .state
            .wads
            .iter()
            .find(|w| w.id == wad_id)
            .map(|w| w.title.clone())
            .unwrap_or_else(|| format!("WAD #{wad_id}"));

        self.state.play_state = PlayState::Playing {
            wad_id,
            wad_title: title,
            downloading: false,
        };

        // Spawn play worker in a background thread
        let sender = self.bg.sender();
        let db_path = self.state.db_path.clone();
        std::thread::spawn(move || {
            // Local error type that distinguishes the outcomes the UI acts on
            // from errors that only surface as a toast. String errors from `?`
            // default to Message; Unavailable is chosen explicitly where the
            // file is not on this machine and caco cannot fetch it, and
            // Changed is the launch's own refusal to play a changed file
            // without asking.
            enum PlayError {
                Message(String),
                /// Says why when caco tried to fetch it and could not.
                Unavailable(Option<String>),
                Changed {
                    title: String,
                    change: caco_core::identity::ContentChange,
                },
                NewerSaves {
                    title: String,
                    saves: caco_core::sync::NewerSaves,
                },
                NoSourceport,
            }
            impl From<String> for PlayError {
                fn from(s: String) -> Self {
                    PlayError::Message(s)
                }
            }

            let outcome = (|| -> Result<caco_core::player::PlayResult, PlayError> {
                let conn = caco_core::db::open_connection(&db_path)
                    .map_err(|e| format!("DB open failed: {e}"))?;

                let wad = caco_core::db::get_wad(&conn, wad_id, false)
                    .map_err(|e| format!("DB error: {e}"))?
                    .ok_or_else(|| format!("WAD #{wad_id} not found"))?;

                // The startup check, for this one WAD: a file that has
                // gone since is recorded as last seen there, and one
                // that has come back is relinked.
                let paths = caco_core::presence::StorePaths::from_config();
                let presence = caco_core::presence::check_wad(&conn, &wad, &paths)
                    .map_err(|e| format!("Checking the WAD file failed: {e}"))?;
                if !presence.is_here() {
                    // Nothing to fetch it from: say so now, rather than
                    // announce a download that cannot start.
                    if !wad.retrievability().is_automatic() {
                        return Err(PlayError::Unavailable(None));
                    }
                    sender.send(AppMessage::PlayDownloading(true));
                    let config = caco_core::config::load_config();
                    caco_sources::wad_fetch::fetch_missing_file(
                        &conn,
                        &wad,
                        &config.mirrors,
                        &paths,
                        config.download_mirror,
                    )
                    .map_err(|e| PlayError::Unavailable(Some(e.to_string())))?;
                    sender.send(AppMessage::PlayDownloading(false));
                }

                caco_core::player::play(&conn, wad_id, opts).map_err(|e| match e {
                    // Gone between the check above and the launch.
                    caco_core::Error::WadFileMissing { .. } => PlayError::Unavailable(None),
                    caco_core::Error::WadFileChanged { title, change } => {
                        PlayError::Changed { title, change }
                    }
                    caco_core::Error::NewerSavesElsewhere { title, saves } => {
                        PlayError::NewerSaves { title, saves }
                    }
                    caco_core::Error::NoSourceport => PlayError::NoSourceport,
                    e => PlayError::Message(e.to_string()),
                })
            })();

            match outcome {
                Ok(pr) => sender.send(AppMessage::PlayFinished { outcome: Ok(pr) }),
                Err(PlayError::Message(msg)) => {
                    sender.send(AppMessage::PlayFinished { outcome: Err(msg) })
                }
                Err(PlayError::Unavailable(reason)) => {
                    sender.send(AppMessage::PlayUnavailable { wad_id, reason })
                }
                Err(PlayError::NoSourceport) => sender.send(AppMessage::PlayNeedsSetup {
                    text: "Nothing to play it with: caco couldn't find a sourceport on this \
                           machine."
                        .to_string(),
                    fix: NoticeAction::OpenSettings,
                }),
                Err(PlayError::Changed { title, change }) => {
                    sender.send(AppMessage::PlayFileChanged {
                        wad_id,
                        title,
                        change,
                        opts,
                    })
                }
                Err(PlayError::NewerSaves { title, saves }) => {
                    sender.send(AppMessage::PlayNewerSaves {
                        wad_id,
                        title,
                        saves,
                        opts,
                    })
                }
            }
        });
    }

    /// Open the best page for what a Get is after, and watch for what comes of it.
    ///
    /// Reads the row or entry again here, because a card or menu can be showing what it
    /// read a while ago. With nothing to send the user to there is nothing to wait for.
    fn start_get(&mut self, target: GetTarget) {
        use caco_core::downloads::{pages_for_cacoward, pages_for_wad};

        let found = match target {
            GetTarget::Wad(id) => caco_core::db::get_wad(&self.conn, id, false)
                .ok()
                .flatten()
                .map(|wad| {
                    let pages = pages_for_wad(&wad);
                    let known_name = wad
                        .last_known_path
                        .as_deref()
                        .and_then(|path| std::path::Path::new(path).file_name())
                        .map(|name| name.to_string_lossy().into_owned())
                        .or(wad.filename);
                    (wad.title, known_name, pages)
                }),
            GetTarget::Cacoward(pk) => caco_core::db::get_cacoward(&self.conn, pk)
                .ok()
                .flatten()
                .map(|record| {
                    let year_page = self.state.cacowards.pages.get(&record.year);
                    let pages = pages_for_cacoward(&record, year_page.map(String::as_str));
                    (record.wad_title, None, pages)
                }),
        };
        let Some((title, known_name, pages)) = found else {
            return;
        };
        let Some(page) = pages.first().cloned() else {
            self.state.notification = Some(Notification::warning(format!(
                "caco knows no page for {title}. If you have the file, link it from Play."
            )));
            return;
        };

        self.state.gets.arm(
            &self.bg.sender(),
            target,
            title,
            known_name,
            pages,
            std::time::Instant::now(),
        );
        self.state.notification = Some(get::opened_notification(&page));
        self.state.gets.open_page(page);
    }

    /// Run an enrichment on a worker thread, streaming progress back to the
    /// dialog. Enrichment does one Doom Wiki lookup per WAD it cannot settle
    /// from the file, so a whole-library run is minutes of network I/O and
    /// must never block the UI thread.
    fn spawn_enrich(&mut self, request: crate::dialogs::enrich::EnrichRequest) {
        use crate::dialogs::enrich::{EnrichRequest, EnrichScope};
        use caco_sources::enrich_service::{self, EnrichProgress};

        let sender = self.bg.sender();
        let db_path = self.state.db_path.clone();
        let EnrichRequest {
            scope,
            dry_run,
            cancel,
        } = request;

        std::thread::spawn(move || {
            let outcome = (|| -> Result<EnrichReport, String> {
                let conn = caco_core::db::open_connection(&db_path)
                    .map_err(|e| format!("DB open failed: {e}"))?;
                let mut wads = caco_core::db::search_wads(&conn, None, None, true, false, 0)
                    .map_err(|e| format!("Search error: {e}"))?;
                if scope == EnrichScope::MissingComplevel {
                    wads.retain(|w| w.complevel.is_none());
                }

                let summary = enrich_service::enrich_wads(
                    &conn,
                    &wads,
                    dry_run,
                    &mut |EnrichProgress {
                              index,
                              total,
                              title,
                          }| {
                        sender.send(AppMessage::EnrichProgress {
                            done: index,
                            total,
                            title: title.to_string(),
                        });
                    },
                    &|| cancel.load(std::sync::atomic::Ordering::Relaxed),
                );

                Ok(EnrichReport {
                    examined: summary.examined,
                    findings: summary.changed.iter().map(describe_outcome).collect(),
                    wiki_lookups: summary.wiki_lookups,
                    wiki_blocked: summary.wiki_blocked.as_ref().map(ToString::to_string),
                    cancelled: summary.cancelled,
                    dry_run,
                })
            })();

            sender.send(AppMessage::EnrichComplete(outcome));
        });
    }

    /// Switch the central panel to `mode`, asking that view to refresh.
    ///
    /// Public for `tests/screenshots.rs`, which walks every view the way the
    /// sidebar does rather than poking `view_mode` and getting a stale page.
    pub fn set_view(&mut self, mode: ViewMode) {
        self.state.show_view(mode);
    }

    /// Hand pages to this instead of a browser. Public for `tests/screenshots.rs`,
    /// which presses Get for real, on a machine where nobody asked for a browser to open.
    pub fn open_pages_with(&mut self, opener: PageOpener) {
        self.state.gets.open_pages_with(opener);
    }

    /// Dismiss whatever dialog is open. Public for `tests/screenshots.rs`,
    /// which walks every dialog in turn.
    pub fn close_dialog(&mut self) {
        self.state.active_dialog = None;
    }

    /// How many uploads to a mirror are still running. Public for
    /// `tests/screenshots.rs`, which waits for one it started.
    pub fn uploads_running(&self) -> usize {
        self.state.uploads.len()
    }

    /// Whether a bulk upload to a mirror is still running. Public for the same reason.
    pub fn bulk_upload_running(&self) -> bool {
        self.state.mirror_bulk.is_some()
    }

    /// Whether the worker keeping the idgames index up to date is running. Public for the
    /// same reason.
    pub fn idgames_index_running(&self) -> bool {
        self.state.import.index.is_running()
    }

    /// Whether a landed file is being linked right now. Public for the same reason.
    pub fn get_linking(&self) -> bool {
        !self.state.gets.linking().is_empty()
    }

    /// How many thumbnails are still on their way. Public for `tests/snapshots.rs`, which
    /// must not compare a picture of a card whose thumbnail is about to arrive.
    pub fn thumbnails_pending(&self) -> usize {
        self.thumbnails.pending_count()
    }

    /// The WAD selected in the library. Public for `tests/interaction.rs`, which presses keys
    /// and has to see where the selection went.
    pub fn selected_wad_title(&self) -> Option<&str> {
        let index = self.state.selected_index()?;
        Some(&self.state.wads[index].title)
    }

    /// Clear the status bar. Public for `tests/snapshots.rs`: a notification goes away by
    /// the clock, and a picture must not depend on how long a run took to get there.
    pub fn dismiss_notification(&mut self) {
        self.state.notification = None;
    }

    /// A file dropped on the window goes to Import, ready to go, unless a dialog is open, which
    /// is the one thing that has to be dealt with first.
    fn take_dropped_file(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let Some(first) = dropped.first() else {
            return;
        };
        if self.state.has_dialog() {
            return;
        }
        self.state.import.local_form.set_path(first.path());
        self.state.import.show_form(FormKind::Local);
        self.state.show_view(ViewMode::Import);
        if dropped.len() > 1 {
            self.state.notification = Some(Notification::warning(
                "One file at a time: this is the first of them.".to_string(),
            ));
        }
    }

    /// An import has finished, wherever it was started from.
    fn import_finished(
        &mut self,
        origin: ImportOrigin,
        result: Result<caco_sources::import_service::ImportResult, String>,
    ) {
        let import = &mut self.state.import;
        match origin {
            ImportOrigin::Form(kind) => {
                let form = import.form_mut(kind);
                form.is_submitting = false;
                if result.as_ref().is_ok_and(|ir| !ir.is_duplicate) {
                    form.reset();
                }
            }
            ImportOrigin::Search(source) => {
                let wad = result
                    .as_ref()
                    .ok()
                    .and_then(|ir| ir.wad_id.or(ir.duplicate_id));
                import.search_state_mut(source).import_finished(wad);
            }
            ImportOrigin::Cacoward => {}
        }

        let ir = match result {
            Ok(ir) => ir,
            Err(e) => {
                self.state.notification = Some(Notification::error(format!("Import failed: {e}")));
                return;
            }
        };
        // A duplicate Cacoward import can still link the existing WAD to the award row, so
        // every successful result may have changed a cached projection.
        self.state.invalidate_wad_views();
        if let Some(wad_id) = ir.wad_id {
            self.state.mirror_offers.push_back(wad_id);
        }
        let (text, wad) = if ir.is_duplicate {
            // Nothing went wrong: it is there, which is what was wanted.
            let title = ir.duplicate_title.unwrap_or_else(|| "unknown".to_string());
            (
                format!("Already in your library, as {title}."),
                ir.duplicate_id,
            )
        } else {
            let title = ir
                .wad_id
                .and_then(|id| caco_core::db::get_wad(&self.conn, id, false).ok().flatten())
                .map_or_else(|| "it".to_string(), |wad| wad.title);
            (format!("Imported {title}."), ir.wad_id)
        };
        let notice = Notification::info(text);
        self.state.notification = Some(match wad {
            Some(id) => notice.with_action(NoticeAction::ShowWad(id)),
            None => notice,
        });
    }

    /// Write something about a WAD that was changed in passing, and show it everywhere.
    fn update_wad(&mut self, wad_id: i64, update: &caco_core::db::WadUpdate) {
        match caco_core::db::update_wad(&self.conn, wad_id, update) {
            Ok(_) => self.state.invalidate_wad_views(),
            Err(e) => {
                self.state.notification =
                    Some(Notification::error(format!("Could not save that: {e}")));
            }
        }
    }

    /// Put the keyboard in the box that searches what is on the screen. In the library that is
    /// the filter. Import has one of its own per source that searches, and the rest of its
    /// sources and views have nothing to search, so there this goes to the library's.
    fn focus_search(&mut self, ctx: &egui::Context) {
        let field = match self.state.view_mode {
            ViewMode::Import if self.state.import.active_search().is_some() => {
                import::search_panel::SEARCH_ID_SOURCE
            }
            ViewMode::Library => panels::filter_bar::FILTER_ID_SOURCE,
            _ => {
                self.state.show_view(ViewMode::Library);
                panels::filter_bar::FILTER_ID_SOURCE
            }
        };
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique(field)));
    }

    /// Dispatch an action request (from detail panel buttons or table shortcuts).
    ///
    /// Public so `tests/screenshots.rs` can open any dialog the same way a
    /// click does, instead of reconstructing dialog state by hand.
    pub fn dispatch_action(&mut self, action: ActionRequest) {
        match action {
            ActionRequest::SetStatus(wad_id, status) => {
                let update = caco_core::db::WadUpdate::new().set_status(status);
                self.update_wad(wad_id, &update);
            }
            ActionRequest::SetRating(wad_id, rating) => {
                let update =
                    caco_core::db::WadUpdate::new().set_int("rating", rating.map(i64::from));
                self.update_wad(wad_id, &update);
            }
            ActionRequest::ShowDetails(wad_id) => {
                self.state.select_wad(wad_id);
                if self.state.wide_window {
                    self.state.details_open = true;
                } else {
                    self.state.active_dialog = Some(ActiveDialog::Details(wad_id));
                }
            }
            ActionRequest::ToggleDetails => {
                if self.state.wide_window {
                    self.state.details_open = !self.state.details_open;
                } else if let Some(wad_id) = self.state.selected_wad_id {
                    self.state.active_dialog = Some(ActiveDialog::Details(wad_id));
                }
            }
            ActionRequest::FilterLibrary(query) => {
                self.state.filter_library(query);
            }
            ActionRequest::ShowCacowards(year) => {
                self.state.show_view(ViewMode::Cacowards);
                self.state.cacowards.selected_year = Some(year);
            }
            ActionRequest::UndoDelete(wad_id) => {
                match caco_core::db::wads::restore_wad(&self.conn, wad_id) {
                    Ok(_) => {
                        self.state.invalidate_wad_views();
                        self.state.select_wad(wad_id);
                        self.state.reveal_selection = true;
                    }
                    Err(e) => {
                        self.state.notification =
                            Some(Notification::error(format!("Could not put it back: {e}")));
                    }
                }
            }
            ActionRequest::ShowWad(wad_id) => self.state.show_wad(wad_id),
            ActionRequest::SaveFilterAsCollection => {
                let dialog = CollectionsDialogState::new_adding(
                    &self.conn,
                    &self.state.filter.applied,
                    crate::state::SORT_FIELDS[self.state.sort_field_index].0,
                    self.state.sort_desc,
                );
                self.state.active_dialog = Some(ActiveDialog::Collections(dialog));
            }
            ActionRequest::Edit(wad_id) => {
                if let Some(dialog) = EditDialogState::new(&self.conn, wad_id) {
                    self.state.active_dialog = Some(ActiveDialog::Edit(Box::new(dialog)));
                }
            }
            ActionRequest::Delete(wad_id) => {
                if let Some(dialog) = DeleteDialogState::new(&self.conn, wad_id) {
                    self.state.active_dialog = Some(ActiveDialog::Delete(dialog));
                }
            }
            ActionRequest::Sessions(wad_id) => {
                if let Some(dialog) = SessionsDialogState::new(&self.conn, wad_id) {
                    self.state.active_dialog = Some(ActiveDialog::Sessions(dialog));
                }
            }
            ActionRequest::Storage(tab) => {
                let dialog = crate::dialogs::storage::StorageDialogState::new(tab);
                self.state.active_dialog = Some(ActiveDialog::Storage(Box::new(dialog)));
            }
            ActionRequest::Profiles => {
                let dialog = crate::dialogs::profiles::ProfilesDialogState::new();
                self.state.active_dialog = Some(ActiveDialog::Profiles(Box::new(dialog)));
            }
            ActionRequest::Settings => {
                let dialog = crate::dialogs::settings::SettingsDialogState::new();
                self.state.active_dialog = Some(ActiveDialog::Settings(dialog));
            }
            ActionRequest::Help => {
                self.state.active_dialog = Some(ActiveDialog::Help);
            }
            ActionRequest::Resources => {
                let dialog = ResourcesDialogState::new(&self.conn);
                self.state.active_dialog = Some(ActiveDialog::Resources(dialog));
            }
            ActionRequest::Collections => {
                let dialog = CollectionsDialogState::new(&self.conn);
                self.state.active_dialog = Some(ActiveDialog::Collections(dialog));
            }
            ActionRequest::Enrich => {
                let dialog = crate::dialogs::enrich::EnrichDialogState::new();
                self.state.active_dialog = Some(ActiveDialog::Enrich(Box::new(dialog)));
            }
            ActionRequest::StartEnrich(request) => {
                self.spawn_enrich(*request);
            }
            ActionRequest::Sourceports => {
                // It says which of them have an update, so nothing else need.
                self.state.sourceport_updates.clear();
                let dialog = crate::dialogs::sourceports::SourceportsDialogState::new();
                self.state.active_dialog = Some(ActiveDialog::Sourceports(Box::new(dialog)));
            }
            ActionRequest::StartSourceportBuild(request) => {
                self.spawn_sourceport_build(*request);
            }
            ActionRequest::CheckSourceportVersions(request) => {
                self.spawn_sourceport_version_check(request);
            }
            ActionRequest::EditCollection(name) => {
                let dialog = CollectionsDialogState::new_editing(&self.conn, &name);
                self.state.active_dialog = Some(ActiveDialog::Collections(dialog));
            }
            ActionRequest::DeleteCollection(name) => {
                if let Ok(true) = caco_core::db::collections::delete_collection(&self.conn, &name) {
                    self.state.refresh_collections(&self.conn);
                    self.state.needs_reload = true;
                    self.state.notification =
                        Some(Notification::info(format!("Deleted collection '{name}'")));
                }
            }
            ActionRequest::MapStats(wad_id) => {
                if let Some(dialog) = WadStatsDialogState::new(&self.conn, wad_id) {
                    self.state.active_dialog = Some(ActiveDialog::WadStats(dialog));
                }
            }
            ActionRequest::WadData(wad_id) => {
                if let Some(dialog) =
                    crate::dialogs::wad_data::WadDataDialogState::new(&self.conn, wad_id)
                {
                    self.state.active_dialog = Some(ActiveDialog::WadData(Box::new(dialog)));
                }
            }
            ActionRequest::PlayDemo(wad_id, demo) => {
                // Playback blocks until the sourceport exits, so it runs on a
                // worker thread. No session is recorded, so unlike Play there
                // is no PlayState to enter — the dialog stays usable.
                let sender = self.bg.sender();
                let db_path = self.state.db_path.clone();
                self.state.notification = Some(Notification::info(format!("Playing demo {demo}…")));
                std::thread::spawn(move || {
                    let outcome = (|| -> Result<(), String> {
                        let conn = caco_core::db::open_connection(&db_path)
                            .map_err(|e| format!("DB open failed: {e}"))?;
                        caco_core::player::play_demo(&conn, wad_id, Some(&demo))
                            .map_err(|e| e.to_string())?;
                        Ok(())
                    })();
                    if let Err(msg) = outcome {
                        sender.send(AppMessage::Notify(Notification::error(format!(
                            "Demo playback failed: {msg}"
                        ))));
                    }
                });
            }
            ActionRequest::ImportCacoward(pk) => {
                import::workers::spawn_import_cacoward(
                    self.bg.sender(),
                    self.state.db_path.clone(),
                    pk,
                );
                self.state.notification =
                    Some(Notification::info("Importing Cacoward entry…".to_string()));
            }
            ActionRequest::LinkCacoward(pk) => {
                if let Some(dialog) =
                    crate::dialogs::cacoward_link::CacowardLinkDialogState::new(&self.conn, pk)
                {
                    self.state.active_dialog = Some(ActiveDialog::CacowardLink(dialog));
                }
            }
            ActionRequest::UnlinkCacoward(pk) => {
                match caco_core::db::cacowards::unlink_wad(&self.conn, pk) {
                    Ok(_) => {
                        self.state.cacowards.needs_reload = true;
                        self.state.notification =
                            Some(Notification::info("Cacoward link cleared".to_string()));
                    }
                    Err(e) => {
                        self.state.notification =
                            Some(Notification::error(format!("Unlink failed: {e}")));
                    }
                }
            }
            ActionRequest::SetCacowardSupported(pk, supported) => {
                match caco_core::db::cacowards::set_supported(&self.conn, pk, supported) {
                    Ok(_) => {
                        self.state.cacowards.needs_reload = true;
                        let msg = if supported {
                            "Cacoward marked supported"
                        } else {
                            "Cacoward marked unsupported"
                        };
                        self.state.notification = Some(Notification::info(msg.to_string()));
                    }
                    Err(e) => {
                        self.state.notification =
                            Some(Notification::error(format!("Set supported failed: {e}")));
                    }
                }
            }
            ActionRequest::StartNewPlaythrough(wad_id) => {
                match caco_core::player::start_new_playthrough(&self.conn, wad_id) {
                    Ok(_) => {
                        self.state.invalidate_wad_views();
                        self.state.notification = Some(Notification::info(
                            "New playthrough started — stats reset".to_string(),
                        ));
                    }
                    Err(e) => {
                        self.state.notification = Some(Notification::error(format!(
                            "Failed to start playthrough: {e}"
                        )));
                    }
                }
            }
            ActionRequest::Play(wad_id) => {
                self.spawn_play(wad_id, caco_core::player::PlayOptions::default());
            }
            ActionRequest::PlayWith(wad_id, opts) => {
                self.spawn_play(wad_id, opts);
            }
            ActionRequest::Sync => {
                self.sync.nudge();
                let dialog =
                    crate::dialogs::sync::SyncDialogState::new(&self.conn, &self.state.db_path);
                self.state.active_dialog = Some(ActiveDialog::Sync(Box::new(dialog)));
            }
            ActionRequest::SyncNow => self.sync.asked(),
            ActionRequest::Mirrors => {
                let dialog = crate::dialogs::mirrors::MirrorsDialogState::new(&self.conn);
                self.state.active_dialog = Some(ActiveDialog::Mirrors(Box::new(dialog)));
            }
            ActionRequest::SyncMirrors => {
                mirrors::spawn_sync(self.bg.sender(), self.state.db_path.clone());
            }
            ActionRequest::ProbeMirror(config) => {
                mirrors::spawn_probe(self.bg.sender(), *config);
            }
            ActionRequest::SetUpMirror(config) => {
                mirrors::spawn_set_up(self.bg.sender(), *config);
            }
            ActionRequest::UploadAllToMirror(mirror) => {
                // One at a time: each rewrites the index, and two lists of the same
                // WADs would race to send them.
                if self.state.mirror_bulk.is_some() {
                    return;
                }
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.state.mirror_bulk = Some(crate::state::MirrorBulk {
                    mirror: mirror.clone(),
                    done: 0,
                    total: 0,
                    title: String::new(),
                    cancel: std::sync::Arc::clone(&cancel),
                });
                mirrors::spawn_upload_all(
                    self.bg.sender(),
                    self.state.db_path.clone(),
                    mirror,
                    cancel,
                );
            }
            ActionRequest::Get(target) => self.start_get(target),
            ActionRequest::UploadToMirror { wad_id, mirror } => {
                let title = caco_core::db::get_wad(&self.conn, wad_id, false)
                    .ok()
                    .flatten()
                    .map_or_else(|| format!("WAD #{wad_id}"), |wad| wad.title);
                self.state.uploads.push(title.clone());
                mirrors::spawn_upload(
                    self.bg.sender(),
                    self.state.db_path.clone(),
                    wad_id,
                    title,
                    mirror,
                );
            }
        }
    }
}

impl eframe::App for CacoApp {
    fn on_exit(&mut self) {
        persist::save(&self.state.to_persisted());
        self.sync.finish();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        // 1. Drain background messages
        for msg in self.bg.drain() {
            match msg {
                AppMessage::Notify(notif) => {
                    self.state.notification = Some(notif);
                }
                AppMessage::PlayDownloading(now) => {
                    if let PlayState::Playing { downloading, .. } = &mut self.state.play_state {
                        *downloading = now;
                    }
                }
                AppMessage::PlayNeedsSetup { text, fix } => {
                    self.state.play_state = PlayState::Idle;
                    self.state.notification = Some(Notification::error(text).with_action(fix));
                }
                AppMessage::PlayUnavailable { wad_id, reason } => {
                    self.state.play_state = PlayState::Idle;
                    // The worker's file check may have just recorded the file
                    // as missing, and Cacoward cards embed the row too.
                    self.state.invalidate_wad_views();
                    if let Some(dialog) = LinkDialogState::new(&self.conn, wad_id) {
                        self.state.active_dialog =
                            Some(ActiveDialog::Link(dialog.with_failure(reason)));
                    } else {
                        self.state.notification = Some(Notification::error(
                            "WAD is not available and could not be located in the database."
                                .to_string(),
                        ));
                    }
                }
                AppMessage::PlayFileChanged {
                    wad_id,
                    title,
                    change,
                    opts,
                } => {
                    self.state.play_state = PlayState::Idle;
                    // The launch hashed the file to find this out, and a file
                    // that plays differently drops the analysis made from the
                    // old one.
                    self.state.invalidate_wad_views();
                    self.state.active_dialog = Some(ActiveDialog::FileChanged(
                        FileChangedDialogState::new(wad_id, title, change, opts),
                    ));
                }
                AppMessage::PlayNewerSaves {
                    wad_id,
                    title,
                    saves,
                    opts,
                } => {
                    self.state.play_state = PlayState::Idle;
                    self.state.invalidate_wad_views();
                    self.state.active_dialog = Some(ActiveDialog::NewerSaves(
                        NewerSavesDialogState::new(wad_id, title, saves, opts),
                    ));
                }
                AppMessage::SyncFinished(run) => {
                    if run.report.changed() || run.adopted > 0 {
                        self.state.invalidate_wad_views();
                        self.state.refresh_collections(&self.conn);
                    }
                    if let Some(notification) = sync::notification(&run) {
                        self.state.notification = Some(notification);
                    }
                    if let Some(ActiveDialog::Sync(dialog)) = &mut self.state.active_dialog {
                        dialog.reload(&self.conn);
                    }
                    self.state.sync_report = Some(run.report);
                }
                AppMessage::DownloadLanded(arrival) => {
                    // They are in their browser, so it takes more than a window in a
                    // corner to bring them back. A taskbar flash, where there is one.
                    if self.state.gets.landed(&self.conn, arrival) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                            egui::UserAttentionType::Informational,
                        ));
                    }
                }
                AppMessage::GetLinked {
                    target,
                    title,
                    filename,
                    outcome,
                } => {
                    self.state
                        .gets
                        .link_finished(target, &title, outcome.is_ok());
                    self.state.notification = Some(get::linked_notification(
                        &title,
                        &filename,
                        outcome.as_ref(),
                    ));
                    if outcome.is_ok() {
                        self.state.invalidate_wad_views();
                    }
                    if let Ok(GetOutcome::Linked { wad_id }) = outcome {
                        self.state.mirror_offers.push_back(wad_id);
                    }
                }
                AppMessage::FilesChecked(report) => {
                    self.state.invalidate_wad_views();
                    if let Some(notification) = file_check::notification(&report) {
                        self.state.notification = Some(notification);
                    }
                }
                AppMessage::FilesIdentified => {
                    self.state.invalidate_wad_views();
                }
                AppMessage::MirrorsSynced(report) => {
                    if report.changed {
                        self.state.invalidate_wad_views();
                    }
                    if let Some(notification) = mirrors::sync_notification(&report) {
                        self.state.notification = Some(notification);
                    }
                    self.state.mirror_report = Some(report);
                    if let Some(ActiveDialog::Mirrors(dialog)) = &mut self.state.active_dialog {
                        dialog.reload(&self.conn);
                    }
                }
                AppMessage::MirrorProbed(outcome) => {
                    if let Some(ActiveDialog::Mirrors(dialog)) = &mut self.state.active_dialog {
                        dialog.probe_finished(outcome);
                    }
                }
                AppMessage::MirrorSetUp { config, outcome } => {
                    let is_copy = config.is_idgames_copy();
                    if let Some(ActiveDialog::Mirrors(dialog)) = &mut self.state.active_dialog
                        && dialog.set_up_finished(&self.conn, *config, outcome)
                    {
                        mirrors::spawn_sync(self.bg.sender(), self.state.db_path.clone());
                        // Where the listing and the text files can now come from, at no
                        // cost to anyone.
                        if is_copy {
                            self.spawn_idgames_index(true);
                        }
                    }
                }
                AppMessage::MirrorBulkProgress { done, total, title } => {
                    if let Some(bulk) = &mut self.state.mirror_bulk {
                        bulk.done = done;
                        bulk.total = total;
                        bulk.title = title;
                    }
                }
                AppMessage::MirrorBulkFinished(outcome) => {
                    let mirror = self
                        .state
                        .mirror_bulk
                        .take()
                        .map(|bulk| bulk.mirror)
                        .unwrap_or_default();
                    self.state.notification = Some(mirrors::bulk_notification(&mirror, &outcome));
                    self.state.invalidate_wad_views();
                    if let Some(ActiveDialog::Mirrors(dialog)) = &mut self.state.active_dialog {
                        match outcome {
                            Ok(report) => dialog.bulk_finished(&self.conn, &mirror, report),
                            Err(_) => dialog.reload(&self.conn),
                        }
                    }
                }
                AppMessage::MirrorUploaded { title, outcome } => {
                    if let Some(position) = self.state.uploads.iter().position(|t| *t == title) {
                        self.state.uploads.remove(position);
                    }
                    // A file now on a mirror changes what the WAD reads as, and an
                    // upload hashes a file that had not been.
                    self.state.invalidate_wad_views();
                    self.state.notification = Some(mirrors::upload_notification(outcome.as_ref()));
                }
                AppMessage::PlayFinished { outcome } => {
                    self.state.play_state = PlayState::Idle;
                    self.state.invalidate_wad_views();
                    // A session is what all of this is for. Worth getting out soon.
                    self.sync.nudge();

                    match outcome {
                        // A missing file arrives as PlayUnavailable, so this is
                        // never one: matching the text used to send a sourceport
                        // that failed to start to the link dialog too.
                        Err(err) => {
                            self.state.notification =
                                Some(Notification::error(format!("Play failed: {err}")));
                        }
                        Ok(pr) => {
                            if pr.crashed() {
                                self.state.notification = Some(Notification::warning(format!(
                                    "Sourceport crashed (exit code {})",
                                    pr.exit_code.unwrap_or(-1)
                                )));
                            } else if pr.auto_complete
                                == caco_core::player::AutoCompleteResult::Completed
                            {
                                self.state.notification = Some(Notification::info(
                                    "All maps completed! Marked as finished.".to_string(),
                                ));
                            } else if let Some(dur) = pr.duration {
                                self.state.notification = Some(Notification::info(format!(
                                    "Played for {}",
                                    caco_core::player::format_duration(dur)
                                )));
                            }
                        }
                    }
                }
                AppMessage::EnrichProgress { done, total, title } => {
                    if let Some(ActiveDialog::Enrich(dialog)) = &mut self.state.active_dialog {
                        dialog.set_progress(done, total, title);
                    }
                }
                AppMessage::EnrichComplete(outcome) => {
                    // The dialog owns the report; if the user closed it mid-run
                    // the result has nowhere to go, so surface the failure at
                    // least as a toast.
                    match &mut self.state.active_dialog {
                        Some(ActiveDialog::Enrich(dialog)) => dialog.finish(outcome),
                        _ => {
                            if let Err(e) = outcome {
                                self.state.notification =
                                    Some(Notification::error(format!("Enrich failed: {e}")));
                            }
                        }
                    }
                    self.state.invalidate_wad_views();
                }
                AppMessage::SourceportBuildStep(step) => {
                    if let Some(ActiveDialog::Sourceports(dialog)) = &mut self.state.active_dialog {
                        dialog.set_step(step);
                    }
                }
                AppMessage::SourceportBuildLine(line) => {
                    if let Some(ActiveDialog::Sourceports(dialog)) = &mut self.state.active_dialog {
                        dialog.push_log(line);
                    }
                }
                AppMessage::SourceportBuildComplete(outcome) => {
                    // As with enrichment: the dialog owns the log, so a run
                    // the user closed out from under still reports failure.
                    match &mut self.state.active_dialog {
                        Some(ActiveDialog::Sourceports(dialog)) => dialog.finish(outcome),
                        _ => {
                            self.state.notification = Some(match outcome {
                                Ok(msg) => Notification::info(msg),
                                Err(e) => Notification::error(format!("Build failed: {e}")),
                            });
                        }
                    }
                }
                AppMessage::SourceportVersionsLoaded {
                    sourceport,
                    outcome,
                } => {
                    if let Some(ActiveDialog::Sourceports(dialog)) = &mut self.state.active_dialog {
                        dialog.versions_loaded(sourceport, outcome);
                    }
                }
                AppMessage::SourceportUpdatesAvailable(names) => {
                    // Informational only — never interrupts, and never starts
                    // a build the user did not ask for.
                    let text = match names.as_slice() {
                        [one] => format!("{one} has an update."),
                        many => format!(
                            "{} sourceports have updates: {}.",
                            many.len(),
                            many.join(", ")
                        ),
                    };
                    self.state.notification =
                        Some(Notification::info(text).with_action(NoticeAction::OpenSourceports));
                    // A notification goes, and this is still true until something is done
                    // about it, so the sidebar goes on saying so.
                    self.state.sourceport_updates = names;
                }
                AppMessage::SearchComplete(source, outcome) => {
                    let search = self.state.import.search_state_mut(source);
                    match outcome {
                        Ok(results) => search.set_results(results),
                        Err(error) => search.set_error(error),
                    }
                }
                AppMessage::NewestUploads(results) => {
                    self.state.import.idgames.set_newest(results);
                }
                AppMessage::IdgamesIndexStatus(status) => {
                    self.state.import.index.status = Some(status);
                }
                AppMessage::IdgamesIndexProgress(progress) => {
                    self.state.import.index.progress = Some(progress);
                }
                AppMessage::IdgamesIndexFinished(outcome) => {
                    self.state.import.index.finished(&outcome);
                    if let Ok(run) = &outcome {
                        if run.ids_filled_in > 0 {
                            self.state.invalidate_wad_views();
                        }
                        // What is on the table may now be out of date.
                        if run.report.changed() && self.state.import.idgames.showing_newest {
                            import::workers::spawn_newest(
                                self.bg.sender(),
                                self.state.db_path.clone(),
                            );
                        }
                        if let Some(notification) = idgames_index::notification(run) {
                            self.state.notification = Some(notification);
                        }
                    }
                }
                AppMessage::ThumbnailReady {
                    wad_id,
                    width,
                    height,
                    pixels,
                } => {
                    self.thumbnails
                        .on_ready(ctx, wad_id, width, height, &pixels);
                }
                AppMessage::ThumbnailFailed { wad_id } => {
                    self.thumbnails.mark_failed(wad_id);
                }
                AppMessage::AnalysesRefreshed(refreshed) => {
                    for (wad_id, analysis) in refreshed {
                        self.state.analyses_map.insert(wad_id, analysis);
                    }
                }
                AppMessage::ImportComplete(origin, result) => {
                    self.import_finished(origin, result);
                }
            }
        }

        // 2. Check filter debounce
        self.state.check_filter_debounce(ctx, &self.conn);

        self.state.wide_window = panels::wad_details::fits(ctx);
        self.take_dropped_file(ctx);

        // 3. Reload data if needed (defer when on Import view)
        if self.state.needs_reload && self.state.view_mode == ViewMode::Library {
            self.state.reload(&self.conn);
            self.kick_reanalysis();
        }
        // Stats says how far through each year you are, by the same reckoning.
        if self.state.cacowards.needs_reload
            && matches!(self.state.view_mode, ViewMode::Cacowards | ViewMode::Stats)
        {
            self.state.reload_cacowards(&self.conn);
        }
        if self.state.stats.needs_reload && self.state.view_mode == ViewMode::Stats {
            self.state.reload_stats(&self.conn);
        }

        // 4. Render active dialog (modal, overlays everything)
        self.state.open_next_mirror_offer(&self.conn);
        if let Some(wad_id) = self.state.details_wanted() {
            self.state.load_details(&self.conn, wad_id);
        }
        let dialog_was_open = self.state.has_dialog();
        if let Some(action) =
            render_active_dialog(&mut self.state, &self.conn, ctx, &self.thumbnails)
        {
            self.dispatch_action(action);
        }
        // What was pressed was for the dialog, even if that closed it: the panels are drawn
        // after this, and Escape closing Edit also deselected the WAD behind it, and Enter
        // in the last field of a dialog that closes on it would play it.
        if dialog_was_open {
            ctx.input_mut(|i| {
                i.events.retain(|event| {
                    !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_))
                });
            });
        }

        // A landed file is asked about whatever else is open, and however it is answered
        // can open a mirror offer on the next frame.
        get::render_question(&mut self.state, &self.conn, ctx, &self.bg.sender());
        let now = std::time::Instant::now();
        let expired = self.state.gets.expire(now);
        if let Some(notification) = get::expired_notification(&expired) {
            self.state.notification = Some(notification);
        }
        if let Some(deadline) = self.state.gets.next_deadline() {
            ctx.request_repaint_after(deadline.saturating_duration_since(now));
        }

        // 5. Handle keyboard accelerators
        let mut quit = false;
        if !self.state.has_dialog() {
            if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
                self.state.needs_reload = true;
            }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Q)) {
                quit = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F1)) || typed_alone(ctx, "?") {
                self.dispatch_action(ActionRequest::Help);
            }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F))
                || typed_alone(ctx, "/")
            {
                self.focus_search(ctx);
            }
            for (key, view) in VIEW_KEYS {
                if ctx.input(|i| i.modifiers.command && i.key_pressed(key)) {
                    self.state.show_view(view);
                }
            }
            // Ctrl+R — reload config from disk. Already-cached state (e.g. window
            // size chosen at startup) does not refresh; subsequent reads pick up
            // new values.
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::R)) {
                caco_core::config::reload_config();
                self.state.notification = Some(Notification::info(
                    "Settings reloaded from disk.".to_string(),
                ));
                // The mirrors may be what was edited.
                mirrors::spawn_sync(self.bg.sender(), self.state.db_path.clone());
            }
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // 6. Render layout
        let mut actions: Vec<ActionRequest> = Vec::new();

        // ── Left sidebar ──
        egui::Panel::left("sidebar_nav")
            .exact_size(200.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG_SIDEBAR).inner_margin(0.0))
            .show(ui, |ui| {
                render_sidebar(ui, &mut self.state, &mut actions);
            });

        // ── Top bar (breadcrumbs + search + sort) ──
        egui::Panel::top("topbar")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_WINDOW)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER)),
            )
            .show(ui, |ui| {
                render_topbar(ui, &mut self.state, &mut actions);
            });

        // ── Bottom status bar ──
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DARK)
                    .inner_margin(egui::Margin::symmetric(16, 4))
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER)),
            )
            .show(ui, |ui| {
                render_status_bar(ui, &mut self.state, &mut actions);
            });

        // ── What is known about the selected WAD, beside the library ──
        if self.state.details_showing() {
            egui::Panel::right("wad_details")
                .exact_size(panels::wad_details::PANEL_WIDTH)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(theme::BG_SIDEBAR)
                        .inner_margin(0.0)
                        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER)),
                )
                .show(ui, |ui| {
                    if let Some(a) =
                        panels::wad_details::render_panel(ui, &self.state, &self.thumbnails)
                    {
                        actions.push(a);
                    }
                });
        }

        // ── Central panel (main content) ──
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_DARK).inner_margin(0.0))
            .show(ui, |ui| {
                match self.state.view_mode {
                    ViewMode::Library => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                let view_top = ui.cursor().top();

                                // Now-playing hero
                                if let Some(a) =
                                    render_now_playing_hero(ui, &self.state, &self.thumbnails)
                                {
                                    actions.push(a);
                                }

                                // Section header with view toggle
                                ui.add_space(4.0);
                                render_section_header(ui, &mut self.state, &mut actions);
                                ui.add_space(8.0);

                                // WAD grid or table, or why there is neither
                                if self.state.wads.is_empty() {
                                    match panels::render_empty_library(
                                        ui,
                                        self.state.total_wad_count,
                                    ) {
                                        panels::EmptyLibraryAction::None => {}
                                        panels::EmptyLibraryAction::Import => {
                                            self.state.view_mode = ViewMode::Import;
                                        }
                                        panels::EmptyLibraryAction::Cacowards => {
                                            self.state.show_view(ViewMode::Cacowards);
                                        }
                                        panels::EmptyLibraryAction::ClearFilter => {
                                            self.state.clear_filters();
                                        }
                                    }
                                    return;
                                }
                                let view_action = match self.state.view_layout {
                                    ViewLayout::List => {
                                        panels::wad_table::render(ui, &mut self.state, view_top)
                                    }
                                    ViewLayout::Grid => panels::wad_grid::render(
                                        ui,
                                        &mut self.state,
                                        Some(&self.thumbnails),
                                        view_top,
                                    ),
                                };
                                if let Some(a) = view_action {
                                    actions.push(a);
                                }
                            });
                    }
                    ViewMode::Import => {
                        // The margin every other view has.
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(20, 8))
                            .show(ui, |ui| {
                                if let Some(import_action) =
                                    import::render(ui, &mut self.state.import)
                                {
                                    self.dispatch_import_action(import_action);
                                }
                            });
                    }
                    ViewMode::Stats => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                panels::stats::render(ui, &self.state);
                            });
                    }
                    ViewMode::Cacowards => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                if let Some(a) = panels::cacowards::render(
                                    ui,
                                    &mut self.state,
                                    Some(&self.thumbnails),
                                ) {
                                    actions.push(a);
                                }
                            });
                    }
                }
            });

        // 7. Request thumbnails for visible WADs (library) or linked
        // cacoward entries (magazine view).
        if self.state.view_mode == ViewMode::Library {
            let sender = self.bg.sender();

            // Request thumbnails for all visible WADs
            if self
                .state
                .wads
                .iter()
                .any(|w| self.thumbnails.needs_request(w.id))
            {
                for wad in &self.state.wads {
                    if self.thumbnails.needs_request(wad.id) {
                        let path = wad.cached_path.as_deref().map(std::path::Path::new);
                        let hint = ThumbnailHint {
                            source_type: wad.source_type.as_str().to_string(),
                            source_url: wad.source_url.clone(),
                            title: wad.title.clone(),
                        };
                        self.thumbnails.request(wad.id, path, &hint, &sender);
                    }
                }
            }
        } else if self.state.view_mode == ViewMode::Cacowards
            && self.state.cacowards.layout == ViewLayout::Grid
        {
            let sender = self.bg.sender();
            // Linked WADs use their wad_id and the existing pipeline
            // (cache → TITLEPIC → wiki scrape).
            for wad in self.state.cacowards.linked_wads.values() {
                if self.thumbnails.needs_request(wad.id) {
                    let path = wad.cached_path.as_deref().map(std::path::Path::new);
                    let hint = ThumbnailHint {
                        source_type: wad.source_type.as_str().to_string(),
                        source_url: wad.source_url.clone(),
                        title: wad.title.clone(),
                    };
                    self.thumbnails.request(wad.id, path, &hint, &sender);
                }
            }
            // Absent entries have no wad row, but most carry a Doom Wiki page:
            // feed it into the same scraper path the import-flow wiki thumbnails
            // use. Only for the year on screen, or opening this view would ask the
            // wiki about every entry in every year. Negative pk keys avoid colliding
            // with wad_ids in the manager's hashmap.
            let year = self.state.cacowards.selected_year;
            for (record, status) in &self.state.cacowards.all_entries {
                if Some(record.year) != year
                    || !matches!(status, caco_core::db::cacowards::EffectiveStatus::Absent)
                {
                    continue;
                }
                let Some(url) = record.doomwiki_url() else {
                    continue;
                };
                let key = panels::cacowards::thumb_key_for_absent(record.id);
                if !self.thumbnails.needs_request(key) {
                    continue;
                }
                let hint = ThumbnailHint {
                    source_type: "doomwiki".to_string(),
                    source_url: Some(url),
                    title: record.wad_title.clone(),
                };
                self.thumbnails.request(key, None, &hint, &sender);
            }
        }

        render_drop_hint(ctx, self.state.has_dialog());

        // 8. Dispatch action requests
        for action in actions {
            self.dispatch_action(action);
        }
    }
}
