//! `caco cache` — list, clear, and prune cached WAD files.

use clap::Subcommand;
use rusqlite::Connection;

use caco_core::config;
use caco_core::db;
use caco_core::utils::format_size;

use crate::output::OutputFormat;
use crate::resolve;

#[derive(Subcommand)]
pub enum CacheCommand {
    /// List cached WADs
    List {
        /// Output format: plain | json | table
        #[arg(short = 'o', long = "output", default_value = "table")]
        output: String,
        /// Show orphaned cache files
        #[arg(long)]
        orphans: bool,
    },
    /// Remove cached WAD files
    Clear {
        /// WAD query
        query: Vec<String>,
        /// Clear all cached files
        #[arg(long)]
        all: bool,
        /// Preview changes
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Remove orphaned cache files
    Prune {
        /// Preview changes
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

pub fn run(conn: &Connection, cmd: &CacheCommand) -> Result<(), String> {
    match cmd {
        CacheCommand::List { output, orphans } => {
            let format: OutputFormat = output.parse()?;
            if *orphans {
                list_orphans(conn, format)
            } else {
                list_cached(conn, format)
            }
        }
        CacheCommand::Clear {
            query,
            all,
            dry_run,
            yes,
        } => {
            if *all {
                clear_all(conn, *dry_run, *yes)
            } else {
                clear_query(conn, query, *dry_run, *yes)
            }
        }
        CacheCommand::Prune { dry_run, yes } => prune(conn, *dry_run, *yes),
    }
}

fn list_cached(conn: &Connection, format: OutputFormat) -> Result<(), String> {
    let wads = db::get_cached_wads(conn).map_err(|e| e.to_string())?;
    if wads.is_empty() && format != OutputFormat::Json {
        println!("No cached WADs.");
        return Ok(());
    }

    // Pre-compute per-row size once.
    let rows: Vec<(i64, &str, &str, Option<&str>, u64)> = wads
        .iter()
        .map(|w| {
            let size = w
                .cached_path
                .as_deref()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len())
                .unwrap_or(0);
            (
                w.id,
                w.title.as_str(),
                w.filename.as_deref().unwrap_or(""),
                w.cached_path.as_deref(),
                size,
            )
        })
        .collect();
    let total_size: u64 = rows.iter().map(|r| r.4).sum();

    match format {
        OutputFormat::Table => {
            use comfy_table::{Cell, CellAlignment, Table, presets};
            let mut table = Table::new();
            table
                .load_preset(presets::UTF8_FULL_CONDENSED)
                .set_header(vec!["ID", "Title", "Filename", "Size"]);
            for (id, title, filename, _path, size) in &rows {
                table.add_row(vec![
                    Cell::new(id).set_alignment(CellAlignment::Right),
                    Cell::new(title),
                    Cell::new(filename),
                    Cell::new(format_size(*size)).set_alignment(CellAlignment::Right),
                ]);
            }
            println!("{table}");
            println!(
                "{} cached WAD(s), {} total",
                rows.len(),
                format_size(total_size)
            );
        }
        OutputFormat::Plain => {
            println!("ID\tTitle\tFilename\tSize");
            for (id, title, filename, _path, size) in &rows {
                println!("{id}\t{title}\t{filename}\t{}", format_size(*size));
            }
            println!("total\t{}\t{}", rows.len(), format_size(total_size));
        }
        OutputFormat::Json => {
            let items: Vec<_> = rows
                .iter()
                .map(|(id, title, filename, path, size)| {
                    serde_json::json!({
                        "id": id,
                        "title": title,
                        "filename": filename,
                        "cached_path": path,
                        "size": size,
                    })
                })
                .collect();
            let out = serde_json::json!({
                "count": rows.len(),
                "total_size": total_size,
                "items": items,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        }
    }
    Ok(())
}

fn list_orphans(conn: &Connection, format: OutputFormat) -> Result<(), String> {
    let cache_dir = config::get_cache_dir();
    if !cache_dir.is_dir() && format != OutputFormat::Json {
        println!("No cache directory found.");
        return Ok(());
    }

    let mut orphans = Vec::new();
    if cache_dir.is_dir()
        && let Ok(entries) = std::fs::read_dir(&cache_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if db::get_wad_by_cached_filename(conn, &filename)
                    .map_err(|e| e.to_string())?
                    .is_none()
                {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    orphans.push((path, filename, size));
                }
            }
        }
    }

    if orphans.is_empty() && format != OutputFormat::Json {
        println!("No orphaned cache files.");
        return Ok(());
    }

    match format {
        OutputFormat::Table => {
            println!("{} orphaned file(s):", orphans.len());
            for (_, name, size) in &orphans {
                println!("  {name} ({})", format_size(*size));
            }
        }
        OutputFormat::Plain => {
            println!("Filename\tSize");
            for (_, name, size) in &orphans {
                println!("{name}\t{}", format_size(*size));
            }
        }
        OutputFormat::Json => {
            let items: Vec<_> = orphans
                .iter()
                .map(|(path, name, size)| {
                    serde_json::json!({
                        "filename": name,
                        "path": path.to_string_lossy(),
                        "size": size,
                    })
                })
                .collect();
            let out = serde_json::json!({
                "count": orphans.len(),
                "items": items,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        }
    }
    Ok(())
}

fn clear_all(conn: &Connection, dry_run: bool, yes: bool) -> Result<(), String> {
    let wads = db::get_cached_wads(conn).map_err(|e| e.to_string())?;
    // Only clear idgames-sourced WADs (always re-downloadable)
    let clearable: Vec<_> = wads
        .iter()
        .filter(|w| w.source_type == db::SourceType::Idgames)
        .collect();

    if clearable.is_empty() {
        println!("No clearable cached files (only idgames sources can be cleared).");
        return Ok(());
    }

    if dry_run {
        println!("Would clear {} cached file(s).", clearable.len());
        return Ok(());
    }

    if !yes && !resolve::confirm(&format!("Clear {} cached file(s)?", clearable.len())) {
        return Err("Aborted.".to_string());
    }

    let mut cleared = 0;
    for wad in &clearable {
        if let Some(ref path) = wad.cached_path {
            let _ = std::fs::remove_file(path);
        }
        db::clear_cached_path(conn, wad.id).map_err(|e| e.to_string())?;
        cleared += 1;
    }
    println!("Cleared {cleared} cached file(s).");
    Ok(())
}

fn clear_query(
    conn: &Connection,
    query: &[String],
    dry_run: bool,
    yes: bool,
) -> Result<(), String> {
    if query.is_empty() {
        return Err("No query specified. Use --all to clear all.".to_string());
    }
    let wads = crate::resolve::resolve_wads(
        conn,
        query,
        crate::resolve::ResolveMode::Multiple,
        yes,
        false,
    )?;

    let clearable: Vec<_> = wads.iter().filter(|w| w.cached_path.is_some()).collect();
    if clearable.is_empty() {
        println!("No cached files for the specified WAD(s).");
        return Ok(());
    }

    if dry_run {
        println!("Would clear {} cached file(s).", clearable.len());
        return Ok(());
    }

    let mut cleared = 0;
    for wad in &clearable {
        if let Some(ref path) = wad.cached_path {
            let _ = std::fs::remove_file(path);
        }
        db::clear_cached_path(conn, wad.id).map_err(|e| e.to_string())?;
        cleared += 1;
    }
    println!("Cleared {cleared} cached file(s).");
    Ok(())
}

fn prune(conn: &Connection, dry_run: bool, yes: bool) -> Result<(), String> {
    let cache_dir = config::get_cache_dir();
    if !cache_dir.is_dir() {
        println!("No cache directory found.");
        return Ok(());
    }

    let mut orphans = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if db::get_wad_by_cached_filename(conn, filename)
                    .map_err(|e| e.to_string())?
                    .is_none()
                {
                    orphans.push(path);
                }
            }
        }
    }

    if orphans.is_empty() {
        println!("No orphaned cache files to prune.");
        return Ok(());
    }

    if dry_run {
        println!("Would prune {} orphaned file(s).", orphans.len());
        return Ok(());
    }

    if !yes && !resolve::confirm(&format!("Prune {} orphaned file(s)?", orphans.len())) {
        return Err("Aborted.".to_string());
    }

    let mut pruned = 0;
    for path in &orphans {
        if std::fs::remove_file(path).is_ok() {
            pruned += 1;
        }
    }
    println!("Pruned {pruned} orphaned file(s).");
    Ok(())
}
