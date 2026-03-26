//! `caco cache` — list, clear, and prune cached WAD files.

use clap::Subcommand;
use rusqlite::Connection;

use caco_core::db;
use caco_core::config;
use caco_core::utils::format_size;

#[derive(Subcommand)]
pub enum CacheCommand {
    /// List cached WADs
    List {
        /// Plain TSV output
        #[arg(long)]
        plain: bool,
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
        CacheCommand::List { plain, orphans } => {
            if *orphans {
                list_orphans(conn, *plain)
            } else {
                list_cached(conn, *plain)
            }
        }
        CacheCommand::Clear { query, all, dry_run, yes } => {
            if *all {
                clear_all(conn, *dry_run, *yes)
            } else {
                clear_query(conn, query, *dry_run, *yes)
            }
        }
        CacheCommand::Prune { dry_run, yes } => {
            prune(conn, *dry_run, *yes)
        }
    }
}

fn list_cached(conn: &Connection, plain: bool) -> Result<(), String> {
    let wads = db::get_cached_wads(conn).map_err(|e| e.to_string())?;
    if wads.is_empty() {
        println!("No cached WADs.");
        return Ok(());
    }

    let mut total_size: u64 = 0;

    if plain {
        println!("ID\tTitle\tFilename\tSize");
    } else {
        use comfy_table::{presets, Table, Cell, CellAlignment};
        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL_CONDENSED)
            .set_header(vec!["ID", "Title", "Filename", "Size"]);

        for wad in &wads {
            let size = wad.cached_path.as_deref()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len())
                .unwrap_or(0);
            total_size += size;

            table.add_row(vec![
                Cell::new(wad.id).set_alignment(CellAlignment::Right),
                Cell::new(&wad.title),
                Cell::new(wad.filename.as_deref().unwrap_or("")),
                Cell::new(format_size(size)).set_alignment(CellAlignment::Right),
            ]);
        }
        println!("{table}");
        println!("{} cached WAD(s), {} total", wads.len(), format_size(total_size));
        return Ok(());
    }

    for wad in &wads {
        let size = wad.cached_path.as_deref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        total_size += size;
        println!(
            "{}\t{}\t{}\t{}",
            wad.id,
            wad.title,
            wad.filename.as_deref().unwrap_or(""),
            format_size(size),
        );
    }
    println!("total\t{}\t{}", wads.len(), format_size(total_size));
    Ok(())
}

fn list_orphans(conn: &Connection, plain: bool) -> Result<(), String> {
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
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if db::get_wad_by_cached_filename(conn, &filename).map_err(|e| e.to_string())?.is_none() {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    orphans.push((path, filename, size));
                }
            }
        }
    }

    if orphans.is_empty() {
        println!("No orphaned cache files.");
        return Ok(());
    }

    if plain {
        println!("Filename\tSize");
        for (_, name, size) in &orphans {
            println!("{name}\t{}", format_size(*size));
        }
    } else {
        println!("{} orphaned file(s):", orphans.len());
        for (_, name, size) in &orphans {
            println!("  {name} ({})", format_size(*size));
        }
    }
    Ok(())
}

fn clear_all(conn: &Connection, dry_run: bool, yes: bool) -> Result<(), String> {
    let wads = db::get_cached_wads(conn).map_err(|e| e.to_string())?;
    // Only clear idgames-sourced WADs (always re-downloadable)
    let clearable: Vec<_> = wads
        .iter()
        .filter(|w| w.source_type == "idgames")
        .collect();

    if clearable.is_empty() {
        println!("No clearable cached files (only idgames sources can be cleared).");
        return Ok(());
    }

    if dry_run {
        println!("Would clear {} cached file(s).", clearable.len());
        return Ok(());
    }

    if !yes {
        eprint!("Clear {} cached file(s)? [y/N] ", clearable.len());
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let mut response = String::new();
        if std::io::stdin().read_line(&mut response).is_err() || !response.trim().to_lowercase().starts_with('y') {
            return Err("Aborted.".to_string());
        }
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

fn clear_query(conn: &Connection, query: &[String], dry_run: bool, yes: bool) -> Result<(), String> {
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
                if db::get_wad_by_cached_filename(conn, filename).map_err(|e| e.to_string())?.is_none() {
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

    if !yes {
        eprint!("Prune {} orphaned file(s)? [y/N] ", orphans.len());
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let mut response = String::new();
        if std::io::stdin().read_line(&mut response).is_err() || !response.trim().to_lowercase().starts_with('y') {
            return Err("Aborted.".to_string());
        }
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
