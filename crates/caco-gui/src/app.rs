use std::path::PathBuf;

use egui::Color32;
use rusqlite::Connection;

use crate::dialogs::cache::CacheDialogState;
use crate::dialogs::collections::CollectionsDialogState;
use crate::dialogs::delete::DeleteDialogState;
use crate::dialogs::edit::EditDialogState;
use crate::dialogs::link::LinkDialogState;
use crate::dialogs::resources::ResourcesDialogState;
use crate::dialogs::sessions::SessionsDialogState;
use crate::dialogs::stats::StatsDialogState;
use crate::dialogs::wad_stats::WadStatsDialogState;
use crate::import;
use crate::import::state::SearchSource;
use crate::message::{AppMessage, Notification};
use crate::panels;
use crate::persist;
use crate::state::{ActionRequest, ActiveDialog, AppState, PlayState, ViewLayout, ViewMode};
use crate::theme;
use crate::thumbnails::{ThumbnailHint, ThumbnailManager};
use crate::workers::{AnalysisJob, BackgroundChannel, spawn_reanalysis};

mod dialogs;
mod help;
mod hero;
mod section_header;
mod sidebar;
mod status_bar;
mod topbar;

use dialogs::render_active_dialog;
use hero::render_now_playing_hero;
use section_header::render_section_header;
use sidebar::render_sidebar;
use status_bar::render_status_bar;
use topbar::render_topbar;

pub struct CacoApp {
    conn: Connection,
    state: AppState,
    bg: BackgroundChannel,
    thumbnails: ThumbnailManager,
}

impl CacoApp {
    pub fn new(conn: Connection, db_path: PathBuf, ctx: &egui::Context) -> Self {
        let mut bg = BackgroundChannel::new();
        bg.set_ctx(ctx.clone());

        Self {
            conn,
            state: AppState::new(db_path),
            bg,
            thumbnails: ThumbnailManager::new(),
        }
    }

    /// Dispatch an import action (from import view).
    fn dispatch_import_action(&mut self, action: import::ImportAction) {
        let sender = self.bg.sender();
        let db_path = self.state.db_path.clone();

        match action {
            import::ImportAction::Search(source, query) => {
                import::workers::spawn_search(sender, source, query);
            }
            import::ImportAction::ImportSearchResult(source, entry) => {
                let source_id = entry.source_id();
                match source {
                    SearchSource::Idgames => {
                        import::workers::spawn_import_idgames(sender, db_path, source_id);
                    }
                    SearchSource::Doomwiki => {
                        import::workers::spawn_import_doomwiki(sender, db_path, source_id);
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
            .filter_map(|w| {
                let path = PathBuf::from(w.cached_path.as_deref()?);
                if !path.exists() {
                    return None;
                }
                Some(AnalysisJob {
                    wad_id: w.id,
                    wad_path: path,
                })
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        spawn_reanalysis(self.bg.sender(), self.state.db_path.clone(), jobs);
    }

    /// Dispatch an action request (from detail panel buttons or table shortcuts).
    fn dispatch_action(&mut self, action: ActionRequest) {
        match action {
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
            ActionRequest::Stats => {
                let dialog = StatsDialogState::new(&self.conn);
                self.state.active_dialog = Some(ActiveDialog::Stats(dialog));
            }
            ActionRequest::Cache => {
                let dialog = CacheDialogState::new(&self.conn);
                self.state.active_dialog = Some(ActiveDialog::Cache(dialog));
            }
            ActionRequest::Settings => {
                let dialog = crate::dialogs::settings::SettingsDialogState::new();
                self.state.active_dialog = Some(ActiveDialog::Settings(dialog));
            }
            ActionRequest::Resources => {
                let dialog = ResourcesDialogState::new(&self.conn);
                self.state.active_dialog = Some(ActiveDialog::Resources(dialog));
            }
            ActionRequest::Collections => {
                let dialog = CollectionsDialogState::new(&self.conn);
                self.state.active_dialog = Some(ActiveDialog::Collections(dialog));
            }
            ActionRequest::EditCollection(name) => {
                let dialog = CollectionsDialogState::new_editing(&self.conn, &name);
                self.state.active_dialog = Some(ActiveDialog::Collections(dialog));
            }
            ActionRequest::DeleteCollection(name) => {
                if let Ok(true) = caco_core::db::collections::delete_collection(&self.conn, &name) {
                    // Clear active collection if we just deleted it
                    if self.state.active_collection.as_deref() == Some(&name) {
                        self.state.active_collection = None;
                        self.state.filter.set_both(String::new());
                    }
                    self.state.sidebar_collections =
                        caco_core::db::collections::get_all_collections(&self.conn)
                            .unwrap_or_default();
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
                        self.state.needs_reload = true;
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
                if self.state.is_playing() {
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
                };

                // Spawn play worker in a background thread
                let sender = self.bg.sender();
                let db_path = self.state.db_path.clone();
                std::thread::spawn(move || {
                    // Local error type that distinguishes "WAD not available —
                    // offer the link dialog" from other errors that should
                    // surface as a toast. String errors from `?` default to
                    // Message; Unavailable is chosen explicitly at the sites
                    // where no downloadable source exists or the download
                    // could not complete.
                    enum PlayError {
                        Message(String),
                        Unavailable,
                    }
                    impl From<String> for PlayError {
                        fn from(s: String) -> Self {
                            PlayError::Message(s)
                        }
                    }

                    let outcome = (|| -> Result<caco_core::player::PlayResult, PlayError> {
                        let conn = caco_core::db::open_connection(&db_path)
                            .map_err(|e| format!("DB open failed: {e}"))?;

                        // Auto-download from idgames if no cached file is available
                        let wad = caco_core::db::get_wad(&conn, wad_id, false)
                            .map_err(|e| format!("DB error: {e}"))?
                            .ok_or_else(|| format!("WAD #{wad_id} not found"))?;
                        let needs_download = wad
                            .cached_path
                            .as_deref()
                            .map(|p| !std::path::Path::new(p).exists())
                            .unwrap_or(true);
                        if needs_download {
                            sender.send(AppMessage::Notify(Notification::info(format!(
                                "Downloading {}...",
                                wad.title
                            ))));
                            let client = caco_sources::idgames::IdgamesClient::new();
                            let cache_dir = caco_core::config::get_cache_dir();
                            std::fs::create_dir_all(&cache_dir)
                                .map_err(|e| format!("Failed to create cache dir: {e}"))?;
                            let mirror = caco_core::config::load_config().download_mirror as usize;

                            let idgames_id = wad
                                .idgames_id
                                .as_deref()
                                .and_then(|id| id.parse::<i64>().ok());

                            let dest = if let Some(ig_id) = idgames_id {
                                match client.get(Some(ig_id), None) {
                                    Ok(entry) => client
                                        .download(&entry, Some(&cache_dir), mirror, None)
                                        .map_err(|e| format!("Download failed: {e}"))?,
                                    Err(caco_sources::SourceError::WafBlocked { .. }) => {
                                        let source_url = wad.source_url.as_deref().unwrap_or("");
                                        let filename = wad.filename.as_deref().unwrap_or("");
                                        if filename.is_empty() || !source_url.contains("/idgames/")
                                        {
                                            return Err(PlayError::Unavailable);
                                        }
                                        client
                                            .download_direct(
                                                source_url, filename, &cache_dir, mirror, None,
                                            )
                                            .map_err(|_| PlayError::Unavailable)?
                                    }
                                    Err(_) => {
                                        return Err(PlayError::Unavailable);
                                    }
                                }
                            } else {
                                // No numeric ID — try direct mirror via source_url
                                let source_url = wad.source_url.as_deref().unwrap_or("");
                                let filename = wad.filename.as_deref().unwrap_or("");
                                if filename.is_empty() || !source_url.contains("/idgames/") {
                                    return Err(PlayError::Unavailable);
                                }
                                client
                                    .download_direct(source_url, filename, &cache_dir, mirror, None)
                                    .map_err(|_| PlayError::Unavailable)?
                            };

                            let update = caco_core::db::WadUpdate::new()
                                .set_text("cached_path", Some(dest.to_string_lossy().to_string()));
                            caco_core::db::update_wad(&conn, wad_id, &update)
                                .map_err(|e| format!("Failed to update WAD record: {e}"))?;
                        }

                        let pr = caco_core::player::play(
                            &conn,
                            wad_id,
                            &caco_core::player::PlayOptions::default(),
                        )
                        .map_err(|e| format!("{e}"))?;
                        Ok(pr)
                    })();

                    match outcome {
                        Ok(pr) => sender.send(AppMessage::PlayFinished {
                            wad_id,
                            outcome: Ok(pr),
                        }),
                        Err(PlayError::Message(msg)) => sender.send(AppMessage::PlayFinished {
                            wad_id,
                            outcome: Err(msg),
                        }),
                        Err(PlayError::Unavailable) => {
                            sender.send(AppMessage::PlayUnavailable { wad_id })
                        }
                    }
                });
            }
        }
    }
}

impl eframe::App for CacoApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        persist::save(&self.state.to_persisted());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Drain background messages
        for msg in self.bg.drain() {
            match msg {
                AppMessage::Notify(notif) => {
                    self.state.notification = Some(notif);
                }
                AppMessage::PlayUnavailable { wad_id } => {
                    self.state.play_state = PlayState::Idle;
                    self.state.needs_reload = true;
                    if let Some(dialog) = LinkDialogState::new(&self.conn, wad_id) {
                        self.state.active_dialog = Some(ActiveDialog::Link(dialog));
                    } else {
                        self.state.notification = Some(Notification::error(
                            "WAD is not available and could not be located in the database."
                                .to_string(),
                        ));
                    }
                }
                AppMessage::PlayFinished { wad_id, outcome } => {
                    self.state.play_state = PlayState::Idle;
                    self.state.needs_reload = true;

                    match outcome {
                        Err(err) => {
                            // Post-download "cached file vanished" still opens the
                            // link dialog. All other string errors surface as a
                            // toast; explicit unavailability uses PlayUnavailable.
                            if err.starts_with("file not found:") {
                                if let Some(dialog) = LinkDialogState::new(&self.conn, wad_id) {
                                    self.state.active_dialog = Some(ActiveDialog::Link(dialog));
                                } else {
                                    self.state.notification =
                                        Some(Notification::error(format!("Play failed: {err}")));
                                }
                            } else {
                                self.state.notification =
                                    Some(Notification::error(format!("Play failed: {err}")));
                            }
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
                AppMessage::SearchComplete(source, results) => {
                    self.state
                        .import
                        .search_state_mut(source)
                        .set_results(results);
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
                AppMessage::ImportComplete(result) => {
                    // Reset only the active form's submitting state
                    let active = self.state.import.active_source;
                    match active {
                        2 => self.state.import.doomworld.is_submitting = false,
                        3 => self.state.import.url_form.is_submitting = false,
                        4 => self.state.import.local_form.is_submitting = false,
                        _ => {}
                    }

                    match result {
                        Ok(ir) => {
                            if ir.is_duplicate {
                                let title =
                                    ir.duplicate_title.unwrap_or_else(|| "unknown".to_string());
                                let id = ir.duplicate_id.unwrap_or(0);
                                self.state.notification = Some(Notification::warning(format!(
                                    "Already imported as {title} (#{id})"
                                )));
                            } else {
                                self.state.notification = Some(Notification::info(
                                    "WAD imported successfully".to_string(),
                                ));
                                self.state.needs_reload = true;
                                // If the Cacowards panel is the source of
                                // this import (or just open), force a
                                // refresh so the new wad link shows up.
                                self.state.cacowards.needs_reload = true;
                                // Reset only the active form on success
                                match active {
                                    2 => self.state.import.doomworld.reset(),
                                    3 => self.state.import.url_form.reset(),
                                    4 => self.state.import.local_form.reset(),
                                    _ => {}
                                }
                            }
                        }
                        Err(e) => {
                            self.state.notification =
                                Some(Notification::error(format!("Import failed: {e}")));
                        }
                    }
                }
            }
        }

        // 2. Check filter debounce
        self.state.check_filter_debounce(ctx, &self.conn);

        // 3. Reload data if needed (defer when on Import view)
        if self.state.needs_reload && self.state.view_mode == ViewMode::Library {
            self.state.reload(&self.conn);
            self.kick_reanalysis();
        }
        if self.state.cacowards.needs_reload && self.state.view_mode == ViewMode::Cacowards {
            self.state.reload_cacowards(&self.conn);
        }

        // 4. Render active dialog (modal, overlays everything)
        if let Some(action) = render_active_dialog(&mut self.state, &self.conn, ctx) {
            self.dispatch_action(action);
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
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                let id = egui::Id::new(panels::filter_bar::FILTER_ID_SOURCE);
                ctx.memory_mut(|m| m.request_focus(id));
            }
            // Ctrl+R — reload config from disk. Already-cached state (e.g. window
            // size chosen at startup) does not refresh; subsequent reads pick up
            // new values.
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::R)) {
                caco_core::config::reload_config();
                self.state.notification =
                    Some(Notification::info("Config reloaded from disk".to_string()));
            }
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // 6. Render layout
        let mut actions: Vec<ActionRequest> = Vec::new();

        // ── Left sidebar ──
        egui::SidePanel::left("sidebar_nav")
            .exact_width(200.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG_SIDEBAR).inner_margin(0.0))
            .show(ctx, |ui| {
                render_sidebar(ui, &mut self.state, &mut actions);
            });

        // ── Top bar (breadcrumbs + search + sort) ──
        egui::TopBottomPanel::top("topbar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x1a, 0x14, 0x10))
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                render_topbar(ui, &mut self.state, &mut actions);
            });

        // ── Bottom status bar ──
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DARK)
                    .inner_margin(egui::Margin::symmetric(16, 4))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                render_status_bar(ui, &mut self.state);
            });

        // ── Central panel (main content) ──
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_DARK).inner_margin(0.0))
            .show(ctx, |ui| {
                match self.state.view_mode {
                    ViewMode::Library => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());

                                // Now-playing hero
                                if let Some(a) =
                                    render_now_playing_hero(ui, &self.state, &self.thumbnails)
                                {
                                    actions.push(a);
                                }

                                // Section header with view toggle
                                ui.add_space(4.0);
                                render_section_header(ui, &mut self.state);
                                ui.add_space(8.0);

                                // WAD grid or table
                                let view_action = match self.state.view_layout {
                                    ViewLayout::List => {
                                        panels::wad_table::render(ui, &mut self.state)
                                    }
                                    ViewLayout::Grid => panels::wad_grid::render(
                                        ui,
                                        &mut self.state,
                                        Some(&self.thumbnails),
                                    ),
                                };
                                if let Some(a) = view_action {
                                    actions.push(a);
                                }
                            });
                    }
                    ViewMode::Import => {
                        if let Some(import_action) = import::render(ui, &mut self.state.import) {
                            self.dispatch_import_action(import_action);
                        }
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
        } else if self.state.view_mode == ViewMode::Cacowards {
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
            // Absent entries have no wad row but always carry a Doom Wiki
            // URL — feed it into the same scraper path the import-flow
            // wiki thumbnails use. Negative pk keys avoid colliding with
            // wad_ids in the manager's hashmap.
            for (record, status) in &self.state.cacowards.all_entries {
                if !matches!(status, caco_core::db::cacowards::EffectiveStatus::Absent) {
                    continue;
                }
                let Some(url) = record.doomwiki_url.as_deref() else {
                    continue;
                };
                let key = panels::cacowards::thumb_key_for_absent(record.id);
                if !self.thumbnails.needs_request(key) {
                    continue;
                }
                let hint = ThumbnailHint {
                    source_type: "doomwiki".to_string(),
                    source_url: Some(url.to_string()),
                    title: record.wad_title.clone(),
                };
                self.thumbnails.request(key, None, &hint, &sender);
            }
        }

        // 8. Dispatch action requests
        for action in actions {
            self.dispatch_action(action);
        }
    }
}
