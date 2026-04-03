//! `caco collection` — manage smart collections (saved queries).

use std::collections::HashMap;

use clap::Subcommand;
use rusqlite::Connection;

use caco_core::db;
use crate::output::{self, OutputFormat};

#[derive(Subcommand)]
pub enum CollectionCommand {
    /// Create a new smart collection
    Add {
        /// Collection name
        name: String,
        /// Query terms (same syntax as `caco ls`)
        query: Vec<String>,
        /// Sort field
        #[arg(long)]
        sort: Option<String>,
        /// Sort descending
        #[arg(long)]
        desc: bool,
    },
    /// Delete a smart collection
    Rm {
        /// Collection name
        name: String,
    },
    /// List all smart collections
    Ls {
        /// Output format
        #[arg(short, long, default_value = "table")]
        output: String,
    },
    /// Run a collection's query and show results
    Run {
        /// Collection name
        name: String,
        /// Output format
        #[arg(short, long, default_value = "table")]
        output: String,
    },
}

pub fn run(conn: &Connection, cmd: &CollectionCommand) -> Result<(), String> {
    match cmd {
        CollectionCommand::Add { name, query, sort, desc } => {
            let query_str = crate::parsing::join_query_args(query);
            cmd_add(conn, name, &query_str, sort.as_deref(), *desc)
        }
        CollectionCommand::Rm { name } => cmd_rm(conn, name),
        CollectionCommand::Ls { output } => cmd_ls(conn, output),
        CollectionCommand::Run { name, output } => cmd_run(conn, name, output),
    }
}

fn cmd_add(conn: &Connection, name: &str, query: &str, sort: Option<&str>, desc: bool) -> Result<(), String> {
    let id = db::create_collection(conn, name, query, sort, desc)
        .map_err(|e| format!("Failed to create collection: {e}"))?;
    println!("Created collection '{name}' (ID: {id})");
    Ok(())
}

fn cmd_rm(conn: &Connection, name: &str) -> Result<(), String> {
    let deleted = db::delete_collection(conn, name)
        .map_err(|e| format!("Failed to delete collection: {e}"))?;
    if deleted {
        println!("Deleted collection '{name}'.");
    } else {
        return Err(format!("Collection '{name}' not found."));
    }
    Ok(())
}

fn cmd_ls(conn: &Connection, output_str: &str) -> Result<(), String> {
    let format: OutputFormat = output_str.parse()?;
    let collections = db::get_all_collections(conn)
        .map_err(|e| format!("Failed to list collections: {e}"))?;

    match format {
        OutputFormat::Table => {
            if collections.is_empty() {
                println!("No collections.");
                return Ok(());
            }
            let mut table = comfy_table::Table::new();
            table
                .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                .set_header(vec!["Name", "Query", "Sort"]);
            for coll in &collections {
                let sort_display = match &coll.sort_by {
                    Some(s) if coll.sort_desc => format!("{s}-"),
                    Some(s) => format!("{s}+"),
                    None => String::new(),
                };
                table.add_row(vec![&coll.name, &coll.query, &sort_display]);
            }
            println!("{table}");
        }
        OutputFormat::Plain => {
            println!("Name\tQuery\tSort");
            for coll in &collections {
                let sort_display = match &coll.sort_by {
                    Some(s) if coll.sort_desc => format!("{s}-"),
                    Some(s) => format!("{s}+"),
                    None => String::new(),
                };
                println!("{}\t{}\t{}", coll.name, coll.query, sort_display);
            }
        }
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = collections
                .iter()
                .map(|c| serde_json::json!({
                    "name": c.name,
                    "query": c.query,
                    "sort_by": c.sort_by,
                    "sort_desc": c.sort_desc,
                    "created_at": c.created_at,
                }))
                .collect();
            println!("{}", serde_json::to_string_pretty(&items).unwrap_or_default());
        }
    }
    Ok(())
}

fn cmd_run(conn: &Connection, name: &str, output_str: &str) -> Result<(), String> {
    let format: OutputFormat = output_str.parse()?;
    let wads = db::run_collection(conn, name)
        .map_err(|e| format!("Failed to run collection: {e}"))?;

    let wad_ids: Vec<i64> = wads.iter().map(|w| w.id).collect();
    let stats: HashMap<i64, db::WadStats> = db::get_wad_stats_batch(conn, &wad_ids)
        .map_err(|e| e.to_string())?;

    output::render_wad_list(&wads, &stats, format);
    Ok(())
}
