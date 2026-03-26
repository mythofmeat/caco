mod commands;
mod output;
mod parsing;
mod picker;
mod resolve;

use std::process;

use clap::Parser;

use commands::Commands;

#[derive(Parser)]
#[command(
    name = "caco",
    about = "Doom WAD library manager",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("CACO_GIT_HASH"), ")")
)]
struct Cli {
    /// Override the database file path
    #[arg(long = "db", global = true, env = "CACO_DB_PATH")]
    db_path: Option<String>,

    /// Override the WAD cache directory
    #[arg(long = "cache-dir", global = true, env = "CACO_CACHE_DIR")]
    cache_dir: Option<String>,

    /// Override the per-WAD data directory
    #[arg(long = "data-dir", global = true, env = "CACO_DATA_DIR")]
    data_dir: Option<String>,

    /// Override the base data directory (~/.local/share/caco)
    #[arg(long = "home", global = true, env = "CACO_HOME")]
    home: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

fn main() {
    let cli = Cli::parse();

    // Apply CLI overrides as env vars before any config access.
    // This ensures all path resolution in caco-core picks them up,
    // and clap's `env` attribute means env vars work too.
    //
    // SAFETY: This runs at the start of main(), before any threads are spawned.
    unsafe {
        if let Some(ref p) = cli.home {
            std::env::set_var("CACO_HOME", p);
        }
        if let Some(ref p) = cli.db_path {
            std::env::set_var("CACO_DB_PATH", p);
        }
        if let Some(ref p) = cli.cache_dir {
            std::env::set_var("CACO_CACHE_DIR", p);
        }
        if let Some(ref p) = cli.data_dir {
            std::env::set_var("CACO_DATA_DIR", p);
        }
    }

    // Load config (triggers ensure_config_keys) and determine DB path.
    // get_db_path() respects CACO_DB_PATH env var > config > default.
    let _ = caco_core::config::load_config();
    let db_path = caco_core::config::get_db_path();

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
        Commands::Companion { ref command } => commands::companion::run(&conn, command),
        Commands::Profile { ref command } => commands::profile::run(&conn, command),
        Commands::Enrich(ref args) => commands::enrich::run(&conn, args),
        Commands::Gc(ref args) => commands::gc::run(&conn, args),
        Commands::Config(ref args) => commands::config::run(args),
        Commands::Completions(ref args) => commands::completions::run_completions(args),
        Commands::Complete(ref args) => commands::completions::run_complete(&conn, args),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
