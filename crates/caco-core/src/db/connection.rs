use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use crate::Result;

/// Conservative limit for SQLite's SQLITE_MAX_VARIABLE_NUMBER (default 999).
pub const SQLITE_MAX_VARS: usize = 900;

/// Open a database connection with recommended pragmas.
///
/// Sets WAL mode, foreign keys, 20 MB cache, and in-memory temp store.
pub fn open_connection(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -20000;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(conn)
}

/// Open an in-memory database with the same pragmas (useful for testing).
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(conn)
}

/// Fetch tags for a single WAD.
pub fn fetch_tags(conn: &Connection, wad_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT tag FROM tags WHERE wad_id = ? ORDER BY tag")?;
    let tags = stmt
        .query_map([wad_id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(tags)
}

/// Fetch tags for multiple WADs efficiently. Returns `{wad_id: [tags]}`.
pub fn fetch_tags_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>> {
    if wad_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result: HashMap<i64, Vec<String>> = HashMap::new();

    for chunk in wad_ids.chunks(SQLITE_MAX_VARS) {
        let placeholders: String = itertools_placeholders(chunk.len());
        let sql = format!(
            "SELECT wad_id, tag FROM tags WHERE wad_id IN ({placeholders}) ORDER BY tag"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (wad_id, tag) = row?;
            result.entry(wad_id).or_default().push(tag);
        }
    }

    Ok(result)
}

/// Attach tags to a `WadRecord` by fetching them from the database.
pub fn attach_tags(conn: &Connection, wad: &mut super::models::WadRecord) -> Result<()> {
    wad.tags = fetch_tags(conn, wad.id)?;
    Ok(())
}

/// Build a comma-separated list of `?` placeholders.
fn itertools_placeholders(n: usize) -> String {
    let mut s = String::with_capacity(n * 2);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('?');
    }
    s
}

/// Generic batch query helper for aggregation queries.
///
/// `query_template` should contain `{placeholders}` which will be replaced.
/// Each row must have columns `wad_id` and the column named by `result_column`.
pub fn batch_query_i64(
    conn: &Connection,
    wad_ids: &[i64],
    query_template: &str,
    result_column: &str,
) -> Result<HashMap<i64, i64>> {
    if wad_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result = HashMap::new();

    for chunk in wad_ids.chunks(SQLITE_MAX_VARS) {
        let placeholders = itertools_placeholders(chunk.len());
        let sql = query_template.replace("{placeholders}", &placeholders);
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            let wad_id: i64 = row.get("wad_id")?;
            let val: i64 = row.get(result_column)?;
            Ok((wad_id, val))
        })?;
        for row in rows {
            let (wad_id, val) = row?;
            result.insert(wad_id, val);
        }
    }

    Ok(result)
}

/// Batch query helper that returns string values.
///
/// Like `batch_query_i64` but for `String` result columns (e.g., timestamps).
pub fn batch_query_string(
    conn: &Connection,
    wad_ids: &[i64],
    query_template: &str,
    result_column: &str,
) -> Result<HashMap<i64, String>> {
    if wad_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result = HashMap::new();

    for chunk in wad_ids.chunks(SQLITE_MAX_VARS) {
        let placeholders = itertools_placeholders(chunk.len());
        let sql = query_template.replace("{placeholders}", &placeholders);
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            let wad_id: i64 = row.get("wad_id")?;
            let val: String = row.get(result_column)?;
            Ok((wad_id, val))
        })?;
        for row in rows {
            let (wad_id, val) = row?;
            result.insert(wad_id, val);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_memory() {
        let conn = open_memory().unwrap();
        // Verify pragmas are set
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn test_placeholders() {
        assert_eq!(itertools_placeholders(1), "?");
        assert_eq!(itertools_placeholders(3), "?,?,?");
        assert_eq!(itertools_placeholders(0), "");
    }

    #[test]
    fn test_fetch_tags_empty_db() {
        let conn = open_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tags (id INTEGER PRIMARY KEY, wad_id INTEGER, tag TEXT);",
        )
        .unwrap();
        let tags = fetch_tags(&conn, 1).unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn test_fetch_tags_batch_empty() {
        let conn = open_memory().unwrap();
        let result = fetch_tags_batch(&conn, &[]).unwrap();
        assert!(result.is_empty());
    }
}
