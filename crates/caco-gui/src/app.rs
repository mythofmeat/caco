use std::path::PathBuf;

use rusqlite::Connection;

use crate::dialogs::cache::{CacheDialogState, CacheResult};
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
            ActionRequest::MapStats(wad_id) => {
                if let Some(dialog) = WadStatsDialogState::new(&self.conn, wad_id) {
                    self.state.active_dialog = Some(ActiveDialog::WadStats(dialog));
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
                    let outcome = (|| -> Result<caco_core::player::PlayResult, String> {
                        let conn = caco_core::db::open_connection(&db_path)
                            .map_err(|e| format!("DB open failed: {e}"))?;
                        caco_core::player::play(
                            &conn,
                            wad_id,
                            &caco_core::player::PlayOptions::default(),
                        )
                        .map_err(|e| format!("{e}"))
                    })();

                    sender.send(AppMessage::PlayFinished { wad_id, outcome });
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
                AppMessage::PlayFinished { wad_id, outcome } => {
                    self.state.play_state = PlayState::Idle;
                    self.state.needs_reload = true;

                    match outcome {
                        Err(err) => {
                            // Show link dialog for missing WAD files
                            if err.starts_with("file not found:") {
                                if let Some(dialog) =
                                    LinkDialogState::new(&self.conn, wad_id)
                                {
                                    self.state.active_dialog =
                                        Some(ActiveDialog::Link(dialog));
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
                                self.state.notification =
                                    Some(Notification::warning(format!(
                                        "Sourceport crashed (exit code {})",
                                        pr.exit_code.unwrap_or(-1)
                                    )));
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
                    self.state.import.search_state_mut(source).set_results(results);
                }
                AppMessage::ThumbnailReady {
                    wad_id,
                    width,
                    height,
                    pixels,
                } => {
                    self.thumbnails.on_ready(ctx, wad_id, width, height, &pixels);
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
                                let title = ir
                                    .duplicate_title
                                    .unwrap_or_else(|| "unknown".to_string());
                                let id = ir.duplicate_id.unwrap_or(0);
                                self.state.notification = Some(Notification::warning(
                                    format!("Already imported as {title} (#{id})"),
                                ));
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
                ActiveDialog::Delete(delete_state) => {
                    match delete_state.render(ctx, &self.conn) {
                        DeleteResult::Confirmed => {
                            close_dialog = true;
                            self.state.needs_reload = true;
                            self.state.notification =
                                Some(Notification::info("WAD deleted".to_string()));
                        }
                        DeleteResult::Cancelled => {
                            close_dialog = true;
                        }
                        DeleteResult::Open => {}
                    }
                }
                ActiveDialog::Sessions(sessions_state) => {
                    match sessions_state.render(ctx) {
                        SessionsResult::Closed => {
                            close_dialog = true;
                        }
                        SessionsResult::Open => {}
                    }
                }
                ActiveDialog::Stats(stats_state) => {
                    match stats_state.render(ctx) {
                        StatsResult::Closed => {
                            close_dialog = true;
                        }
                        StatsResult::Open => {}
                    }
                }
                ActiveDialog::Cache(cache_state) => {
                    match cache_state.render(ctx, &self.conn) {
                        CacheResult::Closed => {
                            close_dialog = true;
                        }
                        CacheResult::Open => {}
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
                ActiveDialog::Link(link_state) => {
                    match link_state.render(ctx, &self.conn) {
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
                    }
                }
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
            // Check if dialog was modified → trigger reload
            let was_modified = match &self.state.active_dialog {
                Some(ActiveDialog::Cache(s)) => s.modified,
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

        // Top panel: menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Import").clicked() {
                        self.state.view_mode = ViewMode::Import;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit                       Ctrl+Q").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("View", |ui| {
                    let list_label = if self.state.view_layout == ViewLayout::List {
                        "\u{2713} List View"
                    } else {
                        "  List View"
                    };
                    if ui.button(list_label).clicked() {
                        self.state.view_layout = ViewLayout::List;
                        ui.close_menu();
                    }

                    let grid_label = if self.state.view_layout == ViewLayout::Grid {
                        "\u{2713} Grid View"
                    } else {
                        "  Grid View"
                    };
                    if ui.button(grid_label).clicked() {
                        self.state.view_layout = ViewLayout::Grid;
                        ui.close_menu();
                    }

                    ui.separator();

                    let detail_label = if self.state.show_detail_panel {
                        "\u{2713} Detail Panel"
                    } else {
                        "  Detail Panel"
                    };
                    if ui.button(detail_label).clicked() {
                        self.state.show_detail_panel = !self.state.show_detail_panel;
                        ui.close_menu();
                    }

                    ui.separator();

                    if ui.button("Library Stats").clicked() {
                        actions.push(ActionRequest::Stats);
                        ui.close_menu();
                    }
                    if ui.button("Resources").clicked() {
                        actions.push(ActionRequest::Resources);
                        ui.close_menu();
                    }
                    if ui.button("Cache Management").clicked() {
                        actions.push(ActionRequest::Cache);
                        ui.close_menu();
                    }

                    ui.separator();

                    if ui.button("Refresh Library          F5").clicked() {
                        self.state.needs_reload = true;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Keyboard Shortcuts").clicked() {
                        self.state.active_dialog = Some(ActiveDialog::Help);
                        ui.close_menu();
                    }
                    if ui.button("About").clicked() {
                        self.state.active_dialog = Some(ActiveDialog::About);
                        ui.close_menu();
                    }
                });
            });
        });

        // Tab bar
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            panels::library::render_tab_bar(ui, &mut self.state);
            ui.add_space(2.0);
        });

        // Bottom panel: status bar
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            render_status_bar(ui, &mut self.state);
            ui.add_space(2.0);
        });

        // Detail panel as a top-level SidePanel (must come before CentralPanel
        // so egui properly partitions the space).
        if self.state.show_detail_panel && self.state.view_mode == ViewMode::Library {
            let mut detail_action = None;
            egui::SidePanel::right("detail_panel")
                .default_width(300.0)
                .min_width(200.0)
                .max_width(500.0)
                .resizable(true)
                .show(ctx, |ui| {
                    detail_action =
                        panels::detail::render(ui, &self.state, &self.conn, Some(&self.thumbnails));
                });
            if let Some(a) = detail_action {
                actions.push(a);
            }
        }

        // Central panel: Library view or Import view based on ViewMode
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.state.view_mode {
                ViewMode::Library => {
                    // Toolbar row
                    panels::library::render_toolbar(ui, &mut self.state);
                    ui.separator();

                    // WAD table or grid fills remaining space
                    let view_action = match self.state.view_layout {
                        ViewLayout::List => panels::wad_table::render(ui, &mut self.state),
                        ViewLayout::Grid => panels::wad_grid::render(
                            ui,
                            &mut self.state,
                            Some(&self.thumbnails),
                        ),
                    };
                    if let Some(a) = view_action {
                        actions.push(a);
                    }
                }
                ViewMode::Import => {
                    if let Some(import_action) =
                        import::render(ui, &mut self.state.import)
                    {
                        self.dispatch_import_action(import_action);
                    }
                }
            }
        });

        // 7. Request thumbnails for visible WADs (grid mode) and selected WAD (detail panel)
        if self.state.view_mode == ViewMode::Library {
            let sender = self.bg.sender();

            // Request thumbnail for selected WAD when detail panel is visible
            if self.state.show_detail_panel
                && let Some(wad) = self.state.selected_wad()
                && self.thumbnails.needs_request(wad.id)
            {
                let path = wad.cached_path.as_deref().map(std::path::Path::new);
                let hint = ThumbnailHint {
                    source_type: wad.source_type.clone(),
                    source_url: wad.source_url.clone(),
                    title: wad.title.clone(),
                };
                self.thumbnails.request(wad.id, path, &hint, &sender);
            }

            // Request thumbnails for all visible WADs in grid mode
            if self.state.view_layout == ViewLayout::Grid
                && self.state.wads.iter().any(|w| self.thumbnails.needs_request(w.id))
            {
                for wad in &self.state.wads {
                    if self.thumbnails.needs_request(wad.id) {
                        let path = wad.cached_path.as_deref().map(std::path::Path::new);
                        let hint = ThumbnailHint {
                            source_type: wad.source_type.clone(),
                            source_url: wad.source_url.clone(),
                            title: wad.title.clone(),
                        };
                        self.thumbnails.request(wad.id, path, &hint, &sender);
                    }
                }
            }
        }

        // 8. Dispatch action requests (only first one — avoid double-triggering)
        if let Some(action) = actions.into_iter().next() {
            self.dispatch_action(action);
        }
    }
}

/// Render the status bar (enhanced with play state).
fn render_status_bar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // Play state indicator
        if let PlayState::Playing { wad_title, .. } = &state.play_state {
            ui.colored_label(
                theme::COLOR_SUCCESS,
                format!("Playing: {wad_title}..."),
            );
            ui.separator();
        }

        // Notification
        if let Some(notif) = &state.notification {
            if notif.is_expired() {
                state.notification = None;
            } else {
                ui.colored_label(theme::severity_color(notif.severity), &notif.text);
            }
        }

        // Right-aligned hints
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hints = if state.view_mode == ViewMode::Import {
                "1-5: switch source"
            } else {
                "\u{2191}\u{2193}/jk: nav  Enter: play  e: edit  d: del  s: sess  Ctrl+F: find  Esc: clear"
            };
            ui.colored_label(theme::TEXT_SECONDARY, hints);
        });
    });
}

/// Render the keyboard shortcuts help dialog. Returns true when closed.
fn render_help_dialog(ctx: &egui::Context) -> bool {
    let mut closed = false;
    egui::Window::new("Keyboard Shortcuts")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(400.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 30.0)
                .show(ui, |ui| {
                    shortcut_section(ui, "Library Navigation", &[
                        ("\u{2191}\u{2193} / j k", "Navigate up/down"),
                        ("\u{2190}\u{2192} / h l", "Navigate left/right (grid)"),
                        ("Home / g g", "Jump to first WAD"),
                        ("End / Shift+G", "Jump to last WAD"),
                        ("Ctrl+F", "Focus search/filter"),
                    ]);
                    shortcut_section(ui, "Library Actions", &[
                        ("Enter / p", "Play selected WAD"),
                        ("e", "Edit selected WAD"),
                        ("d", "Delete selected WAD"),
                        ("s", "View sessions"),
                        ("m", "Map stats"),
                    ]);
                    shortcut_section(ui, "Import", &[
                        ("1\u{2013}5", "Switch source"),
                    ]);
                    shortcut_section(ui, "Dialogs", &[
                        ("Escape", "Close / cancel"),
                        ("Enter", "Confirm / default action"),
                    ]);
                    shortcut_section(ui, "Global", &[
                        ("Ctrl+Q", "Quit"),
                        ("F5", "Refresh library"),
                    ]);
                });
            ui.add_space(8.0);
            if ui.button("Close").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                closed = true;
            }
        });
    closed
}

/// Render a shortcut section with a heading and key/description grid.
fn shortcut_section(ui: &mut egui::Ui, title: &str, shortcuts: &[(&str, &str)]) {
    ui.add_space(4.0);
    ui.strong(title);
    ui.add_space(2.0);
    egui::Grid::new(title)
        .num_columns(2)
        .spacing([40.0, 4.0])
        .show(ui, |ui| {
            for (key, desc) in shortcuts {
                ui.label(egui::RichText::new(*key).monospace());
                ui.label(*desc);
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.separator();
}

/// Render the About dialog. Returns true when closed.
fn render_about_dialog(ctx: &egui::Context) -> bool {
    let mut closed = false;
    egui::Window::new("About Caco")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.heading("Caco");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(4.0);
            ui.label("A Doom WAD library manager");
            ui.add_space(8.0);
            if ui.button("Close").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                closed = true;
            }
        });
    closed
}
