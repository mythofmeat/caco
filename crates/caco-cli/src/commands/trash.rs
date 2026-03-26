//! `caco trash` — soft delete, restore, purge, IWAD/id24 removal.

use std::path::Path;

use clap::Args;
use rusqlite::Connection;

use caco_core::db;
use crate::output::{self, OutputFormat};
use crate::resolve::{self, ResolveMode};

#[derive(Args)]
pub struct TrashArgs {
    /// Query terms
    query: Vec<String>,

    /// Output format
    #[arg(short, long, default_value = "table")]
    output: String,

    /// List deleted WADs
    #[arg(long)]
    list: bool,

    /// Restore from trash
    #[arg(long)]
    restore: bool,

    /// Permanently delete
    #[arg(long)]
    purge: bool,

    /// Remove IWAD (FAMILY[/VARIANT])
    #[arg(long)]
    iwad: Option<String>,

    /// Remove id24 WAD
    #[arg(long)]
    id24: Option<String>,

    /// Preview changes
    #[arg(long)]
    dry_run: bool,

    /// Skip confirmation
    #[arg(short = 'y', long)]
    yes: bool,
}

pub fn run(conn: &Connection, args: &TrashArgs) -> Result<(), String> {
    let format: OutputFormat = args.output.parse()?;

    // IWAD removal
    if let Some(ref iwad_spec) = args.iwad {
        return remove_iwad(conn, iwad_spec, args.yes, args.dry_run);
    }

    // id24 removal
    if let Some(ref id24_name) = args.id24 {
        return remove_id24(conn, id24_name, args.yes, args.dry_run);
    }

    // List trash
    if args.list {
        let wads = db::search_wads(conn, None, None, true, true, 0)
            .map_err(|e| e.to_string())?;
        let wad_ids: Vec<i64> = wads.iter().map(|w| w.id).collect();
        let stats = db::get_wad_stats_batch(conn, &wad_ids).map_err(|e| e.to_string())?;
        output::render_wad_list(&wads, &stats, format);
        return Ok(());
    }

    // Restore from trash
    if args.restore {
        if args.query.is_empty() {
            return Err("No query specified for restore.".to_string());
        }
        let query_str = args.query.join(" ");
        let wads = db::search_wads(conn, Some(&query_str), None, true, true, 0)
            .map_err(|e| e.to_string())?;
        if wads.is_empty() {
            return Err("No deleted WADs match the query.".to_string());
        }
        if args.dry_run {
            println!("Would restore {} WAD(s):", wads.len());
            for wad in &wads {
                println!("  {}: {}", wad.id, wad.title);
            }
            return Ok(());
        }
        let mut restored = 0;
        for wad in &wads {
            if db::restore_wad(conn, wad.id).map_err(|e| e.to_string())? {
                restored += 1;
            }
        }
        println!("Restored {restored} WAD(s).");
        return Ok(());
    }

    // Purge all deleted
    if args.purge {
        if args.dry_run {
            let wads = db::search_wads(conn, None, None, true, true, 0)
                .map_err(|e| e.to_string())?;
            println!("Would permanently delete {} WAD(s).", wads.len());
            return Ok(());
        }
        if !args.yes {
            eprint!("Permanently delete all trashed WADs? [y/N] ");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            let mut response = String::new();
            if std::io::stdin().read_line(&mut response).is_err() || !response.trim().to_lowercase().starts_with('y') {
                return Err("Aborted.".to_string());
            }
        }
        let count = db::purge_all_deleted(conn).map_err(|e| e.to_string())?;
        println!("Purged {count} WAD(s).");
        return Ok(());
    }

    // Soft delete
    if args.query.is_empty() {
        return Err("No query specified. Use --list to view trash.".to_string());
    }

    let wads = resolve::resolve_wads(conn, &args.query, ResolveMode::Multiple, args.yes, false)?;

    if args.dry_run {
        println!("Would move {} WAD(s) to trash:", wads.len());
        for wad in &wads {
            println!("  {}: {}", wad.id, wad.title);
        }
        return Ok(());
    }

    let mut trashed = 0;
    for wad in &wads {
        if db::delete_wad(conn, wad.id, false).map_err(|e| e.to_string())? {
            trashed += 1;
        }
    }
    println!("Moved {trashed} WAD(s) to trash.");
    Ok(())
}

fn remove_iwad(conn: &Connection, spec: &str, yes: bool, dry_run: bool) -> Result<(), String> {
    let (family, variant) = if let Some((f, v)) = spec.split_once('/') {
        (f.to_string(), Some(v.to_string()))
    } else {
        (spec.to_string(), None)
    };

    if let Some(ref var) = variant {
        // Remove specific variant
        if dry_run {
            println!("Would remove IWAD {family}/{var}.");
            return Ok(());
        }
        let removed = db::remove_iwad_with_paths(conn, &family, Some(var))
            .map_err(|e| e.to_string())?;
        for path in &removed {
            let p = Path::new(path);
            if p.exists() {
                let _ = std::fs::remove_file(p);
            }
        }
        println!("Removed IWAD {family}/{var}.");
    } else {
        // Remove all variants
        let variants = db::get_family_iwads(conn, &family, None).map_err(|e| e.to_string())?;
        if variants.is_empty() {
            return Err(format!("No IWAD registered with family '{family}'."));
        }
        if variants.len() > 1 && !yes {
            eprintln!("Warning: {} variants of '{}' will be removed:", variants.len(), family);
            for v in &variants {
                eprintln!("  {}/{}", v.family, v.variant);
            }
            eprint!("Continue? [y/N] ");
            let _ = std::io::Write::flush(&mut std::io::stderr());
            let mut response = String::new();
            if std::io::stdin().read_line(&mut response).is_err() || !response.trim().to_lowercase().starts_with('y') {
                return Err("Aborted.".to_string());
            }
        }
        if dry_run {
            println!("Would remove {} variant(s) of IWAD '{family}'.", variants.len());
            return Ok(());
        }
        let removed = db::remove_iwad_with_paths(conn, &family, None)
            .map_err(|e| e.to_string())?;
        for path in &removed {
            let p = Path::new(path);
            if p.exists() {
                let _ = std::fs::remove_file(p);
            }
        }
        println!("Removed {} variant(s) of IWAD '{family}'.", variants.len());
    }

    Ok(())
}

fn remove_id24(conn: &Connection, name: &str, yes: bool, dry_run: bool) -> Result<(), String> {
    let _entry = db::get_id24(conn, name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("No id24 WAD registered with name '{name}'."))?;

    if dry_run {
        println!("Would remove id24 WAD '{name}'.");
        return Ok(());
    }

    if !yes {
        eprint!("Remove id24 WAD '{name}'? [y/N] ");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let mut response = String::new();
        if std::io::stdin().read_line(&mut response).is_err() || !response.trim().to_lowercase().starts_with('y') {
            return Err("Aborted.".to_string());
        }
    }

    let removed = db::remove_id24_with_paths(conn, name).map_err(|e| e.to_string())?;
    for path in &removed {
        let p = Path::new(path);
        if p.exists() {
            let _ = std::fs::remove_file(p);
        }
    }
    println!("Removed id24 WAD '{name}'.");
    Ok(())
}
