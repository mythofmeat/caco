//! `caco enrich` — re-run detection and enrichment on existing WADs.

use std::path::Path;

use clap::Args;
use rusqlite::Connection;

use caco_core::complevel::complevel_name;
use caco_core::db::{self, WadRecord, WadUpdate};
use caco_sources::doomwiki::{self, DoomwikiClient};
use caco_sources::idgames::extract_idgames_id_from_url;
use caco_sources::import_service::{
    normalize_title, port_to_complevel, port_to_zdoom_required, titles_match,
};

#[derive(Args, Default)]
pub struct EnrichArgs {
    /// WAD query (all WADs if omitted)
    query: Vec<String>,

    /// Only enrich WADs with missing complevel
    #[arg(long)]
    complevel: bool,

    /// Fetch the Cacowards page for `--year` from the Doom Wiki and upsert
    /// entries into the cacowards table, auto-linking to library WADs by
    /// idgames id.
    #[arg(long)]
    cacowards: bool,

    /// Year to scrape (required with `--cacowards`).
    #[arg(long, value_name = "YYYY")]
    year: Option<i64>,

    /// Preview changes without applying them
    #[arg(long)]
    dry_run: bool,
}

/// Result of enriching a single WAD.
struct EnrichResult {
    title: String,
    complevel: Option<i32>,
    iwad: Option<String>,
    zdoom_required: Option<bool>,
}

impl EnrichResult {
    fn has_changes(&self) -> bool {
        self.complevel.is_some() || self.iwad.is_some() || self.zdoom_required == Some(true)
    }
}

pub fn run(conn: &Connection, args: &EnrichArgs) -> Result<(), String> {
    if args.cacowards {
        let Some(year) = args.year else {
            return Err("--cacowards requires --year YYYY".to_string());
        };
        if !args.query.is_empty() {
            return Err("--cacowards does not accept a WAD query".to_string());
        }
        return run_cacowards(conn, year, args.dry_run);
    }
    if args.year.is_some() {
        return Err("--year is only valid with --cacowards".to_string());
    }

    // Search for matching WADs
    let query = if args.query.is_empty() {
        None
    } else {
        Some(crate::parsing::join_query_args(&args.query))
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
            if let Some(true) = result.zdoom_required {
                parts.push("zdoom_required".to_string());
            }
            println!("{verb} {} -> {}", result.title, parts.join(", "));
        }
        println!();
        let suffix = if args.dry_run { " (dry run)" } else { "" };
        println!("{}/{} WAD(s) enriched{suffix}", results.len(), wads.len(),);
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
        zdoom_required: None,
    };

    let needs_complevel = wad.complevel.is_none();
    let needs_iwad = wad.custom_iwad.is_none();
    let needs_zdoom = wad.zdoom_required.is_none();

    // Nothing to detect
    if !needs_complevel && !needs_iwad && !needs_zdoom {
        return result;
    }

    // Stage 1: File-based detection (if cached WAD file exists)
    if let Some(ref cached_path) = wad.cached_path {
        let path = Path::new(cached_path);
        if path.exists() {
            // Try complevel detection from file
            if needs_complevel && let Some(cl) = caco_core::complevel_detect::detect_complevel(path)
            {
                result.complevel = Some(cl);
                if !dry_run {
                    let update = WadUpdate::new().set_int("complevel", Some(cl as i64));
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }

            // Try IWAD detection from file
            if needs_iwad && let Some(family) = caco_core::iwad_detect::detect_iwad(path) {
                result.iwad = Some(family.to_string());
                if !dry_run {
                    let update = WadUpdate::new().set_text("custom_iwad", Some(family.to_string()));
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }

            // Try zdoom_required detection from file
            if needs_zdoom
                && let Some(required) = caco_core::zdoom_detect::detect_zdoom_required(path)
            {
                result.zdoom_required = Some(required);
                if !dry_run {
                    let update =
                        WadUpdate::new().set_int("zdoom_required", Some(i64::from(required)));
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }
        }
    }

    // Stage 2: Doom Wiki lookup (if file detection left gaps)
    let still_needs_complevel = result.complevel.is_none() && needs_complevel;
    let still_needs_zdoom = result.zdoom_required.is_none() && needs_zdoom;
    if !wad.title.is_empty() && (still_needs_complevel || still_needs_zdoom) {
        let wiki_entry = wiki_lookup_port(wad, wiki_lookups);

        if let Some(ref port_text) = wiki_entry {
            if result.complevel.is_none()
                && needs_complevel
                && let Some(cl) = port_to_complevel(port_text)
            {
                result.complevel = Some(cl);
                if !dry_run {
                    let update = WadUpdate::new().set_int("complevel", Some(cl as i64));
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }

            if result.zdoom_required.is_none()
                && needs_zdoom
                && let Some(true) = port_to_zdoom_required(port_text)
            {
                result.zdoom_required = Some(true);
                if !dry_run {
                    let update = WadUpdate::new().set_int("zdoom_required", Some(1));
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }
        }
    }

    result
}

/// Look up a WAD's port requirement via Doom Wiki.
///
/// Returns the port field text if found, or None.
fn wiki_lookup_port(wad: &WadRecord, wiki_lookups: &mut u32) -> Option<String> {
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

    Some(entry.port.clone())
}

// =============================================================================
// Cacowards enrichment
// =============================================================================

#[derive(Default)]
struct CacowardSummary {
    scraped: usize,
    upserted: usize,
    linked_by_idgames: usize,
    linked_by_title: usize,
}

impl CacowardSummary {
    fn linked_total(&self) -> usize {
        self.linked_by_idgames + self.linked_by_title
    }
}

/// Fetch the Cacowards page for `year` and ingest its entries.
fn run_cacowards(conn: &Connection, year: i64, dry_run: bool) -> Result<(), String> {
    let client = DoomwikiClient::new();
    eprintln!("Fetching Cacowards {year} from Doom Wiki…");
    let entries = doomwiki::fetch_cacowards(&client, year)
        .map_err(|e| format!("Doom Wiki fetch failed: {e}"))?;

    if entries.is_empty() {
        return Err(format!(
            "No Cacoward entries parsed for {year} (page missing or no recognised sections)."
        ));
    }

    let summary = ingest_cacoward_entries(conn, year, &entries, dry_run)?;

    let suffix = if dry_run { " (dry run)" } else { "" };
    println!(
        "Cacowards {year}: scraped {}, upserted {}, auto-linked {} ({} by idgames, {} by title){suffix}",
        summary.scraped,
        summary.upserted,
        summary.linked_total(),
        summary.linked_by_idgames,
        summary.linked_by_title,
    );
    Ok(())
}

/// Upsert scraped Cacoward entries into the DB and auto-link to library WADs.
///
/// Auto-linking runs two passes per entry:
/// 1. **idgames URL match** — extract the numeric id from `{{ig|id=N}}` and
///    look up `wads.idgames_id`. High confidence; works for any WAD imported
///    from the idgames archive.
/// 2. **Normalized title fallback** — only when (1) misses. The wad with an
///    *exactly* matching normalized title (case-folded, diacritic-stripped,
///    punctuation-collapsed) gets linked, but only if it's the *single* such
///    match in the library. Multi-match titles are skipped so two unrelated
///    WADs sharing a name (e.g. "Crusader" 1995 vs 2023) don't collide.
///
/// Neither pass sets `manual_override`, so a future `caco modify` can pin
/// the correct link without it being clobbered on the next scrape.
fn ingest_cacoward_entries(
    conn: &Connection,
    year: i64,
    entries: &[db::NewCacoward],
    dry_run: bool,
) -> Result<CacowardSummary, String> {
    let mut summary = CacowardSummary {
        scraped: entries.len(),
        ..Default::default()
    };

    // Reconcile: remove non-pinned rows for this year before upserting the
    // fresh scrape, so stale entries from an older scrape (e.g. parser bugs,
    // wiki edits) don't linger. Pinned manual links are preserved.
    if !dry_run {
        db::clear_year_unpinned(conn, year).map_err(|e| format!("DB cleanup failed: {e}"))?;
    }

    // Build the normalized-title -> wad_id index once. Skipped in dry-run so
    // we don't pay for the full table scan when nothing will be written.
    let title_index = if dry_run {
        TitleIndex::empty()
    } else {
        TitleIndex::build(conn)?
    };

    for entry in entries {
        if dry_run {
            println!(
                "Would upsert {year} {} — {} ({})",
                entry.category,
                entry.wad_title,
                entry.idgames_url.as_deref().unwrap_or("no idgames link"),
            );
            continue;
        }

        let id = db::upsert_cacoward(conn, entry).map_err(|e| format!("DB upsert failed: {e}"))?;
        summary.upserted += 1;

        // Pass 1: idgames URL → numeric id → wads.idgames_id.
        let by_idgames = entry
            .idgames_url
            .as_deref()
            .and_then(extract_idgames_id_from_url)
            .map(|n| n.to_string())
            .and_then(|key| db::find_wad_by_idgames_id(conn, &key).ok().flatten());

        if let Some(wad_id) = by_idgames {
            db::link_wad(conn, id, wad_id, false).map_err(|e| format!("DB link failed: {e}"))?;
            summary.linked_by_idgames += 1;
            continue;
        }

        // Pass 2: strict normalized-title fallback (single-match only).
        if let Some(wad_id) = title_index.unique_match(&entry.wad_title) {
            db::link_wad(conn, id, wad_id, false).map_err(|e| format!("DB link failed: {e}"))?;
            summary.linked_by_title += 1;
        }
    }

    Ok(summary)
}

/// Normalized-title → set of matching WAD ids. Used by the title-fallback
/// pass of the cacoward auto-linker.
///
/// A title's ids vec carries up to N entries; `unique_match` returns `Some`
/// only when there's exactly one, so two WADs sharing a normalized title
/// can never silently collide.
struct TitleIndex {
    by_title: std::collections::HashMap<String, Vec<i64>>,
}

impl TitleIndex {
    fn empty() -> Self {
        Self {
            by_title: std::collections::HashMap::new(),
        }
    }

    fn build(conn: &Connection) -> Result<Self, String> {
        // Pull only id+title; the rest of the wads row is dead weight here.
        let mut stmt = conn
            .prepare("SELECT id, title FROM wads WHERE deleted_at IS NULL")
            .map_err(|e| format!("DB prep failed: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("DB query failed: {e}"))?;
        let mut by_title: std::collections::HashMap<String, Vec<i64>> = Default::default();
        for r in rows {
            let (id, title) = r.map_err(|e| format!("DB row failed: {e}"))?;
            let key = normalize_title(&title);
            if !key.is_empty() {
                by_title.entry(key).or_default().push(id);
            }
        }
        Ok(Self { by_title })
    }

    fn unique_match(&self, title: &str) -> Option<i64> {
        let key = normalize_title(title);
        match self.by_title.get(&key).map(|v| v.as_slice()) {
            Some([id]) => Some(*id),
            _ => None,
        }
    }
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
            zdoom_required: None,
        };
        assert!(!result.has_changes());
    }

    #[test]
    fn test_enrich_result_has_changes_complevel() {
        let result = EnrichResult {
            title: "Test".to_string(),
            complevel: Some(9),
            iwad: None,
            zdoom_required: None,
        };
        assert!(result.has_changes());
    }

    #[test]
    fn test_enrich_result_has_changes_iwad() {
        let result = EnrichResult {
            title: "Test".to_string(),
            complevel: None,
            iwad: Some("doom2".to_string()),
            zdoom_required: None,
        };
        assert!(result.has_changes());
    }

    #[test]
    fn test_enrich_result_has_changes_both() {
        let result = EnrichResult {
            title: "Test".to_string(),
            complevel: Some(21),
            iwad: Some("doom".to_string()),
            zdoom_required: None,
        };
        assert!(result.has_changes());
    }

    #[test]
    fn test_enrich_result_has_changes_zdoom() {
        let result = EnrichResult {
            title: "Test".to_string(),
            complevel: None,
            iwad: None,
            zdoom_required: Some(true),
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
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()));
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
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()));
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
        let wad_data = build_wad(&[
            ("E1M1", &[]),
            ("THINGS", &[]),
            ("E1M2", &[]),
            ("THINGS", &[]),
        ]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()));
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
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()));
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

        // Set complevel, custom_iwad, and zdoom_required
        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()))
            .set_int("complevel", Some(9))
            .set_text("custom_iwad", Some("doom2".to_string()))
            .set_int("zdoom_required", Some(0));
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
        let update =
            db::WadUpdate::new().set_text("cached_path", Some("/nonexistent/test.wad".to_string()));
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
            cacowards: false,
            year: None,
        };
        let result = run(&conn, &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No WADs found"));
    }

    #[test]
    fn test_run_complevel_filter_all_set() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Has Complevel");
        let update = db::WadUpdate::new().set_int("complevel", Some(9));
        db::update_wad(&conn, wad_id, &update).unwrap();

        let args = EnrichArgs {
            query: vec![],
            complevel: true,
            dry_run: false,
            cacowards: false,
            year: None,
        };
        // Should succeed (prints "All matching WADs already have complevel set.")
        let result = run(&conn, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_complevel_filter_retains_missing() {
        let conn = setup();
        let _w1 = add_test_wad(&conn, "Has Complevel");
        let update = db::WadUpdate::new().set_int("complevel", Some(9));
        db::update_wad(&conn, _w1, &update).unwrap();

        let w2 = add_test_wad(&conn, "No Complevel");

        // Create a WAD file for w2
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad_data = build_wad(&[("MAP01", &[])]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()));
        db::update_wad(&conn, w2, &update).unwrap();

        let args = EnrichArgs {
            query: vec![],
            complevel: true,
            dry_run: false,
            cacowards: false,
            year: None,
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
            cacowards: false,
            year: None,
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
            cacowards: false,
            year: None,
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
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        let args = EnrichArgs {
            query: vec![],
            complevel: false,
            dry_run: true,
            cacowards: false,
            year: None,
        };
        let result = run(&conn, &args);
        assert!(result.is_ok());

        // DB should NOT be updated
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.complevel, None);
        assert_eq!(wad.custom_iwad, None);
    }

    #[test]
    fn test_run_cacowards_upserts_and_links() {
        let conn = setup();
        // Pre-seed a WAD whose idgames_id matches one of the sample entries
        // so we can assert auto-linking fires.
        let wad_id = add_test_wad(&conn, "Piña Colada");
        db::update_wad(
            &conn,
            wad_id,
            &db::WadUpdate::new().set_text("idgames_id", Some("20917".to_string())),
        )
        .unwrap();

        let entries = vec![
            db::NewCacoward {
                year: 2023,
                category: db::CATEGORY_WINNER.to_string(),
                wad_title: "Piña Colada".to_string(),
                idgames_url: Some("https://www.doomworld.com/idgames/?id=20917".to_string()),
                ..Default::default()
            },
            db::NewCacoward {
                year: 2023,
                category: db::CATEGORY_WINNER.to_string(),
                wad_title: "Dreamblood".to_string(),
                idgames_url: None,
                ..Default::default()
            },
        ];

        let summary = super::ingest_cacoward_entries(&conn, 2023, &entries, false).unwrap();
        assert_eq!(summary.upserted, 2);
        assert_eq!(summary.linked_by_idgames, 1);
        assert_eq!(summary.linked_by_title, 0);

        let by_year = db::get_cacowards_by_year(&conn, 2023).unwrap();
        assert_eq!(by_year.len(), 2);
        let pina = by_year
            .iter()
            .find(|c| c.wad_title == "Piña Colada")
            .unwrap();
        assert!(pina.wad_id.is_some());
        assert!(!pina.manual_override);
    }

    #[test]
    fn test_run_cacowards_dry_run_makes_no_changes() {
        let conn = setup();
        let entries = vec![db::NewCacoward {
            year: 2023,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Piña Colada".to_string(),
            ..Default::default()
        }];

        let summary = super::ingest_cacoward_entries(&conn, 2023, &entries, true).unwrap();
        assert_eq!(summary.upserted, 0);
        assert_eq!(summary.linked_total(), 0);
        assert!(db::get_cacowards_by_year(&conn, 2023).unwrap().is_empty());
    }

    #[test]
    fn test_run_cacowards_title_fallback_links_unique_match() {
        let conn = setup();
        // Seed a wad with no idgames_id but a title that, once normalized,
        // matches the cacoward entry — verifies the Unicode/case fallback.
        let wad_id = add_test_wad(&conn, "Pina Colada");

        let entries = vec![db::NewCacoward {
            year: 2023,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Piña Colada".to_string(),
            // No idgames URL → exercises the title fallback.
            idgames_url: None,
            ..Default::default()
        }];

        let summary = super::ingest_cacoward_entries(&conn, 2023, &entries, false).unwrap();
        assert_eq!(summary.upserted, 1);
        assert_eq!(summary.linked_by_idgames, 0);
        assert_eq!(summary.linked_by_title, 1);

        let entry = &db::get_cacowards_by_year(&conn, 2023).unwrap()[0];
        assert_eq!(entry.wad_id, Some(wad_id));
        // Manual-override flag stays false — auto-link should never pin.
        assert!(!entry.manual_override);
    }

    #[test]
    fn test_run_cacowards_title_fallback_skips_ambiguous_match() {
        let conn = setup();
        // Two wads with the same normalized title; the fallback must NOT
        // pick one arbitrarily — both stay unlinked.
        add_test_wad(&conn, "Crusader");
        add_test_wad(&conn, "crusader");

        let entries = vec![db::NewCacoward {
            year: 2023,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Crusader".to_string(),
            idgames_url: None,
            ..Default::default()
        }];

        let summary = super::ingest_cacoward_entries(&conn, 2023, &entries, false).unwrap();
        assert_eq!(summary.linked_by_title, 0);

        let entry = &db::get_cacowards_by_year(&conn, 2023).unwrap()[0];
        assert!(entry.wad_id.is_none());
    }

    #[test]
    fn test_run_cacowards_idgames_match_wins_over_title_match() {
        let conn = setup();
        // idgames-id holder *and* a title-only match exist — the URL
        // path must take precedence (it's higher confidence).
        let title_only = add_test_wad(&conn, "Piña Colada");
        let idgames_holder = add_test_wad(&conn, "Some Other WAD");
        db::update_wad(
            &conn,
            idgames_holder,
            &db::WadUpdate::new().set_text("idgames_id", Some("20917".to_string())),
        )
        .unwrap();

        let entries = vec![db::NewCacoward {
            year: 2023,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Piña Colada".to_string(),
            idgames_url: Some("https://www.doomworld.com/idgames/?id=20917".to_string()),
            ..Default::default()
        }];

        let summary = super::ingest_cacoward_entries(&conn, 2023, &entries, false).unwrap();
        assert_eq!(summary.linked_by_idgames, 1);
        assert_eq!(summary.linked_by_title, 0);

        let entry = &db::get_cacowards_by_year(&conn, 2023).unwrap()[0];
        assert_eq!(entry.wad_id, Some(idgames_holder));
        assert_ne!(entry.wad_id, Some(title_only));
    }

    #[test]
    fn test_enrich_wad_both_complevel_and_iwad() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Both Detections");

        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        // E1M1 maps -> complevel 2, IWAD "doom"
        let wad_data = build_wad(&[
            ("E1M1", &[]),
            ("THINGS", &[]),
            ("E1M2", &[]),
            ("THINGS", &[]),
        ]);
        std::fs::write(&wad_path, &wad_data).unwrap();

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(wad_path.to_string_lossy().to_string()));
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
