use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

/// Wrapper around tui-input's Input for text editing in the TUI.
pub struct TextInput {
    inner: Input,
}

impl TextInput {
    pub fn new() -> Self {
        Self {
            inner: Input::default(),
        }
    }

    pub fn with_value(value: &str) -> Self {
        Self {
            inner: Input::new(value.to_string()),
        }
    }

    /// Handle a key event. Returns true if the event was consumed.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.inner.handle_event(&crossterm::event::Event::Key(key));
        true
    }

    /// Get the current input value.
    pub fn value(&self) -> &str {
        self.inner.value()
    }

    /// Reset the input to empty.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// Set the input to a specific value.
    pub fn set_value(&mut self, value: &str) {
        self.inner = Input::new(value.to_string());
    }

    /// Cursor position in the input.
    pub fn cursor(&self) -> usize {
        self.inner.cursor()
    }

    /// Render the input as a Paragraph widget.
    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool, prefix: &str) {
        let style = if focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        let cursor_style = if focused {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            style
        };

        let value = self.inner.value();
        let cursor_pos = self.inner.cursor();

        // Build spans: prefix + text before cursor + cursor char + text after cursor
        let mut spans = vec![Span::styled(prefix, Style::default().fg(Color::DarkGray))];

        if focused && !value.is_empty() {
            let before = &value[..cursor_pos.min(value.len())];
            spans.push(Span::styled(before, style));

            if cursor_pos < value.len() {
                let cursor_char = &value[cursor_pos..cursor_pos + 1];
                spans.push(Span::styled(cursor_char, cursor_style));
                let after = &value[cursor_pos + 1..];
                spans.push(Span::styled(after, style));
            } else {
                spans.push(Span::styled(" ", cursor_style));
            }
        } else if focused {
            spans.push(Span::styled(" ", cursor_style));
        } else {
            spans.push(Span::styled(value, style));
        }

        let paragraph = Paragraph::new(Line::from(spans));
        frame.render_widget(paragraph, area);
    }
}

impl Default for TextInput {
    fn default() -> Self {
        Self::new()
    }
}
