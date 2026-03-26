use std::collections::HashMap;

use rusqlite::Connection;

use super::connection::SQLITE_MAX_VARS;
use crate::Result;

// =============================================================================
// Records
// =============================================================================

/// A companion file in the registry.
#[derive(Debug, Clone)]
pub struct CompanionRecord {
    pub id: i64,
    pub md5: String,
    pub filename: String,
    pub path: String,
    pub size: i64,
}

impl CompanionRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            md5: row.get("md5")?,
            filename: row.get("filename")?,
            path: row.get("path")?,
            size: row.get("size")?,
        })
    }
}

/// A companion file linked to a specific WAD.
#[derive(Debug, Clone)]
pub struct WadCompanionRecord {
    pub companion_id: i64,
    pub md5: String,
    pub filename: String,
    pub path: String,
    pub size: i64,
    pub enabled: bool,
    pub load_order: i64,
}

impl WadCompanionRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            companion_id: row.get("id")?,
            md5: row.get("md5")?,
            filename: row.get("filename")?,
            path: row.get("path")?,
            size: row.get("size")?,
            enabled: row.get::<_, i64>("enabled")? != 0,
            load_order: row.get("load_order")?,
        })
    }
}

// =============================================================================
// Companion registry CRUD
// =============================================================================

/// Add a companion file to the registry. Returns the new ID.
///
/// The `md5` column has a UNIQUE constraint — callers should use
/// `find_companion_by_md5()` first to check for duplicates.
pub fn add_companion(
    conn: &Connection,
    md5: &str,
    filename: &str,
    path: &str,
    size: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO companion_files_registry (md5, filename, path, size) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![md5, filename, path, size],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Remove a companion file from the registry by ID.
///
/// Also removes all junction-table links (ON DELETE CASCADE).
/// Returns `true` if a row was deleted.
pub fn remove_companion(conn: &Connection, companion_id: i64) -> Result<bool> {
    let count = conn.execute(
        "DELETE FROM companion_files_registry WHERE id = ?",
        [companion_id],
    )?;
    Ok(count > 0)
}

/// Find a companion file by MD5 hash.
pub fn find_companion_by_md5(conn: &Connection, md5: &str) -> Result<Option<CompanionRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM companion_files_registry WHERE md5 = ?")?;
    match stmt.query_row([md5], CompanionRecord::from_row) {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all companion files in the registry.
pub fn get_all_companions(conn: &Connection) -> Result<Vec<CompanionRecord>> {
    let mut stmt =
        conn.prepare("SELECT * FROM companion_files_registry ORDER BY filename")?;
    let rows = stmt
        .query_map([], CompanionRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// =============================================================================
// WAD ↔ companion linking
// =============================================================================

/// Link a companion file to a WAD. Returns `true` on success.
///
/// The load_order defaults to one past the current max for that WAD.
/// Uses INSERT OR IGNORE so re-linking the same pair is a no-op.
pub fn link_companion_to_wad(
    conn: &Connection,
    wad_id: i64,
    companion_id: i64,
) -> Result<bool> {
    let next_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(load_order), -1) + 1 FROM wad_companions WHERE wad_id = ?",
            [wad_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let count = conn.execute(
        "INSERT OR IGNORE INTO wad_companions (wad_id, companion_id, enabled, load_order) VALUES (?1, ?2, 1, ?3)",
        rusqlite::params![wad_id, companion_id, next_order],
    )?;
    Ok(count > 0)
}

/// Unlink a companion file from a WAD. Returns `true` if a link was removed.
pub fn unlink_companion_from_wad(
    conn: &Connection,
    wad_id: i64,
    companion_id: i64,
) -> Result<bool> {
    let count = conn.execute(
        "DELETE FROM wad_companions WHERE wad_id = ? AND companion_id = ?",
        rusqlite::params![wad_id, companion_id],
    )?;
    Ok(count > 0)
}

/// Set the enabled state of a companion for a WAD. Returns `true` if updated.
pub fn set_companion_enabled(
    conn: &Connection,
    wad_id: i64,
    companion_id: i64,
    enabled: bool,
) -> Result<bool> {
    let count = conn.execute(
        "UPDATE wad_companions SET enabled = ?1 WHERE wad_id = ?2 AND companion_id = ?3",
        rusqlite::params![enabled as i64, wad_id, companion_id],
    )?;
    Ok(count > 0)
}

/// Get all companion files linked to a WAD, ordered by load_order.
pub fn get_companions_for_wad(
    conn: &Connection,
    wad_id: i64,
) -> Result<Vec<WadCompanionRecord>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.md5, r.filename, r.path, r.size, wc.enabled, wc.load_order
         FROM wad_companions wc
         JOIN companion_files_registry r ON r.id = wc.companion_id
         WHERE wc.wad_id = ?
         ORDER BY wc.load_order",
    )?;
    let rows = stmt
        .query_map([wad_id], WadCompanionRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// =============================================================================
// Orphan detection
// =============================================================================

/// Check if a companion file has no WAD links.
pub fn is_orphan(conn: &Connection, companion_id: i64) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wad_companions WHERE companion_id = ?",
        [companion_id],
        |row| row.get(0),
    )?;
    Ok(count == 0)
}

/// Remove a companion file from the registry and return its managed path.
///
/// Returns `Some(path)` if found, `None` if the companion didn't exist.
/// Also removes all junction-table links (ON DELETE CASCADE).
pub fn remove_companion_with_path(
    conn: &Connection,
    companion_id: i64,
) -> Result<Option<String>> {
    let path: Option<String> = conn
        .query_row(
            "SELECT path FROM companion_files_registry WHERE id = ?",
            [companion_id],
            |row| row.get(0),
        )
        .ok();

    if path.is_some() {
        conn.execute(
            "DELETE FROM companion_files_registry WHERE id = ?",
            [companion_id],
        )?;
    }

    Ok(path)
}

/// Check if unlinking a companion from a specific WAD would leave it orphaned.
///
/// Returns `true` if this WAD is the only one linking to the companion.
pub fn would_be_orphan(conn: &Connection, companion_id: i64, wad_id: i64) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wad_companions WHERE companion_id = ? AND wad_id != ?",
        rusqlite::params![companion_id, wad_id],
        |row| row.get(0),
    )?;
    Ok(count == 0)
}

/// Get companion files that are not linked to any WAD.
pub fn get_orphaned_companions(conn: &Connection) -> Result<Vec<CompanionRecord>> {
    let mut stmt = conn.prepare(
        "SELECT r.* FROM companion_files_registry r
         LEFT JOIN wad_companions wc ON wc.companion_id = r.id
         WHERE wc.wad_id IS NULL
         ORDER BY r.filename",
    )?;
    let rows = stmt
        .query_map([], CompanionRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// =============================================================================
// Batch query
// =============================================================================

/// Batch-fetch companion files for multiple WADs.
///
/// Returns `{wad_id: [WadCompanionRecord]}` — avoids N+1 queries for list views.
pub fn get_companions_batch(
    conn: &Connection,
    wad_ids: &[i64],
) -> Result<HashMap<i64, Vec<WadCompanionRecord>>> {
    if wad_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result: HashMap<i64, Vec<WadCompanionRecord>> = HashMap::new();

    for chunk in wad_ids.chunks(SQLITE_MAX_VARS) {
        let placeholders = build_placeholders(chunk.len());
        let sql = format!(
            "SELECT wc.wad_id, r.id, r.md5, r.filename, r.path, r.size, wc.enabled, wc.load_order
             FROM wad_companions wc
             JOIN companion_files_registry r ON r.id = wc.companion_id
             WHERE wc.wad_id IN ({placeholders})
             ORDER BY wc.wad_id, wc.load_order"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            let wad_id: i64 = row.get("wad_id")?;
            let rec = WadCompanionRecord {
                companion_id: row.get("id")?,
                md5: row.get("md5")?,
                filename: row.get("filename")?,
                path: row.get("path")?,
                size: row.get("size")?,
                enabled: row.get::<_, i64>("enabled")? != 0,
                load_order: row.get("load_order")?,
            };
            Ok((wad_id, rec))
        })?;
        for row in rows {
            let (wad_id, rec) = row?;
            result.entry(wad_id).or_default().push(rec);
        }
    }

    Ok(result)
}

/// Build a comma-separated list of `?` placeholders.
fn build_placeholders(n: usize) -> String {
    let mut s = String::with_capacity(n * 2);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('?');
    }
    s
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::open_memory;
    use crate::db::models::SourceType;
    use crate::db::schema::init_db;
    use crate::db::wads::{add_wad, NewWad};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection) -> i64 {
        add_wad(conn, &NewWad::new("Test WAD", SourceType::Local)).unwrap()
    }

    #[test]
    fn test_add_and_find_companion() {
        let conn = setup();
        let id = add_companion(&conn, "abc123", "patch.deh", "/path/patch.deh", 1024).unwrap();
        assert!(id > 0);

        let found = find_companion_by_md5(&conn, "abc123").unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.md5, "abc123");
        assert_eq!(found.filename, "patch.deh");
        assert_eq!(found.path, "/path/patch.deh");
        assert_eq!(found.size, 1024);
    }

    #[test]
    fn test_find_companion_not_found() {
        let conn = setup();
        assert!(find_companion_by_md5(&conn, "nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_duplicate_md5_fails() {
        let conn = setup();
        add_companion(&conn, "abc123", "a.deh", "/a.deh", 100).unwrap();
        let result = add_companion(&conn, "abc123", "b.deh", "/b.deh", 200);
        assert!(result.is_err()); // UNIQUE constraint on md5
    }

    #[test]
    fn test_remove_companion() {
        let conn = setup();
        let id = add_companion(&conn, "abc123", "a.deh", "/a.deh", 100).unwrap();
        assert!(remove_companion(&conn, id).unwrap());
        assert!(find_companion_by_md5(&conn, "abc123").unwrap().is_none());

        // Remove nonexistent
        assert!(!remove_companion(&conn, id).unwrap());
    }

    #[test]
    fn test_get_all_companions() {
        let conn = setup();
        add_companion(&conn, "md5_b", "beta.deh", "/beta.deh", 200).unwrap();
        add_companion(&conn, "md5_a", "alpha.bex", "/alpha.bex", 100).unwrap();

        let all = get_all_companions(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].filename, "alpha.bex"); // Ordered by filename
        assert_eq!(all[1].filename, "beta.deh");
    }

    #[test]
    fn test_link_and_get_companions_for_wad() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c1 = add_companion(&conn, "md5_1", "a.deh", "/a.deh", 100).unwrap();
        let c2 = add_companion(&conn, "md5_2", "b.bex", "/b.bex", 200).unwrap();

        assert!(link_companion_to_wad(&conn, wad_id, c1).unwrap());
        assert!(link_companion_to_wad(&conn, wad_id, c2).unwrap());

        let companions = get_companions_for_wad(&conn, wad_id).unwrap();
        assert_eq!(companions.len(), 2);
        assert_eq!(companions[0].filename, "a.deh");
        assert_eq!(companions[0].load_order, 0);
        assert!(companions[0].enabled);
        assert_eq!(companions[1].filename, "b.bex");
        assert_eq!(companions[1].load_order, 1);
    }

    #[test]
    fn test_link_idempotent() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5_1", "a.deh", "/a.deh", 100).unwrap();

        assert!(link_companion_to_wad(&conn, wad_id, c_id).unwrap());
        // Re-linking is a no-op (INSERT OR IGNORE)
        assert!(!link_companion_to_wad(&conn, wad_id, c_id).unwrap());

        let companions = get_companions_for_wad(&conn, wad_id).unwrap();
        assert_eq!(companions.len(), 1);
    }

    #[test]
    fn test_unlink_companion() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5_1", "a.deh", "/a.deh", 100).unwrap();

        link_companion_to_wad(&conn, wad_id, c_id).unwrap();
        assert!(unlink_companion_from_wad(&conn, wad_id, c_id).unwrap());
        assert!(get_companions_for_wad(&conn, wad_id).unwrap().is_empty());

        // Unlink nonexistent
        assert!(!unlink_companion_from_wad(&conn, wad_id, c_id).unwrap());
    }

    #[test]
    fn test_set_companion_enabled() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5_1", "a.deh", "/a.deh", 100).unwrap();
        link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        // Default is enabled
        let comps = get_companions_for_wad(&conn, wad_id).unwrap();
        assert!(comps[0].enabled);

        // Disable
        assert!(set_companion_enabled(&conn, wad_id, c_id, false).unwrap());
        let comps = get_companions_for_wad(&conn, wad_id).unwrap();
        assert!(!comps[0].enabled);

        // Re-enable
        assert!(set_companion_enabled(&conn, wad_id, c_id, true).unwrap());
        let comps = get_companions_for_wad(&conn, wad_id).unwrap();
        assert!(comps[0].enabled);
    }

    #[test]
    fn test_orphaned_companions() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c1 = add_companion(&conn, "md5_1", "linked.deh", "/linked.deh", 100).unwrap();
        let _c2 = add_companion(&conn, "md5_2", "orphan.bex", "/orphan.bex", 200).unwrap();

        link_companion_to_wad(&conn, wad_id, c1).unwrap();

        let orphans = get_orphaned_companions(&conn).unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].filename, "orphan.bex");
    }

    #[test]
    fn test_cascade_delete_companion() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5_1", "a.deh", "/a.deh", 100).unwrap();
        link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        // Deleting the companion should cascade to wad_companions
        remove_companion(&conn, c_id).unwrap();
        assert!(get_companions_for_wad(&conn, wad_id).unwrap().is_empty());
    }

    #[test]
    fn test_cascade_delete_wad() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5_1", "a.deh", "/a.deh", 100).unwrap();
        link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        // Deleting the WAD should cascade to wad_companions
        conn.execute("DELETE FROM wads WHERE id = ?", [wad_id]).unwrap();

        // Companion should now be orphaned, not deleted
        let orphans = get_orphaned_companions(&conn).unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].id, c_id);
    }

    #[test]
    fn test_get_companions_batch() {
        let conn = setup();
        let w1 = add_test_wad(&conn);
        let w2 = add_wad(&conn, &NewWad::new("WAD 2", SourceType::Local)).unwrap();
        let w3 = add_wad(&conn, &NewWad::new("WAD 3", SourceType::Local)).unwrap();

        let c1 = add_companion(&conn, "md5_1", "a.deh", "/a.deh", 100).unwrap();
        let c2 = add_companion(&conn, "md5_2", "b.bex", "/b.bex", 200).unwrap();

        link_companion_to_wad(&conn, w1, c1).unwrap();
        link_companion_to_wad(&conn, w1, c2).unwrap();
        link_companion_to_wad(&conn, w2, c1).unwrap();
        // w3 has no companions

        let batch = get_companions_batch(&conn, &[w1, w2, w3]).unwrap();
        assert_eq!(batch.get(&w1).map(|v| v.len()), Some(2));
        assert_eq!(batch.get(&w2).map(|v| v.len()), Some(1));
        assert!(batch.get(&w3).is_none()); // Not in map
    }

    #[test]
    fn test_get_companions_batch_empty() {
        let conn = setup();
        let result = get_companions_batch(&conn, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_companions_for_empty_wad() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let companions = get_companions_for_wad(&conn, wad_id).unwrap();
        assert!(companions.is_empty());
    }
}
