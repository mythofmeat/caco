use caco_core::complevel::parse_complevel;
use caco_core::db::models::Status;
use caco_core::db::wads::{self, WadUpdate, get_wad};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use rusqlite::Connection;

use crate::input::TextInput;
use crate::message::{AppMessage, ScreenResult};
use crate::screens::Screen;
use crate::theme;

/// Editing field types.
enum FieldKind {
    Text,
    StatusCycle,
    RatingCycle,
}

/// A field in the edit form.
struct EditField {
    name: &'static str,
    label: &'static str,
    input: TextInput,
    kind: FieldKind,
}

/// WAD edit modal screen.
pub struct WadEditScreen {
    wad_id: i64,
    fields: Vec<EditField>,
    active_field: usize,
    scroll_offset: usize,
    original_tags: Vec<String>,
    error_message: Option<String>,
}

impl WadEditScreen {
    pub fn new(wad_id: i64, conn: &Connection) -> Self {
        let mut screen = Self {
            wad_id,
            fields: Vec::new(),
            active_field: 0,
            scroll_offset: 0,
            original_tags: Vec::new(),
            error_message: None,
        };
        screen.load(conn);
        screen
    }

    fn load(&mut self, conn: &Connection) {
        let Some(wad) = get_wad(conn, self.wad_id, true).ok().flatten() else {
            return;
        };
        let _ = caco_core::db::connection::attach_tags(conn, &mut {
            let mut w = wad.clone();
            w
        });
        // Re-fetch with tags
        let mut wad = get_wad(conn, self.wad_id, true).ok().flatten().unwrap();
        let _ = caco_core::db::connection::attach_tags(conn, &mut wad);

        self.original_tags = wad.tags.clone();

        self.fields = vec![
            field_text("title", "Title", &wad.title),
            field_text("author", "Author", wad.author.as_deref().unwrap_or("")),
            field_text(
                "year",
                "Year",
                &wad.year.map(|y| y.to_string()).unwrap_or_default(),
            ),
            EditField {
                name: "status",
                label: "Status (←/→ cycle)",
                input: TextInput::with_value(&wad.status),
                kind: FieldKind::StatusCycle,
            },
            EditField {
                name: "rating",
                label: "Rating (←/→ cycle 0-5)",
                input: TextInput::with_value(
                    &wad.rating.map(|r| r.to_string()).unwrap_or_default(),
                ),
                kind: FieldKind::RatingCycle,
            },
            field_text("tags", "Tags (comma-separated)", &wad.tags.join(", ")),
            field_text("notes", "Notes", wad.notes.as_deref().unwrap_or("")),
            field_text(
                "description",
                "Description",
                wad.description.as_deref().unwrap_or(""),
            ),
            field_text(
                "iwad",
                "IWAD",
                wad.custom_iwad.as_deref().unwrap_or(""),
            ),
            field_text(
                "sourceport",
                "Sourceport",
                wad.custom_sourceport.as_deref().unwrap_or(""),
            ),
            field_text(
                "complevel",
                "Complevel",
                &wad.complevel
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
            ),
            field_text(
                "config",
                "Config Profile",
                wad.custom_config.as_deref().unwrap_or(""),
            ),
            field_text(
                "args",
                "Custom Args (JSON array)",
                wad.custom_args.as_deref().unwrap_or(""),
            ),
            field_text(
                "version",
                "Version",
                wad.version.as_deref().unwrap_or(""),
            ),
        ];
    }

    fn save(&mut self, conn: &Connection) -> Option<AppMessage> {
        // Validate
        let title = self.get_field_value("title");
        if title.is_empty() {
            self.error_message = Some("Title is required".to_string());
            return None;
        }

        let year_str = self.get_field_value("year");
        let year: Option<i64> = if year_str.is_empty() {
            None
        } else {
            match year_str.parse::<i64>() {
                Ok(y) if (1993..=2100).contains(&y) => Some(y),
                _ => {
                    self.error_message = Some("Year must be 1993-2100".to_string());
                    return None;
                }
            }
        };

        let complevel_str = self.get_field_value("complevel");
        let complevel: Option<i64> = if complevel_str.is_empty() {
            None
        } else {
            match parse_complevel(&complevel_str) {
                Some(c) => Some(c as i64),
                None => {
                    self.error_message = Some("Invalid complevel".to_string());
                    return None;
                }
            }
        };

        // Build WadUpdate
        let status_str = self.get_field_value("status");
        let status = Status::parse(&status_str);

        let mut update = WadUpdate::new();

        // These unwrap the Result — if field name is invalid, it'd be a bug
        update = update.set_text("title", Some(title)).unwrap();
        update = update
            .set_text("author", opt_str(&self.get_field_value("author")))
            .unwrap();
        update = update.set_int("year", year).unwrap();

        if let Some(s) = status {
            update = update.set_status(s).unwrap();
        }

        let rating_str = self.get_field_value("rating");
        let rating: Option<i64> = if rating_str.is_empty() {
            None
        } else {
            rating_str.parse().ok()
        };
        update = update.set_int("rating", rating).unwrap();

        update = update
            .set_text("notes", opt_str(&self.get_field_value("notes")))
            .unwrap();
        update = update
            .set_text(
                "description",
                opt_str(&self.get_field_value("description")),
            )
            .unwrap();
        update = update
            .set_text("custom_iwad", opt_str(&self.get_field_value("iwad")))
            .unwrap();
        update = update
            .set_text(
                "custom_sourceport",
                opt_str(&self.get_field_value("sourceport")),
            )
            .unwrap();
        update = update.set_int("complevel", complevel).unwrap();
        update = update
            .set_text("custom_config", opt_str(&self.get_field_value("config")))
            .unwrap();
        update = update
            .set_text("custom_args", opt_str(&self.get_field_value("args")))
            .unwrap();
        update = update
            .set_text("version", opt_str(&self.get_field_value("version")))
            .unwrap();

        // Apply update
        if let Err(e) = wads::update_wad(conn, self.wad_id, &update) {
            self.error_message = Some(format!("Save failed: {e}"));
            return None;
        }

        // Handle tags delta
        let new_tags: Vec<String> = self
            .get_field_value("tags")
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();

        // Remove old tags not in new set
        for tag in &self.original_tags {
            if !new_tags.contains(tag) {
                let _ = wads::remove_tag(conn, self.wad_id, tag);
            }
        }
        // Add new tags not in old set
        for tag in &new_tags {
            if !self.original_tags.contains(tag) {
                let _ = wads::add_tag(conn, self.wad_id, tag);
            }
        }

        Some(AppMessage::PopScreen(ScreenResult::Saved))
    }

    fn get_field_value(&self, name: &str) -> String {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.input.value().to_string())
            .unwrap_or_default()
    }

    fn cycle_status(&mut self, forward: bool) {
        let statuses = ["to-play", "backlog", "playing", "finished", "abandoned", "awaiting-update"];
        if let Some(field) = self.fields.iter_mut().find(|f| f.name == "status") {
            let current = field.input.value().to_string();
            let idx = statuses.iter().position(|s| *s == current).unwrap_or(0);
            let new_idx = if forward {
                (idx + 1) % statuses.len()
            } else {
                if idx == 0 { statuses.len() - 1 } else { idx - 1 }
            };
            field.input.set_value(statuses[new_idx]);
        }
    }

    fn cycle_rating(&mut self, forward: bool) {
        if let Some(field) = self.fields.iter_mut().find(|f| f.name == "rating") {
            let current: i32 = field.input.value().parse().unwrap_or(0);
            let new_val = if forward {
                if current >= 5 { 0 } else { current + 1 }
            } else {
                if current <= 0 { 5 } else { current - 1 }
            };
            if new_val == 0 {
                field.input.set_value("");
            } else {
                field.input.set_value(&new_val.to_string());
            }
        }
    }
}

impl Screen for WadEditScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        frame.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Edit WAD ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([
            Constraint::Min(1),   // fields
            Constraint::Length(1), // error / hint
        ])
        .split(inner);

        // Calculate visible fields
        let visible_rows = (layout[0].height as usize) / 2;
        let start = self.scroll_offset;
        let end = (start + visible_rows).min(self.fields.len());

        let mut y = layout[0].y;
        for i in start..end {
            let field = &self.fields[i];
            let is_active = i == self.active_field;

            // Label
            let label_style = if is_active {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let indicator = if is_active { "▸ " } else { "  " };
            let label_line = Line::from(vec![
                Span::styled(indicator, Style::default().fg(Color::Cyan)),
                Span::styled(field.label, label_style),
            ]);
            frame.render_widget(
                Paragraph::new(label_line),
                Rect::new(layout[0].x, y, layout[0].width, 1),
            );
            y += 1;

            // Value
            if y >= layout[0].y + layout[0].height {
                break;
            }

            let value_area = Rect::new(layout[0].x + 2, y, layout[0].width.saturating_sub(2), 1);
            match field.kind {
                FieldKind::StatusCycle => {
                    let status = field.input.value();
                    let display = theme::status_display(status);
                    let style = theme::status_style(status).add_modifier(Modifier::BOLD);
                    let line = if is_active {
                        Line::from(vec![
                            Span::styled("◂ ", theme::dim_style()),
                            Span::styled(display, style),
                            Span::styled(" ▸", theme::dim_style()),
                        ])
                    } else {
                        Line::from(Span::styled(display, style))
                    };
                    frame.render_widget(Paragraph::new(line), value_area);
                }
                FieldKind::RatingCycle => {
                    let val: i32 = field.input.value().parse().unwrap_or(0);
                    let stars = theme::rating_stars(if val > 0 { Some(val) } else { None });
                    let display = if stars.is_empty() {
                        "None".to_string()
                    } else {
                        stars
                    };
                    let line = if is_active {
                        Line::from(vec![
                            Span::styled("◂ ", theme::dim_style()),
                            Span::styled(display, Style::default().fg(Color::Yellow)),
                            Span::styled(" ▸", theme::dim_style()),
                        ])
                    } else {
                        Line::from(Span::styled(
                            display,
                            Style::default().fg(Color::Yellow),
                        ))
                    };
                    frame.render_widget(Paragraph::new(line), value_area);
                }
                FieldKind::Text => {
                    field.input.render(frame, value_area, is_active, "");
                }
            }
            y += 1;
        }

        // Error / hint line
        if let Some(ref err) = self.error_message {
            let line = Line::from(Span::styled(
                err.as_str(),
                Style::default().fg(Color::Red),
            ));
            frame.render_widget(Paragraph::new(line), layout[1]);
        } else {
            let hints = Line::from(vec![
                Span::styled("Ctrl+S", theme::key_style()),
                Span::styled(" save  ", theme::desc_style()),
                Span::styled("Esc", theme::key_style()),
                Span::styled(" cancel  ", theme::desc_style()),
                Span::styled("Tab", theme::key_style()),
                Span::styled(" next field", theme::desc_style()),
            ]);
            frame.render_widget(Paragraph::new(hints), layout[1]);
        }
    }

    fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> Option<AppMessage> {
        self.error_message = None;

        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => Some(AppMessage::PopScreen(ScreenResult::Cancelled)),
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => self.save(conn),
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.active_field = (self.active_field + 1) % self.fields.len();
                // Adjust scroll
                let visible_rows = 10; // approximate
                if self.active_field >= self.scroll_offset + visible_rows {
                    self.scroll_offset = self.active_field - visible_rows + 1;
                }
                if self.active_field < self.scroll_offset {
                    self.scroll_offset = self.active_field;
                }
                None
            }
            (KeyCode::BackTab, KeyModifiers::SHIFT) => {
                if self.active_field == 0 {
                    self.active_field = self.fields.len() - 1;
                } else {
                    self.active_field -= 1;
                }
                if self.active_field < self.scroll_offset {
                    self.scroll_offset = self.active_field;
                }
                None
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                if let Some(field) = self.fields.get(self.active_field) {
                    match field.kind {
                        FieldKind::StatusCycle => self.cycle_status(false),
                        FieldKind::RatingCycle => self.cycle_rating(false),
                        _ => {
                            if let Some(f) = self.fields.get_mut(self.active_field) {
                                f.input.handle_key(key);
                            }
                        }
                    }
                }
                None
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                if let Some(field) = self.fields.get(self.active_field) {
                    match field.kind {
                        FieldKind::StatusCycle => self.cycle_status(true),
                        FieldKind::RatingCycle => self.cycle_rating(true),
                        _ => {
                            if let Some(f) = self.fields.get_mut(self.active_field) {
                                f.input.handle_key(key);
                            }
                        }
                    }
                }
                None
            }
            _ => {
                // Route to active text field
                if let Some(field) = self.fields.get(self.active_field) {
                    if matches!(field.kind, FieldKind::Text) {
                        if let Some(f) = self.fields.get_mut(self.active_field) {
                            f.input.handle_key(key);
                        }
                    }
                }
                None
            }
        }
    }

    fn is_modal(&self) -> bool {
        true
    }
}

fn field_text(name: &'static str, label: &'static str, value: &str) -> EditField {
    EditField {
        name,
        label,
        input: TextInput::with_value(value),
        kind: FieldKind::Text,
    }
}

fn opt_str(s: &str) -> Option<String> {
    if s.is_empty() { None } else { Some(s.to_string()) }
}
