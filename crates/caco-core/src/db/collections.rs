use rusqlite::Connection;

use super::models::WadRecord;
use super::query::search_wads;
use crate::Result;

// ---------------------------------------------------------------------------
// CollectionRecord
// ---------------------------------------------------------------------------

/// A saved smart collection (named query).
#[derive(Debug, Clone)]
pub struct CollectionRecord {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub sort_by: Option<String>,
    pub sort_desc: bool,
    pub created_at: String,
}

impl CollectionRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            query: row.get("query")?,
            sort_by: row.get("sort_by")?,
            sort_desc: row.get::<_, i64>("sort_desc").unwrap_or(1) != 0,
            created_at: row.get("created_at")?,
        })
    }
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

/// Create a new smart collection. Returns its ID.
pub fn create_collection(
    conn: &Connection,
    name: &str,
    query: &str,
    sort_by: Option<&str>,
    sort_desc: bool,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO smart_collections (name, query, sort_by, sort_desc) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![name, query, sort_by, sort_desc as i64],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get a collection by name.
pub fn get_collection(conn: &Connection, name: &str) -> Result<Option<CollectionRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM smart_collections WHERE name = ?1")?;
    match stmt.query_row([name], CollectionRecord::from_row) {
        Ok(c) => Ok(Some(c)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all collections, ordered by name.
pub fn get_all_collections(conn: &Connection) -> Result<Vec<CollectionRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM smart_collections ORDER BY name")?;
    let rows = stmt
        .query_map([], CollectionRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Update a collection's query and/or sort.
pub fn update_collection(
    conn: &Connection,
    name: &str,
    query: Option<&str>,
    sort_by: Option<Option<&str>>,
    sort_desc: Option<bool>,
) -> Result<bool> {
    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(q) = query {
        updates.push("query = ?");
        params.push(Box::new(q.to_string()));
    }
    if let Some(sb) = sort_by {
        updates.push("sort_by = ?");
        params.push(Box::new(sb.map(|s| s.to_string())));
    }
    if let Some(sd) = sort_desc {
        updates.push("sort_desc = ?");
        params.push(Box::new(sd as i64));
    }

    if updates.is_empty() {
        return Ok(false);
    }

    params.push(Box::new(name.to_string()));
    let sql = format!(
        "UPDATE smart_collections SET {} WHERE name = ?",
        updates.join(", ")
    );
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let count = conn.execute(&sql, param_refs.as_slice())?;
    Ok(count > 0)
}

/// Delete a collection by name. Returns true if deleted.
pub fn delete_collection(conn: &Connection, name: &str) -> Result<bool> {
    let count = conn.execute("DELETE FROM smart_collections WHERE name = ?1", [name])?;
    Ok(count > 0)
}

/// Execute a saved collection's query. Returns matching WADs.
pub fn run_collection(conn: &Connection, name: &str) -> Result<Vec<WadRecord>> {
    let coll = get_collection(conn, name)?
        .ok_or_else(|| crate::Error::Config(format!("collection not found: {name}")))?;

    search_wads(
        conn,
        Some(&coll.query),
        coll.sort_by.as_deref(),
        coll.sort_desc,
        false,
        0,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::open_memory;
    use crate::db::models::SourceType;
    use crate::db::schema::init_db;
    use crate::db::wads::{NewWad, add_wad};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_create_and_get_collection() {
        let conn = setup();
        let id = create_collection(&conn, "test", "intent:queued", Some("title"), false).unwrap();
        assert!(id > 0);

        let coll = get_collection(&conn, "test").unwrap().unwrap();
        assert_eq!(coll.name, "test");
        assert_eq!(coll.query, "intent:queued");
        assert_eq!(coll.sort_by.as_deref(), Some("title"));
        assert!(!coll.sort_desc);
    }

    #[test]
    fn test_get_all_collections() {
        let conn = setup();
        create_collection(&conn, "alpha", "play:started", None, true).unwrap();
        create_collection(&conn, "beta", "intent:inbox", None, true).unwrap();

        let all = get_all_collections(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "alpha");
        assert_eq!(all[1].name, "beta");
    }

    #[test]
    fn test_update_collection() {
        let conn = setup();
        create_collection(&conn, "test", "old query", None, true).unwrap();

        let updated = update_collection(&conn, "test", Some("new query"), None, None).unwrap();
        assert!(updated);

        let coll = get_collection(&conn, "test").unwrap().unwrap();
        assert_eq!(coll.query, "new query");
    }

    #[test]
    fn test_delete_collection() {
        let conn = setup();
        create_collection(&conn, "test", "query", None, true).unwrap();

        assert!(delete_collection(&conn, "test").unwrap());
        assert!(get_collection(&conn, "test").unwrap().is_none());
        assert!(!delete_collection(&conn, "test").unwrap());
    }

    #[test]
    fn test_run_collection() {
        let conn = setup();
        add_wad(
            &conn,
            &NewWad::new("Test WAD", SourceType::Local).author("Author"),
        )
        .unwrap();

        create_collection(&conn, "all-local", "source:local", None, true).unwrap();

        let results = run_collection(&conn, "all-local").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test WAD");
    }

    #[test]
    fn test_run_nonexistent_collection() {
        let conn = setup();
        let result = run_collection(&conn, "nope");
        assert!(result.is_err());
    }
}
