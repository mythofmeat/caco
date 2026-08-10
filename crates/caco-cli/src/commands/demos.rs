//! `caco demos` — manage demo recordings (list, play, clean).

use clap::Subcommand;
use rusqlite::Connection;

use crate::output::OutputFormat;
use crate::resolve;
use caco_core::demos;
use caco_core::utils::format_size;

#[derive(Subcommand)]
pub enum DemosCommand {
    /// List demo files for a WAD
    List {
        /// WAD query
        query: Vec<String>,
        /// Output format: plain | json | table
        #[arg(short = 'o', long = "output", default_value = "table")]
        output: String,
        /// Auto-select first match
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Play back a demo
    Play {
        /// WAD query
        query: Vec<String>,
        /// Specific demo filename (most recent if omitted)
        #[arg(long)]
        demo: Option<String>,
        /// Sourceport to use
        #[arg(short = 'p', long)]
        sourceport: Option<String>,
        /// Auto-select first match
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Delete demo files
    Clean {
        /// WAD query
        query: Vec<String>,
        /// Preview changes
        #[arg(long)]
        dry_run: bool,
        /// Auto-select first match
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

pub fn run(conn: &Connection, cmd: &DemosCommand) -> Result<(), String> {
    match cmd {
        DemosCommand::List { query, output, yes } => {
            let format: OutputFormat = output.parse()?;
            list_demos(conn, query, format, *yes)
        }
        DemosCommand::Play {
            query,
            demo,
            sourceport,
            yes,
        } => play_demo(conn, query, demo.as_deref(), sourceport.as_deref(), *yes),
        DemosCommand::Clean {
            query,
            dry_run,
            yes,
        } => clean_demos(conn, query, *dry_run, *yes),
    }
}

fn list_demos(
    conn: &Connection,
    query: &[String],
    format: OutputFormat,
    yes: bool,
) -> Result<(), String> {
    let (wad, data_dir) = resolve::resolve_data_dir(conn, query, yes)?;

    let files = demos::find_demo_files(&data_dir);
    if files.is_empty() && format != OutputFormat::Json {
        println!("No demos for '{}'.", wad.title);
        return Ok(());
    }

    match format {
        OutputFormat::Plain => {
            println!("Name\tSize\tModified");
            for f in &files {
                println!("{}\t{}\t{}", f.name, format_size(f.size), f.mtime_iso);
            }
        }
        OutputFormat::Table => {
            use comfy_table::{Cell, CellAlignment, Table, presets};
            let mut table = Table::new();
            table
                .load_preset(presets::UTF8_FULL_CONDENSED)
                .set_header(vec!["Name", "Size", "Modified"]);
            for f in &files {
                table.add_row(vec![
                    Cell::new(&f.name),
                    Cell::new(format_size(f.size)).set_alignment(CellAlignment::Right),
                    Cell::new(&f.mtime_iso),
                ]);
            }
            println!("Demos for '{}' (ID: {}):", wad.title, wad.id);
            println!("{table}");
        }
        OutputFormat::Json => {
            let items: Vec<_> = files
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "name": f.name,
                        "size": f.size,
                        "modified": f.mtime_iso,
                    })
                })
                .collect();
            let out = serde_json::json!({
                "wad_id": wad.id,
                "wad_title": wad.title,
                "count": files.len(),
                "items": items,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        }
    }
    Ok(())
}

fn play_demo(
    conn: &Connection,
    query: &[String],
    demo_name: Option<&str>,
    sourceport: Option<&str>,
    yes: bool,
) -> Result<(), String> {
    let wad = resolve::resolve_one_wad(conn, query, yes)?;

    let path = caco_core::player::play_demo(conn, wad.id, demo_name, sourceport)
        .map_err(|e| e.to_string())?;
    eprintln!("Played demo: {}", path.display());
    Ok(())
}

fn clean_demos(
    conn: &Connection,
    query: &[String],
    dry_run: bool,
    yes: bool,
) -> Result<(), String> {
    let (wad, data_dir) = resolve::resolve_data_dir(conn, query, yes)?;

    let files = demos::find_demo_files(&data_dir);
    if files.is_empty() {
        println!("No demos for '{}'.", wad.title);
        return Ok(());
    }

    if dry_run {
        println!("Would delete {} demo file(s):", files.len());
        for f in &files {
            println!("  {}", f.name);
        }
        return Ok(());
    }

    if !yes
        && !resolve::confirm(&format!(
            "Delete {} demo file(s) for '{}'?",
            files.len(),
            wad.title
        ))
    {
        return Err("Aborted.".to_string());
    }

    let deleted = demos::clean_demo_files(&data_dir);
    println!("Deleted {} demo file(s).", deleted.len());
    Ok(())
}
