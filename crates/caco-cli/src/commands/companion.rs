//! `caco companion` — manage companion files for WADs.

use std::path::Path;

use clap::Subcommand;
use rusqlite::Connection;

use caco_core::companion_service::{self, OrphanResult};
use caco_core::config;
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

    let (_companion_id, filename) =
        companion_service::register_companion(conn, wad.id, file_path)
            .map_err(|e| format!("Failed to register companion: {e}"))?;

    println!("Added '{}' to '{}'.", filename, wad.title);
    Ok(())
}

fn cmd_rm(conn: &Connection, query: &[String], file: &str, yes: bool) -> Result<(), String> {
    let wad = resolve::resolve_one_wad(conn, query, false)?;

    let companions =
        db::get_companions_for_wad(conn, wad.id).map_err(|e| e.to_string())?;

    let comp = companions
        .iter()
        .find(|c| c.filename == file)
        .ok_or_else(|| format!("Companion '{}' not found for '{}'.", file, wad.title))?;

    let policy = config::get_companion_orphan_cleanup();

    // For "ask" policy, check if it would become orphaned and prompt
    let effective_policy = if policy == "ask" && !yes {
        // Temporarily check: will unlinking make it orphaned?
        // Count other WADs that also have this companion
        let other_links = companions_linked_elsewhere(conn, comp.companion_id, wad.id)?;
        if other_links == 0 {
            // It will become orphaned — ask user
            if resolve::confirm(&format!("'{}' will be orphaned. Delete managed file?", file)) {
                "delete"
            } else {
                "keep"
            }
        } else {
            "keep" // not going to be orphaned
        }
    } else if yes {
        // -y means auto-delete orphans
        "delete"
    } else {
        &policy
    };

    let result =
        companion_service::unregister_companion(conn, wad.id, comp.companion_id, Some(effective_policy))
            .map_err(|e| format!("Failed to remove companion: {e}"))?;

    match result {
        OrphanResult::Deleted => {
            println!("Removed '{}' from '{}' (orphan deleted).", file, wad.title);
        }
        OrphanResult::Kept => {
            println!("Removed '{}' from '{}' (orphan kept).", file, wad.title);
        }
        OrphanResult::NotOrphaned => {
            println!("Removed '{}' from '{}' (still linked to other WADs).", file, wad.title);
        }
    }
    Ok(())
}

fn cmd_enable(conn: &Connection, query: &[String], file: &str) -> Result<(), String> {
    let wad = resolve::resolve_one_wad(conn, query, false)?;

    let companions =
        db::get_companions_for_wad(conn, wad.id).map_err(|e| e.to_string())?;

    let comp = companions
        .iter()
        .find(|c| c.filename == file)
        .ok_or_else(|| format!("Companion '{}' not found for '{}'.", file, wad.title))?;

    if comp.enabled {
        println!("'{}' is already enabled for '{}'.", file, wad.title);
        return Ok(());
    }

    db::set_companion_enabled(conn, wad.id, comp.companion_id, true)
        .map_err(|e| e.to_string())?;
    println!("Enabled '{}' for '{}'.", file, wad.title);
    Ok(())
}

fn cmd_disable(conn: &Connection, query: &[String], file: &str) -> Result<(), String> {
    let wad = resolve::resolve_one_wad(conn, query, false)?;

    let companions =
        db::get_companions_for_wad(conn, wad.id).map_err(|e| e.to_string())?;

    let comp = companions
        .iter()
        .find(|c| c.filename == file)
        .ok_or_else(|| format!("Companion '{}' not found for '{}'.", file, wad.title))?;

    if !comp.enabled {
        println!("'{}' is already disabled for '{}'.", file, wad.title);
        return Ok(());
    }

    db::set_companion_enabled(conn, wad.id, comp.companion_id, false)
        .map_err(|e| e.to_string())?;
    println!("Disabled '{}' for '{}'.", file, wad.title);
    Ok(())
}

fn cmd_ls(conn: &Connection, query: &[String], plain: bool) -> Result<(), String> {
    if query.is_empty() {
        // List all companions
        return list_all_companions(conn, plain);
    }

    let wad = resolve::resolve_one_wad(conn, query, false)?;
    let companions =
        db::get_companions_for_wad(conn, wad.id).map_err(|e| e.to_string())?;

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
            println!("  {} ({}, md5: {})", c.filename, size, &c.md5[..12.min(c.md5.len())]);
        }
    }
    Ok(())
}

/// Count how many other WADs (besides `exclude_wad_id`) link to this companion.
fn companions_linked_elsewhere(
    conn: &Connection,
    companion_id: i64,
    exclude_wad_id: i64,
) -> Result<i64, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wad_companions WHERE companion_id = ? AND wad_id != ?",
            rusqlite::params![companion_id, exclude_wad_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(count)
}


#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{self, init_db, open_memory};
    use caco_core::db::wads::{add_wad, NewWad};
    use caco_core::db::models::SourceType;

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection, title: &str) -> i64 {
        add_wad(conn, &NewWad::new(title, SourceType::Local)).unwrap()
    }

    // -- companions_linked_elsewhere tests --

    #[test]
    fn test_companions_linked_elsewhere_none() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "WAD 1");
        let c_id = db::add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        // Only linked to this WAD — no links elsewhere
        let count = companions_linked_elsewhere(&conn, c_id, wad_id).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_companions_linked_elsewhere_some() {
        let conn = setup();
        let w1 = add_test_wad(&conn, "WAD 1");
        let w2 = add_test_wad(&conn, "WAD 2");
        let c_id = db::add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();

        db::link_companion_to_wad(&conn, w1, c_id).unwrap();
        db::link_companion_to_wad(&conn, w2, c_id).unwrap();

        // w2 also links to it
        let count = companions_linked_elsewhere(&conn, c_id, w1).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_companions_linked_elsewhere_not_linked() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "WAD 1");
        let c_id = db::add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();

        // Not linked at all
        let count = companions_linked_elsewhere(&conn, c_id, wad_id).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_companions_linked_elsewhere_multiple() {
        let conn = setup();
        let w1 = add_test_wad(&conn, "WAD 1");
        let w2 = add_test_wad(&conn, "WAD 2");
        let w3 = add_test_wad(&conn, "WAD 3");
        let c_id = db::add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();

        db::link_companion_to_wad(&conn, w1, c_id).unwrap();
        db::link_companion_to_wad(&conn, w2, c_id).unwrap();
        db::link_companion_to_wad(&conn, w3, c_id).unwrap();

        // w2 and w3 also link to it
        let count = companions_linked_elsewhere(&conn, c_id, w1).unwrap();
        assert_eq!(count, 2);
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
        let c_id = db::add_companion(&conn, "md5test", "patch.deh", "/path/patch.deh", 100).unwrap();
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
        let c_id = db::add_companion(&conn, "md5test", "patch.deh", "/path/patch.deh", 100).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        assert!(db::would_be_orphan(&conn, c_id, wad_id).unwrap());
    }

    #[test]
    fn test_would_be_orphan_shared_link() {
        let conn = setup();
        let w1 = add_test_wad(&conn, "WAD 1");
        let w2 = add_test_wad(&conn, "WAD 2");
        let c_id = db::add_companion(&conn, "md5test", "patch.deh", "/path/patch.deh", 100).unwrap();
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
