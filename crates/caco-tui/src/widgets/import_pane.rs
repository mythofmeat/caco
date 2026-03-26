use std::sync::mpsc;
use std::thread;

use caco_sources::import_service::ImportService;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use rusqlite::Connection;

use crate::message::{AppMessage, SearchResultEntry, SearchSource, SearchSourceData};
use crate::theme;
use crate::widgets::form_pane::{self, FormAction, FormKind, FormPaneState};
use crate::widgets::search_pane::{self, SearchAction, SearchPaneState};

/// Import source tabs.
const SOURCES: &[(&str, &str)] = &[
    ("1", "idgames"),
    ("2", "doomwiki"),
    ("3", "doomworld"),
    ("4", "URL"),
    ("5", "local"),
];

/// State for the import pane.
pub struct ImportPaneState {
    pub active_source: usize,
    pub idgames_search: SearchPaneState,
    pub doomwiki_search: SearchPaneState,
    pub doomworld_form: FormPaneState,
    pub url_form: FormPaneState,
    pub local_form: FormPaneState,
    bg_tx: mpsc::Sender<AppMessage>,
}

impl ImportPaneState {
    pub fn new(bg_tx: mpsc::Sender<AppMessage>) -> Self {
        Self {
            active_source: 0,
            idgames_search: SearchPaneState::new(),
            doomwiki_search: SearchPaneState::new(),
            doomworld_form: FormPaneState::new(FormKind::Doomworld),
            url_form: FormPaneState::new(FormKind::Url),
            local_form: FormPaneState::new(FormKind::Local),
            bg_tx,
        }
    }

    /// Handle key events.
    pub fn handle_key(&mut self, key: KeyEvent, _conn: &Connection) -> Option<AppMessage> {
        // Source switching with number keys (only if not in a text input)
        let in_text = match self.active_source {
            0 => self.idgames_search.search_focused,
            1 => self.doomwiki_search.search_focused,
            2..=4 => true,
            _ => false,
        };

        if !in_text {
            match key.code {
                KeyCode::Char('1') => {
                    self.active_source = 0;
                    return None;
                }
                KeyCode::Char('2') => {
                    self.active_source = 1;
                    return None;
                }
                KeyCode::Char('3') => {
                    self.active_source = 2;
                    return None;
                }
                KeyCode::Char('4') => {
                    self.active_source = 3;
                    return None;
                }
                KeyCode::Char('5') => {
                    self.active_source = 4;
                    return None;
                }
                _ => {}
            }
        }

        // Route to active source
        match self.active_source {
            0 => {
                if let Some(action) = self.idgames_search.handle_key(key) {
                    self.handle_search_action(action, SearchSource::Idgames);
                }
            }
            1 => {
                if let Some(action) = self.doomwiki_search.handle_key(key) {
                    self.handle_search_action(action, SearchSource::Doomwiki);
                }
            }
            2 => {
                if let Some(action) = self.doomworld_form.handle_key(key) {
                    self.handle_form_action(action, FormKind::Doomworld);
                }
            }
            3 => {
                if let Some(action) = self.url_form.handle_key(key) {
                    self.handle_form_action(action, FormKind::Url);
                }
            }
            4 => {
                if let Some(action) = self.local_form.handle_key(key) {
                    self.handle_form_action(action, FormKind::Local);
                }
            }
            _ => {}
        }

        None
    }

    fn handle_search_action(&mut self, action: SearchAction, source: SearchSource) {
        match action {
            SearchAction::Search(query) => {
                let tx = self.bg_tx.clone();
                let source_clone = match source {
                    SearchSource::Idgames => SearchSource::Idgames,
                    SearchSource::Doomwiki => SearchSource::Doomwiki,
                };
                thread::spawn(move || {
                    let results = match source_clone {
                        SearchSource::Idgames => search_idgames(&query),
                        SearchSource::Doomwiki => search_doomwiki(&query),
                    };
                    let _ = tx.send(AppMessage::SearchComplete(source, results));
                });
            }
            SearchAction::Import(entry) => {
                self.import_search_result(entry, &source);
            }
        }
    }

    fn import_search_result(&mut self, entry: SearchResultEntry, source: &SearchSource) {
        let tx = self.bg_tx.clone();
        let db_path = caco_core::config::get_db_path();
        let source_id = entry.source_id.clone();

        match source {
            SearchSource::Idgames => {
                thread::spawn(move || {
                    let result: Result<_, String> = (|| {
                        let conn =
                            caco_core::db::open_connection(&db_path).map_err(|e| e.to_string())?;
                        let client = caco_sources::idgames::IdgamesClient::new();
                        let id: i64 = source_id.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
                        let file_entry = client
                            .get(Some(id), None)
                            .map_err(|e| e.to_string())?;
                        let service = ImportService;
                        let result = service.import_idgames(&conn, &file_entry, None, false);
                        if let Some(err) = &result.error {
                            return Err(err.clone());
                        }
                        Ok(result)
                    })();
                    let _ = tx.send(AppMessage::ImportComplete(result));
                });
            }
            SearchSource::Doomwiki => {
                thread::spawn(move || {
                    let result: Result<_, String> = (|| {
                        let conn =
                            caco_core::db::open_connection(&db_path).map_err(|e| e.to_string())?;
                        let client = caco_sources::doomwiki::DoomwikiClient::new();
                        let wiki_entry = client
                            .get_entry(&source_id)
                            .map_err(|e| e.to_string())?
                            .ok_or_else(|| "Wiki page not found".to_string())?;
                        let service = ImportService;
                        let result = service.import_doomwiki(&conn, &wiki_entry, None, false);
                        if let Some(err) = &result.error {
                            return Err(err.clone());
                        }
                        Ok(result)
                    })();
                    let _ = tx.send(AppMessage::ImportComplete(result));
                });
            }
        }
    }

    fn handle_form_action(&mut self, action: FormAction, kind: FormKind) {
        match action {
            FormAction::Submit(values) => {
                let tx = self.bg_tx.clone();
                let db_path = caco_core::config::get_db_path();

                match kind {
                    FormKind::Url => {
                        let title = get_field(&values, "title");
                        let url = get_field(&values, "url");
                        let author = get_opt_field(&values, "author");
                        let year =
                            get_opt_field(&values, "year").and_then(|y| y.parse::<i32>().ok());
                        let tags = parse_tags(&values);
                        let notes = get_opt_field(&values, "notes");

                        thread::spawn(move || {
                            let result: Result<_, String> = (|| {
                                let conn = caco_core::db::open_connection(&db_path)
                                    .map_err(|e| e.to_string())?;
                                let service = ImportService;
                                let result = service.import_url(
                                    &conn,
                                    &title,
                                    &url,
                                    author.as_deref(),
                                    year,
                                    notes.as_deref(),
                                    if tags.is_empty() { None } else { Some(tags) },
                                    false,
                                );
                                if let Some(err) = &result.error {
                                    return Err(err.clone());
                                }
                                Ok(result)
                            })();
                            let _ = tx.send(AppMessage::ImportComplete(result));
                        });
                    }
                    FormKind::Local => {
                        let path = get_field(&values, "path");
                        let title = get_field(&values, "title");
                        let author = get_opt_field(&values, "author");
                        let year =
                            get_opt_field(&values, "year").and_then(|y| y.parse::<i32>().ok());
                        let tags = parse_tags(&values);

                        thread::spawn(move || {
                            let result: Result<_, String> = (|| {
                                let conn = caco_core::db::open_connection(&db_path)
                                    .map_err(|e| e.to_string())?;
                                let service = ImportService;
                                let result = service.import_local(
                                    &conn,
                                    &title,
                                    &std::path::PathBuf::from(&path),
                                    author.as_deref(),
                                    year,
                                    None,
                                    if tags.is_empty() { None } else { Some(tags) },
                                    false,
                                );
                                if let Some(err) = &result.error {
                                    return Err(err.clone());
                                }
                                Ok(result)
                            })();
                            let _ = tx.send(AppMessage::ImportComplete(result));
                        });
                    }
                    FormKind::Doomworld => {
                        let url = get_field(&values, "url");
                        let title_override = get_opt_field(&values, "title");
                        let author_override = get_opt_field(&values, "author");
                        let year_override =
                            get_opt_field(&values, "year").and_then(|y| y.parse::<i32>().ok());
                        let tags = parse_tags(&values);

                        thread::spawn(move || {
                            let result: Result<_, String> = (|| {
                                let conn = caco_core::db::open_connection(&db_path)
                                    .map_err(|e| e.to_string())?;
                                let client = caco_sources::doomworld::DoomworldClient::new();
                                let thread = client
                                    .get_thread(&url)
                                    .map_err(|e| e.to_string())?;
                                let service = ImportService;
                                let result = service.import_doomworld(
                                    &conn,
                                    &thread,
                                    if tags.is_empty() { None } else { Some(tags) },
                                    title_override.as_deref(),
                                    author_override.as_deref(),
                                    year_override,
                                    None,
                                    None,
                                    false,
                                );
                                if let Some(err) = &result.error {
                                    return Err(err.clone());
                                }
                                Ok(result)
                            })();
                            let _ = tx.send(AppMessage::ImportComplete(result));
                        });
                    }
                }
            }
        }
    }

    /// Process search results from background thread.
    pub fn on_search_complete(&mut self, source: SearchSource, results: Vec<SearchResultEntry>) {
        match source {
            SearchSource::Idgames => self.idgames_search.set_results(results),
            SearchSource::Doomwiki => self.doomwiki_search.set_results(results),
        }
    }
}

/// Render the import pane.
pub fn render_import_pane(state: &mut ImportPaneState, frame: &mut Frame, area: Rect) {
    let layout =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);

    // Source selector tabs
    let mut tab_spans: Vec<Span> = Vec::new();
    for (i, (key, name)) in SOURCES.iter().enumerate() {
        if i > 0 {
            tab_spans.push(Span::raw("  "));
        }
        let style = if i == state.active_source {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        tab_spans.push(Span::styled(*key, theme::key_style()));
        tab_spans.push(Span::styled(format!(" {name}"), style));
    }
    frame.render_widget(Paragraph::new(Line::from(tab_spans)), layout[0]);

    // Content
    let content = layout[1];
    match state.active_source {
        0 => {
            let columns = &[
                ("Title", Constraint::Min(20)),
                ("Author", Constraint::Length(20)),
                ("Rating/Date", Constraint::Length(15)),
            ];
            search_pane::render_search_pane(
                &mut state.idgames_search,
                frame,
                content,
                "idgames",
                columns,
            );
        }
        1 => {
            let columns = &[
                ("Title", Constraint::Min(20)),
                ("Author", Constraint::Length(20)),
                ("Year/Port", Constraint::Length(15)),
            ];
            search_pane::render_search_pane(
                &mut state.doomwiki_search,
                frame,
                content,
                "doomwiki",
                columns,
            );
        }
        2 => form_pane::render_form_pane(&state.doomworld_form, frame, content),
        3 => form_pane::render_form_pane(&state.url_form, frame, content),
        4 => form_pane::render_form_pane(&state.local_form, frame, content),
        _ => {}
    }
}

/// Search idgames (runs in background thread).
fn search_idgames(query: &str) -> Vec<SearchResultEntry> {
    let client = caco_sources::idgames::IdgamesClient::new();
    let entries = match client.search(query, None, None, None) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    entries
        .into_iter()
        .map(|entry| {
            let rating_str = if entry.rating > 0.0 {
                format!("{:.1}", entry.rating)
            } else {
                String::new()
            };
            let extra = format!(
                "{}  {}",
                rating_str,
                if entry.date.is_empty() {
                    ""
                } else {
                    &entry.date
                }
            );
            SearchResultEntry {
                title: entry.title.clone(),
                author: if entry.author.is_empty() {
                    None
                } else {
                    Some(entry.author.clone())
                },
                extra,
                description: if entry.description.is_empty() {
                    None
                } else {
                    Some(entry.description.clone())
                },
                source_id: entry.id.to_string(),
                source_data: SearchSourceData::Idgames {
                    id: entry.id,
                    rating: if entry.rating > 0.0 {
                        Some(entry.rating)
                    } else {
                        None
                    },
                    date: if entry.date.is_empty() {
                        None
                    } else {
                        Some(entry.date.clone())
                    },
                    filename: if entry.filename.is_empty() {
                        None
                    } else {
                        Some(entry.filename.clone())
                    },
                },
            }
        })
        .collect()
}

/// Search doomwiki (runs in background thread).
fn search_doomwiki(query: &str) -> Vec<SearchResultEntry> {
    let client = caco_sources::doomwiki::DoomwikiClient::new();
    let entries = match client.search_wads(query, 50) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    entries
        .into_iter()
        .map(|entry| {
            let year_str = entry.year.map(|y: i32| y.to_string()).unwrap_or_default();
            let extra = format!(
                "{}  {}",
                year_str,
                if entry.port.is_empty() {
                    ""
                } else {
                    &entry.port
                }
            );
            SearchResultEntry {
                title: entry.title.clone(),
                author: if entry.author.is_empty() {
                    None
                } else {
                    Some(entry.author.clone())
                },
                extra,
                description: if entry.description.is_empty() {
                    None
                } else {
                    Some(entry.description.clone())
                },
                source_id: entry.title.clone(),
                source_data: SearchSourceData::Doomwiki {
                    year: entry.year,
                    iwad: if entry.iwad.is_empty() {
                        None
                    } else {
                        Some(entry.iwad.clone())
                    },
                    port: if entry.port.is_empty() {
                        None
                    } else {
                        Some(entry.port.clone())
                    },
                },
            }
        })
        .collect()
}

fn get_field(values: &[(String, String)], name: &str) -> String {
    values
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn get_opt_field(values: &[(String, String)], name: &str) -> Option<String> {
    values
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

fn parse_tags(values: &[(String, String)]) -> Vec<String> {
    get_opt_field(values, "tags")
        .unwrap_or_default()
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}
