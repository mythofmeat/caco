pub mod cache;
pub mod confirm_delete;
pub mod resources;
pub mod sessions;
pub mod stats;
pub mod tabbed_library;
pub mod wad_detail;
pub mod wad_edit;
pub mod wad_stats;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use rusqlite::Connection;

use crate::message::{AppMessage, ScreenResult};

/// Trait for all TUI screens.
pub trait Screen {
    /// Render the screen into the given area.
    fn render(&mut self, frame: &mut Frame, area: Rect, conn: &Connection);

    /// Handle a key event. Return Some(message) to communicate with the App.
    fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> Option<AppMessage>;

    /// Called on each tick (50ms). For timers, debounce, etc.
    fn tick(&mut self, _conn: &Connection) -> Option<AppMessage> {
        None
    }

    /// Called when the terminal is resized.
    fn on_resize(&mut self, _width: u16, _height: u16) {}

    /// Whether this screen is a modal overlay.
    fn is_modal(&self) -> bool {
        false
    }

    /// Called when this screen becomes active again after a pushed screen pops.
    fn on_resume(&mut self, _conn: &Connection, _result: Option<ScreenResult>) {}
}
