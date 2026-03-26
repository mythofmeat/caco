use caco_core::db::sessions::{SessionRecord, get_sessions};
use caco_core::db::wads::get_wad;
use caco_core::player::format_duration;
use caco_core::wad_stats;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenResult};
use crate::screens::Screen;
use crate::theme;

/// Session history screen.
pub struct SessionsScreen {
    #[allow(dead_code)]
    wad_id: i64,
    wad_title: String,
    sessions: Vec<SessionRecord>,
    table_state: TableState,
}

impl SessionsScreen {
    pub fn new(wad_id: i64, conn: &Connection) -> Self {
        let wad_title = get_wad(conn, wad_id, true)
            .ok()
            .flatten()
            .map(|w| w.title)
            .unwrap_or_else(|| format!("WAD #{wad_id}"));

        let sessions = get_sessions(conn, wad_id).unwrap_or_default();
        let mut table_state = TableState::default();
        if !sessions.is_empty() {
            table_state.select(Some(0));
        }

        Self {
            wad_id,
            wad_title,
            sessions,
            table_state,
        }
    }
}

impl Screen for SessionsScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_style())
            .title(format!(" Sessions — {} ", self.wad_title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

        if self.sessions.is_empty() {
            frame.render_widget(
                Paragraph::new("No sessions recorded").style(theme::dim_style()),
                layout[0],
            );
        } else {
            // Table
            let header = Row::new(vec![
                Cell::from("Date").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Time").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Duration").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Sourceport").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Maps").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
            ])
            .height(1);

            let rows: Vec<Row> = self
                .sessions
                .iter()
                .map(|s| {
                    // Parse date/time from started_at
                    let (date, time) = if let Some(idx) = s.started_at.find('T') {
                        (
                            s.started_at[..idx].to_string(),
                            s.started_at[idx + 1..].split('.').next().unwrap_or("").to_string(),
                        )
                    } else if let Some(idx) = s.started_at.find(' ') {
                        (
                            s.started_at[..idx].to_string(),
                            s.started_at[idx + 1..].to_string(),
                        )
                    } else {
                        (s.started_at.clone(), String::new())
                    };

                    let duration = s
                        .duration_seconds
                        .map(format_duration)
                        .unwrap_or_else(|| "—".to_string());

                    let sourceport = s.sourceport.clone().unwrap_or_default();

                    // Compute maps played from stats_before/stats_after
                    let maps = match (&s.stats_before, &s.stats_after) {
                        (_, Some(after)) => {
                            let before = s.stats_before.as_deref()
                                .and_then(|s| wad_stats::stats_from_json(s).ok());
                            let after = wad_stats::stats_from_json(after).ok();
                            match (before.as_ref(), after) {
                                (_, Some(after_stats)) => {
                                    let delta = wad_stats::compute_stats_delta(
                                        before.as_ref(),
                                        &after_stats,
                                    );
                                    if delta.maps_played.is_empty() {
                                        "—".to_string()
                                    } else {
                                        let names = &delta.maps_played;
                                        if names.len() > 3 {
                                            format!(
                                                "{}, ... (+{})",
                                                names[..3].join(", "),
                                                names.len() - 3
                                            )
                                        } else {
                                            names.join(", ")
                                        }
                                    }
                                }
                                _ => "—".to_string(),
                            }
                        }
                        _ => "—".to_string(),
                    };

                    // Status: crash indicator
                    let (status_text, status_style) = match s.exit_code {
                        Some(code) if code != 0 => (
                            format!("Crash ({code})"),
                            Style::default().fg(Color::Red),
                        ),
                        Some(0) => ("OK".to_string(), Style::default().fg(Color::Green)),
                        _ => ("—".to_string(), theme::dim_style()),
                    };

                    Row::new(vec![
                        Cell::from(date),
                        Cell::from(time),
                        Cell::from(duration),
                        Cell::from(sourceport),
                        Cell::from(maps),
                        Cell::from(status_text).style(status_style),
                    ])
                })
                .collect();

            let widths = [
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(16),
                Constraint::Min(15),
                Constraint::Length(12),
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
            Span::styled("j/k", theme::key_style()),
            Span::styled(" navigate", theme::desc_style()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[1]);
    }

    fn handle_key(&mut self, key: KeyEvent, _conn: &Connection) -> Option<AppMessage> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                Some(AppMessage::PopScreen(ScreenResult::Cancelled))
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.sessions.is_empty() {
                    let i = match self.table_state.selected() {
                        Some(i) => (i + 1).min(self.sessions.len() - 1),
                        None => 0,
                    };
                    self.table_state.select(Some(i));
                }
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !self.sessions.is_empty() {
                    let i = match self.table_state.selected() {
                        Some(i) => i.saturating_sub(1),
                        None => 0,
                    };
                    self.table_state.select(Some(i));
                }
                None
            }
            _ => None,
        }
    }
}
