//! Re-run detection and metadata enrichment over WADs already in the library.
//!
//! Two jobs share this module because both re-derive metadata for rows that
//! are already imported:
//!
//! - [`enrich_wads`] fills in `complevel`, `custom_iwad` and `zdoom_required`,
//!   preferring the cached WAD file and falling back to the Doom Wiki.
//! - [`enrich_cacowards`] scrapes a year's Cacowards page and auto-links the
//!   entries to library WADs.
//!
//! Enrichment is network-bound: one Doom Wiki lookup per WAD that file
//! detection could not settle. That is why the batch driver reports progress
//! and honours a cancel check rather than just returning a summary — a GUI
//! running this over a whole library needs to show movement and let the user
//! stop.

use std::path::Path;

use rusqlite::Connection;

use caco_core::db::{self, WadRecord, WadUpdate};

use crate::cacoward_link;
use crate::doomwiki::{self, DoomwikiClient};
use crate::import_service::{port_to_complevel, port_to_zdoom_required, titles_match};
use crate::{Result, SourceError};

// =============================================================================
// WAD enrichment
// =============================================================================

/// What enrichment found for a single WAD.
#[derive(Debug, Clone, Default)]
pub struct EnrichOutcome {
    pub wad_id: i64,
    pub title: String,
    pub complevel: Option<i32>,
    pub iwad: Option<String>,
    pub zdoom_required: Option<bool>,
}

impl EnrichOutcome {
    /// Whether anything worth reporting was detected.
    ///
    /// `zdoom_required: Some(false)` is a real detection but not news — it
    /// only records that the WAD does *not* need a zdoom-family sourceport.
    fn has_changes(&self) -> bool {
        self.complevel.is_some() || self.iwad.is_some() || self.zdoom_required == Some(true)
    }
}

/// Progress signal from [`enrich_wads`].
#[derive(Debug, Clone)]
pub enum EnrichProgress<'a> {
    /// About to work on `index` of `total` (0-based).
    Started {
        index: usize,
        total: usize,
        title: &'a str,
    },
    /// Finished a WAD. Reported even when nothing was detected, so a caller
    /// can drive a progress bar off it.
    Finished(&'a EnrichOutcome),
}

/// Aggregate result of an enrichment run.
#[derive(Debug, Clone, Default)]
pub struct EnrichSummary {
    /// WADs actually visited (less than the input when cancelled).
    pub examined: usize,
    /// Outcomes with something to report, in input order.
    pub changed: Vec<EnrichOutcome>,
    /// Doom Wiki requests made — the expensive part, worth surfacing.
    pub wiki_lookups: u32,
    /// Whether `cancel` cut the run short.
    pub cancelled: bool,
}

/// Enrich a batch of WADs, reporting progress as it goes.
///
/// `cancel` is polled before each WAD; returning `true` stops the run and
/// leaves everything already written in place — enrichment is per-WAD
/// idempotent, so a cancelled run is just a shorter one, not a partial write
/// that needs undoing.
///
/// With `dry_run` nothing is written, but detection still runs (including the
/// wiki lookups), because "what would this change?" is the question being
/// asked.
pub fn enrich_wads(
    conn: &Connection,
    wads: &[WadRecord],
    dry_run: bool,
    progress: &mut dyn FnMut(EnrichProgress<'_>),
    cancel: &dyn Fn() -> bool,
) -> EnrichSummary {
    let mut summary = EnrichSummary::default();
    let total = wads.len();

    for (index, wad) in wads.iter().enumerate() {
        if cancel() {
            summary.cancelled = true;
            break;
        }

        progress(EnrichProgress::Started {
            index,
            total,
            title: &wad.title,
        });

        let outcome = enrich_one(conn, wad, dry_run, &mut summary.wiki_lookups);
        summary.examined += 1;

        progress(EnrichProgress::Finished(&outcome));
        if outcome.has_changes() {
            summary.changed.push(outcome);
        }
    }

    summary
}

/// Enrich a single WAD, applying the findings unless `dry_run`.
///
/// Only fields that are still unset are detected: enrichment never overwrites
/// a value the user (or a previous run) already chose.
fn enrich_one(
    conn: &Connection,
    wad: &WadRecord,
    dry_run: bool,
    wiki_lookups: &mut u32,
) -> EnrichOutcome {
    let mut outcome = EnrichOutcome {
        wad_id: wad.id,
        title: wad.title.clone(),
        ..Default::default()
    };

    let needs_complevel = wad.complevel.is_none();
    let needs_iwad = wad.custom_iwad.is_none();
    let needs_zdoom = wad.zdoom_required.is_none();

    if !needs_complevel && !needs_iwad && !needs_zdoom {
        return outcome;
    }

    // Stage 1: the WAD file itself, when one is cached. Cheap and offline, so
    // it always runs before reaching for the network.
    if let Some(ref cached_path) = wad.cached_path {
        let path = Path::new(cached_path);
        if path.exists() {
            if needs_complevel && let Some(cl) = caco_core::complevel_detect::detect_complevel(path)
            {
                outcome.complevel = Some(cl);
                if !dry_run {
                    let update = WadUpdate::new().set_int("complevel", Some(cl as i64));
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }

            if needs_iwad && let Some(family) = caco_core::iwad_detect::detect_iwad(path) {
                outcome.iwad = Some(family.to_string());
                if !dry_run {
                    let update = WadUpdate::new().set_text("custom_iwad", Some(family.to_string()));
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }

            if needs_zdoom
                && let Some(required) = caco_core::zdoom_detect::detect_zdoom_required(path)
            {
                outcome.zdoom_required = Some(required);
                if !dry_run {
                    let update =
                        WadUpdate::new().set_int("zdoom_required", Some(i64::from(required)));
                    let _ = db::update_wad(conn, wad.id, &update);
                }
            }
        }
    }

    // Stage 2: Doom Wiki, only for the gaps the file could not fill. The IWAD
    // is deliberately not looked up here — the wiki's sourceport field says nothing
    // about which IWAD a PWAD needs.
    let still_needs_complevel = outcome.complevel.is_none() && needs_complevel;
    let still_needs_zdoom = outcome.zdoom_required.is_none() && needs_zdoom;
    if wad.title.is_empty() || !(still_needs_complevel || still_needs_zdoom) {
        return outcome;
    }

    let Some(port_text) = wiki_lookup_port(&wad.title, wiki_lookups) else {
        return outcome;
    };

    if still_needs_complevel && let Some(cl) = port_to_complevel(&port_text) {
        outcome.complevel = Some(cl);
        if !dry_run {
            let update = WadUpdate::new().set_int("complevel", Some(cl as i64));
            let _ = db::update_wad(conn, wad.id, &update);
        }
    }

    if still_needs_zdoom && let Some(true) = port_to_zdoom_required(&port_text) {
        outcome.zdoom_required = Some(true);
        if !dry_run {
            let update = WadUpdate::new().set_int("zdoom_required", Some(1));
            let _ = db::update_wad(conn, wad.id, &update);
        }
    }

    outcome
}

/// Look up a WAD's sourceport requirement on the Doom Wiki.
///
/// Returns the port field text of the first search hit whose title matches,
/// or `None` — a failed lookup is not an error worth propagating, it just
/// means this WAD keeps its gaps.
fn wiki_lookup_port(title: &str, wiki_lookups: &mut u32) -> Option<String> {
    let client = DoomwikiClient::new();
    *wiki_lookups += 1;

    let results = client.search_wads(title, 5).ok()?;
    let entry = results
        .iter()
        .find(|r| titles_match(title, r.display_name()))?;

    if entry.port.is_empty() {
        return None;
    }
    Some(entry.port.clone())
}

// =============================================================================
// Cacowards enrichment
// =============================================================================

/// What a dry-run cacoward ingest would have written.
#[derive(Debug, Clone)]
pub struct CacowardPreview {
    pub category: String,
    pub wad_title: String,
    pub idgames_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CacowardSummary {
    pub year: i64,
    pub scraped: usize,
    pub upserted: usize,
    pub linked_by_idgames: usize,
    pub linked_by_title: usize,
    /// Populated only on a dry run.
    pub previews: Vec<CacowardPreview>,
}

impl CacowardSummary {
    pub fn linked_total(&self) -> usize {
        self.linked_by_idgames + self.linked_by_title
    }
}

/// Fetch the Cacowards page for `year` and ingest its entries.
pub fn enrich_cacowards(conn: &Connection, year: i64, dry_run: bool) -> Result<CacowardSummary> {
    let client = DoomwikiClient::new();
    let entries = doomwiki::fetch_cacowards(&client, year)?;

    if entries.is_empty() {
        return Err(SourceError::Api(format!(
            "No Cacoward entries parsed for {year} (page missing or no recognised sections)."
        )));
    }

    ingest_cacoward_entries(conn, year, &entries, dry_run)
}

/// Upsert scraped Cacoward entries into the DB and auto-link the year's rows to
/// library WADs; [`cacoward_link`] has the rules.
///
/// One transaction, because the year's unpinned rows are deleted first and a
/// failure partway through would lose them and their links.
fn ingest_cacoward_entries(
    conn: &Connection,
    year: i64,
    entries: &[db::NewCacoward],
    dry_run: bool,
) -> Result<CacowardSummary> {
    let mut summary = CacowardSummary {
        year,
        scraped: entries.len(),
        ..Default::default()
    };

    if dry_run {
        summary.previews = entries
            .iter()
            .map(|entry| CacowardPreview {
                category: entry.category.clone(),
                wad_title: entry.wad_title.clone(),
                idgames_url: entry.idgames_url.clone(),
            })
            .collect();
        return Ok(summary);
    }

    let links = db::with_transaction(conn, |tx| {
        // Reconcile: remove non-pinned rows for this year before upserting the
        // fresh scrape, so stale entries from an older scrape (e.g. parser bugs,
        // wiki edits) don't linger. Pinned manual links are preserved.
        db::clear_year_unpinned(tx, year)?;
        for entry in entries {
            db::upsert_cacoward(tx, entry)?;
        }
        cacoward_link::link_year(tx, year)
    })?;

    summary.upserted = entries.len();
    summary.linked_by_idgames = links.by_idgames;
    summary.linked_by_title = links.by_title;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::models::SourceType;
    use caco_core::db::wads::{NewWad, add_wad};
    use caco_core::db::{init_db, open_memory};

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

    /// Add a WAD with a real cached file containing `lumps`.
    fn add_wad_with_file(
        conn: &Connection,
        dir: &Path,
        title: &str,
        lumps: &[(&str, &[u8])],
    ) -> i64 {
        let wad_id = add_test_wad(conn, title);
        let path = dir.join(format!("{}.wad", wad_id));
        std::fs::write(&path, build_wad(lumps)).unwrap();
        db::update_wad(
            conn,
            wad_id,
            &WadUpdate::new().set_text("cached_path", Some(path.to_string_lossy().to_string())),
        )
        .unwrap();
        wad_id
    }

    fn never_cancel() -> impl Fn() -> bool {
        || false
    }

    // -- EnrichOutcome tests --

    #[test]
    fn test_has_changes() {
        let base = EnrichOutcome {
            wad_id: 1,
            title: "Test".to_string(),
            ..Default::default()
        };
        assert!(!base.has_changes());
        assert!(
            EnrichOutcome {
                complevel: Some(9),
                ..base.clone()
            }
            .has_changes()
        );
        assert!(
            EnrichOutcome {
                iwad: Some("doom2".to_string()),
                ..base.clone()
            }
            .has_changes()
        );
        assert!(
            EnrichOutcome {
                zdoom_required: Some(true),
                ..base.clone()
            }
            .has_changes()
        );
        // "Confirmed not zdoom" is a detection, but nothing to report.
        assert!(
            !EnrichOutcome {
                zdoom_required: Some(false),
                ..base
            }
            .has_changes()
        );
    }

    // -- enrich_one tests --

    #[test]
    fn test_enrich_one_detects_complevel_from_file() {
        let conn = setup();
        let dir = tempfile::tempdir().unwrap();
        let wad_id = add_wad_with_file(&conn, dir.path(), "Test", &[("MAP01", &[])]);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut lookups = 0;
        let outcome = enrich_one(&conn, &wad, false, &mut lookups);

        assert_eq!(outcome.complevel, Some(4));
        assert_eq!(
            db::get_wad(&conn, wad_id, false)
                .unwrap()
                .unwrap()
                .complevel,
            Some(4)
        );
    }

    #[test]
    fn test_enrich_one_dry_run_writes_nothing() {
        let conn = setup();
        let dir = tempfile::tempdir().unwrap();
        let wad_id = add_wad_with_file(&conn, dir.path(), "Test", &[("E1M1", &[])]);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut lookups = 0;
        let outcome = enrich_one(&conn, &wad, true, &mut lookups);

        assert_eq!(outcome.complevel, Some(2));
        assert_eq!(
            db::get_wad(&conn, wad_id, false)
                .unwrap()
                .unwrap()
                .complevel,
            None,
            "dry run must not write"
        );
    }

    #[test]
    fn test_enrich_one_skips_when_everything_set() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Already Set");
        db::update_wad(
            &conn,
            wad_id,
            &WadUpdate::new()
                .set_int("complevel", Some(9))
                .set_text("custom_iwad", Some("doom2".to_string()))
                .set_int("zdoom_required", Some(0)),
        )
        .unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut lookups = 0;
        let outcome = enrich_one(&conn, &wad, false, &mut lookups);

        assert!(!outcome.has_changes());
        // Critically: no network call for a WAD with nothing to fill in.
        assert_eq!(lookups, 0);
    }

    #[test]
    fn test_enrich_one_no_cached_file_makes_no_lookup_without_title_gaps() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "");
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut lookups = 0;
        let outcome = enrich_one(&conn, &wad, false, &mut lookups);

        assert!(!outcome.has_changes());
        // Empty title — nothing to search the wiki for.
        assert_eq!(lookups, 0);
    }

    #[test]
    fn test_enrich_one_missing_cached_file_is_not_fatal() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "");
        db::update_wad(
            &conn,
            wad_id,
            &WadUpdate::new().set_text("cached_path", Some("/nope/gone.wad".to_string())),
        )
        .unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let mut lookups = 0;
        assert!(!enrich_one(&conn, &wad, false, &mut lookups).has_changes());
    }

    // -- enrich_wads tests --

    #[test]
    fn test_enrich_wads_reports_progress_and_changes() {
        let conn = setup();
        let dir = tempfile::tempdir().unwrap();
        add_wad_with_file(&conn, dir.path(), "First", &[("MAP01", &[])]);
        add_wad_with_file(&conn, dir.path(), "Second", &[("E1M1", &[])]);

        let wads = db::search_wads(&conn, None, None, true, false, 0).unwrap();
        assert_eq!(wads.len(), 2);

        let mut started = Vec::new();
        let mut finished = 0;
        let summary = enrich_wads(
            &conn,
            &wads,
            false,
            &mut |p| match p {
                EnrichProgress::Started { index, total, .. } => started.push((index, total)),
                EnrichProgress::Finished(_) => finished += 1,
            },
            &never_cancel(),
        );

        assert_eq!(started, vec![(0, 2), (1, 2)]);
        assert_eq!(finished, 2);
        assert_eq!(summary.examined, 2);
        assert_eq!(summary.changed.len(), 2);
        assert!(!summary.cancelled);
    }

    #[test]
    fn test_enrich_wads_honours_cancel() {
        let conn = setup();
        let dir = tempfile::tempdir().unwrap();
        add_wad_with_file(&conn, dir.path(), "First", &[("MAP01", &[])]);
        add_wad_with_file(&conn, dir.path(), "Second", &[("MAP01", &[])]);
        add_wad_with_file(&conn, dir.path(), "Third", &[("MAP01", &[])]);

        let wads = db::search_wads(&conn, None, None, true, false, 0).unwrap();
        let seen = std::cell::Cell::new(0usize);
        let summary = enrich_wads(
            &conn,
            &wads,
            false,
            &mut |p| {
                if matches!(p, EnrichProgress::Finished(_)) {
                    seen.set(seen.get() + 1);
                }
            },
            // Stop once one WAD is done.
            &|| seen.get() >= 1,
        );

        assert!(summary.cancelled);
        assert_eq!(summary.examined, 1);
    }

    #[test]
    fn test_enrich_wads_empty_input() {
        let conn = setup();
        let summary = enrich_wads(&conn, &[], false, &mut |_| {}, &never_cancel());
        assert_eq!(summary.examined, 0);
        assert!(!summary.cancelled);
        assert!(summary.changed.is_empty());
    }

    // -- cacoward ingest tests --

    #[test]
    fn test_ingest_upserts_and_links_by_idgames() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Piña Colada");
        db::update_wad(
            &conn,
            wad_id,
            &WadUpdate::new().set_text("idgames_id", Some("20917".to_string())),
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

        let summary = ingest_cacoward_entries(&conn, 2023, &entries, false).unwrap();
        assert_eq!(summary.upserted, 2);
        assert_eq!(summary.linked_by_idgames, 1);
        assert_eq!(summary.linked_by_title, 0);
        assert_eq!(summary.linked_total(), 1);

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
    fn test_ingest_dry_run_previews_without_writing() {
        let conn = setup();
        let entries = vec![db::NewCacoward {
            year: 2023,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Dreamblood".to_string(),
            ..Default::default()
        }];

        let summary = ingest_cacoward_entries(&conn, 2023, &entries, true).unwrap();
        assert_eq!(summary.upserted, 0);
        assert_eq!(summary.previews.len(), 1);
        assert_eq!(summary.previews[0].wad_title, "Dreamblood");
        assert!(db::get_cacowards_by_year(&conn, 2023).unwrap().is_empty());
    }

    #[test]
    fn test_ingest_title_fallback_links_unique_match() {
        let conn = setup();
        add_test_wad(&conn, "Dreamblood");

        let entries = vec![db::NewCacoward {
            year: 2023,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "dreamblood".to_string(),
            ..Default::default()
        }];

        let summary = ingest_cacoward_entries(&conn, 2023, &entries, false).unwrap();
        assert_eq!(summary.linked_by_title, 1);
    }

    #[test]
    fn test_ingest_title_fallback_skips_ambiguous_match() {
        let conn = setup();
        add_test_wad(&conn, "Crusader");
        add_test_wad(&conn, "crusader");

        let entries = vec![db::NewCacoward {
            year: 2023,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Crusader".to_string(),
            ..Default::default()
        }];

        let summary = ingest_cacoward_entries(&conn, 2023, &entries, false).unwrap();
        assert_eq!(summary.linked_by_title, 0, "two candidates must not link");
    }

    #[test]
    fn test_ingest_idgames_match_wins_over_title_match() {
        let conn = setup();
        // One WAD has the entry's title, another its idgames id.
        add_test_wad(&conn, "Ray Mohawk");
        let with_id = add_test_wad(&conn, "A Different Title");
        db::update_wad(
            &conn,
            with_id,
            &WadUpdate::new().set_text("idgames_id", Some("12345".to_string())),
        )
        .unwrap();

        let entries = vec![db::NewCacoward {
            year: 2023,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Ray Mohawk".to_string(),
            idgames_url: Some("https://www.doomworld.com/idgames/?id=12345".to_string()),
            ..Default::default()
        }];

        let summary = ingest_cacoward_entries(&conn, 2023, &entries, false).unwrap();
        assert_eq!(summary.linked_by_idgames, 1);
        assert_eq!(summary.linked_by_title, 0);

        let linked = db::get_cacowards_by_year(&conn, 2023).unwrap();
        assert_eq!(linked[0].wad_id, Some(with_id));
    }

    #[test]
    fn test_idgames_import_then_ingest_links_on_the_idgames_pass() {
        // The titles differ on purpose: only the id can connect these two.
        let conn = setup();
        let entry: crate::idgames::FileEntry = serde_json::from_value(serde_json::json!({
            "id": 21233,
            "title": "Liminal Doom 2",
            "filename": "ld2.zip",
        }))
        .unwrap();
        let wad_id = crate::import_service::ImportService::new()
            .import_idgames(&conn, &entry, None, false)
            .wad_id
            .unwrap();

        let entries = vec![db::NewCacoward {
            year: 2024,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Liminal Doom 2: Crossing the Threshold".to_string(),
            idgames_url: Some("https://www.doomworld.com/idgames/?id=21233".to_string()),
            ..Default::default()
        }];

        let summary = ingest_cacoward_entries(&conn, 2024, &entries, false).unwrap();
        assert_eq!(summary.linked_by_idgames, 1);
        assert_eq!(summary.linked_by_title, 0);

        let rows = db::get_cacowards_by_year(&conn, 2024).unwrap();
        assert_eq!(rows[0].wad_id, Some(wad_id));
    }

    #[test]
    fn test_reingest_does_not_overwrite_a_pinned_link() {
        let conn = setup();
        let pinned_to = add_test_wad(&conn, "Hand Picked");
        add_test_wad(&conn, "Eisberg");

        let entries = vec![db::NewCacoward {
            year: 2025,
            category: db::CATEGORY_HONORABLE_MENTION.to_string(),
            wad_title: "Eisberg".to_string(),
            ..Default::default()
        }];
        let row = db::upsert_cacoward(&conn, &entries[0]).unwrap();
        db::link_wad(&conn, row, pinned_to, true).unwrap();

        let summary = ingest_cacoward_entries(&conn, 2025, &entries, false).unwrap();
        assert_eq!(summary.linked_total(), 0);

        let record = db::get_cacoward(&conn, row).unwrap().unwrap();
        assert_eq!(record.wad_id, Some(pinned_to));
        assert!(record.manual_override);
    }

    #[test]
    fn test_ingest_does_not_touch_other_years() {
        // Unlink has to stick: scraping 2025 is no reason to reconsider a 2024
        // row the user cleared.
        let conn = setup();
        add_test_wad(&conn, "Eisberg");
        let row_in = |year| db::NewCacoward {
            year,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: "Eisberg".to_string(),
            ..Default::default()
        };
        let other_year = db::upsert_cacoward(&conn, &row_in(2024)).unwrap();

        let summary = ingest_cacoward_entries(&conn, 2025, &[row_in(2025)], false).unwrap();

        assert_eq!(summary.linked_by_title, 1);
        let record = db::get_cacoward(&conn, other_year).unwrap().unwrap();
        assert_eq!(record.wad_id, None);
    }

    #[test]
    fn test_failed_ingest_keeps_the_rows_it_started_with() {
        let conn = setup();
        let wad = add_test_wad(&conn, "Survivor");
        let row = |title: &str| db::NewCacoward {
            year: 2025,
            category: db::CATEGORY_WINNER.to_string(),
            wad_title: title.to_string(),
            ..Default::default()
        };

        ingest_cacoward_entries(&conn, 2025, &[row("Survivor")], false).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER refuse_row BEFORE INSERT ON cacowards
             WHEN NEW.wad_title = 'Refused'
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();

        let entries = [row("New Entry"), row("Refused")];
        assert!(ingest_cacoward_entries(&conn, 2025, &entries, false).is_err());

        let rows = db::get_cacowards_by_year(&conn, 2025).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].wad_title, "Survivor");
        assert_eq!(rows[0].wad_id, Some(wad));
    }
}
