use caco_core::db::sessions::{StatsSnapshot, get_stats_snapshot};
use caco_core::player::format_duration;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenResult};
use crate::screens::Screen;
use crate::theme;

/// Library statistics screen.
pub struct StatsScreen {
    snapshot: Option<StatsSnapshot>,
    scroll_offset: u16,
}

impl StatsScreen {
    pub fn new(conn: &Connection) -> Self {
        let snapshot = get_stats_snapshot(conn, "month").ok();
        Self {
            snapshot,
            scroll_offset: 0,
        }
    }
}

impl Screen for StatsScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_style())
            .title(" Library Statistics ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

        let Some(ref snap) = self.snapshot else {
            frame.render_widget(
                Paragraph::new("No statistics available").style(theme::dim_style()),
                layout[0],
            );
            return;
        };

        let section_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);

        let mut lines = Vec::new();

        // Overview
        lines.push(Line::from(Span::styled("── Overview ──", section_style)));
        lines.push(Line::from(vec![
            Span::styled("Total WADs: ", theme::dim_style()),
            Span::raw(snap.total_wads.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Total Sessions: ", theme::dim_style()),
            Span::raw(snap.total_sessions.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Total Playtime: ", theme::dim_style()),
            Span::raw(if snap.total_playtime > 0 {
                format_duration(snap.total_playtime)
            } else {
                "—".to_string()
            }),
        ]));
        lines.push(Line::from(vec![
            Span::styled("WADs Played: ", theme::dim_style()),
            Span::raw(format!(
                "{}/{}",
                snap.wads_with_sessions, snap.total_wads
            )),
        ]));

        // By Status
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("── By Status ──", section_style)));
        let status_order = [
            "unplayed",
            "in-progress",
            "completed",
            "abandoned",
        ];
        for status in &status_order {
            let count = snap.wads_by_status.get(*status).copied().unwrap_or(0);
            if count > 0 {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}: ", theme::status_display(status)),
                        theme::status_style(status),
                    ),
                    Span::raw(count.to_string()),
                ]));
            }
        }

        // Completion
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "── Completion ──",
            section_style,
        )));
        lines.push(Line::from(vec![
            Span::styled("Completed: ", theme::dim_style()),
            Span::raw(snap.completed_wads.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Total Completions: ", theme::dim_style()),
            Span::raw(snap.total_completions.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Completion Rate: ", theme::dim_style()),
            Span::raw(format!("{:.1}%", snap.completion_rate * 100.0)),
        ]));

        // Monthly Activity
        if !snap.activity.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "── Monthly Activity ──",
                section_style,
            )));

            for period in &snap.activity {
                let playtime = if period.total_playtime > 0 {
                    format_duration(period.total_playtime)
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    Span::styled(&period.period, Style::default().fg(Color::White)),
                    Span::raw(format!(
                        "  {} WADs, {} sessions, {}",
                        period.wad_count, period.session_count, playtime
                    )),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));
        frame.render_widget(paragraph, layout[0]);

        // Key hints
        let hints = Line::from(vec![
            Span::styled("q/Esc", theme::key_style()),
            Span::styled(" back  ", theme::desc_style()),
            Span::styled("j/k", theme::key_style()),
            Span::styled(" scroll", theme::desc_style()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[1]);
    }

    fn handle_key(&mut self, key: KeyEvent, _conn: &Connection) -> Option<AppMessage> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                Some(AppMessage::PopScreen(ScreenResult::Cancelled))
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                None
            }
            _ => None,
        }
    }
}
