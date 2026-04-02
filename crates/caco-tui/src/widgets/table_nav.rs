//! Shared table navigation helpers for j/k and Up/Down key handling.

use ratatui::widgets::TableState;

/// Move table selection down by one, clamping to `len - 1`.
pub fn table_nav_next(state: &mut TableState, len: usize) {
    if len == 0 {
        return;
    }
    let i = match state.selected() {
        Some(i) => (i + 1).min(len - 1),
        None => 0,
    };
    state.select(Some(i));
}

/// Move table selection up by one, clamping to 0.
pub fn table_nav_prev(state: &mut TableState, len: usize) {
    if len == 0 {
        return;
    }
    let i = match state.selected() {
        Some(i) => i.saturating_sub(1),
        None => 0,
    };
    state.select(Some(i));
}
