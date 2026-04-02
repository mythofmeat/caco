//! `caco saves` — manage save files (list, backup, restore, clean, backups).

use clap::Subcommand;
use rusqlite::Connection;

use caco_core::saves;
use caco_core::utils::format_size;
use crate::resolve;

#[derive(Subcommand)]
pub enum SavesCommand {
    /// List save files for a WAD
    List {
        /// WAD query
        query: Vec<String>,
        /// Plain TSV output
        #[arg(long)]
        plain: bool,
        /// Auto-select first match
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Backup WAD data directory
    Backup {
        /// WAD query
        query: Vec<String>,
        /// Auto-select first match
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Restore from backup
    Restore {
        /// WAD query
        query: Vec<String>,
        /// Specific backup filename (latest if omitted)
        #[arg(long)]
        backup: Option<String>,
        /// Auto-select first match
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Delete save files only
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
    /// List existing backups
    Backups {
        /// WAD query (optional)
        query: Vec<String>,
        /// Plain TSV output
        #[arg(long)]
        plain: bool,
        /// Auto-select first match
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

pub fn run(conn: &Connection, cmd: &SavesCommand) -> Result<(), String> {
    match cmd {
        SavesCommand::List { query, plain, yes } => list_saves(conn, query, *plain, *yes),
        SavesCommand::Backup { query, yes } => backup(conn, query, *yes),
        SavesCommand::Restore { query, backup, yes } => restore(conn, query, backup.as_deref(), *yes),
        SavesCommand::Clean { query, dry_run, yes } => clean(conn, query, *dry_run, *yes),
        SavesCommand::Backups { query, plain, yes } => list_backups(conn, query, *plain, *yes),
    }
}

fn list_saves(conn: &Connection, query: &[String], plain: bool, yes: bool) -> Result<(), String> {
    let (wad, data_dir) = resolve::resolve_data_dir(conn, query, yes)?;

    let files = saves::find_save_files(&data_dir);
    if files.is_empty() {
        println!("No save files for '{}'.", wad.title);
        return Ok(());
    }

    if plain {
        println!("Name\tSize\tModified");
        for f in &files {
            println!("{}\t{}\t{}", f.name, format_size(f.size), f.mtime_iso);
        }
    } else {
        use comfy_table::{presets, Table, Cell, CellAlignment};
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
        println!("Save files for '{}' (ID: {}):", wad.title, wad.id);
        println!("{table}");
    }
    Ok(())
}

fn backup(conn: &Connection, query: &[String], yes: bool) -> Result<(), String> {
    let (wad, data_dir) = resolve::resolve_data_dir(conn, query, yes)?;

    if !data_dir.is_dir() {
        return Err(format!("No data directory for '{}'.", wad.title));
    }

    let path = saves::create_backup(wad.id, &wad.title, &data_dir).map_err(|e| e.to_string())?;
    println!("Backup created: {}", path.display());
    Ok(())
}

fn restore(conn: &Connection, query: &[String], backup_arg: Option<&str>, yes: bool) -> Result<(), String> {
    let (wad, data_dir) = resolve::resolve_data_dir(conn, query, yes)?;

    let backup_path = saves::resolve_backup_path(wad.id, backup_arg)
        .ok_or_else(|| format!("No backup found for '{}'. Create one with: caco saves backup {}", wad.title, wad.id))?;

    if !yes {
        let name = backup_path.file_name().and_then(|n| n.to_str()).unwrap_or("backup");
        if !resolve::confirm(&format!("Restore from {name}? This will overwrite existing data.")) {
            return Err("Aborted.".to_string());
        }
    }

    let count = saves::restore_backup(&backup_path, &data_dir).map_err(|e| e.to_string())?;
    println!("Restored {count} file(s) to {}.", data_dir.display());
    Ok(())
}

fn clean(conn: &Connection, query: &[String], dry_run: bool, yes: bool) -> Result<(), String> {
    let (wad, data_dir) = resolve::resolve_data_dir(conn, query, yes)?;

    let files = saves::find_save_files(&data_dir);
    if files.is_empty() {
        println!("No save files for '{}'.", wad.title);
        return Ok(());
    }

    if dry_run {
        println!("Would delete {} save file(s):", files.len());
        for f in &files {
            println!("  {}", f.name);
        }
        return Ok(());
    }

    if !yes && !resolve::confirm(&format!("Delete {} save file(s) for '{}'?", files.len(), wad.title)) {
        return Err("Aborted.".to_string());
    }

    let deleted = saves::clean_save_files(&data_dir);
    println!("Deleted {} save file(s).", deleted.len());
    Ok(())
}

fn list_backups(conn: &Connection, query: &[String], plain: bool, yes: bool) -> Result<(), String> {
    let backups = if query.is_empty() {
        saves::list_all_backups()
    } else {
        let wad = resolve::resolve_one_wad(conn, query, yes)?;
        saves::list_backups(wad.id)
    };

    if backups.is_empty() {
        println!("No backups found.");
        return Ok(());
    }

    if plain {
        println!("Name\tWadID\tSize\tCreated");
        for b in &backups {
            let wad_id_str = b.wad_id.map(|id| id.to_string()).unwrap_or_default();
            println!("{}\t{}\t{}\t{}", b.name, wad_id_str, format_size(b.size), b.created_iso);
        }
    } else {
        use comfy_table::{presets, Table, Cell, CellAlignment};
        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL_CONDENSED)
            .set_header(vec!["Name", "WAD ID", "Size", "Created"]);
        for b in &backups {
            table.add_row(vec![
                Cell::new(&b.name),
                Cell::new(b.wad_id.map(|id| id.to_string()).unwrap_or_default()).set_alignment(CellAlignment::Right),
                Cell::new(format_size(b.size)).set_alignment(CellAlignment::Right),
                Cell::new(&b.created_iso),
            ]);
        }
        println!("{table}");
    }
    Ok(())
}
