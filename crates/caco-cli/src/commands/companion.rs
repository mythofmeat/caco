//! `caco companion` — manage companion files for WADs.

use std::path::Path;

use clap::Subcommand;
use rusqlite::Connection;

use caco_core::companion_service::{self, OrphanPolicy, OrphanResult};
use caco_core::db;
use caco_core::utils::format_size;

use crate::resolve;

#[derive(Subcommand)]
pub enum CompanionCommand {
    /// Register a companion file and link to a WAD
    Add {
        /// WAD query
        query: Vec<String>,
        /// Path to companion file
        #[arg(long = "file", short = 'f')]
        file: String,
    },
    /// Unlink a companion file from a WAD
    Rm {
        /// WAD query
        query: Vec<String>,
        /// Companion filename to remove
        #[arg(long = "file", short = 'f')]
        file: String,
        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Enable a disabled companion file
    Enable {
        /// WAD query
        query: Vec<String>,
        /// Companion filename to enable
        #[arg(long = "file", short = 'f')]
        file: String,
    },
    /// Disable a companion file without removing
    Disable {
        /// WAD query
        query: Vec<String>,
        /// Companion filename to disable
        #[arg(long = "file", short = 'f')]
        file: String,
    },
    /// List companion files
    Ls {
        /// WAD query (optional — lists all if omitted)
        query: Vec<String>,
        /// Plain output
        #[arg(long)]
        plain: bool,
    },
}

pub fn run(conn: &Connection, cmd: &CompanionCommand) -> Result<(), String> {
    match cmd {
        CompanionCommand::Add { query, file } => cmd_add(conn, query, file),
        CompanionCommand::Rm { query, file, yes } => cmd_rm(conn, query, file, *yes),
        CompanionCommand::Enable { query, file } => cmd_enable(conn, query, file),
        CompanionCommand::Disable { query, file } => cmd_disable(conn, query, file),
        CompanionCommand::Ls { query, plain } => cmd_ls(conn, query, *plain),
    }
}

fn cmd_add(conn: &Connection, query: &[String], file: &str) -> Result<(), String> {
    let wad = resolve::resolve_one_wad(conn, query, false)?;
    let file_path = Path::new(file);

    let (_companion_id, filename) = companion_service::register_companion(conn, wad.id, file_path)
        .map_err(|e| format!("Failed to register companion: {e}"))?;

    println!("Added '{}' to '{}'.", filename, wad.title);
    Ok(())
}

fn cmd_rm(conn: &Connection, query: &[String], file: &str, yes: bool) -> Result<(), String> {
    let wad = resolve::resolve_one_wad(conn, query, false)?;
    let comp =
        companion_service::find_by_filename(conn, wad.id, file).map_err(|e| e.to_string())?;

    // `None` means the file would be orphaned and the config says to ask.
    // `-y` answers "delete" without a prompt, matching the rest of the CLI.
    let policy = match companion_service::plan_unregister(conn, wad.id, comp.companion_id)
        .map_err(|e| e.to_string())?
    {
        Some(policy) => policy,
        None if yes => OrphanPolicy::Delete,
        None => {
            if resolve::confirm(&format!("'{file}' will be orphaned. Delete managed file?")) {
                OrphanPolicy::Delete
            } else {
                OrphanPolicy::Keep
            }
        }
    };

    let result = companion_service::unregister_companion(conn, wad.id, comp.companion_id, policy)
        .map_err(|e| format!("Failed to remove companion: {e}"))?;

    match result {
        OrphanResult::Deleted => {
            println!("Removed '{}' from '{}' (orphan deleted).", file, wad.title);
        }
        OrphanResult::Kept => {
            println!("Removed '{}' from '{}' (orphan kept).", file, wad.title);
        }
        OrphanResult::NotOrphaned => {
            println!(
                "Removed '{}' from '{}' (still linked to other WADs).",
                file, wad.title
            );
        }
    }
    Ok(())
}

fn cmd_enable(conn: &Connection, query: &[String], file: &str) -> Result<(), String> {
    set_enabled(conn, query, file, true)
}

fn cmd_disable(conn: &Connection, query: &[String], file: &str) -> Result<(), String> {
    set_enabled(conn, query, file, false)
}

fn set_enabled(
    conn: &Connection,
    query: &[String],
    file: &str,
    enabled: bool,
) -> Result<(), String> {
    let wad = resolve::resolve_one_wad(conn, query, false)?;
    let comp =
        companion_service::find_by_filename(conn, wad.id, file).map_err(|e| e.to_string())?;

    let verb = if enabled { "Enabled" } else { "Disabled" };
    let changed =
        companion_service::set_enabled(conn, wad.id, &comp, enabled).map_err(|e| e.to_string())?;

    if changed {
        println!("{verb} '{file}' for '{}'.", wad.title);
    } else {
        println!(
            "'{file}' is already {} for '{}'.",
            verb.to_lowercase(),
            wad.title
        );
    }
    Ok(())
}

fn cmd_ls(conn: &Connection, query: &[String], plain: bool) -> Result<(), String> {
    if query.is_empty() {
        // List all companions
        return list_all_companions(conn, plain);
    }

    let wad = resolve::resolve_one_wad(conn, query, false)?;
    let companions = db::get_companions_for_wad(conn, wad.id).map_err(|e| e.to_string())?;

    if companions.is_empty() {
        println!("No companion files for '{}'.", wad.title);
        return Ok(());
    }

    if plain {
        println!("Filename\tSize\tEnabled\tOrder");
        for c in &companions {
            println!(
                "{}\t{}\t{}\t{}",
                c.filename,
                c.size,
                if c.enabled { "yes" } else { "no" },
                c.load_order,
            );
        }
    } else {
        println!("Companions for '{}' (ID: {}):", wad.title, wad.id);
        for c in &companions {
            let status = if c.enabled { "enabled" } else { "disabled" };
            let size = format_size(c.size as u64);
            println!(
                "  [{}] {} ({}, order: {})",
                status, c.filename, size, c.load_order,
            );
        }
    }
    Ok(())
}

fn list_all_companions(conn: &Connection, plain: bool) -> Result<(), String> {
    let all = db::get_all_companions(conn).map_err(|e| e.to_string())?;

    if all.is_empty() {
        println!("No companion files registered.");
        return Ok(());
    }

    if plain {
        println!("ID\tFilename\tSize\tMD5");
        for c in &all {
            println!("{}\t{}\t{}\t{}", c.id, c.filename, c.size, c.md5);
        }
    } else {
        println!("{} companion file(s) registered:", all.len());
        for c in &all {
            let size = format_size(c.size as u64);
            println!(
                "  {} ({}, md5: {})",
                c.filename,
                size,
                &c.md5[..12.min(c.md5.len())]
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::models::SourceType;
    use caco_core::db::wads::{NewWad, add_wad};
    use caco_core::db::{self, init_db, open_memory};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection, title: &str) -> i64 {
        add_wad(conn, &NewWad::new(title, SourceType::Local)).unwrap()
    }

    // -- list formatting tests --

    #[test]
    fn test_list_all_companions_empty() {
        let conn = setup();
        // Should not panic, just prints "No companion files registered."
        let result = list_all_companions(&conn, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_all_companions_plain() {
        let conn = setup();
        db::add_companion(&conn, "abc123def456", "test.deh", "/path/test.deh", 1024).unwrap();
        // Should print plain TSV output
        let result = list_all_companions(&conn, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_all_companions_rich() {
        let conn = setup();
        db::add_companion(&conn, "abc123def456", "test.deh", "/path/test.deh", 2048).unwrap();
        db::add_companion(&conn, "def789ghi012", "patch.bex", "/path/patch.bex", 512).unwrap();
        // Should print rich formatted output
        let result = list_all_companions(&conn, false);
        assert!(result.is_ok());
    }

    // -- enable/disable DB integration --

    #[test]
    fn test_enable_disable_companion_db() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Test WAD");
        let c_id =
            db::add_companion(&conn, "md5test", "patch.deh", "/path/patch.deh", 100).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        // Default: enabled
        let comps = db::get_companions_for_wad(&conn, wad_id).unwrap();
        assert!(comps[0].enabled);

        // Disable
        db::set_companion_enabled(&conn, wad_id, c_id, false).unwrap();
        let comps = db::get_companions_for_wad(&conn, wad_id).unwrap();
        assert!(!comps[0].enabled);

        // Enable again
        db::set_companion_enabled(&conn, wad_id, c_id, true).unwrap();
        let comps = db::get_companions_for_wad(&conn, wad_id).unwrap();
        assert!(comps[0].enabled);
    }

    #[test]
    fn test_enable_nonexistent_companion() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Test WAD");
        // Should return false (no rows updated)
        let result = db::set_companion_enabled(&conn, wad_id, 999, true).unwrap();
        assert!(!result);
    }

    // -- would_be_orphan tests --

    #[test]
    fn test_would_be_orphan_sole_link() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "WAD");
        let c_id =
            db::add_companion(&conn, "md5test", "patch.deh", "/path/patch.deh", 100).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        assert!(db::would_be_orphan(&conn, c_id, wad_id).unwrap());
    }

    #[test]
    fn test_would_be_orphan_shared_link() {
        let conn = setup();
        let w1 = add_test_wad(&conn, "WAD 1");
        let w2 = add_test_wad(&conn, "WAD 2");
        let c_id =
            db::add_companion(&conn, "md5test", "patch.deh", "/path/patch.deh", 100).unwrap();
        db::link_companion_to_wad(&conn, w1, c_id).unwrap();
        db::link_companion_to_wad(&conn, w2, c_id).unwrap();

        // Not orphaned if removed from w1 — still linked to w2
        assert!(!db::would_be_orphan(&conn, c_id, w1).unwrap());
    }

    // -- load_order tests --

    #[test]
    fn test_companion_load_order_auto_increment() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "WAD");
        let c1 = db::add_companion(&conn, "md5_1", "first.deh", "/first.deh", 100).unwrap();
        let c2 = db::add_companion(&conn, "md5_2", "second.deh", "/second.deh", 200).unwrap();
        let c3 = db::add_companion(&conn, "md5_3", "third.deh", "/third.deh", 300).unwrap();

        db::link_companion_to_wad(&conn, wad_id, c1).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c2).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c3).unwrap();

        let comps = db::get_companions_for_wad(&conn, wad_id).unwrap();
        assert_eq!(comps.len(), 3);
        assert_eq!(comps[0].load_order, 0);
        assert_eq!(comps[1].load_order, 1);
        assert_eq!(comps[2].load_order, 2);
    }
}
