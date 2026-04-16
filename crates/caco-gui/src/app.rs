use std::path::PathBuf;

use rusqlite::Connection;

use crate::dialogs::cache::{CacheDialogState, CacheResult};
use crate::dialogs::collections::{CollectionsDialogState, CollectionsResult};
use crate::dialogs::delete::{DeleteDialogState, DeleteResult};
use crate::dialogs::edit::{EditDialogState, EditResult};
use crate::dialogs::link::{LinkDialogState, LinkResult};
use crate::dialogs::resources::{ResourcesDialogState, ResourcesResult};
use crate::dialogs::sessions::{SessionsDialogState, SessionsResult};
use crate::dialogs::stats::{StatsDialogState, StatsResult};
use crate::dialogs::wad_stats::{WadStatsDialogState, WadStatsResult};
use crate::import;
use crate::import::state::SearchSource;
use crate::message::{AppMessage, Notification};
use crate::panels;
use crate::persist;
use crate::state::{ActionRequest, ActiveDialog, AppState, PlayState, ViewLayout, ViewMode};
use crate::theme;
use crate::thumbnails::{ThumbnailHint, ThumbnailManager};
use crate::workers::BackgroundChannel;

mod help;
mod status_bar;

use help::{render_about_dialog, render_help_dialog};
use status_bar::render_status_bar;

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
        }

        // 4. Render dialogs (modal, overlays everything)
        let mut close_dialog = false;
        if let Some(dialog) = &mut self.state.active_dialog {
            match dialog {
                ActiveDialog::Edit(edit_state) => match edit_state.render(ctx, &self.conn) {
                    EditResult::Saved => {
                        close_dialog = true;
                        self.state.needs_reload = true;
                        self.state.notification =
                            Some(Notification::info("WAD updated".to_string()));
                    }
                    EditResult::Cancelled => {
                        close_dialog = true;
                    }
                    EditResult::Modified => {
                        close_dialog = true;
                        self.state.needs_reload = true;
                    }
                    EditResult::Open => {}
                },
                ActiveDialog::Delete(delete_state) => match delete_state.render(ctx, &self.conn) {
                    DeleteResult::Confirmed => {
                        close_dialog = true;
                        self.state.needs_reload = true;
                        self.state.notification =
                            Some(Notification::info("WAD deleted".to_string()));
                    }
                    DeleteResult::Error(msg) => {
                        close_dialog = true;
                        self.state.notification = Some(Notification::error(msg));
                    }
                    DeleteResult::Cancelled => {
                        close_dialog = true;
                    }
                    DeleteResult::Open => {}
                },
                ActiveDialog::Sessions(sessions_state) => match sessions_state.render(ctx) {
                    SessionsResult::Closed => {
                        close_dialog = true;
                    }
                    SessionsResult::Open => {}
                },
                ActiveDialog::Stats(stats_state) => match stats_state.render(ctx) {
                    StatsResult::Closed => {
                        close_dialog = true;
                    }
                    StatsResult::Open => {}
                },
                ActiveDialog::Cache(cache_state) => match cache_state.render(ctx, &self.conn) {
                    CacheResult::Closed => {
                        close_dialog = true;
                    }
                    CacheResult::Open => {}
                },
                ActiveDialog::Collections(collections_state) => {
                    let modified = collections_state.modified;
                    match collections_state.render(ctx, &self.conn) {
                        CollectionsResult::Closed => {
                            if modified {
                                self.state.refresh_collections(&self.conn);
                            }
                            close_dialog = true;
                        }
                        CollectionsResult::LoadQuery(query) => {
                            close_dialog = true;
                            self.state.refresh_collections(&self.conn);
                            self.state.active_collection = None;
                            self.state.filter.input = query;
                            self.state.filter.mark_changed(std::time::Instant::now());
                        }
                        CollectionsResult::Open => {}
                    }
                }
                ActiveDialog::Resources(resources_state) => {
                    match resources_state.render(ctx, &self.conn) {
                        ResourcesResult::Closed => {
                            close_dialog = true;
                        }
                        ResourcesResult::Open => {}
                    }
                }
                ActiveDialog::WadStats(wad_stats_state) => {
                    match wad_stats_state.render(ctx, &self.conn) {
                        WadStatsResult::Closed => {
                            close_dialog = true;
                        }
                        WadStatsResult::Modified => {
                            close_dialog = true;
                            self.state.needs_reload = true;
                        }
                        WadStatsResult::Open => {}
                    }
                }
                ActiveDialog::Link(link_state) => match link_state.render(ctx, &self.conn) {
                    LinkResult::Linked => {
                        close_dialog = true;
                        self.state.needs_reload = true;
                        self.state.notification =
                            Some(Notification::info("WAD file linked".to_string()));
                    }
                    LinkResult::Cancelled => {
                        close_dialog = true;
                    }
                    LinkResult::Open => {}
                },
                ActiveDialog::Help => {
                    if render_help_dialog(ctx) {
                        close_dialog = true;
                    }
                }
                ActiveDialog::About => {
                    if render_about_dialog(ctx) {
                        close_dialog = true;
                    }
                }
            }
        }
        if close_dialog {
            // Check if dialog was modified -> trigger reload
            let was_modified = match &self.state.active_dialog {
                Some(ActiveDialog::Cache(s)) => s.modified,
                Some(ActiveDialog::Collections(s)) => s.modified,
                Some(ActiveDialog::Resources(s)) => s.modified,
                _ => false,
            };
            if was_modified {
                self.state.needs_reload = true;
            }
            self.state.active_dialog = None;
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
                                if let Some(a) = render_now_playing_hero(
                                    ui,
                                    &self.state,
                                    &self.thumbnails,
                                    &self.conn,
                                ) {
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
                }
            });

        // 7. Request thumbnails for visible WADs
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
        }

        // 8. Dispatch action requests
        for action in actions {
            self.dispatch_action(action);
        }
    }
}

use egui::Color32;

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

fn render_sidebar(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<ActionRequest>) {
    ui.add_space(16.0);

    // Logo
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.colored_label(
            theme::TEXT_ACCENT,
            egui::RichText::new("caco").size(22.0).strong(),
        );
    });

    ui.add_space(24.0);

    // Navigation items
    if theme::sidebar_nav_item(ui, "Library", state.view_mode == ViewMode::Library) {
        state.view_mode = ViewMode::Library;
        if state.needs_reload || state.wads.is_empty() {
            state.needs_reload = true;
        }
    }
    if theme::sidebar_nav_item(ui, "Import", state.view_mode == ViewMode::Import) {
        state.view_mode = ViewMode::Import;
    }

    // Divider
    ui.add_space(12.0);
    let rect = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x + 20.0, rect.min.y),
            egui::pos2(rect.max.x - 20.0, rect.min.y),
        ],
        egui::Stroke::new(1.0, theme::BORDER),
    );
    ui.add_space(16.0);

    // Collections section
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.colored_label(
            theme::TEXT_MUTED,
            egui::RichText::new("COLLECTIONS").size(11.0).strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(16.0);
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("+").size(13.0).color(theme::TEXT_MUTED))
                        .frame(false),
                )
                .on_hover_text("Manage collections")
                .clicked()
            {
                actions.push(ActionRequest::Collections);
            }
        });
    });
    ui.add_space(4.0);

    if state.sidebar_collections.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new("No collections yet").size(12.0),
            );
        });
    } else {
        // Clone names to avoid borrow conflict
        let collection_names: Vec<String> = state
            .sidebar_collections
            .iter()
            .map(|c| c.name.clone())
            .collect();

        for name in &collection_names {
            let is_active = state.active_collection.as_deref() == Some(name.as_str());
            let resp = theme::sidebar_collection_item(ui, name, is_active);

            if resp.clicked() {
                // Find the collection and load its query + sort
                if let Some(coll) = state.sidebar_collections.iter().find(|c| c.name == *name) {
                    state.active_collection = Some(name.clone());
                    state.filter.set_both(coll.query.clone());
                    // Apply collection sort settings
                    if let Some(ref sort_by) = coll.sort_by {
                        if let Some(idx) = crate::state::SORT_FIELDS
                            .iter()
                            .position(|(key, _)| *key == sort_by.as_str())
                        {
                            state.sort_field_index = idx;
                        }
                        state.sort_desc = coll.sort_desc;
                    }
                    state.view_mode = ViewMode::Library;
                    state.needs_reload = true;
                }
            }

            // Right-click context menu
            let ctx_name = name.clone();
            resp.context_menu(|ui| {
                if ui.button("Edit").clicked() {
                    actions.push(ActionRequest::EditCollection(ctx_name.clone()));
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    actions.push(ActionRequest::DeleteCollection(ctx_name.clone()));
                    ui.close_menu();
                }
            });
        }
    }

    // (Status filter pills are now rendered in the section header)

    // Bottom spacer + admin links
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(11.0),
            );
        });
        ui.add_space(4.0);

        // Divider above bottom links
        let rect = ui.available_rect_before_wrap();
        ui.painter().line_segment(
            [
                egui::pos2(rect.min.x + 20.0, rect.max.y),
                egui::pos2(rect.max.x - 20.0, rect.max.y),
            ],
            egui::Stroke::new(1.0, theme::BORDER),
        );
        ui.add_space(12.0);

        // Small action links
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Stats")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .frame(false),
                )
                .clicked()
            {
                actions.push(ActionRequest::Stats);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Cache")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .frame(false),
                )
                .clicked()
            {
                actions.push(ActionRequest::Cache);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("IWADs")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .frame(false),
                )
                .clicked()
            {
                actions.push(ActionRequest::Resources);
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Top bar (breadcrumbs + search + sort)
// ---------------------------------------------------------------------------

fn render_topbar(ui: &mut egui::Ui, state: &mut AppState, _actions: &mut Vec<ActionRequest>) {
    ui.horizontal(|ui| {
        // Breadcrumbs
        render_breadcrumbs(ui, state);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Sort controls (right-aligned)
            panels::sort_controls::render(ui, state);

            // Search/filter
            panels::filter_bar::render(ui, state);
        });
    });
}

fn render_breadcrumbs(ui: &mut egui::Ui, state: &AppState) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        // Base crumb
        let base = if state.view_mode == ViewMode::Import {
            "Import"
        } else {
            "Library"
        };

        // If a dialog is open, the base is clickable (concept: navigate back)
        let has_detail = state.active_dialog.is_some();
        if has_detail {
            ui.colored_label(theme::TEXT_SECONDARY, base);
        } else {
            ui.colored_label(theme::TEXT_PRIMARY, egui::RichText::new(base).strong());
        }

        // WAD name crumb (when edit/sessions/etc dialog is open)
        if let Some(ref dialog) = state.active_dialog {
            let wad_title = match dialog {
                ActiveDialog::Edit(e) => Some(e.title()),
                _ => None,
            };
            if let Some(title) = wad_title {
                ui.colored_label(theme::TEXT_MUTED, "  /  ");
                ui.colored_label(theme::TEXT_SECONDARY, title);
                ui.colored_label(theme::TEXT_MUTED, "  /  ");
                ui.colored_label(theme::TEXT_ACCENT, egui::RichText::new("Edit").strong());
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Now Playing hero
// ---------------------------------------------------------------------------

fn render_now_playing_hero(
    ui: &mut egui::Ui,
    state: &AppState,
    thumbnails: &ThumbnailManager,
    conn: &Connection,
) -> Option<ActionRequest> {
    // Find the first WAD with "playing" status, or show active play state
    let (wad_title, wad_author, wad_id, is_active) =
        if let PlayState::Playing {
            wad_id, wad_title, ..
        } = &state.play_state
        {
            let author = state
                .wads
                .iter()
                .find(|w| w.id == *wad_id)
                .and_then(|w| w.author.clone());
            (wad_title.clone(), author, *wad_id, true)
        } else {
            // Show the first WAD with in-progress status
            let playing_wad = state
                .wads
                .iter()
                .find(|w| w.status == caco_core::db::Status::InProgress);
            match playing_wad {
                Some(w) => (w.title.clone(), w.author.clone(), w.id, false),
                None => return None, // No hero to show
            }
        };

    let mut action = None;

    let stats = state.stats_map.get(&wad_id);

    ui.add_space(16.0);

    // Hero frame
    let hero_frame = egui::Frame::new()
        .fill(Color32::from_rgb(0x22, 0x18, 0x0c))
        .corner_radius(16)
        .inner_margin(egui::Margin::symmetric(24, 20))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x3a, 0x2e, 0x1a)))
        .outer_margin(egui::Margin::symmetric(20, 0));

    hero_frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            // Thumbnail (real or placeholder) — double-click to play, right-click for menu
            let thumb_size = egui::vec2(120.0, 90.0);
            let (thumb_rect, thumb_resp) =
                ui.allocate_exact_size(thumb_size, egui::Sense::click());

            if let Some(tex) = thumbnails.get(wad_id) {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .rect_filled(thumb_rect, 10.0, Color32::BLACK);
                ui.painter()
                    .image(tex.id(), thumb_rect, uv, Color32::WHITE);
            } else {
                let (c1, _c2, ci) = theme::thumb_colors(wad_id);
                ui.painter().rect_filled(thumb_rect, 10.0, c1);
                let initials: String = wad_title
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .take(2)
                    .flat_map(|c| c.to_uppercase())
                    .collect();
                ui.painter().text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &initials,
                    egui::FontId::proportional(28.0),
                    ci,
                );
            }

            // Hover outline on thumbnail
            if thumb_resp.hovered() {
                ui.painter().rect_stroke(
                    thumb_rect,
                    10.0,
                    egui::Stroke::new(1.5, theme::TEXT_ACCENT),
                    egui::StrokeKind::Outside,
                );
            }

            // Double-click to play
            if thumb_resp.double_clicked() {
                action = Some(ActionRequest::Play(wad_id));
            }

            // Right-click context menu
            let wad_status = state
                .wads
                .iter()
                .find(|w| w.id == wad_id)
                .map(|w| w.status)
                .unwrap_or(caco_core::db::Status::Unplayed);
            if let Some(a) = panels::wad_context_menu(&thumb_resp, wad_id, wad_status) {
                action = Some(a);
            }

            ui.add_space(16.0);

            // Info area
            ui.vertical(|ui| {
                let label_color = if is_active {
                    theme::COLOR_SUCCESS
                } else {
                    Color32::from_rgb(0x55, 0x88, 0xdd)
                };
                let label_text = if is_active {
                    "NOW PLAYING"
                } else {
                    "CONTINUE PLAYING"
                };
                ui.colored_label(
                    label_color,
                    egui::RichText::new(label_text).size(11.0).strong(),
                );
                ui.add_space(2.0);
                ui.colored_label(
                    theme::TEXT_PRIMARY,
                    egui::RichText::new(&wad_title).size(20.0).strong(),
                );
                ui.add_space(2.0);
                let meta = wad_author.as_deref().unwrap_or("");
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    egui::RichText::new(meta).size(13.0),
                );
            });

            // Right side: playtime + progress
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
                        if let Some(s) = stats
                            && s.playtime > 0
                        {
                            ui.colored_label(
                                theme::TEXT_PRIMARY,
                                egui::RichText::new(
                                    caco_core::player::format_duration(s.playtime),
                                )
                                .size(22.0)
                                .strong(),
                            );
                            ui.colored_label(
                                theme::TEXT_MUTED,
                                egui::RichText::new("TOTAL PLAYTIME").size(10.0),
                            );
                        }

                        // Progress bar (from stats_snapshot if available)
                        let wad = state.wads.iter().find(|w| w.id == wad_id);
                        if let Some(wad) = wad
                            && let Some(ref snapshot_json) = wad.stats_snapshot
                            && let Ok(wad_stats) =
                                serde_json::from_str::<caco_core::wad_stats::WadStats>(
                                    snapshot_json,
                                )
                            && !wad_stats.maps.is_empty()
                        {
                            let analysis = caco_core::db::analysis::get_analysis(conn, wad_id)
                                .ok()
                                .flatten();
                            let secret_set: std::collections::HashSet<&str> = analysis
                                .as_ref()
                                .map(|a| a.secret_maps.iter().map(|s| s.as_str()).collect())
                                .unwrap_or_default();

                            let total = wad_stats.maps.len();
                            let played_required = wad_stats
                                .played_maps()
                                .iter()
                                .filter(|m| !secret_set.contains(m.lump.as_str()))
                                .count();
                            let required_total = analysis
                                .as_ref()
                                .map(|a| a.required_maps)
                                .unwrap_or(total);
                            let secret_total = secret_set.len();
                            let played_secret = wad_stats
                                .played_maps()
                                .iter()
                                .filter(|m| secret_set.contains(m.lump.as_str()))
                                .count();

                            // Bar tracks required maps only
                            let pct = if required_total > 0 {
                                played_required as f32 / required_total as f32
                            } else {
                                0.0
                            };

                            ui.add_space(8.0);

                            let bar_width = 200.0_f32;
                            let bar_height = 6.0;
                            let (bar_rect, _) = ui.allocate_exact_size(
                                egui::vec2(bar_width, bar_height),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(
                                bar_rect,
                                3.0,
                                Color32::from_rgb(0x3a, 0x2e, 0x1a),
                            );
                            if pct > 0.0 {
                                let fill_rect = egui::Rect::from_min_size(
                                    bar_rect.min,
                                    egui::vec2(
                                        bar_rect.width() * pct.min(1.0),
                                        bar_height,
                                    ),
                                );
                                ui.painter().rect_filled(
                                    fill_rect,
                                    3.0,
                                    theme::COLOR_SUCCESS,
                                );
                            }

                            let pct_display = (pct * 100.0).min(100.0) as u32;
                            // Label with secret badge when applicable
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    theme::TEXT_MUTED,
                                    egui::RichText::new(format!(
                                        "{played_required} / {required_total} maps \u{00b7} {pct_display}%"
                                    ))
                                    .size(11.0),
                                );
                                if secret_total > 0 {
                                    let badge = egui::RichText::new(format!(
                                        "{played_secret}/{secret_total} secret"
                                    ))
                                    .size(9.0)
                                    .color(theme::TEXT_PRIMARY);
                                    let badge_resp = ui.add(
                                        egui::Label::new(badge)
                                            .selectable(false),
                                    );
                                    let badge_rect = badge_resp.rect.expand2(
                                        egui::vec2(4.0, 1.0),
                                    );
                                    ui.painter_at(badge_rect).rect_filled(
                                        badge_rect,
                                        3.0,
                                        theme::COLOR_SECRET_FILL,
                                    );
                                    // Re-draw text on top of the background
                                    ui.painter_at(badge_rect).text(
                                        badge_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        format!("{played_secret}/{secret_total} secret"),
                                        egui::FontId::proportional(9.0),
                                        theme::TEXT_PRIMARY,
                                    );
                                }
                            });
                        }
                    });
                });
            });
        });
    });

    action
}

// ---------------------------------------------------------------------------
// Section header (above grid/table)
// ---------------------------------------------------------------------------

fn render_section_header(ui: &mut egui::Ui, state: &mut AppState) {
    let margin = egui::Margin::symmetric(20, 0);
    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new(format!("ALL WADS \u{00b7} {}", state.wads.len()))
                    .size(13.0)
                    .strong(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // View toggle buttons
                let list_selected = state.view_layout == ViewLayout::List;
                let grid_selected = state.view_layout == ViewLayout::Grid;

                // List button
                let list_text = if list_selected {
                    egui::RichText::new("List")
                        .size(12.0)
                        .color(theme::TEXT_PRIMARY)
                } else {
                    egui::RichText::new("List")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY)
                };
                let list_btn = ui.add(
                    egui::Button::new(list_text)
                        .fill(if list_selected {
                            theme::BORDER_MED
                        } else {
                            theme::BG_LIGHT
                        })
                        .corner_radius(egui::CornerRadius {
                            nw: 0,
                            ne: 6,
                            se: 6,
                            sw: 0,
                        }),
                );
                if list_btn.clicked() {
                    state.view_layout = ViewLayout::List;
                }

                // Grid button
                let grid_text = if grid_selected {
                    egui::RichText::new("Grid")
                        .size(12.0)
                        .color(theme::TEXT_PRIMARY)
                } else {
                    egui::RichText::new("Grid")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY)
                };
                let grid_btn = ui.add(
                    egui::Button::new(grid_text)
                        .fill(if grid_selected {
                            theme::BORDER_MED
                        } else {
                            theme::BG_LIGHT
                        })
                        .corner_radius(egui::CornerRadius {
                            nw: 6,
                            ne: 0,
                            se: 0,
                            sw: 6,
                        }),
                );
                if grid_btn.clicked() {
                    state.view_layout = ViewLayout::Grid;
                }
            });
        });

        // Status filter pills
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // "All" pill
            let all_active = state.status_filters.is_empty();
            if theme::filter_pill(ui, "All", all_active, None, state.total_wad_count) {
                state.status_filters.clear();
                state.needs_reload = true;
            }

            for &status in caco_core::db::Status::ALL {
                let status_str = status.as_str();
                let is_active = state.status_filters.contains(status_str);
                let count = state.status_count(Some(status_str));
                let color = theme::status_color(status);
                if theme::filter_pill(
                    ui,
                    theme::status_display(status),
                    is_active,
                    Some(color),
                    count,
                ) {
                    if is_active {
                        state.status_filters.remove(status_str);
                    } else {
                        state.status_filters.insert(status_str.to_string());
                    }
                    state.needs_reload = true;
                }
            }
        });
    });
}

