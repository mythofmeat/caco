use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Available sort fields.
pub const SORT_FIELDS: &[(&str, &str)] = &[
    ("id", "ID"),
    ("title", "Title"),
    ("author", "Author"),
    ("playtime", "Playtime"),
    ("last_played", "Last Played"),
    ("year", "Year"),
    ("rating", "Rating"),
];

/// State for the sort selector widget.
pub struct SortSelectState {
    pub current_index: usize,
    pub sort_desc: bool,
}

impl Default for SortSelectState {
    fn default() -> Self {
        Self::new()
    }
}

impl SortSelectState {
    pub fn new() -> Self {
        Self {
            current_index: 0,
            sort_desc: false,
        }
    }

    pub fn from_config(sort_field: &str, sort_desc: bool) -> Self {
        let index = SORT_FIELDS
            .iter()
            .position(|(f, _)| *f == sort_field)
            .unwrap_or(0);
        Self {
            current_index: index,
            sort_desc,
        }
    }

    /// Cycle to the next sort field.
    pub fn cycle(&mut self) {
        self.current_index = (self.current_index + 1) % SORT_FIELDS.len();
    }

    /// Toggle sort direction.
    pub fn toggle_direction(&mut self) {
        self.sort_desc = !self.sort_desc;
    }

    /// Get the current sort field database name.
    pub fn field(&self) -> &str {
        SORT_FIELDS[self.current_index].0
    }

    /// Get the current sort field display name.
    pub fn field_display(&self) -> &str {
        SORT_FIELDS[self.current_index].1
    }

    /// Get the sort direction arrow.
    pub fn direction_arrow(&self) -> &str {
        if self.sort_desc { "↓" } else { "↑" }
    }
}

/// Render the sort selector inline.
pub fn render_sort_select(state: &SortSelectState, frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled("Sort: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            state.field_display(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", state.direction_arrow()),
            Style::default().fg(Color::Cyan),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
