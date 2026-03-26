use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState, Wrap,
};
use ratatui::Frame;

use crate::input::TextInput;
use crate::message::SearchResultEntry;
use crate::theme;

/// State for the search pane (shared between idgames and doomwiki).
pub struct SearchPaneState {
    pub search_input: TextInput,
    pub search_focused: bool,
    pub results: Vec<SearchResultEntry>,
    pub table_state: TableState,
    pub is_searching: bool,
    pub status_text: String,
}

impl Default for SearchPaneState {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchPaneState {
    pub fn new() -> Self {
        Self {
            search_input: TextInput::new(),
            search_focused: true,
            results: Vec::new(),
            table_state: TableState::default(),
            is_searching: false,
            status_text: String::new(),
        }
    }

    /// Handle key events. Returns a SearchAction if an action is needed.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<SearchAction> {
        if self.search_focused {
            match key.code {
                KeyCode::Enter => {
                    let query = self.search_input.value().to_string();
                    if !query.is_empty() {
                        self.search_focused = false;
                        self.is_searching = true;
                        self.status_text = "Searching...".to_string();
                        return Some(SearchAction::Search(query));
                    }
                }
                KeyCode::Esc => {
                    self.search_focused = false;
                }
                _ => {
                    self.search_input.handle_key(key);
                }
            }
            return None;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.search_focused = true;
                None
            }
            (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => {
                self.next();
                None
            }
            (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => {
                self.previous();
                None
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if let Some(idx) = self.table_state.selected() {
                    if let Some(entry) = self.results.get(idx) {
                        return Some(SearchAction::Import(entry.clone()));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Set search results from a background search.
    pub fn set_results(&mut self, results: Vec<SearchResultEntry>) {
        self.is_searching = false;
        self.status_text = format!("{} results", results.len());
        self.results = results;
        if !self.results.is_empty() {
            self.table_state.select(Some(0));
        } else {
            self.table_state.select(None);
        }
    }

    fn next(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => (i + 1).min(self.results.len() - 1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Get the currently selected search result.
    pub fn selected(&self) -> Option<&SearchResultEntry> {
        self.table_state
            .selected()
            .and_then(|i| self.results.get(i))
    }
}

/// Actions that the search pane requests from the parent.
pub enum SearchAction {
    Search(String),
    Import(SearchResultEntry),
}

/// Render the search pane.
pub fn render_search_pane(
    state: &mut SearchPaneState,
    frame: &mut Frame,
    area: Rect,
    source_name: &str,
    columns: &[(&str, Constraint)],
) {
    let layout = Layout::vertical([
        Constraint::Length(1), // search bar
        Constraint::Length(1), // status
        Constraint::Min(1),   // content: results + preview
    ])
    .split(area);

    // Search bar
    state
        .search_input
        .render(frame, layout[0], state.search_focused, &format!("{source_name}> "));

    // Status
    let status_style = if state.is_searching {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new(state.status_text.as_str()).style(status_style),
        layout[1],
    );

    // Content: results table + preview
    let content_layout = Layout::horizontal([
        Constraint::Percentage(60),
        Constraint::Percentage(40),
    ])
    .split(layout[2]);

    // Results table
    let header_cells: Vec<Cell> = columns
        .iter()
        .map(|(name, _)| Cell::from(*name).style(Style::default().add_modifier(Modifier::BOLD)))
        .collect();
    let header = Row::new(header_cells).height(1);

    let widths: Vec<Constraint> = columns.iter().map(|(_, w)| *w).collect();

    let rows: Vec<Row> = state
        .results
        .iter()
        .map(|entry| {
            Row::new(vec![
                Cell::from(entry.title.clone()),
                Cell::from(entry.author.clone().unwrap_or_default()),
                Cell::from(entry.extra.clone()),
            ])
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(theme::highlight_style());

    frame.render_stateful_widget(table, content_layout[0], &mut state.table_state);

    // Preview panel
    let preview_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style())
        .title(" Preview ");
    let preview_inner = preview_block.inner(content_layout[1]);
    frame.render_widget(preview_block, content_layout[1]);

    if let Some(entry) = state.selected() {
        let mut lines = vec![
            Line::from(Span::styled(&entry.title, theme::title_style())),
            Line::from(""),
        ];
        if let Some(ref author) = entry.author {
            lines.push(Line::from(vec![
                Span::styled("Author: ", theme::dim_style()),
                Span::raw(author),
            ]));
        }
        if !entry.extra.is_empty() {
            lines.push(Line::from(Span::styled(
                &entry.extra,
                theme::dim_style(),
            )));
        }
        if let Some(ref desc) = entry.description {
            lines.push(Line::from(""));
            let snippet: String = desc.chars().take(300).collect();
            lines.push(Line::from(Span::styled(snippet, theme::dim_style())));
        }
        let preview = Paragraph::new(lines).wrap(Wrap { trim: true });
        frame.render_widget(preview, preview_inner);
    }
}
