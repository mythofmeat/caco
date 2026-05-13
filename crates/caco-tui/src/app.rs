use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use rusqlite::Connection;

use crate::event::{self, AppEvent};
use crate::message::{AppMessage, ScreenId, ScreenResult, Severity};
use crate::screens::Screen;
use crate::screens::cache::CacheScreen;
use crate::screens::cacowards::CacowardsScreen;
use crate::screens::confirm_delete::ConfirmDeleteScreen;
use crate::screens::resources::ResourcesScreen;
use crate::screens::sessions::SessionsScreen;
use crate::screens::stats::StatsScreen;
use crate::screens::tabbed_library::TabbedLibraryScreen;
use crate::screens::wad_detail::WadDetailScreen;
use crate::screens::wad_edit::WadEditScreen;
use crate::screens::wad_stats::WadStatsScreen;
use crate::theme;

const TICK_RATE: Duration = Duration::from_millis(50);
const NOTIFICATION_DURATION: Duration = Duration::from_secs(3);

/// Notification state.
struct Notification {
    message: String,
    severity: Severity,
    expires_at: Instant,
}

/// The main application struct that manages the screen stack and event loop.
pub struct App {
    conn: Connection,
    screen_stack: Vec<Box<dyn Screen>>,
    should_quit: bool,
    notification: Option<Notification>,
    bg_tx: mpsc::Sender<AppMessage>,
    bg_rx: mpsc::Receiver<AppMessage>,
    terminal_width: u16,
    terminal_height: u16,
}

impl App {
    pub fn new(conn: Connection) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel();
        let initial_screen = TabbedLibraryScreen::new(&conn, bg_tx.clone());
        Self {
            conn,
            screen_stack: vec![Box::new(initial_screen)],
            should_quit: false,
            notification: None,
            bg_tx,
            bg_rx,
            terminal_width: 0,
            terminal_height: 0,
        }
    }

    /// Get a sender for background threads to communicate with the app.
    pub fn bg_sender(&self) -> mpsc::Sender<AppMessage> {
        self.bg_tx.clone()
    }

    /// Main event loop.
    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
        let size = terminal.size()?;
        self.terminal_width = size.width;
        self.terminal_height = size.height;

        loop {
            // Render
            terminal.draw(|frame| self.render(frame))?;

            if self.should_quit {
                break;
            }

            // Poll events
            match event::poll_event(TICK_RATE)? {
                Some(AppEvent::Key(key)) => {
                    // Ignore key release events (crossterm sends both press and release)
                    if key.kind != crossterm::event::KeyEventKind::Press {
                        continue;
                    }
                    self.handle_key(key);
                }
                Some(AppEvent::Resize(w, h)) => {
                    self.terminal_width = w;
                    self.terminal_height = h;
                    if let Some(screen) = self.screen_stack.last_mut() {
                        screen.on_resize(w, h);
                    }
                }
                Some(AppEvent::Tick) => {
                    self.tick();
                }
                None => {}
            }

            // Drain background messages
            self.drain_bg_messages();

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();

        // Reserve bottom line for notification
        let (content_area, notif_area) = if self.notification.is_some() {
            let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };

        // Render screen stack: bottom screen full, modals on top
        let stack_len = self.screen_stack.len();
        for (i, screen) in self.screen_stack.iter_mut().enumerate() {
            if i == 0 || !screen.is_modal() {
                screen.render(frame, content_area, &self.conn);
            } else {
                // Modal: render a dimmed overlay then the modal centered
                let modal_area = centered_rect(70, 60, content_area);
                frame.render_widget(Clear, modal_area);
                screen.render(frame, modal_area, &self.conn);
            }
            // Only render the top screen (and base) - skip middle ones
            if i > 0 && i < stack_len - 1 {
                continue;
            }
        }

        // Render notification
        if let Some(ref notif) = self.notification {
            if let Some(area) = notif_area {
                let style = theme::notify_style(notif.severity.as_str());
                let prefix = match notif.severity {
                    Severity::Error => "✗ ",
                    Severity::Warning => "! ",
                    Severity::Info => "✓ ",
                };
                let line = Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(&notif.message, style),
                ]);
                frame.render_widget(Paragraph::new(line), area);
            }
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Let top screen handle the key
        if let Some(screen) = self.screen_stack.last_mut() {
            if let Some(msg) = screen.handle_key(key, &self.conn) {
                self.process_message(msg);
            }
        }
    }

    fn tick(&mut self) {
        // Expire notifications
        if let Some(ref notif) = self.notification {
            if Instant::now() >= notif.expires_at {
                self.notification = None;
            }
        }

        // Let top screen tick
        let msg = if let Some(screen) = self.screen_stack.last_mut() {
            screen.tick(&self.conn)
        } else {
            None
        };
        if let Some(msg) = msg {
            self.process_message(msg);
        }
    }

    fn drain_bg_messages(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            self.process_message(msg);
        }
    }

    fn process_message(&mut self, msg: AppMessage) {
        match msg {
            AppMessage::Quit => {
                self.should_quit = true;
            }
            AppMessage::PushScreen(screen_id) => {
                self.push_screen(screen_id);
            }
            AppMessage::PopScreen(result) => {
                self.pop_screen(Some(result));
            }
            AppMessage::Notify(message, severity) => {
                self.notification = Some(Notification {
                    message,
                    severity,
                    expires_at: Instant::now() + NOTIFICATION_DURATION,
                });
            }
            AppMessage::WadUpdated(wad_id) => {
                // Propagate to all screens
                for screen in &mut self.screen_stack {
                    screen.on_resume(&self.conn, Some(ScreenResult::Saved));
                }
                self.notification = Some(Notification {
                    message: format!("WAD #{wad_id} updated"),
                    severity: Severity::Info,
                    expires_at: Instant::now() + NOTIFICATION_DURATION,
                });
            }
            AppMessage::WadImported(wad_id) => {
                for screen in &mut self.screen_stack {
                    screen.on_resume(&self.conn, Some(ScreenResult::Saved));
                }
                self.notification = Some(Notification {
                    message: format!("WAD #{wad_id} imported"),
                    severity: Severity::Info,
                    expires_at: Instant::now() + NOTIFICATION_DURATION,
                });
            }
            AppMessage::WadDeleted(wad_id) => {
                for screen in &mut self.screen_stack {
                    screen.on_resume(&self.conn, Some(ScreenResult::Confirmed(wad_id)));
                }
                self.notification = Some(Notification {
                    message: format!("WAD #{wad_id} trashed"),
                    severity: Severity::Info,
                    expires_at: Instant::now() + NOTIFICATION_DURATION,
                });
            }
            AppMessage::RefreshLibrary => {
                for screen in &mut self.screen_stack {
                    screen.on_resume(&self.conn, None);
                }
            }
            AppMessage::PlayWad(wad_id) => {
                self.play_wad(wad_id);
            }
            AppMessage::SearchComplete(source, results) => {
                // Forward to the base screen (TabbedLibraryScreen), which owns the import pane.
                if let Some(screen) = self.screen_stack.first_mut() {
                    screen.on_search_complete(source, results);
                }
            }
            AppMessage::ImportComplete(result) => match result {
                Ok(import_result) => {
                    if let Some(wad_id) = import_result.wad_id {
                        self.process_message(AppMessage::WadImported(wad_id));
                    } else if import_result.is_duplicate {
                        let title = import_result
                            .duplicate_title
                            .unwrap_or_else(|| "unknown".to_string());
                        self.notification = Some(Notification {
                            message: format!("Duplicate: {title}"),
                            severity: Severity::Warning,
                            expires_at: Instant::now() + NOTIFICATION_DURATION,
                        });
                    }
                }
                Err(e) => {
                    self.notification = Some(Notification {
                        message: format!("Import failed: {e}"),
                        severity: Severity::Error,
                        expires_at: Instant::now() + NOTIFICATION_DURATION,
                    });
                }
            },
        }
    }

    fn push_screen(&mut self, screen_id: ScreenId) {
        let screen: Box<dyn Screen> = match screen_id {
            ScreenId::WadDetail(wad_id) => Box::new(WadDetailScreen::new(wad_id, &self.conn)),
            ScreenId::WadEdit(wad_id) => Box::new(WadEditScreen::new(wad_id, &self.conn)),
            ScreenId::Sessions(wad_id) => Box::new(SessionsScreen::new(wad_id, &self.conn)),
            ScreenId::ConfirmDelete(wad_id) => {
                Box::new(ConfirmDeleteScreen::new(wad_id, &self.conn))
            }
            ScreenId::Stats => Box::new(StatsScreen::new(&self.conn)),
            ScreenId::WadStats(wad_id) => Box::new(WadStatsScreen::new(wad_id, &self.conn)),
            ScreenId::Cache => Box::new(CacheScreen::new(&self.conn)),
            ScreenId::Resources => Box::new(ResourcesScreen::new(&self.conn)),
            ScreenId::Cacowards => Box::new(CacowardsScreen::new(&self.conn)),
        };
        self.screen_stack.push(screen);
    }

    fn pop_screen(&mut self, result: Option<ScreenResult>) {
        if self.screen_stack.len() > 1 {
            self.screen_stack.pop();
            if let Some(screen) = self.screen_stack.last_mut() {
                screen.on_resume(&self.conn, result);
            }
        }
    }

    fn play_wad(&mut self, wad_id: i64) {
        use caco_core::player::{self, PlayOptions};

        // Suspend terminal
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);

        // Play the WAD
        let opts = PlayOptions::default();
        let result = player::play(&self.conn, wad_id, &opts);

        // Restore terminal
        let _ = enable_raw_mode();
        let _ = execute!(io::stdout(), EnterAlternateScreen);

        match result {
            Ok(play_result) => {
                let msg = if play_result.crashed() {
                    let code = play_result.exit_code.unwrap_or(-1);
                    format!("Sourceport exited with code {code}")
                } else if let Some(dur) = play_result.duration {
                    format!("Played for {}", player::format_duration(dur))
                } else {
                    "Play session ended".to_string()
                };
                let severity = if play_result.crashed() {
                    Severity::Warning
                } else {
                    Severity::Info
                };
                self.notification = Some(Notification {
                    message: msg,
                    severity,
                    expires_at: Instant::now() + NOTIFICATION_DURATION,
                });
                // Refresh library after play
                for screen in &mut self.screen_stack {
                    screen.on_resume(&self.conn, Some(ScreenResult::Saved));
                }
            }
            Err(e) => {
                self.notification = Some(Notification {
                    message: format!("Play failed: {e}"),
                    severity: Severity::Error,
                    expires_at: Instant::now() + NOTIFICATION_DURATION,
                });
            }
        }
    }
}

/// Helper to create a centered rect (percentage-based).
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
