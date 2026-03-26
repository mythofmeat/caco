mod commands;
mod output;
mod parsing;
mod picker;
mod resolve;

use std::process;

use clap::Parser;

use commands::Commands;

#[derive(Parser)]
#[command(name = "caco", about = "Doom WAD library manager", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn main() {
    let cli = Cli::parse();

    // Load config and determine DB path
    let config = caco_core::config::load_config();
    let db_path = config.db_path.clone();
    let db_path = if db_path.is_empty() {
        caco_core::config::default_db_path()
    } else {
        let p = std::path::PathBuf::from(&db_path);
        if db_path.starts_with('~') {
            dirs::home_dir()
                .map(|h| h.join(db_path.trim_start_matches("~/")))
                .unwrap_or(p)
        } else {
            p
        }
    };

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent()
        && !parent.exists()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("Error creating database directory: {e}");
        process::exit(1);
    }

    // Open connection and initialize schema
    let conn = match caco_core::db::open_connection(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error opening database: {e}");
            process::exit(1);
        }
    };

    if let Err(e) = caco_core::db::init_db(&conn) {
        eprintln!("Error initializing database: {e}");
        process::exit(1);
    }

    // Dispatch to command handler
    let result = match cli.command {
        Commands::Ls(ref args) => commands::ls::run(&conn, args),
        Commands::Info(ref args) | Commands::InfoAlias(ref args) => commands::info::run(&conn, args),
        Commands::Modify(ref args) => commands::modify::run(&conn, args),
        Commands::Trash(ref args) => commands::trash::run(&conn, args),
        Commands::Random(ref args) => commands::random::run(&conn, args),
        Commands::Import(ref args) => commands::import::run(&conn, args),
        Commands::Play(ref args) => commands::play::run(&conn, args),
        Commands::Cache { ref command } => commands::cache::run(&conn, command),
        Commands::Stats(ref args) => commands::stats::run_stats(&conn, args),
        Commands::Sessions(ref args) => commands::stats::run_sessions(&conn, args),
        Commands::Saves { ref command } => commands::saves::run(&conn, command),
        Commands::Demos { ref command } => commands::demos::run(&conn, command),
        Commands::Profile { ref command } => commands::profile::run(&conn, command),
        Commands::Config(ref args) => commands::config::run(args),
        Commands::Completions(ref args) => commands::completions::run_completions(args),
        Commands::Complete(ref args) => commands::completions::run_complete(&conn, args),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
