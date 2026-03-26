use std::io;
use std::path::PathBuf;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse optional --db-path argument
    let args: Vec<String> = std::env::args().collect();
    let db_path = if let Some(idx) = args.iter().position(|a| a == "--db-path") {
        args.get(idx + 1).map(PathBuf::from)
    } else {
        None
    };

    // Load config and open database
    let db_path = db_path.unwrap_or_else(caco_core::config::get_db_path);
    let conn = caco_core::db::open_connection(&db_path)?;
    caco_core::db::init_db(&conn)?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run application
    let mut app = caco_tui::app::App::new(conn);
    let result = app.run(&mut terminal);

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result?;
    Ok(())
}
