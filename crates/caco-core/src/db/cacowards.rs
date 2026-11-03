//! Cacowards: annual Doomworld "best WAD" awards.
//!
//! The `cacowards` table is independent of `wads` — entries exist whether or
//! not the user owns the WAD, so the completion-rate grid can compute
//! "x of N runners-up beaten." Linking to a `wads` row is best-effort: caco-sources'
//! `cacoward_link` matches rows to WADs when a year is scraped and when a WAD is
//! imported, and the user can pin or correct a link via `manual_override`.

use rusqlite::{Connection, OptionalExtension};

use crate::Result;
use crate::db::models::Status;

// =============================================================================
// Category constants
// =============================================================================

/// Core Cacoward categories. Stored as TEXT in the DB so future categories
/// (Mockaward, Multiplayer, Espi Memorial, etc.) can be added without a
/// schema migration.
pub const CATEGORY_WINNER: &str = "winner";
pub const CATEGORY_RUNNER_UP: &str = "runner-up";
pub const CATEGORY_HONORABLE_MENTION: &str = "honorable-mention";
pub const CATEGORY_MORDETH: &str = "mordeth";

/// The core four categories the grid is built around. Other category strings
/// are accepted by the DB but won't appear in the headline UI.
pub const CORE_CATEGORIES: &[&str] = &[
    CATEGORY_WINNER,
    CATEGORY_RUNNER_UP,
    CATEGORY_HONORABLE_MENTION,
    CATEGORY_MORDETH,
];

/// Normalize a user-typed category string (`winner` / `r` / `runner-up` /
/// `hm` / `honorable` / `mordeth` / `m`) to a canonical category slug.
/// Returns `None` for unrecognised input — callers should treat that as a
/// parse error rather than passing the raw string through.
pub fn normalize_category(s: &str) -> Option<&'static str> {
    match s.trim().to_lowercase().as_str() {
        "w" | "winner" | "winners" => Some(CATEGORY_WINNER),
        "r" | "ru" | "runner" | "runners" | "runner-up" | "runners-up" => Some(CATEGORY_RUNNER_UP),
        "hm" | "honorable" | "honorable-mention" | "honourable" | "honorable-mentions"
        | "honourable-mention" => Some(CATEGORY_HONORABLE_MENTION),
        "m" | "mordeth" => Some(CATEGORY_MORDETH),
        _ => None,
    }
}

// =============================================================================
// Records
// =============================================================================

#[derive(Debug, Clone)]
pub struct CacowardRecord {
    pub id: i64,
    pub year: i64,
    pub category: String,
    pub rank: Option<i64>,
    pub wad_title: String,
    pub wad_author: Option<String>,
    pub idgames_url: Option<String>,
    pub doomwiki_url: Option<String>,
    pub blurb: Option<String>,
    pub wad_id: Option<i64>,
    pub manual_override: bool,
    /// User-controlled flag for entries caco can't currently play (Doom 64,
    /// Hedon, etc.). Unsupported rows are still listed in the magazine view
    /// but excluded from per-year / per-category completion totals so the
    /// progress bars reflect playable entries only.
    pub supported: bool,
}

impl CacowardRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            year: row.get("year")?,
            category: row.get("category")?,
            rank: row.get("rank")?,
            wad_title: row.get("wad_title")?,
            wad_author: row.get("wad_author")?,
            idgames_url: row.get("idgames_url")?,
            doomwiki_url: row.get("doomwiki_url")?,
            blurb: row.get("blurb")?,
            wad_id: row.get("wad_id")?,
            manual_override: row.get::<_, i64>("manual_override")? != 0,
            supported: row.get::<_, i64>("supported")? != 0,
        })
    }
}

/// Input for upserting a scraped Cacoward entry. The enrichment pipeline
/// populates this from the Doom Wiki; downstream `upsert_cacoward` reconciles
/// it against any existing row keyed by `(year, category, wad_title)`.
#[derive(Debug, Clone, Default)]
pub struct NewCacoward {
    pub year: i64,
    pub category: String,
    pub rank: Option<i64>,
    pub wad_title: String,
    pub wad_author: Option<String>,
    pub idgames_url: Option<String>,
    pub doomwiki_url: Option<String>,
    pub blurb: Option<String>,
}

// =============================================================================
// CRUD
// =============================================================================

/// Insert or update a Cacoward entry, keyed by `(year, category, wad_title)`.
///
/// On conflict: re-scraped metadata (rank, author, URLs, blurb) overwrites
/// the existing row, but `wad_id` and `manual_override` are preserved so the
/// user's manual links survive re-enrichment. Returns the row's id.
pub fn upsert_cacoward(conn: &Connection, entry: &NewCacoward) -> Result<i64> {
    conn.execute(
        "INSERT INTO cacowards (year, category, rank, wad_title, wad_author, idgames_url, doomwiki_url, blurb, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, CURRENT_TIMESTAMP)
         ON CONFLICT(year, category, wad_title) DO UPDATE SET
             rank         = excluded.rank,
             wad_author   = excluded.wad_author,
             idgames_url  = excluded.idgames_url,
             doomwiki_url = excluded.doomwiki_url,
             blurb        = excluded.blurb,
             updated_at   = CURRENT_TIMESTAMP",
        rusqlite::params![
            entry.year,
            entry.category,
            entry.rank,
            entry.wad_title,
            entry.wad_author,
            entry.idgames_url,
            entry.doomwiki_url,
            entry.blurb,
        ],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM cacowards WHERE year = ?1 AND category = ?2 AND wad_title = ?3",
        rusqlite::params![entry.year, entry.category, entry.wad_title],
        |row| row.get(0),
    )?;
    Ok(id)
}

pub fn get_cacoward(conn: &Connection, id: i64) -> Result<Option<CacowardRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM cacowards WHERE id = ?")?;
    Ok(stmt.query_row([id], CacowardRecord::from_row).optional()?)
}

/// All Cacoward entries for a given year, ordered by category (winner first)
/// then rank.
pub fn get_cacowards_by_year(conn: &Connection, year: i64) -> Result<Vec<CacowardRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM cacowards
         WHERE year = ?
         ORDER BY
             CASE category
                 WHEN 'winner' THEN 0
                 WHEN 'runner-up' THEN 1
                 WHEN 'honorable-mention' THEN 2
                 WHEN 'mordeth' THEN 3
                 ELSE 99
             END,
             COALESCE(rank, 9999),
             wad_title",
    )?;
    Ok(stmt
        .query_map([year], CacowardRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Rows auto-linking may still claim: not pinned, and with no live WAD behind
/// them. A link to a trashed WAD counts as no link, the same way
/// [`search_cacowards`] reports such a row as [`EffectiveStatus::Absent`].
///
/// A pin outlives its WAD being trashed, since the WAD can come back, but not
/// being purged: the foreign key nulls `wad_id` and leaves a pin on nothing,
/// which would otherwise block the row for good.
pub fn get_linkable_cacowards(conn: &Connection) -> Result<Vec<CacowardRecord>> {
    let mut stmt = conn.prepare(
        "SELECT c.* FROM cacowards c
         LEFT JOIN wads w ON w.id = c.wad_id AND w.deleted_at IS NULL
         WHERE w.id IS NULL AND (c.manual_override = 0 OR c.wad_id IS NULL)
         ORDER BY c.id",
    )?;
    Ok(stmt
        .query_map([], CacowardRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Link a Cacoward entry to a WAD. If `manual` is true, the link is pinned —
/// subsequent auto-linking passes will not overwrite it.
pub fn link_wad(conn: &Connection, cacoward_id: i64, wad_id: i64, manual: bool) -> Result<()> {
    conn.execute(
        "UPDATE cacowards SET wad_id = ?1, manual_override = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
        rusqlite::params![wad_id, manual as i64, cacoward_id],
    )?;
    Ok(())
}

/// Clear a Cacoward entry's WAD link and its manual-override flag, so future
/// auto-linking can re-evaluate it.
pub fn unlink_wad(conn: &Connection, cacoward_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE cacowards SET wad_id = NULL, manual_override = 0, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        [cacoward_id],
    )?;
    Ok(())
}

/// Find a WAD whose `idgames_id` matches the given numeric idgames id string.
/// Returns the first match, or None. Used by the Cacoward auto-linker.
pub fn find_wad_by_idgames_id(conn: &Connection, idgames_id: &str) -> Result<Option<i64>> {
    let mut stmt =
        conn.prepare("SELECT id FROM wads WHERE idgames_id = ?1 AND deleted_at IS NULL LIMIT 1")?;
    Ok(stmt.query_row([idgames_id], |row| row.get(0)).optional()?)
}

/// Flip the `supported` flag on a Cacoward entry. Unsupported entries are
/// preserved across re-scrapes (clear_year_unpinned respects them) so the
/// user's marking sticks.
pub fn set_supported(conn: &Connection, cacoward_id: i64, supported: bool) -> Result<()> {
    conn.execute(
        "UPDATE cacowards SET supported = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        rusqlite::params![supported as i64, cacoward_id],
    )?;
    Ok(())
}

// =============================================================================
// Effective status and search
// =============================================================================

/// Status of a Cacoward entry from the user's perspective — `Absent` extends
/// the standard `Status` enum to cover entries with no linked library WAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveStatus {
    /// No linked library WAD — the user hasn't imported this entry yet.
    Absent,
    /// Has a linked WAD; carries that WAD's status.
    Library(Status),
}

/// Every Cacoward entry, joined to its linked WAD's status (or `Absent` if
/// unlinked).
///
/// Results are ordered newest year first, then by canonical category order
/// (winner → runner-up → honorable-mention → mordeth), then by rank within
/// the category.
pub fn search_cacowards(conn: &Connection) -> Result<Vec<(CacowardRecord, EffectiveStatus)>> {
    let mut stmt = conn.prepare(
        "SELECT c.*, w.status AS wad_status
         FROM cacowards c
         LEFT JOIN wads w ON w.id = c.wad_id AND w.deleted_at IS NULL
         ORDER BY
             c.year DESC,
             CASE c.category
                 WHEN 'winner' THEN 0
                 WHEN 'runner-up' THEN 1
                 WHEN 'honorable-mention' THEN 2
                 WHEN 'mordeth' THEN 3
                 ELSE 99
             END,
             COALESCE(c.rank, 9999),
             c.wad_title",
    )?;
    let rows = stmt.query_map([], |row| {
        let record = CacowardRecord::from_row(row)?;
        let status: Option<String> = row.get("wad_status")?;
        let effective = match status.as_deref() {
            None => EffectiveStatus::Absent,
            Some(s) => match s.parse::<Status>() {
                Ok(parsed) => EffectiveStatus::Library(parsed),
                // A wad row exists but its status is unparseable — treat as
                // "Library(Unplayed)" rather than Absent so the link is
                // still represented in the UI.
                Err(_) => EffectiveStatus::Library(Status::Unplayed),
            },
        };
        Ok((record, effective))
    })?;

    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Delete every non-pinned Cacoward entry for `year`. Used by the enricher
/// before a fresh scrape so the scrape result is the canonical state for the
/// year — without this, a stale row from an older buggy scrape would linger.
///
/// Rows with `manual_override = 1` are preserved on the assumption the user
/// curated them by hand and an upstream wiki edit shouldn't blow that away.
/// Returns the number of rows removed.
pub fn clear_year_unpinned(conn: &Connection, year: i64) -> Result<usize> {
    // Preserve both manually-pinned links AND entries the user has marked
    // unsupported — both represent user-curated state that the next scrape
    // would otherwise wipe.
    let count = conn.execute(
        "DELETE FROM cacowards
         WHERE year = ?1 AND manual_override = 0 AND supported = 1",
        rusqlite::params![year],
    )?;
    Ok(count)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::open_memory;
    use crate::db::schema::init_db;

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn sample(year: i64, category: &str, title: &str) -> NewCacoward {
        NewCacoward {
            year,
            category: category.to_string(),
            wad_title: title.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn upsert_inserts_new_entry() {
        let conn = setup();
        let id = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Going Down")).unwrap();
        let record = get_cacoward(&conn, id).unwrap().unwrap();
        assert_eq!(record.year, 2023);
        assert_eq!(record.category, "winner");
        assert_eq!(record.wad_title, "Going Down");
        assert!(!record.manual_override);
        assert!(record.wad_id.is_none());
    }

    #[test]
    fn upsert_updates_metadata_but_preserves_links() {
        let conn = setup();
        // Seed a fake WAD so the foreign key has a real target
        conn.execute(
            "INSERT INTO wads (id, title, source_type) VALUES (1, 'fake', 'manual')",
            [],
        )
        .unwrap();

        let id = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Going Down")).unwrap();
        link_wad(&conn, id, 1, true).unwrap();

        // Re-scrape with new metadata
        let updated = NewCacoward {
            year: 2023,
            category: CATEGORY_WINNER.to_string(),
            wad_title: "Going Down".to_string(),
            blurb: Some("A 32-map megawad by mouldy.".to_string()),
            wad_author: Some("Cyriak Harris".to_string()),
            ..Default::default()
        };
        let id2 = upsert_cacoward(&conn, &updated).unwrap();
        assert_eq!(id, id2);

        let record = get_cacoward(&conn, id).unwrap().unwrap();
        assert_eq!(record.blurb.as_deref(), Some("A 32-map megawad by mouldy."));
        assert_eq!(record.wad_author.as_deref(), Some("Cyriak Harris"));
        // Manual link survives re-scrape
        assert_eq!(record.wad_id, Some(1));
        assert!(record.manual_override);
    }

    #[test]
    fn get_by_year_orders_categories_correctly() {
        let conn = setup();
        upsert_cacoward(&conn, &sample(2023, CATEGORY_RUNNER_UP, "B")).unwrap();
        upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "A")).unwrap();
        upsert_cacoward(&conn, &sample(2023, CATEGORY_HONORABLE_MENTION, "C")).unwrap();
        upsert_cacoward(&conn, &sample(2023, CATEGORY_MORDETH, "D")).unwrap();

        let records = get_cacowards_by_year(&conn, 2023).unwrap();
        let categories: Vec<&str> = records.iter().map(|r| r.category.as_str()).collect();
        assert_eq!(
            categories,
            vec!["winner", "runner-up", "honorable-mention", "mordeth"]
        );
    }

    #[test]
    fn link_and_unlink_wad() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (id, title, source_type) VALUES (42, 'fake', 'manual')",
            [],
        )
        .unwrap();

        let id = upsert_cacoward(&conn, &sample(2022, CATEGORY_WINNER, "Eviternity II")).unwrap();
        link_wad(&conn, id, 42, false).unwrap();

        let linked = get_cacoward(&conn, id).unwrap().unwrap();
        assert_eq!(linked.wad_id, Some(42));
        assert!(!linked.manual_override);

        unlink_wad(&conn, id).unwrap();
        let record = get_cacoward(&conn, id).unwrap().unwrap();
        assert!(record.wad_id.is_none());
        assert!(!record.manual_override);
    }

    #[test]
    fn linkable_rows_are_the_unpinned_ones_with_no_live_wad() {
        let conn = setup();
        conn.execute_batch(
            "INSERT INTO wads (id, title, source_type) VALUES (1, 'live', 'manual');
             INSERT INTO wads (id, title, source_type, deleted_at)
                 VALUES (2, 'trashed', 'manual', CURRENT_TIMESTAMP);
             INSERT INTO wads (id, title, source_type) VALUES (3, 'purged', 'manual');",
        )
        .unwrap();

        let unlinked = upsert_cacoward(&conn, &sample(2025, CATEGORY_WINNER, "Unlinked")).unwrap();
        let on_trash = upsert_cacoward(&conn, &sample(2025, CATEGORY_WINNER, "OnTrash")).unwrap();
        link_wad(&conn, on_trash, 2, false).unwrap();

        let on_live = upsert_cacoward(&conn, &sample(2025, CATEGORY_WINNER, "OnLive")).unwrap();
        link_wad(&conn, on_live, 1, false).unwrap();
        let pinned_live =
            upsert_cacoward(&conn, &sample(2025, CATEGORY_WINNER, "PinnedLive")).unwrap();
        link_wad(&conn, pinned_live, 1, true).unwrap();
        let pinned_trash =
            upsert_cacoward(&conn, &sample(2025, CATEGORY_WINNER, "PinnedTrash")).unwrap();
        link_wad(&conn, pinned_trash, 2, true).unwrap();
        let pinned_purged =
            upsert_cacoward(&conn, &sample(2025, CATEGORY_WINNER, "PinnedPurged")).unwrap();
        link_wad(&conn, pinned_purged, 3, true).unwrap();
        conn.execute("DELETE FROM wads WHERE id = 3", []).unwrap();

        let ids: Vec<i64> = get_linkable_cacowards(&conn)
            .unwrap()
            .iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(ids, vec![unlinked, on_trash, pinned_purged]);
    }

    #[test]
    fn wad_id_cleared_when_wad_deleted() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (id, title, source_type) VALUES (7, 'fake', 'manual')",
            [],
        )
        .unwrap();
        let id = upsert_cacoward(&conn, &sample(2021, CATEGORY_WINNER, "X")).unwrap();
        link_wad(&conn, id, 7, false).unwrap();

        conn.execute("DELETE FROM wads WHERE id = 7", []).unwrap();
        let record = get_cacoward(&conn, id).unwrap().unwrap();
        assert!(record.wad_id.is_none());
    }

    #[test]
    fn find_wad_by_idgames_id_matches() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (id, title, source_type, idgames_id) VALUES (5, 'foo', 'idgames', '18184')",
            [],
        )
        .unwrap();

        assert_eq!(find_wad_by_idgames_id(&conn, "18184").unwrap(), Some(5));
        assert_eq!(find_wad_by_idgames_id(&conn, "99999").unwrap(), None);
    }

    #[test]
    fn find_wad_by_idgames_id_skips_deleted() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (id, title, source_type, idgames_id, deleted_at) VALUES (5, 'foo', 'idgames', '18184', CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        assert_eq!(find_wad_by_idgames_id(&conn, "18184").unwrap(), None);
    }

    #[test]
    fn search_cacowards_joins_status_from_linked_wad() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (id, title, source_type, status) VALUES (1, 'Foo', 'manual', 'completed')",
            [],
        )
        .unwrap();
        let id = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Foo")).unwrap();
        link_wad(&conn, id, 1, false).unwrap();

        let results = search_cacowards(&conn).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, EffectiveStatus::Library(Status::Completed));
    }

    #[test]
    fn set_supported_toggles_flag() {
        let conn = setup();
        let id = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "X")).unwrap();
        let initial = get_cacoward(&conn, id).unwrap().unwrap();
        assert!(initial.supported); // default is true

        set_supported(&conn, id, false).unwrap();
        let unsupported = get_cacoward(&conn, id).unwrap().unwrap();
        assert!(!unsupported.supported);

        set_supported(&conn, id, true).unwrap();
        let supported = get_cacoward(&conn, id).unwrap().unwrap();
        assert!(supported.supported);
    }

    #[test]
    fn clear_year_unpinned_preserves_unsupported_entries() {
        let conn = setup();
        let keep = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "KeepMe")).unwrap();
        set_supported(&conn, keep, false).unwrap();
        let _drop = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "DropMe")).unwrap();

        let removed = clear_year_unpinned(&conn, 2023).unwrap();
        assert_eq!(removed, 1);

        let remaining = get_cacowards_by_year(&conn, 2023).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].wad_title, "KeepMe");
        assert!(!remaining[0].supported);
    }

    #[test]
    fn clear_year_unpinned_keeps_manual_links() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (id, title, source_type) VALUES (1, 'fake', 'manual')",
            [],
        )
        .unwrap();

        let pinned = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Pinned")).unwrap();
        link_wad(&conn, pinned, 1, true).unwrap();
        let _orphan = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Orphan")).unwrap();
        let _other_year = upsert_cacoward(&conn, &sample(2022, CATEGORY_WINNER, "Other")).unwrap();

        let removed = clear_year_unpinned(&conn, 2023).unwrap();
        assert_eq!(removed, 1);

        // Pinned 2023 entry survives; 2022 untouched.
        assert!(get_cacoward(&conn, pinned).unwrap().is_some());
        let remaining_2023 = get_cacowards_by_year(&conn, 2023).unwrap();
        assert_eq!(remaining_2023.len(), 1);
        assert_eq!(remaining_2023[0].wad_title, "Pinned");
        assert_eq!(get_cacowards_by_year(&conn, 2022).unwrap().len(), 1);
    }
}
