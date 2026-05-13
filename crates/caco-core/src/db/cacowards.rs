//! Cacowards: annual Doomworld "best WAD" awards.
//!
//! The `cacowards` table is independent of `wads` — entries exist whether or
//! not the user owns the WAD, so the completion-rate grid can compute
//! "x of N runners-up beaten." Linking to a `wads` row is best-effort: the
//! enrichment pipeline tries to match by idgames URL, and the user can pin or
//! correct a link via `manual_override`.

use rusqlite::{Connection, OptionalExtension};

use crate::Result;

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

/// All Cacoward entries currently linked to a given WAD.
pub fn get_cacowards_for_wad(conn: &Connection, wad_id: i64) -> Result<Vec<CacowardRecord>> {
    let mut stmt =
        conn.prepare("SELECT * FROM cacowards WHERE wad_id = ? ORDER BY year DESC, category")?;
    Ok(stmt
        .query_map([wad_id], CacowardRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get_all_cacowards(conn: &Connection) -> Result<Vec<CacowardRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM cacowards ORDER BY year DESC, category, rank")?;
    Ok(stmt
        .query_map([], CacowardRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Distinct years with at least one Cacoward entry, newest first.
pub fn get_years(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT DISTINCT year FROM cacowards ORDER BY year DESC")?;
    Ok(stmt
        .query_map([], |row| row.get(0))?
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
/// Returns the first match, or None. Used by the enrichment auto-linker.
pub fn find_wad_by_idgames_id(conn: &Connection, idgames_id: &str) -> Result<Option<i64>> {
    let mut stmt =
        conn.prepare("SELECT id FROM wads WHERE idgames_id = ?1 AND deleted_at IS NULL LIMIT 1")?;
    Ok(stmt.query_row([idgames_id], |row| row.get(0)).optional()?)
}

/// Delete a single Cacoward entry. Mostly useful for tests and manual cleanup
/// of entries that turn out to be wiki noise.
pub fn delete_cacoward(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM cacowards WHERE id = ?", [id])?;
    Ok(())
}

/// Delete every non-pinned Cacoward entry for `year`. Used by the enricher
/// before a fresh scrape so the scrape result is the canonical state for the
/// year — without this, a stale row from an older buggy scrape would linger.
///
/// Rows with `manual_override = 1` are preserved on the assumption the user
/// curated them by hand and an upstream wiki edit shouldn't blow that away.
/// Returns the number of rows removed.
pub fn clear_year_unpinned(conn: &Connection, year: i64) -> Result<usize> {
    let count = conn.execute(
        "DELETE FROM cacowards WHERE year = ?1 AND manual_override = 0",
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

        let linked = get_cacowards_for_wad(&conn, 42).unwrap();
        assert_eq!(linked.len(), 1);
        assert!(!linked[0].manual_override);

        unlink_wad(&conn, id).unwrap();
        let record = get_cacoward(&conn, id).unwrap().unwrap();
        assert!(record.wad_id.is_none());
        assert!(!record.manual_override);
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
    fn get_years_returns_distinct_descending() {
        let conn = setup();
        upsert_cacoward(&conn, &sample(2021, CATEGORY_WINNER, "A")).unwrap();
        upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "B")).unwrap();
        upsert_cacoward(&conn, &sample(2022, CATEGORY_WINNER, "C")).unwrap();
        upsert_cacoward(&conn, &sample(2023, CATEGORY_RUNNER_UP, "D")).unwrap();

        assert_eq!(get_years(&conn).unwrap(), vec![2023, 2022, 2021]);
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
    fn delete_cacoward_removes_row() {
        let conn = setup();
        let id = upsert_cacoward(&conn, &sample(2020, CATEGORY_WINNER, "Z")).unwrap();
        delete_cacoward(&conn, id).unwrap();
        assert!(get_cacoward(&conn, id).unwrap().is_none());
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
