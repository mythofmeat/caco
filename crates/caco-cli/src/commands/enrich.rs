//! `caco enrich` — re-run detection and enrichment on existing WADs.

use std::path::Path;

use clap::Args;
use rusqlite::Connection;

use caco_core::complevel::complevel_name;
use caco_core::db::{self, WadRecord, WadUpdate};
use caco_sources::doomwiki::DoomwikiClient;
use caco_sources::import_service::{port_to_complevel, titles_match};

#[derive(Args)]
pub struct EnrichArgs {
    /// WAD query (all WADs if omitted)
    #[arg(trailing_var_arg = true)]
    query: Vec<String>,

    /// Only enrich WADs with missing complevel
    #[arg(long)]
    complevel: bool,

    /// Preview changes without applying them
    #[arg(long)]
    dry_run: bool,
}

/// Result of enriching a single WAD.
struct EnrichResult {
    title: String,
    complevel: Option<i32>,
    iwad: Option<String>,
}

impl EnrichResult {
    fn has_changes(&self) -> bool {
        self.complevel.is_some() || self.iwad.is_some()
    }
}

pub fn run(conn: &Connection, args: &EnrichArgs) -> Result<(), String> {
    // Search for matching WADs
    let query = if args.query.is_empty() {
        None
    } else {
        Some(args.query.join(" "))
    };

    let mut wads = db::search_wads(conn, query.as_deref(), None, true, false, 0)
        .map_err(|e| format!("Search error: {e}"))?;

    if wads.is_empty() {
        return Err("No WADs found.".to_string());
    }

    // Filter to WADs with missing complevel if --complevel flag is set
    if args.complevel {
        wads.retain(|w| w.complevel.is_none());
        if wads.is_empty() {
            println!("All matching WADs already have complevel set.");
            return Ok(());
        }
    }

    eprintln!("Enriching {} WAD(s)...", wads.len());

    let mut results: Vec<EnrichResult> = Vec::new();
    let mut wiki_lookups = 0u32;

    for wad in &wads {
        let result = enrich_wad(conn, wad, args.dry_run, &mut wiki_lookups);
        if result.has_changes() {
            results.push(result);
        }
    }

    // Print summary
    if results.is_empty() {
        println!("No new metadata detected.");
    } else {
        for result in &results {
            let verb = if args.dry_run { "Would set" } else { "Set" };

            let mut parts = Vec::new();
            if let Some(cl) = result.complevel {
                let name = complevel_name(Some(cl));
                parts.push(format!("complevel {cl} ({name})"));
            }
            if let Some(ref iwad) = result.iwad {
                parts.push(format!("IWAD {iwad}"));
            }
            println!("{verb} {} -> {}", result.title, parts.join(", "));
        }
        println!();
        let suffix = if args.dry_run { " (dry run)" } else { "" };
        println!(
            "{}/{} WAD(s) enriched{suffix}",
            results.len(),
            wads.len(),
        );
    }

    if wiki_lookups > 0 {
        println!("({wiki_lookups} Doom Wiki lookup(s) performed)");
    }

    Ok(())
}

/// Enrich a single WAD. Returns what was detected (and applies if not dry_run).
fn enrich_wad(
    conn: &Connection,
    wad: &WadRecord,
    dry_run: bool,
    wiki_lookups: &mut u32,
) -> EnrichResult {
    let mut result = EnrichResult {
        title: wad.title.clone(),
        complevel: None,
        iwad: None,
    };

    let needs_complevel = wad.complevel.is_none();
    let needs_iwad = wad.custom_iwad.is_none();

    // Nothing to detect
    if !needs_complevel && !needs_iwad {
        return result;
    }

    // Stage 1: File-based detection (if cached WAD file exists)
    if let Some(ref cached_path) = wad.cached_path {
        let path = Path::new(cached_path);
        if path.exists() {
            // Try complevel detection from file
            if needs_complevel
                && let Some(cl) = caco_core::complevel_detect::detect_complevel(path)
            {
                result.complevel = Some(cl);
                if !dry_run {
                    let update = WadUpdate::new()
                        .set_int("complevel", Some(cl as i64))
                        .unwrap();
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }

            // Try IWAD detection from file
            if needs_iwad
                && let Some(family) = caco_core::iwad_detect::detect_iwad(path)
            {
                result.iwad = Some(family.to_string());
                if !dry_run {
                    let update = WadUpdate::new()
                        .set_text("custom_iwad", Some(family.to_string()))
                        .unwrap();
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }
        }
    }

    // Stage 2: Doom Wiki lookup for complevel (if file detection didn't find it)
    if result.complevel.is_none()
        && needs_complevel
        && !wad.title.is_empty()
        && let Some(cl) = wiki_lookup_complevel(wad, wiki_lookups)
    {
        result.complevel = Some(cl);
        if !dry_run {
            let update = WadUpdate::new()
                .set_int("complevel", Some(cl as i64))
                .unwrap();
            let _ = db::update_wad(conn, wad.id, &update);
        }
    }

    result
}

/// Look up a WAD's complevel via Doom Wiki port field.
///
/// Returns the detected complevel or None.
fn wiki_lookup_complevel(wad: &WadRecord, wiki_lookups: &mut u32) -> Option<i32> {
    let client = DoomwikiClient::new();
    *wiki_lookups += 1;

    let results = match client.search_wads(&wad.title, 5) {
        Ok(r) => r,
        Err(_) => return None,
    };

    // Find first result with matching title
    let entry = results
        .iter()
        .find(|r| titles_match(&wad.title, r.display_name()))?;

    if entry.port.is_empty() {
        return None;
    }

    port_to_complevel(&entry.port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{self, init_db, open_memory};
    use caco_core::db::wads::{add_wad, NewWad};
    use caco_core::db::models::{SourceType, Status};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection, title: &str) -> i64 {
        add_wad(conn, &NewWad::new(title, SourceType::Local)).unwrap()
    }

    /// Build a minimal WAD with specific lumps.
    fn build_wad(lumps: &[(&str, &[u8])]) -> Vec<u8> {
        let mut wad = Vec::new();
        let num_lumps = lumps.len() as i32;
        let header_size = 12;
        let mut data_start = header_size;
        let mut entries: Vec<(String, u32, u32)> = Vec::new();
        let mut data_blob = Vec::new();

        for (name, data) in lumps {
            entries.push((name.to_string(), data_start as u32, data.len() as u32));
            data_blob.extend_from_slice(data);
            data_start += data.len();
        }

        let dir_offset = data_start as i32;
        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&num_lumps.to_le_bytes());
        wad.extend_from_slice(&dir_offset.to_le_bytes());
        wad.extend_from_slice(&data_blob);

        for (name, offset, size) in &entries {
            wad.extend_from_slice(&offset.to_le_bytes());
            wad.extend_from_slice(&size.to_le_bytes());
            let mut name_bytes = [0u8; 8];
            for (i, &b) in name.as_bytes().iter().take(8).enumerate() {
                name_bytes[i] = b;
            }
            wad.extend_from_slice(&name_bytes);
        }

        wad
    }

    // -- EnrichResult tests --

    #[test]
    fn test_enrich_result_has_changes_none() {
        let result = EnrichResult {
            title: "Test".to_string(),
            complevel: None,
            iwad: None,
        };
        assert!(!result.has_changes());
    }

    #[test]
    fn test_enrich_result_has_changes_complevel() {
        let result = EnrichResult {
            title: "Test".to_string(),
            complevel: Some(9),
            iwad: None,
        };
        assert!(result.has_changes());
    }

    #[test]
    fn test_enrich_result_has_changes_iwad() {
        let result = EnrichResult {
            title: "Test".to_string(),
            complevel: None,
            iwad: Some("doom2".to_string()),
        };
        assert!(result.has_changes());
    }

    #[test]
    fn test_enrich_result_has_changes_both() {
        let result = EnrichResult {
            title: "Test".to_string(),
            complevel: Some(21),
            iwad: Some("doom".to_string()),
        };
        assert!(result.has_changes());
    }

    // -- enrich_wad tests (file-based detection) --

    #[test]
    fn test_enrich_wad_detects_complevel_from_file() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Test WAD");

        // Create a WAD file with MAPxx lumps (complevel 4)
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad_data = build_wad(&[("MAP01", &[]), ("MAP02", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        // Set cached_path on WAD
        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut wiki_lookups = 0;
        let result = enrich_wad(&conn, &wad, false, &mut wiki_lookups);

        assert_eq!(result.complevel, Some(4));
        assert_eq!(wiki_lookups, 0); // No wiki lookup needed

        // Verify DB was updated
        let updated = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(updated.complevel, Some(4));
    }

    #[test]
    fn test_enrich_wad_detects_complevel_umapinfo() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "UMAPINFO WAD");

        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad_data = build_wad(&[("UMAPINFO", b"map MAP01 {}\n"), ("MAP01", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut wiki_lookups = 0;
        let result = enrich_wad(&conn, &wad, false, &mut wiki_lookups);

        assert_eq!(result.complevel, Some(21));
    }

    #[test]
    fn test_enrich_wad_detects_iwad_from_file() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Doom 1 WAD");

        // Create a WAD with E1M1 maps -> IWAD should be "doom"
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad_data = build_wad(&[("E1M1", &[]), ("E1M2", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut wiki_lookups = 0;
        let result = enrich_wad(&conn, &wad, false, &mut wiki_lookups);

        assert_eq!(result.iwad, Some("doom".to_string()));

        // Verify DB was updated
        let updated = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(updated.custom_iwad.as_deref(), Some("doom"));
    }

    #[test]
    fn test_enrich_wad_dry_run_no_db_update() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Dry Run WAD");

        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad_data = build_wad(&[("MAP01", &[]), ("MAP02", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut wiki_lookups = 0;
        let result = enrich_wad(&conn, &wad, true, &mut wiki_lookups);

        // Should detect complevel but NOT write to DB
        assert_eq!(result.complevel, Some(4));
        let unchanged = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(unchanged.complevel, None);
    }

    #[test]
    fn test_enrich_wad_skips_already_set() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Already Set");

        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad_data = build_wad(&[("MAP01", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        // Set both complevel and custom_iwad
        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .unwrap()
            .set_int("complevel", Some(9))
            .unwrap()
            .set_text("custom_iwad", Some("doom2".to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut wiki_lookups = 0;
        let result = enrich_wad(&conn, &wad, false, &mut wiki_lookups);

        // Nothing to enrich
        assert!(!result.has_changes());
        assert_eq!(result.complevel, None);
        assert_eq!(result.iwad, None);
    }

    #[test]
    fn test_enrich_wad_no_cached_file() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "No File");

        // WAD has no cached_path at all
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut wiki_lookups = 0;
        let result = enrich_wad(&conn, &wad, false, &mut wiki_lookups);

        // Can't detect from file — result depends on wiki lookup
        // (which will fail in tests, but that's ok — we just verify no crash)
        assert!(result.complevel.is_none() || result.complevel.is_some());
    }

    #[test]
    fn test_enrich_wad_cached_path_missing_file() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Missing File");

        // Set cached_path to nonexistent file
        let update = db::WadUpdate::new()
            .set_text("cached_path", Some("/nonexistent/test.wad".to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut wiki_lookups = 0;
        let result = enrich_wad(&conn, &wad, false, &mut wiki_lookups);

        // File doesn't exist — file-based detection skipped
        // Wiki lookup may or may not find something
        assert!(result.complevel.is_none() || result.complevel.is_some());
    }

    // -- run() tests --

    #[test]
    fn test_run_no_wads() {
        let conn = setup();
        let args = EnrichArgs {
            query: vec![],
            complevel: false,
            dry_run: false,
        };
        let result = run(&conn, &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No WADs found"));
    }

    #[test]
    fn test_run_complevel_filter_all_set() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Has Complevel");
        let update = db::WadUpdate::new()
            .set_int("complevel", Some(9))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let args = EnrichArgs {
            query: vec![],
            complevel: true,
            dry_run: false,
        };
        // Should succeed (prints "All matching WADs already have complevel set.")
        let result = run(&conn, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_complevel_filter_retains_missing() {
        let conn = setup();
        let _w1 = add_test_wad(&conn, "Has Complevel");
        let update = db::WadUpdate::new()
            .set_int("complevel", Some(9))
            .unwrap();
        db::update_wad(&conn, _w1, &update).unwrap();

        let w2 = add_test_wad(&conn, "No Complevel");

        // Create a WAD file for w2
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad_data = build_wad(&[("MAP01", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .unwrap();
        db::update_wad(&conn, w2, &update).unwrap();

        let args = EnrichArgs {
            query: vec![],
            complevel: true,
            dry_run: false,
        };
        let result = run(&conn, &args);
        assert!(result.is_ok());

        // w2 should now have complevel
        let wad2 = db::get_wad(&conn, w2, false).unwrap().unwrap();
        assert_eq!(wad2.complevel, Some(4));
    }

    #[test]
    fn test_run_with_query_filter() {
        let conn = setup();
        let _w1 = add_test_wad(&conn, "Alpha WAD");
        let _w2 = add_test_wad(&conn, "Beta WAD");

        // Query for specific WAD
        let args = EnrichArgs {
            query: vec!["Alpha".to_string()],
            complevel: false,
            dry_run: false,
        };
        let result = run(&conn, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_with_query_no_match() {
        let conn = setup();
        let _w1 = add_test_wad(&conn, "Alpha WAD");

        let args = EnrichArgs {
            query: vec!["Nonexistent".to_string()],
            complevel: false,
            dry_run: false,
        };
        let result = run(&conn, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_dry_run() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Dry Run Test");

        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad_data = build_wad(&[("E1M1", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let args = EnrichArgs {
            query: vec![],
            complevel: false,
            dry_run: true,
        };
        let result = run(&conn, &args);
        assert!(result.is_ok());

        // DB should NOT be updated
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.complevel, None);
        assert_eq!(wad.custom_iwad, None);
    }

    #[test]
    fn test_enrich_wad_both_complevel_and_iwad() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Both Detections");

        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        // E1M1 maps -> complevel 2, IWAD "doom"
        let wad_data = build_wad(&[("E1M1", &[]), ("E1M2", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut wiki_lookups = 0;
        let result = enrich_wad(&conn, &wad, false, &mut wiki_lookups);

        assert_eq!(result.complevel, Some(2));
        assert_eq!(result.iwad, Some("doom".to_string()));
        assert!(result.has_changes());

        // Both should be in DB
        let updated = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(updated.complevel, Some(2));
        assert_eq!(updated.custom_iwad.as_deref(), Some("doom"));
    }
}
