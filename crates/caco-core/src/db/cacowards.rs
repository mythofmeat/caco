//! Cacowards: annual Doomworld "best WAD" awards.
//!
//! The `cacowards` table is independent of `wads` — entries exist whether or
//! not the user owns the WAD, so the completion-rate grid can compute
//! "x of N runners-up beaten." Its facts come from the curated dataset
//! ([`crate::cacowards`]), written here by caco-sources' `cacoward_sync` and
//! keyed on the entry's `key`, so a corrected title updates a row instead of
//! leaving a duplicate. What belongs to the user survives every update: the link
//! to a `wads` row, its `manual_override` pin, and `supported_override`.

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

use crate::Result;
use crate::db::models::Status;

// =============================================================================
// Category constants
// =============================================================================

pub const CATEGORY_WINNER: &str = "winner";
pub const CATEGORY_RUNNER_UP: &str = "runner-up";
pub const CATEGORY_HONORABLE_MENTION: &str = "honorable-mention";
pub const CATEGORY_MORDETH: &str = "mordeth";

/// What a year's completion is counted over, and the order they lead the view in.
///
/// `category` is free text, because the awards change every few years (Doom MOMA,
/// the Espi, Spaceship of Theseus…). Every other category is shown under these and
/// counts for nothing: a year is not unfinished because its multiplayer maps, which
/// have no ending to reach, were never "beaten".
pub const CORE_CATEGORIES: &[&str] = &[
    CATEGORY_WINNER,
    CATEGORY_RUNNER_UP,
    CATEGORY_HONORABLE_MENTION,
    CATEGORY_MORDETH,
];

/// Whether `category` counts towards a year's completion.
pub fn is_core_category(category: &str) -> bool {
    CORE_CATEGORIES.contains(&category)
}

/// What a query's `cacoward:` value means as a category: the core four answer to
/// shortcuts (`w`, `r`, `hm`, `m`) and spelling variants, and anything else is
/// taken as a category's own name, since those are free text.
pub fn normalize_category(s: &str) -> String {
    let lower = s.trim().to_lowercase();
    match lower.as_str() {
        "w" | "winner" | "winners" => CATEGORY_WINNER.to_string(),
        "r" | "ru" | "runner" | "runners" | "runner-up" | "runners-up" => {
            CATEGORY_RUNNER_UP.to_string()
        }
        "hm" | "honorable" | "honorable-mention" | "honourable" | "honorable-mentions"
        | "honourable-mention" => CATEGORY_HONORABLE_MENTION.to_string(),
        "m" | "mordeth" => CATEGORY_MORDETH.to_string(),
        _ => lower,
    }
}

// =============================================================================
// Records
// =============================================================================

#[derive(Debug, Clone)]
pub struct CacowardRecord {
    pub id: i64,
    pub key: String,
    pub year: i64,
    pub category: String,
    /// Where the entry falls on the year's page. Not a ranking.
    pub page_order: i64,
    pub wad_title: String,
    pub wad_author: Option<String>,
    pub idgames_id: Option<i64>,
    /// The entry's Doomworld forum topic.
    pub doomworld_id: Option<i64>,
    pub doomwiki_page: Option<String>,
    pub sha256: Option<String>,
    pub wad_id: Option<i64>,
    pub manual_override: bool,
    /// What the dataset says: `false` for an entry nothing caco launches can run
    /// (Doom 64, Hedon, etc.).
    pub supported_default: bool,
    /// What the user said, if they ever did. A separate column so that a data
    /// update, which rewrites the default, cannot undo their choice.
    pub supported_override: Option<bool>,
    /// The user's word over the dataset's. Unsupported rows are still listed in
    /// the magazine view but excluded from per-year / per-category completion
    /// totals, so the progress bars reflect playable entries only.
    pub supported: bool,
}

impl CacowardRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let supported_default = row.get::<_, i64>("supported_default")? != 0;
        let supported_override = row
            .get::<_, Option<i64>>("supported_override")?
            .map(|value| value != 0);
        Ok(Self {
            id: row.get("id")?,
            key: row.get("key")?,
            year: row.get("year")?,
            category: row.get("category")?,
            page_order: row.get("page_order")?,
            wad_title: row.get("wad_title")?,
            wad_author: row.get("wad_author")?,
            idgames_id: row.get("idgames_id")?,
            doomworld_id: row.get("doomworld_id")?,
            doomwiki_page: row.get("doomwiki_page")?,
            sha256: row.get("sha256")?,
            wad_id: row.get("wad_id")?,
            manual_override: row.get::<_, i64>("manual_override")? != 0,
            supported_default,
            supported_override,
            supported: supported_override.unwrap_or(supported_default),
        })
    }

    /// The entry's Doom Wiki article.
    pub fn doomwiki_url(&self) -> Option<String> {
        self.doomwiki_page
            .as_deref()
            .map(|page| format!("https://doomwiki.org/wiki/{page}"))
    }

    /// Whether this row already says everything `entry` does, so that an update
    /// would change nothing.
    pub fn matches(&self, entry: &NewCacoward) -> bool {
        self.year == entry.year
            && self.category == entry.category
            && self.page_order == entry.page_order
            && self.wad_author == entry.wad_author
            && self.supported_default == entry.supported
            && self.same_evidence(entry)
    }

    /// Whether this row and `entry` give auto-linking the same things to go on.
    pub fn same_evidence(&self, entry: &NewCacoward) -> bool {
        self.wad_title == entry.wad_title
            && self.idgames_id == entry.idgames_id
            && self.doomworld_id == entry.doomworld_id
            && self.doomwiki_page == entry.doomwiki_page
            && self.sha256 == entry.sha256
    }
}

/// One dataset entry, as `[[entry]]` in a year's file spells it and as it is
/// written to the table. `year` comes from the file around it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCacoward {
    /// Stable forever: rows key on it, so it survives any edit to the title.
    pub key: String,
    #[serde(skip)]
    pub year: i64,
    pub category: String,
    #[serde(rename = "order")]
    pub page_order: i64,
    #[serde(rename = "title")]
    pub wad_title: String,
    #[serde(default, rename = "author")]
    pub wad_author: Option<String>,
    #[serde(default, rename = "idgames")]
    pub idgames_id: Option<i64>,
    #[serde(default, rename = "doomworld")]
    pub doomworld_id: Option<i64>,
    #[serde(default, rename = "doomwiki")]
    pub doomwiki_page: Option<String>,
    /// The release's SHA-256. Matched against any hash `identity` recorded.
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default = "supported_by_default")]
    pub supported: bool,
}

fn supported_by_default() -> bool {
    true
}

impl Default for NewCacoward {
    fn default() -> Self {
        Self {
            key: String::new(),
            year: 0,
            category: String::new(),
            page_order: 0,
            wad_title: String::new(),
            wad_author: None,
            idgames_id: None,
            doomworld_id: None,
            doomwiki_page: None,
            sha256: None,
            supported: supported_by_default(),
        }
    }
}

// =============================================================================
// CRUD
// =============================================================================

/// Insert or update a Cacoward entry, keyed by `key`.
///
/// On conflict the entry's facts overwrite the row's, but `wad_id`,
/// `manual_override` and `supported_override` are left alone, because they are the
/// user's rather than the dataset's. Returns the row's id.
pub fn upsert_cacoward(conn: &Connection, entry: &NewCacoward) -> Result<i64> {
    let id = conn.query_row(
        "INSERT INTO cacowards (key, year, category, page_order, wad_title, wad_author,
                                idgames_id, doomworld_id, doomwiki_page, sha256,
                                supported_default, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET
             year              = excluded.year,
             category          = excluded.category,
             page_order        = excluded.page_order,
             wad_title         = excluded.wad_title,
             wad_author        = excluded.wad_author,
             idgames_id        = excluded.idgames_id,
             doomworld_id      = excluded.doomworld_id,
             doomwiki_page     = excluded.doomwiki_page,
             sha256            = excluded.sha256,
             supported_default = excluded.supported_default,
             updated_at        = CURRENT_TIMESTAMP
         RETURNING id",
        rusqlite::params![
            entry.key,
            entry.year,
            entry.category,
            entry.page_order,
            entry.wad_title,
            entry.wad_author,
            entry.idgames_id,
            entry.doomworld_id,
            entry.doomwiki_page,
            entry.sha256,
            entry.supported,
        ],
        |row| row.get(0),
    )?;
    Ok(id)
}

pub fn get_cacoward(conn: &Connection, id: i64) -> Result<Option<CacowardRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM cacowards WHERE id = ?")?;
    Ok(stmt.query_row([id], CacowardRecord::from_row).optional()?)
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

/// Record the user's word on whether a Cacoward entry is playable. It outranks
/// whatever the dataset says, now and after any update.
pub fn set_supported(conn: &Connection, cacoward_id: i64, supported: bool) -> Result<()> {
    conn.execute(
        "UPDATE cacowards SET supported_override = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        rusqlite::params![supported, cacoward_id],
    )?;
    Ok(())
}

/// Remove a row that no longer has an entry behind it. The caller decides which
/// those are: see `cacoward_sync`.
pub fn delete_cacoward(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM cacowards WHERE id = ?", [id])?;
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
/// Results are ordered newest year first, then the core categories in their
/// canonical order, then every other category by name, then page order within
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
             c.category,
             c.page_order,
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
            key: format!("{year}-{category}-{title}"),
            year,
            category: category.to_string(),
            page_order: 1,
            wad_title: title.to_string(),
            ..Default::default()
        }
    }

    fn record(conn: &Connection, id: i64) -> CacowardRecord {
        get_cacoward(conn, id).unwrap().unwrap()
    }

    #[test]
    fn upsert_inserts_new_entry() {
        let conn = setup();
        let id = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Going Down")).unwrap();
        let record = record(&conn, id);
        assert_eq!(record.key, "2023-winner-Going Down");
        assert_eq!(record.year, 2023);
        assert_eq!(record.category, "winner");
        assert_eq!(record.wad_title, "Going Down");
        assert!(!record.manual_override);
        assert!(record.wad_id.is_none());
    }

    #[test]
    fn upsert_is_keyed_on_the_key_so_a_new_title_updates_the_row() {
        let conn = setup();
        let id = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Going Down")).unwrap();

        let renamed = NewCacoward {
            wad_title: "Going Down: Director's Cut".to_string(),
            ..sample(2023, CATEGORY_WINNER, "Going Down")
        };
        assert_eq!(upsert_cacoward(&conn, &renamed).unwrap(), id);

        let rows = search_cacowards(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.wad_title, "Going Down: Director's Cut");
    }

    #[test]
    fn upsert_updates_the_facts_but_leaves_the_users_state() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (id, title, source_type) VALUES (1, 'fake', 'manual')",
            [],
        )
        .unwrap();

        let id = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Going Down")).unwrap();
        link_wad(&conn, id, 1, true).unwrap();
        set_supported(&conn, id, false).unwrap();

        let updated = NewCacoward {
            wad_author: Some("Cyriak Harris".to_string()),
            idgames_id: Some(17500),
            doomworld_id: Some(70000),
            doomwiki_page: Some("Going_Down".to_string()),
            sha256: Some("abc".to_string()),
            page_order: 4,
            ..sample(2023, CATEGORY_WINNER, "Going Down")
        };
        assert_eq!(upsert_cacoward(&conn, &updated).unwrap(), id);

        let record = record(&conn, id);
        assert_eq!(record.wad_author.as_deref(), Some("Cyriak Harris"));
        assert_eq!(record.idgames_id, Some(17500));
        assert_eq!(record.doomworld_id, Some(70000));
        assert_eq!(record.doomwiki_page.as_deref(), Some("Going_Down"));
        assert_eq!(record.sha256.as_deref(), Some("abc"));
        assert_eq!(record.page_order, 4);
        assert!(record.matches(&updated));
        // The link, its pin and the toggle all survive.
        assert_eq!(record.wad_id, Some(1));
        assert!(record.manual_override);
        assert!(!record.supported);
    }

    #[test]
    fn a_data_update_cannot_undo_the_users_supported_choice() {
        let conn = setup();
        let entry = sample(2019, CATEGORY_WINNER, "Hedon");
        let id = upsert_cacoward(&conn, &entry).unwrap();
        set_supported(&conn, id, true).unwrap();

        let curated = NewCacoward {
            supported: false,
            ..entry
        };
        upsert_cacoward(&conn, &curated).unwrap();

        let record = record(&conn, id);
        assert!(!record.supported_default);
        assert_eq!(record.supported_override, Some(true));
        assert!(record.supported);
    }

    #[test]
    fn with_no_word_from_the_user_the_data_decides_supported() {
        let conn = setup();
        let entry = sample(2019, CATEGORY_WINNER, "Hedon");
        let id = upsert_cacoward(&conn, &entry).unwrap();
        assert!(record(&conn, id).supported);

        let curated = NewCacoward {
            supported: false,
            ..entry
        };
        upsert_cacoward(&conn, &curated).unwrap();

        let record = record(&conn, id);
        assert_eq!(record.supported_override, None);
        assert!(!record.supported);
    }

    #[test]
    fn set_supported_toggles_the_effective_flag() {
        let conn = setup();
        let id = upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "X")).unwrap();
        assert!(record(&conn, id).supported); // default is true

        set_supported(&conn, id, false).unwrap();
        assert!(!record(&conn, id).supported);

        set_supported(&conn, id, true).unwrap();
        assert!(record(&conn, id).supported);
    }

    #[test]
    fn evidence_is_what_linking_reads_and_nothing_else() {
        let conn = setup();
        let entry = sample(2023, CATEGORY_WINNER, "X");
        let id = upsert_cacoward(&conn, &entry).unwrap();
        let row = record(&conn, id);
        assert!(row.matches(&entry));
        assert!(row.same_evidence(&entry));

        let reordered = NewCacoward {
            page_order: 9,
            ..entry.clone()
        };
        assert!(!row.matches(&reordered));
        assert!(row.same_evidence(&reordered));

        for changed in [
            NewCacoward {
                wad_title: "Y".to_string(),
                ..entry.clone()
            },
            NewCacoward {
                idgames_id: Some(1),
                ..entry.clone()
            },
            NewCacoward {
                doomworld_id: Some(1),
                ..entry.clone()
            },
            NewCacoward {
                doomwiki_page: Some("X".to_string()),
                ..entry.clone()
            },
            NewCacoward {
                sha256: Some("abc".to_string()),
                ..entry.clone()
            },
        ] {
            assert!(!row.same_evidence(&changed), "{changed:?}");
            assert!(!row.matches(&changed), "{changed:?}");
        }
    }

    #[test]
    fn search_orders_the_core_four_first_then_the_rest_by_name_then_by_page() {
        let conn = setup();
        for (category, title, page_order) in [
            ("multiplayer", "M", 1),
            ("doom-moma", "D2", 2),
            ("doom-moma", "D1", 1),
            (CATEGORY_MORDETH, "Mo", 1),
            (CATEGORY_RUNNER_UP, "B", 1),
            (CATEGORY_WINNER, "W2", 2),
            (CATEGORY_WINNER, "W1", 1),
            (CATEGORY_HONORABLE_MENTION, "C", 1),
        ] {
            let entry = NewCacoward {
                page_order,
                ..sample(2023, category, title)
            };
            upsert_cacoward(&conn, &entry).unwrap();
        }

        let titles: Vec<String> = search_cacowards(&conn)
            .unwrap()
            .into_iter()
            .map(|(row, _)| row.wad_title)
            .collect();
        assert_eq!(
            titles,
            ["W1", "W2", "B", "C", "Mo", "D1", "D2", "M"].map(str::to_string)
        );
    }

    #[test]
    fn search_puts_the_newest_year_first() {
        let conn = setup();
        for year in [2021, 2023, 2022] {
            upsert_cacoward(&conn, &sample(year, CATEGORY_WINNER, "A")).unwrap();
        }
        let years: Vec<i64> = search_cacowards(&conn)
            .unwrap()
            .into_iter()
            .map(|(row, _)| row.year)
            .collect();
        assert_eq!(years, [2023, 2022, 2021]);
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

        let linked = record(&conn, id);
        assert_eq!(linked.wad_id, Some(42));
        assert!(!linked.manual_override);

        unlink_wad(&conn, id).unwrap();
        let unlinked = record(&conn, id);
        assert!(unlinked.wad_id.is_none());
        assert!(!unlinked.manual_override);
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
        assert!(record(&conn, id).wad_id.is_none());
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
    fn an_unlinked_row_is_absent() {
        let conn = setup();
        upsert_cacoward(&conn, &sample(2023, CATEGORY_WINNER, "Z")).unwrap();
        assert_eq!(
            search_cacowards(&conn).unwrap()[0].1,
            EffectiveStatus::Absent
        );
    }

    #[test]
    fn the_doomwiki_url_comes_from_the_page() {
        let conn = setup();
        let entry = NewCacoward {
            doomwiki_page: Some("Crusader_(2023_WAD)".to_string()),
            ..sample(2023, CATEGORY_WINNER, "Crusader")
        };
        let id = upsert_cacoward(&conn, &entry).unwrap();
        assert_eq!(
            record(&conn, id).doomwiki_url().as_deref(),
            Some("https://doomwiki.org/wiki/Crusader_(2023_WAD)")
        );
    }

    #[test]
    fn category_shortcuts_expand_and_anything_else_passes_through() {
        for (input, expected) in [
            ("w", "winner"),
            ("Winners", "winner"),
            ("r", "runner-up"),
            ("hm", "honorable-mention"),
            ("m", "mordeth"),
            (" Doom-MoMA ", "doom-moma"),
            ("multiplayer", "multiplayer"),
        ] {
            assert_eq!(normalize_category(input), expected);
        }
    }

    #[test]
    fn only_the_core_four_count_towards_a_year() {
        assert!(is_core_category(CATEGORY_MORDETH));
        assert!(!is_core_category("multiplayer"));
    }
}
