use caco_core::db::sessions;
use caco_core::db::wads;
use caco_core::player::format_duration;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenResult};
use crate::screens::Screen;
use crate::theme;

/// Confirm delete modal screen.
pub struct ConfirmDeleteScreen {
    wad_id: i64,
    wad_title: String,
    wad_author: Option<String>,
    session_count: i64,
    total_playtime: i64,
}

impl ConfirmDeleteScreen {
    pub fn new(wad_id: i64, conn: &Connection) -> Self {
        let (title, author) = wads::get_wad(conn, wad_id, false)
            .ok()
            .flatten()
            .map(|w| (w.title, w.author))
            .unwrap_or_else(|| (format!("WAD #{wad_id}"), None));

        let (session_count, total_playtime) =
            sessions::get_wad_stats(conn, wad_id).unwrap_or((0, 0));

        Self {
            wad_id,
            wad_title: title,
            wad_author: author,
            session_count,
            total_playtime,
        }
    }
}

impl Screen for ConfirmDeleteScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        // Modal overlay
        frame.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Red))
            .title(" Confirm Delete ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Delete this WAD?",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Title: ", theme::dim_style()),
                Span::styled(
                    &self.wad_title,
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
        ];

        if let Some(ref author) = self.wad_author {
            lines.push(Line::from(vec![
                Span::styled("Author: ", theme::dim_style()),
                Span::raw(author.as_str()),
            ]));
        }

        lines.push(Line::from(""));

        if self.session_count > 0 {
            lines.push(Line::from(vec![
                Span::styled("Sessions: ", theme::dim_style()),
                Span::raw(self.session_count.to_string()),
            ]));
        }
        if self.total_playtime > 0 {
            lines.push(Line::from(vec![
                Span::styled("Playtime: ", theme::dim_style()),
                Span::raw(format_duration(self.total_playtime)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("y", theme::key_style()),
            Span::styled(" confirm  ", theme::desc_style()),
            Span::styled("n/Esc", theme::key_style()),
            Span::styled(" cancel", theme::desc_style()),
        ]));

        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner);
    }

    fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> Option<AppMessage> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Soft-delete (trash)
                let _ = wads::delete_wad(conn, self.wad_id, false);
                Some(AppMessage::PopScreen(ScreenResult::Confirmed(self.wad_id)))
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                Some(AppMessage::PopScreen(ScreenResult::Cancelled))
            }
            _ => None,
        }
    }

    fn is_modal(&self) -> bool {
        true
    }
}
