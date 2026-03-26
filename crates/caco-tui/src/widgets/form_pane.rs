use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::input::TextInput;

/// A single form field.
pub struct FormField {
    pub name: String,
    pub label: String,
    pub input: TextInput,
    pub required: bool,
}

impl FormField {
    pub fn new(name: &str, label: &str, required: bool) -> Self {
        Self {
            name: name.to_string(),
            label: label.to_string(),
            input: TextInput::new(),
            required,
        }
    }
}

/// Which import source this form serves.
#[derive(Clone, Copy, PartialEq)]
pub enum FormKind {
    Doomworld,
    Url,
    Local,
}

/// State for the form pane (shared between doomworld URL, URL import, local file).
pub struct FormPaneState {
    pub kind: FormKind,
    pub fields: Vec<FormField>,
    pub active_field: usize,
    pub status_text: String,
    pub is_submitting: bool,
}

impl FormPaneState {
    pub fn new(kind: FormKind) -> Self {
        let fields = match kind {
            FormKind::Doomworld => vec![
                FormField::new("url", "Doomworld URL", true),
                FormField::new("title", "Title", false),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
            ],
            FormKind::Url => vec![
                FormField::new("title", "Title", true),
                FormField::new("url", "URL", true),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
                FormField::new("notes", "Notes", false),
            ],
            FormKind::Local => vec![
                FormField::new("path", "File Path", true),
                FormField::new("title", "Title", true),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
            ],
        };

        Self {
            kind,
            fields,
            active_field: 0,
            status_text: String::new(),
            is_submitting: false,
        }
    }

    /// Handle key events. Returns a FormAction if action is needed.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<FormAction> {
        match (key.code, key.modifiers) {
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.active_field = (self.active_field + 1) % self.fields.len();
                None
            }
            (KeyCode::BackTab, KeyModifiers::SHIFT) => {
                if self.active_field == 0 {
                    self.active_field = self.fields.len() - 1;
                } else {
                    self.active_field -= 1;
                }
                None
            }
            (KeyCode::Enter, KeyModifiers::CONTROL) => {
                // Validate required fields
                for field in &self.fields {
                    if field.required && field.input.value().is_empty() {
                        self.status_text = format!("{} is required", field.label);
                        return None;
                    }
                }
                self.is_submitting = true;
                self.status_text = "Importing...".to_string();

                // Collect field values
                let values: Vec<(String, String)> = self
                    .fields
                    .iter()
                    .map(|f| (f.name.clone(), f.input.value().to_string()))
                    .collect();
                Some(FormAction::Submit(values))
            }
            _ => {
                // Route to active field
                if let Some(field) = self.fields.get_mut(self.active_field) {
                    field.input.handle_key(key);
                }
                None
            }
        }
    }

    /// Get a field value by name.
    pub fn get_value(&self, name: &str) -> &str {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.input.value())
            .unwrap_or("")
    }

    /// Set a field value by name.
    pub fn set_value(&mut self, name: &str, value: &str) {
        if let Some(field) = self.fields.iter_mut().find(|f| f.name == name) {
            field.input.set_value(value);
        }
    }

    /// Reset all fields.
    pub fn reset(&mut self) {
        for field in &mut self.fields {
            field.input.reset();
        }
        self.active_field = 0;
        self.status_text.clear();
        self.is_submitting = false;
    }
}

/// Actions that the form pane requests from the parent.
pub enum FormAction {
    Submit(Vec<(String, String)>),
}

/// Render the form pane.
pub fn render_form_pane(state: &FormPaneState, frame: &mut Frame, area: Rect) {
    let field_count = state.fields.len();
    let mut constraints: Vec<Constraint> = Vec::new();
    for _ in 0..field_count {
        constraints.push(Constraint::Length(2)); // label + input
    }
    constraints.push(Constraint::Length(1)); // status
    constraints.push(Constraint::Min(0)); // spacer

    let layout = Layout::vertical(constraints).split(area);

    for (i, field) in state.fields.iter().enumerate() {
        let field_area = layout[i];
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)])
            .split(field_area);

        // Label
        let label_style = if field.required {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = if field.required { "*" } else { " " };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Red)),
                Span::styled(&field.label, label_style),
            ])),
            rows[0],
        );

        // Input
        let is_active = i == state.active_field;
        field
            .input
            .render(frame, rows[1], is_active, "  ");
    }

    // Status line
    let status_idx = field_count;
    if status_idx < layout.len() {
        let status_style = if state.is_submitting {
            Style::default().fg(Color::Yellow)
        } else if state.status_text.contains("required") || state.status_text.contains("failed") {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        frame.render_widget(
            Paragraph::new(state.status_text.as_str()).style(status_style),
            layout[status_idx],
        );
    }
}
