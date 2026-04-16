//! `caco demos` — manage demo recordings (list, play, clean).

use clap::Subcommand;
use rusqlite::Connection;

use crate::resolve;
use caco_core::demos;
use caco_core::utils::format_size;

#[derive(Subcommand)]
pub enum DemosCommand {
    /// List demo files for a WAD
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
        DemosCommand::List { query, plain, yes } => list_demos(conn, query, *plain, *yes),
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

fn list_demos(conn: &Connection, query: &[String], plain: bool, yes: bool) -> Result<(), String> {
    let (wad, data_dir) = resolve::resolve_data_dir(conn, query, yes)?;

    let files = demos::find_demo_files(&data_dir);
    if files.is_empty() {
        println!("No demos for '{}'.", wad.title);
        return Ok(());
    }

    if plain {
        println!("Name\tSize\tModified");
        for f in &files {
            println!("{}\t{}\t{}", f.name, format_size(f.size), f.mtime_iso);
        }
    } else {
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
    Ok(())
}

fn play_demo(
    conn: &Connection,
    query: &[String],
    demo_name: Option<&str>,
    sourceport: Option<&str>,
    yes: bool,
) -> Result<(), String> {
    let (wad, data_dir) = resolve::resolve_data_dir(conn, query, yes)?;
    let demos_dir = demos::get_demos_dir(&data_dir);

    let files = demos::find_demo_files(&data_dir);
    if files.is_empty() {
        return Err(format!("No demos for '{}'.", wad.title));
    }

    let demo_path = if let Some(name) = demo_name {
        let target = demos_dir.join(name);
        if target.exists() {
            target
        } else {
            // Try with .lmp extension
            let with_ext = demos_dir.join(format!("{name}.lmp"));
            if with_ext.exists() {
                with_ext
            } else {
                return Err(format!("Demo '{name}' not found."));
            }
        }
    } else {
        // Most recent demo
        files
            .first()
            .map(|f| f.path.clone())
            .ok_or_else(|| "No demos found.".to_string())?
    };

    // Build sourceport command for demo playback
    let port = sourceport
        .map(|s| s.to_string())
        .or(wad.custom_sourceport.clone())
        .unwrap_or_else(caco_core::config::get_default_sourceport);

    if port.is_empty() {
        return Err("No sourceport configured.".to_string());
    }

    let port = caco_core::config::resolve_sourceport(&port);
    let mut cmd = std::process::Command::new(&port);

    // Add IWAD
    let default_iwad = caco_core::config::get_iwad();
    let iwad_name = wad.custom_iwad.as_deref().or(if default_iwad.is_empty() {
        None
    } else {
        Some(default_iwad.as_str())
    });
    if let Some(iwad) = iwad_name {
        let db_resolved = caco_core::db::resolve_iwad_from_db(conn, iwad, None);
        let resolved = caco_core::config::resolve_iwad_path(iwad, db_resolved.as_deref());
        cmd.args(["-iwad", &resolved]);
    }

    // Add WAD file
    if let Some(ref cached) = wad.cached_path {
        cmd.args(["-file", cached]);
    }

    cmd.args(["-playdemo", &demo_path.to_string_lossy()]);

    eprintln!("Playing demo: {}", demo_path.display());
    cmd.stdin(std::process::Stdio::null());
    let status = cmd
        .spawn()
        .map_err(|e| format!("Failed to launch sourceport: {e}"))?
        .wait()
        .map_err(|e| format!("Failed to wait for sourceport: {e}"))?;

    if !status.success() {
        eprintln!(
            "Warning: Sourceport exited with code {}",
            status.code().unwrap_or(-1)
        );
    }

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
