use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent};

/// Application event types.
pub enum AppEvent {
    /// Keyboard input.
    Key(KeyEvent),
    /// Terminal resized.
    Resize(u16, u16),
    /// Periodic tick (for debounce, animations, etc.).
    Tick,
}

/// Poll for the next event with a timeout.
/// Returns `Some(event)` if one is available, or `None` on timeout.
pub fn poll_event(timeout: Duration) -> std::io::Result<Option<AppEvent>> {
    if event::poll(timeout)? {
        match event::read()? {
            Event::Key(key) => Ok(Some(AppEvent::Key(key))),
            Event::Resize(w, h) => Ok(Some(AppEvent::Resize(w, h))),
            _ => Ok(None),
        }
    } else {
        Ok(Some(AppEvent::Tick))
    }
}
