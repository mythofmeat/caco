use std::sync::mpsc;

use caco_core::config;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenResult, SearchSource, SearchResultEntry};
use crate::screens::Screen;
use crate::widgets::import_pane::{self, ImportPaneState};
use crate::widgets::library_pane::{self, LibraryPaneState};

/// Tab definitions: (name, display, query_filter)
/// The third element is a complete query string (or None for All/Import).
const TABS: &[(&str, &str, Option<&str>)] = &[
    ("all",     "All",     None),
    ("inbox",   "Inbox",   Some("intent:inbox")),
    ("queued",  "Queued",  Some("intent:queued")),
    ("playing", "Playing", Some("play:started")),
    ("shelved", "Shelved", Some("intent:shelved")),
    ("dropped", "Dropped", Some("intent:dropped")),
    ("import",  "Import",  None),
];

/// Main screen with tabbed library views.
pub struct TabbedLibraryScreen {
    active_tab: usize,
    library_panes: Vec<LibraryPaneState>,
    import_pane: ImportPaneState,
    terminal_width: u16,
}

impl TabbedLibraryScreen {
    pub fn new(conn: &Connection) -> Self {
        let cfg = config::load_config();
        let sort_field = &cfg.tui.default_sort;
        let sort_desc = cfg.tui.default_sort_desc;

        // Determine initial tab from config
        let initial_tab = TABS
            .iter()
            .position(|(name, _, _)| *name == cfg.tui.default_tab)
            .unwrap_or(0);

        // Create library panes for tabs 0-5 (all non-import tabs)
        let mut library_panes = Vec::new();
        for (_, _, query_filter) in TABS.iter().take(6) {
            let tab_query = query_filter.map(|q| q.to_string());
            library_panes.push(LibraryPaneState::new(tab_query, sort_field, sort_desc));
        }

        // Create import pane (tab 6) with a dummy sender for now
        // The sender will be set by the App after construction
        let (tx, _rx) = mpsc::channel();
        let import_pane = ImportPaneState::new(tx);

        let mut screen = Self {
            active_tab: initial_tab,
            library_panes,
            import_pane,
            terminal_width: 0,
        };

        // Initial load for the active tab
        if screen.active_tab < 6 {
            screen.library_panes[screen.active_tab].reload(conn);
        }

        screen
    }

    /// Set the background sender for import operations.
    pub fn set_bg_sender(&mut self, tx: mpsc::Sender<AppMessage>) {
        self.import_pane = ImportPaneState::new(tx);
    }

    /// Process search results from background thread.
    pub fn on_search_complete(&mut self, source: SearchSource, results: Vec<SearchResultEntry>) {
        self.import_pane.on_search_complete(source, results);
    }

    fn is_import_tab(&self) -> bool {
        self.active_tab == 6
    }
}

impl Screen for TabbedLibraryScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        self.terminal_width = area.width;

        let layout = Layout::vertical([
            Constraint::Length(1), // tab bar
            Constraint::Min(1),   // content
        ])
        .split(area);

        // Tab bar
        let mut tab_spans: Vec<Span> = Vec::new();
        for (i, (_, display, _)) in TABS.iter().enumerate() {
            if i > 0 {
                tab_spans.push(Span::raw(" "));
            }
            let style = if i == self.active_tab {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            tab_spans.push(Span::styled(format!(" {display} "), style));
        }
        frame.render_widget(Paragraph::new(Line::from(tab_spans)), layout[0]);

        // Content
        if self.is_import_tab() {
            import_pane::render_import_pane(&mut self.import_pane, frame, layout[1]);
        } else if let Some(pane) = self.library_panes.get_mut(self.active_tab) {
            library_pane::render_library_pane(pane, frame, layout[1], self.terminal_width);
        }
    }

    fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> Option<AppMessage> {
        match (key.code, key.modifiers) {
            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => {
                // Only quit if not in filter mode or import text input
                if self.is_import_tab() {
                    // Let import pane handle it
                } else if let Some(pane) = self.library_panes.get(self.active_tab) {
                    if pane.filter.focused || pane.status_mode {
                        // Don't quit, let pane handle
                    } else {
                        return Some(AppMessage::Quit);
                    }
                }
            }
            // Tab switching
            (KeyCode::Tab, KeyModifiers::NONE) => {
                let old_tab = self.active_tab;
                self.active_tab = (self.active_tab + 1) % TABS.len();
                if self.active_tab < 6 && self.active_tab != old_tab {
                    self.library_panes[self.active_tab].reload(conn);
                }
                return None;
            }
            (KeyCode::BackTab, KeyModifiers::SHIFT) => {
                let old_tab = self.active_tab;
                if self.active_tab == 0 {
                    self.active_tab = TABS.len() - 1;
                } else {
                    self.active_tab -= 1;
                }
                if self.active_tab < 6 && self.active_tab != old_tab {
                    self.library_panes[self.active_tab].reload(conn);
                }
                return None;
            }
            _ => {}
        }

        // Route to active content
        if self.is_import_tab() {
            self.import_pane.handle_key(key, conn)
        } else if let Some(pane) = self.library_panes.get_mut(self.active_tab) {
            pane.handle_key(key, conn)
        } else {
            None
        }
    }

    fn tick(&mut self, conn: &Connection) -> Option<AppMessage> {
        if !self.is_import_tab() {
            if let Some(pane) = self.library_panes.get_mut(self.active_tab) {
                return pane.tick(conn);
            }
        }
        None
    }

    fn on_resize(&mut self, width: u16, _height: u16) {
        self.terminal_width = width;
    }

    fn on_resume(&mut self, conn: &Connection, _result: Option<ScreenResult>) {
        // Refresh all library panes
        for pane in &mut self.library_panes {
            pane.reload(conn);
        }
    }
}
