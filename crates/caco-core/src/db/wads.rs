use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Connection;

use super::connection::attach_tags;
use super::models::{ALLOWED_UPDATE_FIELDS, Availability, SourceType, Status, WadRecord};
use crate::Result;

/// Compute availability from WAD fields.
pub fn compute_availability(cached_path: Option<&str>, source_url: Option<&str>) -> Availability {
    if cached_path.is_some() {
        Availability::Cached
    } else if source_url.is_some() {
        Availability::Downloadable
    } else {
        Availability::Unavailable
    }
}

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
            status: Status::Unplayed,
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
///
/// Setter methods consume `self` and return `Self` — validation errors are
/// accumulated in `errors` and surfaced when the update is applied
/// (see [`WadUpdate::validate`] and [`update_wad`]). This lets callers chain
/// setters fluently without bailing out on the first bad field, and preserves
/// any valid state built up before an error.
#[derive(Default)]
pub struct WadUpdate {
    fields: HashMap<&'static str, FieldValue>,
    /// Whether to record a completion when status is set to finished.
    pub record_completion: bool,
    /// Field names that were rejected by a setter. Surfaced by
    /// [`validate`](Self::validate).
    errors: Vec<String>,
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
            errors: Vec::new(),
        }
    }

    /// Set a text field.
    pub fn set_text(mut self, field: &'static str, value: Option<String>) -> Self {
        if !ALLOWED_UPDATE_FIELDS.contains(field) {
            self.errors.push(field.to_string());
            return self;
        }
        self.fields.insert(field, FieldValue::Text(value));
        self
    }

    /// Set an integer field.
    pub fn set_int(mut self, field: &'static str, value: Option<i64>) -> Self {
        if !ALLOWED_UPDATE_FIELDS.contains(field) {
            self.errors.push(field.to_string());
            return self;
        }
        self.fields.insert(field, FieldValue::Int(value));
        self
    }

    /// Set the status field (convenience).
    pub fn set_status(self, status: Status) -> Self {
        self.set_text("status", Some(status.as_str().to_string()))
    }

    /// Set the availability.
    pub fn set_availability(self, avail: Availability) -> Self {
        self.set_text("availability", Some(avail.as_str().to_string()))
    }

    /// Disable automatic completion recording when status is set to finished.
    pub fn no_completion(mut self) -> Self {
        self.record_completion = false;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Return `Err` if any setter call rejected a field name.
    pub fn validate(&self) -> crate::Result<()> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(crate::Error::InvalidFields(self.errors.clone()))
        }
    }
}

// ---------------------------------------------------------------------------
// CRUD functions
// ---------------------------------------------------------------------------

/// Add a WAD to the library. Returns the new WAD ID.
pub fn add_wad(conn: &Connection, wad: &NewWad) -> Result<i64> {
    let avail = compute_availability(wad.cached_path.as_deref(), wad.source_url.as_deref());
    conn.execute(
        "INSERT INTO wads (title, author, year, description, source_type,
                          source_id, source_url, filename, cached_path, status, version,
                          availability)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            avail.as_str(),
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
    update.validate()?;
    if update.is_empty() {
        return Ok(false);
    }

    // Check if setting status to finished
    let recording_completion = update.record_completion
        && update.fields.get("status").is_some_and(
            |v| matches!(v, FieldValue::Text(Some(s)) if s == Status::Completed.as_str()),
        );

    // Auto-maintain availability when cached_path or source_url change.
    let needs_avail_update = (update.fields.contains_key("cached_path")
        || update.fields.contains_key("source_url"))
        && !update.fields.contains_key("availability");

    // If we're already inside a transaction (e.g. an import service wrapping
    // add_wad + update_wad atomically), run the body directly against the
    // current connection. Otherwise open a dedicated transaction.
    if conn.is_autocommit() {
        super::connection::with_transaction(conn, |tx| {
            update_wad_body(tx, wad_id, update, recording_completion, needs_avail_update)
        })
    } else {
        update_wad_body(
            conn,
            wad_id,
            update,
            recording_completion,
            needs_avail_update,
        )
    }
}

fn update_wad_body(
    tx: &Connection,
    wad_id: i64,
    update: &WadUpdate,
    recording_completion: bool,
    needs_avail_update: bool,
) -> Result<bool> {
    let mut extra_fields: Vec<(&str, FieldValue)> = Vec::new();

    if needs_avail_update {
        let (cur_cached, cur_source): (Option<String>, Option<String>) = tx.query_row(
            "SELECT cached_path, source_url FROM wads WHERE id = ?1",
            [wad_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let eff_cached = match update.fields.get("cached_path") {
            Some(FieldValue::Text(v)) => v.as_deref(),
            _ => cur_cached.as_deref(),
        };
        let eff_source = match update.fields.get("source_url") {
            Some(FieldValue::Text(v)) => v.as_deref(),
            _ => cur_source.as_deref(),
        };

        let avail = compute_availability(eff_cached, eff_source);
        extra_fields.push((
            "availability",
            FieldValue::Text(Some(avail.as_str().to_string())),
        ));
    }

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

    for (field, value) in &extra_fields {
        set_parts.push(format!("{field} = ?"));
        match value {
            FieldValue::Text(v) => params.push(Box::new(v.clone())),
            FieldValue::Int(v) => params.push(Box::new(*v)),
        }
    }

    set_parts.push("updated_at = ?".to_string());
    params.push(Box::new(Utc::now().to_rfc3339()));
    params.push(Box::new(wad_id));

    let sql = format!("UPDATE wads SET {} WHERE id = ?", set_parts.join(", "));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let count = tx.execute(&sql, param_refs.as_slice())?;
    let updated = count > 0;

    if updated && recording_completion {
        let snapshot: Option<String> = tx
            .query_row(
                "SELECT stats_snapshot FROM wads WHERE id = ?1",
                [wad_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        tx.execute(
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

/// Count non-deleted WADs grouped by status.
///
/// Keyed by the raw `status` column string ("unplayed", "in-progress", etc.).
/// Returns an empty map if no WADs match; statuses with zero WADs are absent.
pub fn get_status_counts(conn: &Connection) -> Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare(
        "SELECT status, COUNT(*) FROM wads
         WHERE deleted_at IS NULL
         GROUP BY status",
    )?;
    let mut counts = HashMap::new();
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (status, count) = row?;
        counts.insert(status, count);
    }
    Ok(counts)
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
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
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
        assert_eq!(wad.status, "unplayed");
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
            .set_status(Status::InProgress);
        assert!(update_wad(&conn, id, &update).unwrap());

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.title, "Updated Title");
        assert_eq!(wad.status, "in-progress");
    }

    #[test]
    fn test_update_invalid_field() {
        let update = WadUpdate::new().set_text("invalid_field", Some("value".to_string()));
        assert!(update.validate().is_err());
    }

    #[test]
    fn test_update_accumulates_multiple_invalid_fields() {
        let update = WadUpdate::new()
            .set_text("bogus_one", Some("a".to_string()))
            .set_int("bogus_two", Some(1))
            .set_text("title", Some("ok".to_string()));
        match update.validate() {
            Err(crate::Error::InvalidFields(fields)) => {
                assert!(fields.iter().any(|f| f == "bogus_one"));
                assert!(fields.iter().any(|f| f == "bogus_two"));
                assert_eq!(fields.len(), 2);
            }
            other => panic!("expected InvalidFields, got {other:?}"),
        }
    }

    #[test]
    fn test_update_status_finished_records_completion() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update = WadUpdate::new().set_status(Status::Completed);
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
            .set_status(Status::Completed)
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
        assert_eq!(
            counts,
            vec![("doom".to_string(), 2), ("megawad".to_string(), 1)]
        );
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
    fn test_new_wad_defaults_to_unplayed() {
        let conn = setup();
        let id = add_wad(&conn, &NewWad::new("Test", SourceType::Local)).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.status, "unplayed");
    }

    #[test]
    fn test_update_status() {
        let conn = setup();
        let id = add_test_wad(&conn);

        // Set to in-progress
        let update = WadUpdate::new().set_status(Status::InProgress);
        update_wad(&conn, id, &update).unwrap();
        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.status, "in-progress");

        // Set to completed
        let update = WadUpdate::new()
            .set_status(Status::Completed)
            .no_completion();
        update_wad(&conn, id, &update).unwrap();
        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.status, "completed");
    }

    #[test]
    fn test_availability_auto_computed_on_add() {
        let conn = setup();

        // No cached_path, no source_url → unavailable
        let id1 = add_wad(&conn, &NewWad::new("No URL", SourceType::Local)).unwrap();
        let w1 = get_wad(&conn, id1, false).unwrap().unwrap();
        assert_eq!(w1.availability, "unavailable");

        // With source_url → downloadable
        let id2 = add_wad(
            &conn,
            &NewWad::new("Has URL", SourceType::Idgames).source_url("https://example.com/wad.zip"),
        )
        .unwrap();
        let w2 = get_wad(&conn, id2, false).unwrap().unwrap();
        assert_eq!(w2.availability, "downloadable");

        // With cached_path → cached
        let id3 = add_wad(
            &conn,
            &NewWad::new("Cached", SourceType::Local).cached_path("/tmp/test.wad"),
        )
        .unwrap();
        let w3 = get_wad(&conn, id3, false).unwrap().unwrap();
        assert_eq!(w3.availability, "cached");
    }

    #[test]
    fn test_availability_auto_maintained_on_update() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Test", SourceType::Idgames).source_url("https://example.com/wad.zip"),
        )
        .unwrap();

        // Initially downloadable
        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.availability, "downloadable");

        // Set cached_path → should auto-update to cached
        let update = WadUpdate::new().set_text("cached_path", Some("/tmp/test.wad".to_string()));
        update_wad(&conn, id, &update).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.availability, "cached");

        // Clear cached_path → should auto-update back to downloadable
        let update = WadUpdate::new().set_text("cached_path", None);
        update_wad(&conn, id, &update).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.availability, "downloadable");
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
