use std::time::Instant;

use caco_core::db;
use caco_core::db::models::Status;
use caco_core::db::sessions;
use caco_core::db::wads;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Frame;
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenId, Severity};
use crate::widgets::filter_input::{self, FilterInputState};
use crate::widgets::sort_select::{self, SortSelectState};
use crate::widgets::status_bar;
use crate::widgets::wad_info::{self, WadInfoState};
use crate::widgets::wad_table::{self, WadTableState};

const GG_TIMEOUT_MS: u128 = 500;

/// State for the library pane — composes all sub-widget states.
pub struct LibraryPaneState {
    pub table: WadTableState,
    pub filter: FilterInputState,
    pub sort: SortSelectState,
    pub info: WadInfoState,
    /// Tab query filter (e.g. "intent:inbox", "play:started"), or None for "All".
    pub tab_query: Option<String>,
    pub status_mode: bool,
    pub show_panel: bool,
    pub show_trash: bool,
}

impl LibraryPaneState {
    pub fn new(
        tab_query: Option<String>,
        sort_field: &str,
        sort_desc: bool,
    ) -> Self {
        Self {
            table: WadTableState::new(),
            filter: FilterInputState::new(),
            sort: SortSelectState::from_config(sort_field, sort_desc),
            info: WadInfoState::new(),
            tab_query,
            status_mode: false,
            show_panel: true,
            show_trash: false,
        }
    }

    /// Build the effective query string combining tab query filter and user query.
    fn effective_query(&self) -> Option<String> {
        let mut parts = Vec::new();

        if let Some(ref tab_q) = self.tab_query {
            parts.push(tab_q.clone());
        }

        let user_query = self.filter.query();
        if !user_query.is_empty() {
            parts.push(user_query.to_string());
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }

    /// Reload WADs from the database.
    pub fn reload(&mut self, conn: &Connection) {
        let query = self.effective_query();
        let count = self.table.load_wads(
            conn,
            query.as_deref(),
            Some(self.sort.field()),
            self.sort.sort_desc,
            self.show_trash,
        );
        self.filter.wad_count = count;
    }

    /// Handle key events. Returns `Some(AppMessage)` for actions that need app-level handling.
    pub fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> Option<AppMessage> {
        // Status mode captures single key
        if self.status_mode {
            return self.handle_status_mode_key(key, conn);
        }

        // Filter captures all keys when focused
        if self.filter.focused {
            if let Some(_query) = self.filter.handle_key(key) {
                self.reload(conn);
            }
            return None;
        }

        match (key.code, key.modifiers) {
            // Filter
            (KeyCode::Char('/') | KeyCode::Char('f'), KeyModifiers::NONE) => {
                self.filter.focused = true;
                None
            }

            // Vim navigation
            (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                self.table.next();
                None
            }
            (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                self.table.previous();
                None
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                // gg = go to top
                if let Some(first_g) = self.table.g_pressed {
                    if first_g.elapsed().as_millis() < GG_TIMEOUT_MS {
                        self.table.first();
                        self.table.g_pressed = None;
                    } else {
                        self.table.g_pressed = Some(Instant::now());
                    }
                } else {
                    self.table.g_pressed = Some(Instant::now());
                }
                None
            }
            (KeyCode::Char('G'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                self.table.last();
                None
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.table.page_down(20);
                None
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.table.page_up(20);
                None
            }

            // Actions
            (KeyCode::Char('i'), KeyModifiers::NONE) => {
                if let Some(id) = self.table.selected_wad_id() {
                    Some(AppMessage::PushScreen(ScreenId::WadDetail(id)))
                } else {
                    None
                }
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) => {
                if let Some(id) = self.table.selected_wad_id() {
                    Some(AppMessage::PushScreen(ScreenId::Sessions(id)))
                } else {
                    None
                }
            }
            (KeyCode::Char('e'), KeyModifiers::NONE) => {
                if let Some(id) = self.table.selected_wad_id() {
                    Some(AppMessage::PushScreen(ScreenId::WadEdit(id)))
                } else {
                    None
                }
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                if let Some(id) = self.table.selected_wad_id() {
                    Some(AppMessage::PushScreen(ScreenId::ConfirmDelete(id)))
                } else {
                    None
                }
            }

            // Status mode
            (KeyCode::Char('s'), KeyModifiers::NONE) => {
                if self.table.selected_wad_id().is_some() {
                    self.status_mode = true;
                }
                None
            }

            // Sort
            (KeyCode::Char('o'), KeyModifiers::NONE) => {
                self.sort.cycle();
                self.reload(conn);
                None
            }
            (KeyCode::Char('O'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                self.sort.toggle_direction();
                self.reload(conn);
                None
            }

            // Rating
            (KeyCode::Char('r'), KeyModifiers::NONE) => {
                self.cycle_rating(conn);
                None
            }
            (KeyCode::Char('R'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                self.clear_rating(conn);
                None
            }

            // Beaten
            (KeyCode::Char('+') | KeyCode::Char('='), _) => {
                self.add_beaten(conn);
                None
            }
            (KeyCode::Char('-'), KeyModifiers::NONE) => {
                self.remove_beaten(conn);
                None
            }

            // Map stats
            (KeyCode::Char('M'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                if let Some(id) = self.table.selected_wad_id() {
                    Some(AppMessage::PushScreen(ScreenId::WadStats(id)))
                } else {
                    None
                }
            }

            // Trash
            (KeyCode::Char('T'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                self.show_trash = !self.show_trash;
                self.reload(conn);
                None
            }
            (KeyCode::Char('u'), KeyModifiers::NONE) => {
                if self.show_trash {
                    if let Some(id) = self.table.selected_wad_id() {
                        let _ = wads::restore_wad(conn, id);
                        self.reload(conn);
                        return Some(AppMessage::Notify(
                            format!("WAD #{id} restored"),
                            Severity::Info,
                        ));
                    }
                }
                None
            }

            // Screens
            (KeyCode::Char('S'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                Some(AppMessage::PushScreen(ScreenId::Stats))
            }
            (KeyCode::Char('C'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                Some(AppMessage::PushScreen(ScreenId::Cache))
            }
            (KeyCode::Char('W'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                Some(AppMessage::PushScreen(ScreenId::Resources))
            }

            // Panel toggle
            (KeyCode::Char('P'), KeyModifiers::SHIFT | KeyModifiers::NONE) => {
                self.show_panel = !self.show_panel;
                None
            }

            // Play
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if let Some(id) = self.table.selected_wad_id() {
                    Some(AppMessage::PlayWad(id))
                } else {
                    None
                }
            }

            _ => None,
        }
    }

    fn handle_status_mode_key(
        &mut self,
        key: KeyEvent,
        conn: &Connection,
    ) -> Option<AppMessage> {
        self.status_mode = false;

        let status = match key.code {
            KeyCode::Char('p') => "playing",
            KeyCode::Char('f') => "finished",
            KeyCode::Char('t') => "to-play",
            KeyCode::Char('b') => "backlog",
            KeyCode::Char('a') => "abandoned",
            KeyCode::Char('w') => "awaiting-update",
            KeyCode::Esc => return None,
            _ => return None,
        };

        if let Some(id) = self.table.selected_wad_id() {
            if let Ok(update) = db::wads::WadUpdate::new()
                .set_status(Status::parse(status).unwrap_or(Status::ToPlay))
            {
                let _ = wads::update_wad(conn, id, &update);
                self.table.update_row(conn, id);
                return Some(AppMessage::Notify(
                    format!("Status → {}", crate::theme::status_display(status)),
                    Severity::Info,
                ));
            }
        }
        None
    }

    fn cycle_rating(&mut self, conn: &Connection) {
        if let Some(wad) = self.table.selected_wad() {
            let current = wad.rating.unwrap_or(0);
            let new_rating = if current >= 5 { 0 } else { current + 1 };
            let value = if new_rating == 0 {
                None
            } else {
                Some(new_rating as i64)
            };
            if let Ok(update) = db::wads::WadUpdate::new().set_int("rating", value) {
                let _ = wads::update_wad(conn, wad.id, &update);
                self.table.update_row(conn, wad.id);
            }
        }
    }

    fn clear_rating(&mut self, conn: &Connection) {
        if let Some(wad) = self.table.selected_wad() {
            if let Ok(update) = db::wads::WadUpdate::new().set_int("rating", None) {
                let _ = wads::update_wad(conn, wad.id, &update);
                self.table.update_row(conn, wad.id);
            }
        }
    }

    fn add_beaten(&mut self, conn: &Connection) {
        if let Some(id) = self.table.selected_wad_id() {
            let _ = sessions::add_wad_completion(conn, id, None, None, None);
            self.table.update_row(conn, id);
        }
    }

    fn remove_beaten(&mut self, conn: &Connection) {
        if let Some(id) = self.table.selected_wad_id() {
            if let Ok(completions) = sessions::get_wad_completions(conn, id) {
                if let Some(last) = completions.last() {
                    let _ = sessions::delete_wad_completion(conn, last.id);
                    self.table.update_row(conn, id);
                }
            }
        }
    }

    /// Called on tick to check filter debounce.
    pub fn tick(&mut self, conn: &Connection) -> Option<AppMessage> {
        if let Some(_query) = self.filter.tick() {
            self.reload(conn);
        }
        None
    }
}

/// Render the library pane.
pub fn render_library_pane(
    state: &mut LibraryPaneState,
    frame: &mut Frame,
    area: Rect,
    terminal_width: u16,
) {
    // Layout: header (filter+sort) | content | status bar
    let show_status_mode = state.status_mode;
    let bottom_height = if show_status_mode { 1 } else { 1 };

    let layout = Layout::vertical([
        Constraint::Length(1), // filter + sort
        Constraint::Min(1),   // content
        Constraint::Length(bottom_height),  // status/hints
    ])
    .split(area);

    let header_area = layout[0];
    let content_area = layout[1];
    let bottom_area = layout[2];

    // Header: filter (left) + sort (right)
    let header_layout = Layout::horizontal([
        Constraint::Min(20),
        Constraint::Length(25),
    ])
    .split(header_area);

    filter_input::render_filter_input(&state.filter, frame, header_layout[0]);
    sort_select::render_sort_select(&state.sort, frame, header_layout[1]);

    // Content: table (left) + info panel (right)
    let show_panel = state.show_panel && terminal_width >= 100;
    if show_panel {
        let content_layout = Layout::horizontal([
            Constraint::Percentage(65),
            Constraint::Percentage(35),
        ])
        .split(content_area);

        wad_table::render_wad_table(&mut state.table, frame, content_layout[0]);

        let wad = state.table.selected_wad();
        let stats = state.table.selected_stats();
        wad_info::render_wad_info(&mut state.info, wad, stats, frame, content_layout[1]);
    } else {
        wad_table::render_wad_table(&mut state.table, frame, content_area);
    }

    // Bottom: status mode bar or key hints
    if show_status_mode {
        status_bar::render_status_mode_bar(frame, bottom_area);
    } else {
        let hints = if state.show_trash {
            vec![
                ("j/k", "nav"),
                ("u", "restore"),
                ("T", "exit trash"),
                ("q", "quit"),
            ]
        } else {
            vec![
                ("j/k", "nav"),
                ("/", "filter"),
                ("i", "info"),
                ("e", "edit"),
                ("s", "status"),
                ("Enter", "play"),
                ("q", "quit"),
            ]
        };
        status_bar::render_status_bar(&hints, frame, bottom_area);
    }
}
