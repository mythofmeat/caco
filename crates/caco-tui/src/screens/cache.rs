use std::fs;

use crate::widgets::table_nav::{table_nav_next, table_nav_prev};
use caco_core::db::sessions::{clear_all_cached_paths, clear_cached_path, get_cached_wads};
use caco_core::utils::format_size;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenResult, Severity};
use crate::screens::Screen;
use crate::theme;

/// Cache entry with filesystem info.
struct CacheEntry {
    wad_id: i64,
    title: String,
    path: String,
    size: Option<u64>,
}

/// Cache management screen.
pub struct CacheScreen {
    entries: Vec<CacheEntry>,
    table_state: TableState,
    total_size: u64,
}

impl CacheScreen {
    pub fn new(conn: &Connection) -> Self {
        let mut screen = Self {
            entries: Vec::new(),
            table_state: TableState::default(),
            total_size: 0,
        };
        screen.load(conn);
        screen
    }

    fn load(&mut self, conn: &Connection) {
        let wads = get_cached_wads(conn).unwrap_or_default();
        self.total_size = 0;
        self.entries = wads
            .into_iter()
            .filter_map(|w| {
                let path = w.cached_path?;
                let size = fs::metadata(&path).ok().map(|m| m.len());
                if let Some(s) = size {
                    self.total_size += s;
                }
                Some(CacheEntry {
                    wad_id: w.id,
                    title: w.title,
                    path,
                    size,
                })
            })
            .collect();

        if !self.entries.is_empty() {
            let current = self.table_state.selected().unwrap_or(0);
            if current >= self.entries.len() {
                self.table_state.select(Some(self.entries.len() - 1));
            } else {
                self.table_state.select(Some(current));
            }
        } else {
            self.table_state.select(None);
        }
    }
}

impl Screen for CacheScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_style())
            .title(format!(
                " Cache — {} files, {} ",
                self.entries.len(),
                format_size(self.total_size)
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

        if self.entries.is_empty() {
            frame.render_widget(
                Paragraph::new("Cache is empty").style(theme::dim_style()),
                layout[0],
            );
        } else {
            let header = Row::new(vec![
                Cell::from("ID").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Path").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Size").style(Style::default().add_modifier(Modifier::BOLD)),
            ])
            .height(1);

            let rows: Vec<Row> = self
                .entries
                .iter()
                .map(|e| {
                    let size_str = match e.size {
                        Some(s) => format_size(s),
                        None => "missing".to_string(),
                    };
                    let size_style = if e.size.is_none() {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default()
                    };
                    Row::new(vec![
                        Cell::from(e.wad_id.to_string()),
                        Cell::from(e.title.clone()),
                        Cell::from(e.path.clone()),
                        Cell::from(size_str).style(size_style),
                    ])
                })
                .collect();

            let widths = [
                Constraint::Length(5),
                Constraint::Min(20),
                Constraint::Min(30),
                Constraint::Length(10),
            ];

            let table = Table::new(rows, widths)
                .header(header)
                .row_highlight_style(theme::highlight_style());
            frame.render_stateful_widget(table, layout[0], &mut self.table_state);
        }

        // Key hints
        let hints = Line::from(vec![
            Span::styled("q/Esc", theme::key_style()),
            Span::styled(" back  ", theme::desc_style()),
            Span::styled("d", theme::key_style()),
            Span::styled(" delete  ", theme::desc_style()),
            Span::styled("D", theme::key_style()),
            Span::styled(" delete all  ", theme::desc_style()),
            Span::styled("j/k", theme::key_style()),
            Span::styled(" navigate", theme::desc_style()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[1]);
    }

    fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> Option<AppMessage> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                Some(AppMessage::PopScreen(ScreenResult::Cancelled))
            }
            KeyCode::Char('j') | KeyCode::Down => {
                table_nav_next(&mut self.table_state, self.entries.len());
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                table_nav_prev(&mut self.table_state, self.entries.len());
                None
            }
            KeyCode::Char('d') => {
                // Delete selected cached file
                if let Some(idx) = self.table_state.selected() {
                    if let Some(entry) = self.entries.get(idx) {
                        let _ = fs::remove_file(&entry.path);
                        let _ = clear_cached_path(conn, entry.wad_id);
                        self.load(conn);
                        return Some(AppMessage::Notify(
                            "Cache entry cleared".to_string(),
                            Severity::Info,
                        ));
                    }
                }
                None
            }
            KeyCode::Char('D') => {
                // Delete all cached files
                for entry in &self.entries {
                    let _ = fs::remove_file(&entry.path);
                }
                let _ = clear_all_cached_paths(conn);
                self.load(conn);
                Some(AppMessage::Notify(
                    "All cache entries cleared".to_string(),
                    Severity::Info,
                ))
            }
            _ => None,
        }
    }
}
