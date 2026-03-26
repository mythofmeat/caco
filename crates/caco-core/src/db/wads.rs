use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Connection;

use super::connection::attach_tags;
use super::models::{WadRecord, ALLOWED_UPDATE_FIELDS, SourceType, Status};
use crate::Result;

// ---------------------------------------------------------------------------
// NewWad builder
// ---------------------------------------------------------------------------

/// Builder for inserting a new WAD.
pub struct NewWad {
    pub title: String,
    pub source_type: SourceType,
    pub author: Option<String>,
    pub year: Option<i32>,
    pub description: Option<String>,
    pub source_id: Option<String>,
    pub source_url: Option<String>,
    pub filename: Option<String>,
    pub cached_path: Option<String>,
    pub status: Status,
    pub version: Option<String>,
    pub tags: Vec<String>,
}

impl NewWad {
    pub fn new(title: impl Into<String>, source_type: SourceType) -> Self {
        Self {
            title: title.into(),
            source_type,
            author: None,
            year: None,
            description: None,
            source_id: None,
            source_url: None,
            filename: None,
            cached_path: None,
            status: Status::Backlog,
            version: None,
            tags: Vec::new(),
        }
    }

    pub fn author(mut self, v: impl Into<String>) -> Self {
        self.author = Some(v.into());
        self
    }

    pub fn year(mut self, v: i32) -> Self {
        self.year = Some(v);
        self
    }

    pub fn description(mut self, v: impl Into<String>) -> Self {
        self.description = Some(v.into());
        self
    }

    pub fn source_id(mut self, v: impl Into<String>) -> Self {
        self.source_id = Some(v.into());
        self
    }

    pub fn source_url(mut self, v: impl Into<String>) -> Self {
        self.source_url = Some(v.into());
        self
    }

    pub fn filename(mut self, v: impl Into<String>) -> Self {
        self.filename = Some(v.into());
        self
    }

    pub fn cached_path(mut self, v: impl Into<String>) -> Self {
        self.cached_path = Some(v.into());
        self
    }

    pub fn status(mut self, v: Status) -> Self {
        self.status = v;
        self
    }

    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = Some(v.into());
        self
    }

    pub fn tags(mut self, v: Vec<String>) -> Self {
        self.tags = v;
        self
    }
}

// ---------------------------------------------------------------------------
// WadUpdate builder
// ---------------------------------------------------------------------------

/// Builder for updating WAD fields. Only fields in `ALLOWED_UPDATE_FIELDS` are accepted.
#[derive(Default)]
pub struct WadUpdate {
    fields: HashMap<&'static str, FieldValue>,
    /// Whether to record a completion when status is set to finished.
    pub record_completion: bool,
}

/// A dynamically-typed field value for SQL binding.
#[derive(Debug, Clone)]
pub enum FieldValue {
    Text(Option<String>),
    Int(Option<i64>),
}

impl WadUpdate {
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
            record_completion: true,
        }
    }

    /// Set a text field.
    pub fn set_text(mut self, field: &'static str, value: Option<String>) -> crate::Result<Self> {
        if !ALLOWED_UPDATE_FIELDS.contains(field) {
            return Err(crate::Error::InvalidField(field.to_string()));
        }
        self.fields.insert(field, FieldValue::Text(value));
        Ok(self)
    }

    /// Set an integer field.
    pub fn set_int(mut self, field: &'static str, value: Option<i64>) -> crate::Result<Self> {
        if !ALLOWED_UPDATE_FIELDS.contains(field) {
            return Err(crate::Error::InvalidField(field.to_string()));
        }
        self.fields.insert(field, FieldValue::Int(value));
        Ok(self)
    }

    /// Set the status field (convenience).
    pub fn set_status(self, status: Status) -> crate::Result<Self> {
        self.set_text("status", Some(status.as_str().to_string()))
    }

    /// Disable automatic completion recording when status is set to finished.
    pub fn no_completion(mut self) -> Self {
        self.record_completion = false;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

// ---------------------------------------------------------------------------
// CRUD functions
// ---------------------------------------------------------------------------

/// Add a WAD to the library. Returns the new WAD ID.
pub fn add_wad(conn: &Connection, wad: &NewWad) -> Result<i64> {
    conn.execute(
        "INSERT INTO wads (title, author, year, description, source_type,
                          source_id, source_url, filename, cached_path, status, version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            wad.title,
            wad.author,
            wad.year,
            wad.description,
            wad.source_type.as_str(),
            wad.source_id,
            wad.source_url,
            wad.filename,
            wad.cached_path,
            wad.status.as_str(),
            wad.version,
        ],
    )?;

    let wad_id = conn.last_insert_rowid();

    for tag in &wad.tags {
        conn.execute(
            "INSERT OR IGNORE INTO tags (wad_id, tag) VALUES (?1, ?2)",
            rusqlite::params![wad_id, tag.to_lowercase()],
        )?;
    }

    Ok(wad_id)
}

/// Get a WAD by ID.
///
/// If `include_deleted` is false (default), deleted WADs are excluded.
pub fn get_wad(conn: &Connection, wad_id: i64, include_deleted: bool) -> Result<Option<WadRecord>> {
    let sql = if include_deleted {
        "SELECT * FROM wads WHERE id = ?1"
    } else {
        "SELECT * FROM wads WHERE id = ?1 AND deleted_at IS NULL"
    };

    let mut stmt = conn.prepare(sql)?;
    let mut wad = match stmt.query_row([wad_id], WadRecord::from_row) {
        Ok(w) => w,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    attach_tags(conn, &mut wad)?;
    Ok(Some(wad))
}

/// Update a WAD's fields. Returns `true` if updated.
///
/// If status is set to "finished", automatically records a completion
/// (unless `record_completion` is false on the `WadUpdate`).
pub fn update_wad(conn: &Connection, wad_id: i64, update: &WadUpdate) -> Result<bool> {
    if update.is_empty() {
        return Ok(false);
    }

    // Check if setting status to finished
    let recording_completion = update.record_completion
        && update.fields.get("status").is_some_and(|v| {
            matches!(v, FieldValue::Text(Some(s)) if s == Status::Finished.as_str())
        });

    // Build SET clause
    let mut set_parts = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    for (&field, value) in &update.fields {
        set_parts.push(format!("{field} = ?"));
        match value {
            FieldValue::Text(v) => params.push(Box::new(v.clone())),
            FieldValue::Int(v) => params.push(Box::new(*v)),
        }
    }

    // Always update updated_at
    set_parts.push("updated_at = ?".to_string());
    params.push(Box::new(Utc::now().to_rfc3339()));

    // Add wad_id as final param
    params.push(Box::new(wad_id));

    let sql = format!(
        "UPDATE wads SET {} WHERE id = ?",
        set_parts.join(", ")
    );

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let count = conn.execute(&sql, param_refs.as_slice())?;
    let updated = count > 0;

    // Record completion atomically if status was set to 'finished'
    if updated && recording_completion {
        let snapshot: Option<String> = conn
            .query_row(
                "SELECT stats_snapshot FROM wads WHERE id = ?1",
                [wad_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        conn.execute(
            "INSERT INTO wad_completions (wad_id, stats_snapshot) VALUES (?1, ?2)",
            rusqlite::params![wad_id, snapshot],
        )?;
    }

    Ok(updated)
}

/// Soft-delete a WAD (or permanently delete with `purge=true`).
pub fn delete_wad(conn: &Connection, wad_id: i64, purge: bool) -> Result<bool> {
    let count = if purge {
        conn.execute("DELETE FROM wads WHERE id = ?1", [wad_id])?
    } else {
        conn.execute(
            "UPDATE wads SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            rusqlite::params![Utc::now().to_rfc3339(), wad_id],
        )?
    };
    Ok(count > 0)
}

/// Restore a soft-deleted WAD.
pub fn restore_wad(conn: &Connection, wad_id: i64) -> Result<bool> {
    let count = conn.execute(
        "UPDATE wads SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
        [wad_id],
    )?;
    Ok(count > 0)
}

/// Permanently delete all soft-deleted WADs. Returns count purged.
pub fn purge_all_deleted(conn: &Connection) -> Result<usize> {
    let count = conn.execute("DELETE FROM wads WHERE deleted_at IS NOT NULL", [])?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// Tag operations
// ---------------------------------------------------------------------------

/// Add a tag to a WAD. Returns `true` if added.
pub fn add_tag(conn: &Connection, wad_id: i64, tag: &str) -> Result<bool> {
    match conn.execute(
        "INSERT OR IGNORE INTO tags (wad_id, tag) VALUES (?1, ?2)",
        rusqlite::params![wad_id, tag.to_lowercase()],
    ) {
        Ok(n) => Ok(n > 0),
        Err(e) => Err(e.into()),
    }
}

/// Remove a tag from a WAD. Returns `true` if removed.
pub fn remove_tag(conn: &Connection, wad_id: i64, tag: &str) -> Result<bool> {
    let count = conn.execute(
        "DELETE FROM tags WHERE wad_id = ?1 AND tag = ?2",
        rusqlite::params![wad_id, tag.to_lowercase()],
    )?;
    Ok(count > 0)
}

/// Remove all tags from a WAD. Returns count removed.
pub fn remove_all_tags(conn: &Connection, wad_id: i64) -> Result<usize> {
    let count = conn.execute("DELETE FROM tags WHERE wad_id = ?1", [wad_id])?;
    Ok(count)
}

/// Get all unique tags.
pub fn get_all_tags(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT tag FROM tags ORDER BY tag")?;
    let tags = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(tags)
}

/// Get all tags with their WAD counts (excluding deleted WADs).
pub fn get_tag_counts(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT t.tag, COUNT(*) as count
         FROM tags t
         JOIN wads w ON w.id = t.wad_id
         WHERE w.deleted_at IS NULL
         GROUP BY t.tag
         ORDER BY t.tag",
    )?;
    let counts = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(counts)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::{fetch_tags, open_memory};
    use crate::db::schema::init_db;

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection) -> i64 {
        add_wad(
            conn,
            &NewWad::new("Test WAD", SourceType::Local)
                .author("Test Author")
                .year(2024),
        )
        .unwrap()
    }

    #[test]
    fn test_add_and_get_wad() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Scythe", SourceType::Idgames)
                .author("Erik Alm")
                .year(2003)
                .description("Great megawad")
                .tags(vec!["cacoward".into(), "megawad".into()]),
        )
        .unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.title, "Scythe");
        assert_eq!(wad.author.as_deref(), Some("Erik Alm"));
        assert_eq!(wad.year, Some(2003));
        assert_eq!(wad.source_type, "idgames");
        assert_eq!(wad.status, "backlog");
        assert_eq!(wad.tags, vec!["cacoward", "megawad"]);
    }

    #[test]
    fn test_get_nonexistent() {
        let conn = setup();
        assert!(get_wad(&conn, 999, false).unwrap().is_none());
    }

    #[test]
    fn test_update_wad() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update = WadUpdate::new()
            .set_text("title", Some("Updated Title".to_string()))
            .unwrap()
            .set_status(Status::Playing)
            .unwrap();
        assert!(update_wad(&conn, id, &update).unwrap());

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.title, "Updated Title");
        assert_eq!(wad.status, "playing");
    }

    #[test]
    fn test_update_invalid_field() {
        let result = WadUpdate::new().set_text("invalid_field", Some("value".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_update_status_finished_records_completion() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update = WadUpdate::new().set_status(Status::Finished).unwrap();
        update_wad(&conn, id, &update).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wad_completions WHERE wad_id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_update_status_finished_no_completion() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update = WadUpdate::new()
            .set_status(Status::Finished)
            .unwrap()
            .no_completion();
        update_wad(&conn, id, &update).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wad_completions WHERE wad_id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_soft_delete_and_restore() {
        let conn = setup();
        let id = add_test_wad(&conn);

        // Soft delete
        assert!(delete_wad(&conn, id, false).unwrap());
        assert!(get_wad(&conn, id, false).unwrap().is_none());

        // Still visible with include_deleted
        assert!(get_wad(&conn, id, true).unwrap().is_some());

        // Restore
        assert!(restore_wad(&conn, id).unwrap());
        assert!(get_wad(&conn, id, false).unwrap().is_some());
    }

    #[test]
    fn test_hard_delete() {
        let conn = setup();
        let id = add_test_wad(&conn);

        assert!(delete_wad(&conn, id, true).unwrap());
        assert!(get_wad(&conn, id, true).unwrap().is_none());
    }

    #[test]
    fn test_purge_all_deleted() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_test_wad(&conn);
        let _id3 = add_test_wad(&conn);

        delete_wad(&conn, id1, false).unwrap();
        delete_wad(&conn, id2, false).unwrap();

        let purged = purge_all_deleted(&conn).unwrap();
        assert_eq!(purged, 2);
    }

    #[test]
    fn test_tags() {
        let conn = setup();
        let id = add_test_wad(&conn);

        // Add tags
        assert!(add_tag(&conn, id, "megawad").unwrap());
        assert!(add_tag(&conn, id, "Cacoward").unwrap()); // should be lowercased

        // Duplicate tag should return false
        assert!(!add_tag(&conn, id, "megawad").unwrap());

        // Fetch tags
        let tags = fetch_tags(&conn, id).unwrap();
        assert_eq!(tags, vec!["cacoward", "megawad"]);

        // Remove tag
        assert!(remove_tag(&conn, id, "megawad").unwrap());
        let tags = fetch_tags(&conn, id).unwrap();
        assert_eq!(tags, vec!["cacoward"]);

        // Remove nonexistent tag
        assert!(!remove_tag(&conn, id, "nonexistent").unwrap());
    }

    #[test]
    fn test_remove_all_tags() {
        let conn = setup();
        let id = add_test_wad(&conn);
        add_tag(&conn, id, "a").unwrap();
        add_tag(&conn, id, "b").unwrap();
        add_tag(&conn, id, "c").unwrap();

        let removed = remove_all_tags(&conn, id).unwrap();
        assert_eq!(removed, 3);
        assert!(fetch_tags(&conn, id).unwrap().is_empty());
    }

    #[test]
    fn test_get_all_tags() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_test_wad(&conn);
        add_tag(&conn, id1, "doom").unwrap();
        add_tag(&conn, id1, "megawad").unwrap();
        add_tag(&conn, id2, "doom").unwrap();
        add_tag(&conn, id2, "slaughter").unwrap();

        let tags = get_all_tags(&conn).unwrap();
        assert_eq!(tags, vec!["doom", "megawad", "slaughter"]);
    }

    #[test]
    fn test_get_tag_counts() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_test_wad(&conn);
        add_tag(&conn, id1, "doom").unwrap();
        add_tag(&conn, id2, "doom").unwrap();
        add_tag(&conn, id1, "megawad").unwrap();

        let counts = get_tag_counts(&conn).unwrap();
        assert_eq!(counts, vec![("doom".to_string(), 2), ("megawad".to_string(), 1)]);
    }

    #[test]
    fn test_tag_counts_exclude_deleted() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_test_wad(&conn);
        add_tag(&conn, id1, "doom").unwrap();
        add_tag(&conn, id2, "doom").unwrap();

        // Soft-delete one WAD
        delete_wad(&conn, id2, false).unwrap();

        let counts = get_tag_counts(&conn).unwrap();
        assert_eq!(counts, vec![("doom".to_string(), 1)]);
    }

    #[test]
    fn test_cascade_delete_tags() {
        let conn = setup();
        let id = add_test_wad(&conn);
        add_tag(&conn, id, "doom").unwrap();
        add_tag(&conn, id, "megawad").unwrap();

        // Hard delete should cascade to tags
        delete_wad(&conn, id, true).unwrap();
        let tags = fetch_tags(&conn, id).unwrap();
        assert!(tags.is_empty());
    }
}
