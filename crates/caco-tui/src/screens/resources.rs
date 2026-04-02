use std::fs;
use std::path::PathBuf;

use caco_core::config;
use caco_core::db::id24::{self, Id24Record};
use caco_core::db::iwads::{self, IwadRecord};
use caco_core::resource_service;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::widgets::table_nav::{table_nav_next, table_nav_prev};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;
use rusqlite::Connection;

use crate::input::TextInput;
use crate::message::{AppMessage, ScreenResult, Severity};
use crate::screens::Screen;
use crate::theme;

/// Resources screen sub-tab.
#[derive(Clone, Copy, PartialEq)]
enum ResourceTab {
    Iwad,
    Id24,
}

/// IWAD/id24 registry management screen.
pub struct ResourcesScreen {
    active_tab: ResourceTab,
    iwads: Vec<IwadRecord>,
    id24s: Vec<Id24Record>,
    iwad_table: TableState,
    id24_table: TableState,
    import_input: TextInput,
    import_focused: bool,
    preferred_iwads: Vec<String>,
}

impl ResourcesScreen {
    pub fn new(conn: &Connection) -> Self {
        let mut screen = Self {
            active_tab: ResourceTab::Iwad,
            iwads: Vec::new(),
            id24s: Vec::new(),
            iwad_table: TableState::default(),
            id24_table: TableState::default(),
            import_input: TextInput::new(),
            import_focused: false,
            preferred_iwads: Vec::new(),
        };
        screen.load(conn);
        screen
    }

    fn load(&mut self, conn: &Connection) {
        self.iwads = iwads::get_all_iwads(conn).unwrap_or_default();
        self.id24s = id24::get_all_id24(conn).unwrap_or_default();

        // Get preferred IWAD variants
        let cfg = config::load_config();
        self.preferred_iwads = self
            .iwads
            .iter()
            .filter_map(|iwad| {
                let priority = iwads::get_iwad_priority(&iwad.family, Some(&cfg.iwad_priority));
                if priority.first().map(|v| v == &iwad.variant).unwrap_or(false) {
                    Some(format!("{}/{}", iwad.family, iwad.variant))
                } else {
                    None
                }
            })
            .collect();

        if !self.iwads.is_empty() && self.iwad_table.selected().is_none() {
            self.iwad_table.select(Some(0));
        }
        if !self.id24s.is_empty() && self.id24_table.selected().is_none() {
            self.id24_table.select(Some(0));
        }
    }
}

impl Screen for ResourcesScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect, _conn: &Connection) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_style())
            .title(" Resources ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([
            Constraint::Length(1), // tab bar
            Constraint::Min(1),   // table
            Constraint::Length(1), // import input
            Constraint::Length(1), // hints
        ])
        .split(inner);

        // Tab bar
        let iwad_style = if self.active_tab == ResourceTab::Iwad {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let id24_style = if self.active_tab == ResourceTab::Id24 {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let tabs = Line::from(vec![
            Span::styled(" IWAD ", iwad_style),
            Span::raw(" "),
            Span::styled(" id24 ", id24_style),
        ]);
        frame.render_widget(Paragraph::new(tabs), layout[0]);

        // Table
        match self.active_tab {
            ResourceTab::Iwad => {
                if self.iwads.is_empty() {
                    frame.render_widget(
                        Paragraph::new("No IWADs registered").style(theme::dim_style()),
                        layout[1],
                    );
                } else {
                    let header = Row::new(vec![
                        Cell::from("Family").style(Style::default().add_modifier(Modifier::BOLD)),
                        Cell::from("Variant").style(Style::default().add_modifier(Modifier::BOLD)),
                        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
                        Cell::from("Path").style(Style::default().add_modifier(Modifier::BOLD)),
                    ])
                    .height(1);

                    let rows: Vec<Row> = self
                        .iwads
                        .iter()
                        .map(|iwad| {
                            let key = format!("{}/{}", iwad.family, iwad.variant);
                            let is_preferred = self.preferred_iwads.contains(&key);
                            let variant_display = if is_preferred {
                                format!("{}*", iwad.variant)
                            } else {
                                iwad.variant.clone()
                            };
                            Row::new(vec![
                                Cell::from(iwad.family.clone()),
                                Cell::from(variant_display),
                                Cell::from(
                                    iwad.title.clone().unwrap_or_default(),
                                ),
                                Cell::from(iwad.path.clone()),
                            ])
                        })
                        .collect();

                    let widths = [
                        Constraint::Length(12),
                        Constraint::Length(12),
                        Constraint::Min(20),
                        Constraint::Min(30),
                    ];

                    let table = Table::new(rows, widths)
                        .header(header)
                        .row_highlight_style(theme::highlight_style());
                    frame.render_stateful_widget(table, layout[1], &mut self.iwad_table);
                }
            }
            ResourceTab::Id24 => {
                if self.id24s.is_empty() {
                    frame.render_widget(
                        Paragraph::new("No id24 WADs registered").style(theme::dim_style()),
                        layout[1],
                    );
                } else {
                    let header = Row::new(vec![
                        Cell::from("Name").style(Style::default().add_modifier(Modifier::BOLD)),
                        Cell::from("Version").style(Style::default().add_modifier(Modifier::BOLD)),
                        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
                        Cell::from("Path").style(Style::default().add_modifier(Modifier::BOLD)),
                    ])
                    .height(1);

                    let rows: Vec<Row> = self
                        .id24s
                        .iter()
                        .map(|r| {
                            Row::new(vec![
                                Cell::from(r.name.clone()),
                                Cell::from(r.version.clone().unwrap_or_default()),
                                Cell::from(r.title.clone().unwrap_or_default()),
                                Cell::from(r.path.clone()),
                            ])
                        })
                        .collect();

                    let widths = [
                        Constraint::Length(15),
                        Constraint::Length(10),
                        Constraint::Min(20),
                        Constraint::Min(30),
                    ];

                    let table = Table::new(rows, widths)
                        .header(header)
                        .row_highlight_style(theme::highlight_style());
                    frame.render_stateful_widget(table, layout[1], &mut self.id24_table);
                }
            }
        }

        // Import input
        self.import_input.render(
            frame,
            layout[2],
            self.import_focused,
            "Import path: ",
        );

        // Key hints
        let hints = Line::from(vec![
            Span::styled("q/Esc", theme::key_style()),
            Span::styled(" back  ", theme::desc_style()),
            Span::styled("Tab", theme::key_style()),
            Span::styled(" switch tab  ", theme::desc_style()),
            Span::styled("a", theme::key_style()),
            Span::styled(" add  ", theme::desc_style()),
            Span::styled("d", theme::key_style()),
            Span::styled(" remove  ", theme::desc_style()),
            Span::styled("j/k", theme::key_style()),
            Span::styled(" navigate", theme::desc_style()),
        ]);
        frame.render_widget(Paragraph::new(hints), layout[3]);
    }

    fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> Option<AppMessage> {
        // Import input handling
        if self.import_focused {
            match key.code {
                KeyCode::Enter => {
                    let path_str = self.import_input.value().to_string();
                    if !path_str.is_empty() {
                        let path = PathBuf::from(&path_str);
                        // Try IWAD first, then id24
                        let result = resource_service::register_iwad(conn, &path)
                            .or_else(|_| resource_service::register_id24(conn, &path));
                        match result {
                            Ok(Some((name, _, title))) => {
                                self.import_input.reset();
                                self.import_focused = false;
                                self.load(conn);
                                return Some(AppMessage::Notify(
                                    format!("Registered: {title} ({name})"),
                                    Severity::Info,
                                ));
                            }
                            Ok(None) => {
                                return Some(AppMessage::Notify(
                                    "Not recognized as IWAD or id24".to_string(),
                                    Severity::Warning,
                                ));
                            }
                            Err(e) => {
                                return Some(AppMessage::Notify(
                                    format!("Error: {e}"),
                                    Severity::Error,
                                ));
                            }
                        }
                    }
                    self.import_focused = false;
                    return None;
                }
                KeyCode::Esc => {
                    self.import_focused = false;
                    self.import_input.reset();
                    return None;
                }
                _ => {
                    self.import_input.handle_key(key);
                    return None;
                }
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('q') | KeyCode::Esc, _) => {
                Some(AppMessage::PopScreen(ScreenResult::Cancelled))
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.active_tab = match self.active_tab {
                    ResourceTab::Iwad => ResourceTab::Id24,
                    ResourceTab::Id24 => ResourceTab::Iwad,
                };
                None
            }
            (KeyCode::Char('a'), KeyModifiers::NONE) => {
                self.import_focused = true;
                None
            }
            (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                match self.active_tab {
                    ResourceTab::Iwad => table_nav_next(&mut self.iwad_table, self.iwads.len()),
                    ResourceTab::Id24 => table_nav_next(&mut self.id24_table, self.id24s.len()),
                }
                None
            }
            (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                match self.active_tab {
                    ResourceTab::Iwad => table_nav_prev(&mut self.iwad_table, self.iwads.len()),
                    ResourceTab::Id24 => table_nav_prev(&mut self.id24_table, self.id24s.len()),
                }
                None
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                match self.active_tab {
                    ResourceTab::Iwad => {
                        if let Some(idx) = self.iwad_table.selected() {
                            if let Some(iwad) = self.iwads.get(idx) {
                                let paths = iwads::remove_iwad_with_paths(
                                    conn,
                                    &iwad.family,
                                    Some(&iwad.variant),
                                )
                                .unwrap_or_default();
                                // Delete managed files
                                let iwad_dir = config::get_iwad_dir();
                                for p in &paths {
                                    if PathBuf::from(p).starts_with(&iwad_dir) {
                                        let _ = fs::remove_file(p);
                                    }
                                }
                                self.load(conn);
                                return Some(AppMessage::Notify(
                                    "IWAD removed".to_string(),
                                    Severity::Info,
                                ));
                            }
                        }
                    }
                    ResourceTab::Id24 => {
                        if let Some(idx) = self.id24_table.selected() {
                            if let Some(r) = self.id24s.get(idx) {
                                let paths =
                                    id24::remove_id24_with_paths(conn, &r.name).unwrap_or_default();
                                let id24_dir = config::get_id24_dir();
                                for p in &paths {
                                    if PathBuf::from(p).starts_with(&id24_dir) {
                                        let _ = fs::remove_file(p);
                                    }
                                }
                                self.load(conn);
                                return Some(AppMessage::Notify(
                                    "id24 WAD removed".to_string(),
                                    Severity::Info,
                                ));
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}
