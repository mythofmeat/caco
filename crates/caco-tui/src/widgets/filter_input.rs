use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::input::TextInput;

const DEBOUNCE_MS: u128 = 150;

/// State for the filter input widget.
pub struct FilterInputState {
    pub input: TextInput,
    pub focused: bool,
    last_query: String,
    debounce_at: Option<Instant>,
    pub wad_count: usize,
}

impl Default for FilterInputState {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterInputState {
    pub fn new() -> Self {
        Self {
            input: TextInput::new(),
            focused: false,
            last_query: String::new(),
            debounce_at: None,
            wad_count: 0,
        }
    }

    /// Handle a key event while the filter is focused.
    /// Returns `Some(query)` if the filter value should be applied immediately.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key.code {
            KeyCode::Enter => {
                // Apply immediately
                let query = self.input.value().to_string();
                self.last_query = query.clone();
                self.debounce_at = None;
                self.focused = false;
                return Some(query);
            }
            KeyCode::Esc => {
                if !self.input.value().is_empty() {
                    self.input.reset();
                    self.last_query.clear();
                    self.debounce_at = None;
                    self.focused = false;
                    return Some(String::new());
                }
                self.focused = false;
                return None;
            }
            _ => {
                self.input.handle_key(key);
                self.debounce_at = Some(Instant::now());
            }
        }
        None
    }

    /// Check if the debounce period has elapsed and return the new query if so.
    pub fn tick(&mut self) -> Option<String> {
        if let Some(debounce_at) = self.debounce_at {
            if debounce_at.elapsed().as_millis() >= DEBOUNCE_MS {
                let query = self.input.value().to_string();
                if query != self.last_query {
                    self.last_query = query.clone();
                    self.debounce_at = None;
                    return Some(query);
                }
                self.debounce_at = None;
            }
        }
        None
    }

    /// Get the current query value.
    pub fn query(&self) -> &str {
        self.input.value()
    }
}

/// Render the filter input.
pub fn render_filter_input(state: &FilterInputState, frame: &mut Frame, area: Rect) {
    let prefix = "/ ";
    let count_str = format!(" ({} WADs)", state.wad_count);

    let mut spans = vec![Span::styled(prefix, Style::default().fg(Color::DarkGray))];

    let value = state.input.value();
    if state.focused {
        let cursor = state.input.cursor();
        if !value.is_empty() {
            let before = &value[..cursor.min(value.len())];
            spans.push(Span::styled(before, Style::default().fg(Color::White)));
            if cursor < value.len() {
                let cursor_char = &value[cursor..cursor + 1];
                spans.push(Span::styled(
                    cursor_char,
                    Style::default().fg(Color::Black).bg(Color::White),
                ));
                let after = &value[cursor + 1..];
                spans.push(Span::styled(after, Style::default().fg(Color::White)));
            } else {
                spans.push(Span::styled(
                    " ",
                    Style::default().fg(Color::Black).bg(Color::White),
                ));
            }
        } else {
            spans.push(Span::styled(
                " ",
                Style::default().fg(Color::Black).bg(Color::White),
            ));
        }
    } else if !value.is_empty() {
        spans.push(Span::styled(value, Style::default().fg(Color::White)));
    } else {
        spans.push(Span::styled(
            "type to filter...",
            Style::default().fg(Color::DarkGray),
        ));
    }

    spans.push(Span::styled(
        count_str,
        Style::default().fg(Color::DarkGray),
    ));

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::thread::sleep;
    use std::time::Duration;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_str(state: &mut FilterInputState, s: &str) {
        for c in s.chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn enter_applies_immediately() {
        let mut state = FilterInputState::new();
        state.focused = true;
        type_str(&mut state, "abc");

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result.as_deref(), Some("abc"));
        assert!(!state.focused);
        assert!(state.debounce_at.is_none());
    }

    #[test]
    fn esc_clears_non_empty_input() {
        let mut state = FilterInputState::new();
        state.focused = true;
        type_str(&mut state, "xyz");

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result.as_deref(), Some(""));
        assert_eq!(state.query(), "");
        assert!(!state.focused);
    }

    #[test]
    fn esc_on_empty_blurs_without_emitting() {
        let mut state = FilterInputState::new();
        state.focused = true;

        let result = state.handle_key(key(KeyCode::Esc));
        assert!(result.is_none());
        assert!(!state.focused);
    }

    #[test]
    fn tick_returns_query_after_debounce() {
        let mut state = FilterInputState::new();
        state.focused = true;
        type_str(&mut state, "ab");

        assert!(state.tick().is_none());

        sleep(Duration::from_millis(DEBOUNCE_MS as u64 + 20));
        let result = state.tick();
        assert_eq!(result.as_deref(), Some("ab"));

        assert!(state.tick().is_none());
    }

    #[test]
    fn tick_does_not_emit_when_value_unchanged() {
        let mut state = FilterInputState::new();
        state.focused = true;
        type_str(&mut state, "hi");
        sleep(Duration::from_millis(DEBOUNCE_MS as u64 + 20));
        assert_eq!(state.tick().as_deref(), Some("hi"));

        state.debounce_at = Some(Instant::now() - Duration::from_millis(DEBOUNCE_MS as u64 + 10));
        assert!(state.tick().is_none());
    }

    #[test]
    fn typing_arms_debounce() {
        let mut state = FilterInputState::new();
        state.focused = true;
        assert!(state.debounce_at.is_none());

        state.handle_key(key(KeyCode::Char('a')));
        assert!(state.debounce_at.is_some());
    }
}
