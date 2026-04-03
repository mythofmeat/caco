//! `caco random` — pick a random WAD from the library.

use clap::Args;
use rusqlite::Connection;

use caco_core::db;

#[derive(Args)]
pub struct RandomArgs {
    /// Query to filter candidates
    query: Vec<String>,

    /// Print ID, title, author (TSV)
    #[arg(long)]
    info: bool,
}

pub fn run(conn: &Connection, args: &RandomArgs) -> Result<(), String> {
    let query_str = if args.query.is_empty() {
        None
    } else {
        Some(crate::parsing::join_query_args(&args.query))
    };

    let results = db::search_wads(
        conn,
        query_str.as_deref(),
        Some("random"),
        true,
        false,
        1,
    )
    .map_err(|e| e.to_string())?;

    match results.first() {
        Some(wad) => {
            if args.info {
                println!(
                    "{}\t{}\t{}",
                    wad.id,
                    wad.title,
                    wad.author.as_deref().unwrap_or(""),
                );
            } else {
                println!("{}", wad.id);
            }
            Ok(())
        }
        None => Err("No WADs found.".to_string()),
    }
}
