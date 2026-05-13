//! Cacowards screen — yearly winners, runners-up, honorable mentions, and
//! Mordeth award, with completion status sourced from linked library WADs.
//!
//! Navigation: `[` / `]` step between years that have entries in the database;
//! `j` / `k` scroll the entry list; `q` / `Esc` pops back; pressing `Enter`
//! on a linked entry dives into the WAD detail screen.

use std::collections::HashMap;

use caco_core::db::{self, CORE_CATEGORIES, CacowardRecord, Status};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenId, ScreenResult};
use crate::screens::Screen;
use crate::theme;

pub struct CacowardsScreen {
    years: Vec<i64>,
    selected_year_idx: usize,
    entries: Vec<CacowardRecord>,
    statuses: HashMap<i64, Status>,
    /// Flat index of currently-selected entry, used for "Enter -> WadDetail".
    /// `None` when the year has no entries.
    selected_entry: Option<usize>,
    scroll_offset: u16,
}

impl CacowardsScreen {
    pub fn new(conn: &Connection) -> Self {
        let years = db::get_years(conn).unwrap_or_default();
        let mut screen = Self {
            years,
            selected_year_idx: 0,
            entries: Vec::new(),
            statuses: HashMap::new(),
            selected_entry: None,
            scroll_offset: 0,
        };
        screen.reload(conn);
        screen
    }

    fn current_year(&self) -> Option<i64> {
        self.years.get(self.selected_year_idx).copied()
    }

    fn reload(&mut self, conn: &Connection) {
        self.entries = self
            .current_year()
            .and_then(|y| db::get_cacowards_by_year(conn, y).ok())
            .unwrap_or_default();
        // Pull statuses for every linked WAD in one pass — the entry list is
        // bounded by the count of awards in a year (a few dozen at most), so
        // a single search is cheaper than per-row queries.
        self.statuses = load_statuses(conn, &self.entries);
        self.selected_entry = if self.entries.is_empty() {
            None
        } else {
            Some(0)
        };
        self.scroll_offset = 0;
    }

    fn move_selection(&mut self, delta: isize) {
        let Some(idx) = self.selected_entry else {
            return;
        };
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let next = (idx as isize + delta).clamp(0, (len - 1) as isize) as usize;
        self.selected_entry = Some(next);
    }

    fn step_year(&mut self, conn: &Connection, delta: isize) {
        if self.years.is_empty() {
            return;
        }
        let next = (self.selected_year_idx as isize + delta)
            .clamp(0, (self.years.len() - 1) as isize) as usize;
        if next != self.selected_year_idx {
            self.selected_year_idx = next;
            self.reload(conn);
        }
    }
}

fn load_statuses(conn: &Connection, entries: &[CacowardRecord]) -> HashMap<i64, Status> {
    // We only need statuses for entries with a linked WAD; bulk-load all
    // wads once rather than running per-id queries.
    let wad_ids: std::collections::HashSet<i64> = entries.iter().filter_map(|e| e.wad_id).collect();
    if wad_ids.is_empty() {
        return HashMap::new();
    }
    let wads = db::search_wads(conn, None, None, true, false, 0).unwrap_or_default();
    wads.into_iter()
        .filter(|w| wad_ids.contains(&w.id))
        .map(|w| (w.id, w.status))
        .collect()
}

fn category_display(category: &str) -> &'static str {
    match category {
        "winner" => "Winners",
        "runner-up" => "Runners-up",
        "honorable-mention" => "Honorable Mentions",
        "mordeth" => "Mordeth Award",
        _ => "Other",
    }
}

fn status_glyph(status: Option<Status>) -> (&'static str, Color) {
    match status {
        Some(Status::Completed) => ("[done]    ", Color::Green),
        Some(Status::InProgress) => ("[playing] ", Color::Yellow),
        Some(Status::Abandoned) => ("[dropped] ", Color::Red),
        Some(Status::Unplayed) => ("[in lib]  ", Color::Cyan),
        None => ("[--]      ", Color::DarkGray),
    }
}

impl Screen for CacowardsScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_style())
            .title(" Cacowards ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([
            Constraint::Length(1), // year header
            Constraint::Min(1),    // entry list
            Constraint::Length(1), // hint bar
        ])
        .split(inner);

        // --- Year header ----------------------------------------------------
        let header_line = match self.current_year() {
            Some(year) => {
                let counts = self.category_counts();
                let mut spans: Vec<Span> = vec![
                    Span::styled(
                        format!("Cacowards {year}"),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("   "),
                ];
                let mut emitted = 0;
                for &cat in CORE_CATEGORIES {
                    let (total, done) = counts.get(cat).copied().unwrap_or((0, 0));
                    if total == 0 {
                        continue;
                    }
                    if emitted > 0 {
                        spans.push(Span::raw("  "));
                    }
                    emitted += 1;
                    spans.push(Span::styled(
                        format!("{}: ", short_category(cat)),
                        theme::dim_style(),
                    ));
                    let color = if done == total {
                        Color::Green
                    } else if done > 0 {
                        Color::Yellow
                    } else {
                        Color::White
                    };
                    spans.push(Span::styled(
                        format!("{done}/{total}"),
                        Style::default().fg(color),
                    ));
                }
                Line::from(spans)
            }
            None => Line::from(Span::styled(
                "No Cacoward data — run `caco enrich --cacowards --year YYYY`",
                theme::dim_style(),
            )),
        };
        frame.render_widget(Paragraph::new(header_line), layout[0]);

        // --- Entry list -----------------------------------------------------
        let lines = self.render_entry_lines();
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));
        frame.render_widget(paragraph, layout[1]);

        // --- Hints ----------------------------------------------------------
        let hints = Line::from(vec![
            Span::styled("q/Esc", theme::key_style()),
            Span::styled(" back  ", theme::desc_style()),
            Span::styled("[ ]", theme::key_style()),
            Span::styled(" year  ", theme::desc_style()),
            Span::styled("j/k", theme::key_style()),
            Span::styled(" move  ", theme::desc_style()),
            Span::styled("Enter", theme::key_style()),
            Span::styled(" open WAD", theme::desc_style()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[2]);
    }

    fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> Option<AppMessage> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                Some(AppMessage::PopScreen(ScreenResult::Cancelled))
            }
            KeyCode::Char('[') => {
                // Older year = larger index in `years` (which is sorted DESC).
                self.step_year(conn, 1);
                None
            }
            KeyCode::Char(']') => {
                self.step_year(conn, -1);
                None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                None
            }
            KeyCode::Char('g') | KeyCode::Home => {
                if !self.entries.is_empty() {
                    self.selected_entry = Some(0);
                }
                self.scroll_offset = 0;
                None
            }
            KeyCode::Char('G') | KeyCode::End => {
                if !self.entries.is_empty() {
                    self.selected_entry = Some(self.entries.len() - 1);
                }
                None
            }
            KeyCode::Enter => {
                let idx = self.selected_entry?;
                let entry = self.entries.get(idx)?;
                entry
                    .wad_id
                    .map(|id| AppMessage::PushScreen(ScreenId::WadDetail(id)))
            }
            _ => None,
        }
    }

    fn on_resume(&mut self, conn: &Connection, _result: Option<ScreenResult>) {
        // WAD status may have changed (e.g. user marked completed from the
        // detail screen) — refresh the linked-status map.
        self.statuses = load_statuses(conn, &self.entries);
    }
}

impl CacowardsScreen {
    fn render_entry_lines(&self) -> Vec<Line<'_>> {
        let mut lines: Vec<Line<'_>> = Vec::new();
        if self.entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "No entries for this year.",
                theme::dim_style(),
            )));
            return lines;
        }

        let section_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);

        let mut last_category: Option<&str> = None;
        for (idx, entry) in self.entries.iter().enumerate() {
            if last_category != Some(entry.category.as_str()) {
                if last_category.is_some() {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(Span::styled(
                    format!("── {} ──", category_display(&entry.category)),
                    section_style,
                )));
                last_category = Some(entry.category.as_str());
            }

            let status = entry.wad_id.and_then(|id| self.statuses.get(&id).copied());
            let (glyph, color) = status_glyph(status);

            let cursor = if self.selected_entry == Some(idx) {
                ">"
            } else {
                " "
            };
            let rank = entry
                .rank
                .map(|r| format!("{r:>2}. "))
                .unwrap_or_else(|| "   ".to_string());
            let author = entry.wad_author.as_deref().unwrap_or("Unknown");

            let mut spans: Vec<Span> = vec![
                Span::styled(format!("{cursor} "), theme::dim_style()),
                Span::raw(rank),
                Span::styled(glyph, Style::default().fg(color)),
                Span::styled(
                    entry.wad_title.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" — {author}"), theme::dim_style()),
            ];
            if entry.manual_override {
                spans.push(Span::styled(" 📌", theme::dim_style()));
            }
            lines.push(Line::from(spans));
        }
        lines
    }
}

impl CacowardsScreen {
    /// `(total, completed)` per category for the current year, with
    /// completion resolved against the loaded statuses map.
    fn category_counts(&self) -> HashMap<&str, (usize, usize)> {
        let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
        for entry in &self.entries {
            let slot = counts.entry(entry.category.as_str()).or_insert((0, 0));
            slot.0 += 1;
            if let Some(id) = entry.wad_id
                && self.statuses.get(&id) == Some(&Status::Completed)
            {
                slot.1 += 1;
            }
        }
        counts
    }
}

fn short_category(category: &str) -> &'static str {
    match category {
        "winner" => "W",
        "runner-up" => "R",
        "honorable-mention" => "HM",
        "mordeth" => "M",
        _ => "?",
    }
}
