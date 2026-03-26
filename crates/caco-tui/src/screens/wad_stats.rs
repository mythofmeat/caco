use caco_core::db::sessions::get_wad_completions;
use caco_core::db::wads::get_wad;
use caco_core::wad_stats::{
    self, WadStats, format_time_secs, format_time_tics, skill_name,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState,
};
use ratatui::Frame;
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenResult};
use crate::screens::Screen;
use crate::theme;

/// A stats entry (either live or from a completion).
struct StatsEntry {
    label: String,
    stats: Option<WadStats>,
}

/// Per-map stats screen with completion switching.
pub struct WadStatsScreen {
    #[allow(dead_code)]
    wad_id: i64,
    wad_title: String,
    entries: Vec<StatsEntry>,
    current_entry: usize,
    table_state: TableState,
}

impl WadStatsScreen {
    pub fn new(wad_id: i64, conn: &Connection) -> Self {
        let wad_title = get_wad(conn, wad_id, true)
            .ok()
            .flatten()
            .map(|w| w.title.clone())
            .unwrap_or_else(|| format!("WAD #{wad_id}"));

        let mut entries = Vec::new();

        // Live stats from stats_snapshot
        if let Ok(Some(wad)) = get_wad(conn, wad_id, true) {
            if let Some(ref snapshot) = wad.stats_snapshot {
                if let Ok(stats) = wad_stats::stats_from_json(snapshot) {
                    entries.push(StatsEntry {
                        label: "Current (live)".to_string(),
                        stats: Some(stats),
                    });
                }
            }
        }

        // Completion entries
        if let Ok(completions) = get_wad_completions(conn, wad_id) {
            for (i, comp) in completions.iter().enumerate() {
                let stats = comp
                    .stats_snapshot
                    .as_ref()
                    .and_then(|s| wad_stats::stats_from_json(s).ok());
                let date = comp.completed_at.split('T').next().unwrap_or(&comp.completed_at);
                let date = date.split(' ').next().unwrap_or(date);
                entries.push(StatsEntry {
                    label: format!("#{} ({})", i + 1, date),
                    stats,
                });
            }
        }

        let mut table_state = TableState::default();
        if let Some(first) = entries.first() {
            if first.stats.as_ref().is_some_and(|s| !s.maps.is_empty()) {
                table_state.select(Some(0));
            }
        }

        Self {
            wad_id,
            wad_title,
            entries,
            current_entry: 0,
            table_state,
        }
    }

    fn current_stats(&self) -> Option<&WadStats> {
        self.entries
            .get(self.current_entry)
            .and_then(|e| e.stats.as_ref())
    }
}

impl Screen for WadStatsScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_style())
            .title(format!(" Map Stats — {} ", self.wad_title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Min(1),   // table
            Constraint::Length(1), // hints
        ])
        .split(inner);

        // Header: entry label + navigation hint
        if !self.entries.is_empty() {
            let label = &self.entries[self.current_entry].label;
            let nav = format!(
                "({}/{}, n/p to switch)",
                self.current_entry + 1,
                self.entries.len()
            );
            let header = Line::from(vec![
                Span::styled(label.as_str(), theme::title_style()),
                Span::raw("  "),
                Span::styled(nav, theme::dim_style()),
            ]);
            frame.render_widget(Paragraph::new(header), layout[0]);
        }

        // Table — clone stats to avoid borrow conflict with table_state
        let stats = self
            .entries
            .get(self.current_entry)
            .and_then(|e| e.stats.clone());

        let Some(stats) = stats else {
            frame.render_widget(
                Paragraph::new("No stats data").style(theme::dim_style()),
                layout[1],
            );
            render_hints(frame, layout[2]);
            return;
        };

        let is_stats_txt = stats.format == "stats_txt";

        if is_stats_txt {
            let header = Row::new(vec![
                Cell::from("Map").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Skill").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Time").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Max").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("NM").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Exits").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("K").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("I").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("S").style(Style::default().add_modifier(Modifier::BOLD)),
            ])
            .height(1);

            let rows: Vec<Row> = stats
                .maps
                .iter()
                .filter(|m| m.played())
                .map(|m| {
                    Row::new(vec![
                        Cell::from(m.lump.clone()),
                        Cell::from(skill_name(m.best_skill).to_string()),
                        Cell::from(format_time_tics(m.best_time)),
                        Cell::from(format_time_tics(m.best_max_time)),
                        Cell::from(format_time_tics(m.best_nm_time)),
                        Cell::from(m.total_exits.to_string()),
                        Cell::from(format_ratio(m.kills, m.total_kills)),
                        Cell::from(format_ratio(m.items, m.total_items)),
                        Cell::from(format_ratio(m.secrets, m.total_secrets)),
                    ])
                })
                .collect();

            let widths = [
                Constraint::Length(10),
                Constraint::Length(6),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(6),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(8),
            ];

            let table = Table::new(rows, widths)
                .header(header)
                .row_highlight_style(theme::highlight_style());
            frame.render_stateful_widget(table, layout[1], &mut self.table_state);
        } else {
            let header = Row::new(vec![
                Cell::from("Map").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Time").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Total").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("K").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("I").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("S").style(Style::default().add_modifier(Modifier::BOLD)),
            ])
            .height(1);

            let rows: Vec<Row> = stats
                .maps
                .iter()
                .filter(|m| m.played())
                .map(|m| {
                    Row::new(vec![
                        Cell::from(m.lump.clone()),
                        Cell::from(format_time_secs(m.time_secs)),
                        Cell::from(format_time_secs(m.total_time_secs)),
                        Cell::from(format_ratio(m.kills, m.total_kills)),
                        Cell::from(format_ratio(m.items, m.total_items)),
                        Cell::from(format_ratio(m.secrets, m.total_secrets)),
                    ])
                })
                .collect();

            let widths = [
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(8),
            ];

            let table = Table::new(rows, widths)
                .header(header)
                .row_highlight_style(theme::highlight_style());
            frame.render_stateful_widget(table, layout[1], &mut self.table_state);
        }

        render_hints(frame, layout[2]);
    }

    fn handle_key(&mut self, key: KeyEvent, _conn: &Connection) -> Option<AppMessage> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                Some(AppMessage::PopScreen(ScreenResult::Cancelled))
            }
            KeyCode::Char('n') => {
                if !self.entries.is_empty() {
                    self.current_entry = (self.current_entry + 1) % self.entries.len();
                    self.table_state.select(Some(0));
                }
                None
            }
            KeyCode::Char('p') => {
                if !self.entries.is_empty() {
                    if self.current_entry == 0 {
                        self.current_entry = self.entries.len() - 1;
                    } else {
                        self.current_entry -= 1;
                    }
                    self.table_state.select(Some(0));
                }
                None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(stats) = self.current_stats() {
                    let count = stats.maps.iter().filter(|m| m.played()).count();
                    if count > 0 {
                        let i = match self.table_state.selected() {
                            Some(i) => (i + 1).min(count - 1),
                            None => 0,
                        };
                        self.table_state.select(Some(i));
                    }
                }
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.current_stats().is_some() {
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

fn format_ratio(count: i32, total: i32) -> String {
    if total < 0 {
        count.to_string()
    } else {
        format!("{count}/{total}")
    }
}

fn render_hints(frame: &mut Frame, area: Rect) {
    let hints = Line::from(vec![
        Span::styled("q/Esc", theme::key_style()),
        Span::styled(" back  ", theme::desc_style()),
        Span::styled("n/p", theme::key_style()),
        Span::styled(" switch  ", theme::desc_style()),
        Span::styled("j/k", theme::key_style()),
        Span::styled(" navigate", theme::desc_style()),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}
