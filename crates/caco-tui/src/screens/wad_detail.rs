use caco_core::db::models::WadRecord;
use caco_core::db::sessions::{WadStats, get_wad_stats_batch};
use caco_core::db::wads::get_wad;
use caco_core::player::format_duration;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenId, ScreenResult};
use crate::screens::Screen;
use crate::theme;

/// Full WAD detail view screen.
pub struct WadDetailScreen {
    wad_id: i64,
    wad: Option<WadRecord>,
    stats: Option<WadStats>,
    scroll_offset: u16,
}

impl WadDetailScreen {
    pub fn new(wad_id: i64, conn: &Connection) -> Self {
        let mut screen = Self {
            wad_id,
            wad: None,
            stats: None,
            scroll_offset: 0,
        };
        screen.load(conn);
        screen
    }

    fn load(&mut self, conn: &Connection) {
        if let Ok(Some(mut wad)) = get_wad(conn, self.wad_id, true) {
            let _ = caco_core::db::connection::attach_tags(conn, &mut wad);
            if let Ok(stats_map) = get_wad_stats_batch(conn, &[wad.id]) {
                self.stats = stats_map.into_values().next();
            }
            self.wad = Some(wad);
        }
    }

    fn build_lines(&self) -> Vec<Line<'_>> {
        let Some(ref wad) = self.wad else {
            return vec![Line::from("WAD not found")];
        };

        let mut lines = Vec::new();

        // Title
        lines.push(Line::from(Span::styled(
            wad.title.as_str(),
            theme::title_style(),
        )));
        lines.push(Line::from(""));

        // Basic info section
        let section_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);

        lines.push(Line::from(Span::styled("── Basic Info ──", section_style)));

        lines.push(Line::from(vec![
            Span::styled("ID: ", theme::dim_style()),
            Span::raw(wad.id.to_string()),
        ]));

        if let Some(ref author) = wad.author {
            lines.push(Line::from(vec![
                Span::styled("Author: ", theme::dim_style()),
                Span::raw(author.as_str()),
            ]));
        }

        if let Some(year) = wad.year {
            lines.push(Line::from(vec![
                Span::styled("Year: ", theme::dim_style()),
                Span::raw(year.to_string()),
            ]));
        }

        let status_display = theme::status_display(&wad.status);
        lines.push(Line::from(vec![
            Span::styled("Status: ", theme::dim_style()),
            Span::styled(status_display, theme::status_style(&wad.status)),
        ]));

        let stars = theme::rating_stars(wad.rating);
        if !stars.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Rating: ", theme::dim_style()),
                Span::styled(stars, Style::default().fg(Color::Yellow)),
            ]));
        }

        // Source section
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("── Source ──", section_style)));

        lines.push(Line::from(vec![
            Span::styled("Type: ", theme::dim_style()),
            Span::raw(wad.source_type.as_str()),
        ]));

        if let Some(ref url) = wad.source_url {
            lines.push(Line::from(vec![
                Span::styled("URL: ", theme::dim_style()),
                Span::raw(url.as_str()),
            ]));
        }

        if let Some(ref filename) = wad.filename {
            lines.push(Line::from(vec![
                Span::styled("Filename: ", theme::dim_style()),
                Span::raw(filename.as_str()),
            ]));
        }

        if let Some(ref version) = wad.version {
            lines.push(Line::from(vec![
                Span::styled("Version: ", theme::dim_style()),
                Span::raw(version.as_str()),
            ]));
        }

        // Play stats section
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("── Play Stats ──", section_style)));

        if let Some(ref stats) = self.stats {
            lines.push(Line::from(vec![
                Span::styled("Playtime: ", theme::dim_style()),
                Span::raw(if stats.playtime > 0 {
                    format_duration(stats.playtime)
                } else {
                    "—".to_string()
                }),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Sessions: ", theme::dim_style()),
                Span::raw(stats.session_count.to_string()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Beaten: ", theme::dim_style()),
                Span::raw(if stats.times_beaten > 0 {
                    format!("{}×", stats.times_beaten)
                } else {
                    "—".to_string()
                }),
            ]));
            if let Some(ref last) = stats.last_played {
                let date = last.split('T').next().unwrap_or(last);
                let date = date.split(' ').next().unwrap_or(date);
                lines.push(Line::from(vec![
                    Span::styled("Last played: ", theme::dim_style()),
                    Span::raw(date),
                ]));
            }
        }

        // Config section
        if wad.custom_iwad.is_some()
            || wad.custom_sourceport.is_some()
            || wad.complevel.is_some()
            || wad.custom_config.is_some()
        {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("── Config ──", section_style)));

            if let Some(ref iwad) = wad.custom_iwad {
                lines.push(Line::from(vec![
                    Span::styled("IWAD: ", theme::dim_style()),
                    Span::raw(iwad.as_str()),
                ]));
            }
            if let Some(ref sp) = wad.custom_sourceport {
                lines.push(Line::from(vec![
                    Span::styled("Sourceport: ", theme::dim_style()),
                    Span::raw(sp.as_str()),
                ]));
            }
            if let Some(cl) = wad.complevel {
                let name = caco_core::complevel::complevel_name(Some(cl));
                lines.push(Line::from(vec![
                    Span::styled("Complevel: ", theme::dim_style()),
                    Span::raw(format!("{cl} ({name})")),
                ]));
            }
            if let Some(ref cfg) = wad.custom_config {
                lines.push(Line::from(vec![
                    Span::styled("Config: ", theme::dim_style()),
                    Span::raw(cfg.as_str()),
                ]));
            }
        }

        // Tags
        if !wad.tags.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("── Tags ──", section_style)));
            lines.push(Line::from(wad.tags.join(", ")));
        }

        // Description
        if let Some(ref desc) = wad.description {
            if !desc.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "── Description ──",
                    section_style,
                )));
                for line in desc.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        }

        // Notes
        if let Some(ref notes) = wad.notes {
            if !notes.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("── Notes ──", section_style)));
                for line in notes.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        }

        lines
    }
}

impl Screen for WadDetailScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_style())
            .title(" WAD Detail ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

        let lines = self.build_lines();
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));
        frame.render_widget(paragraph, layout[0]);

        // Key hints
        let hints = Line::from(vec![
            Span::styled("q/Esc", theme::key_style()),
            Span::styled(" back  ", theme::desc_style()),
            Span::styled("Enter", theme::key_style()),
            Span::styled(" play  ", theme::desc_style()),
            Span::styled("e", theme::key_style()),
            Span::styled(" edit  ", theme::desc_style()),
            Span::styled("h", theme::key_style()),
            Span::styled(" history  ", theme::desc_style()),
            Span::styled("j/k", theme::key_style()),
            Span::styled(" scroll", theme::desc_style()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[1]);
    }

    fn handle_key(&mut self, key: KeyEvent, _conn: &Connection) -> Option<AppMessage> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('q') | KeyCode::Esc, _) => {
                Some(AppMessage::PopScreen(ScreenResult::Cancelled))
            }
            (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                None
            }
            (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                None
            }
            (KeyCode::Enter, KeyModifiers::NONE) => Some(AppMessage::PlayWad(self.wad_id)),
            (KeyCode::Char('e'), KeyModifiers::NONE) => {
                Some(AppMessage::PushScreen(ScreenId::WadEdit(self.wad_id)))
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) => {
                Some(AppMessage::PushScreen(ScreenId::Sessions(self.wad_id)))
            }
            _ => None,
        }
    }

    fn on_resume(&mut self, conn: &Connection, _result: Option<ScreenResult>) {
        self.load(conn);
    }
}
