use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Connection;

use super::connection::attach_tags;
use super::models::{ALLOWED_UPDATE_FIELDS, SourceType, Status, WAD_COLUMNS, WadRecord};
use crate::Result;

/// A `source_url` as it should be stored and compared.
///
/// Whitespace on a URL is invisible until something compares it. Five Doomworld
/// thread URLs once came in with a trailing `\r`, by their timestamps from a CRLF
/// import list: reqwest strips it while parsing, so the fetch worked and the raw
/// string was what got saved.
pub(crate) fn clean_source_url(url: &str) -> &str {
    url.trim()
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
    pub idgames_id: Option<String>,
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
            idgames_id: None,
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
        self.source_url = Some(clean_source_url(&v.into()).to_string());
        self
    }

    pub fn idgames_id(mut self, v: impl Into<String>) -> Self {
        self.idgames_id = Some(v.into());
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
        Self::default()
    }

    /// Set a text field.
    pub fn set_text(mut self, field: &'static str, value: Option<String>) -> Self {
        if !ALLOWED_UPDATE_FIELDS.contains(field) {
            self.errors.push(field.to_string());
            return self;
        }
        let value = match field {
            "source_url" => value.map(|url| clean_source_url(&url).to_string()),
            _ => value,
        };
        // A file caco has a path for supersedes wherever one was last seen, and
        // the two columns must never both be set: `presence` would otherwise
        // read a stale last-known location back as somewhere to restore from.
        // Not an allowed field of its own because `presence` is its only writer.
        if field == "cached_path" && value.as_deref().is_some_and(|p| !p.is_empty()) {
            self.fields
                .insert("last_known_path", FieldValue::Text(None));
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
    conn.execute(
        "INSERT INTO wads (title, author, year, description, source_type, source_id,
                          source_url, idgames_id, filename, cached_path, status, version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            wad.title,
            wad.author,
            wad.year,
            wad.description,
            wad.source_type.as_str(),
            wad.source_id,
            wad.source_url,
            wad.idgames_id,
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
    let live_only = if include_deleted {
        ""
    } else {
        " AND deleted_at IS NULL"
    };

    let mut stmt = conn.prepare(&format!(
        "SELECT {WAD_COLUMNS} FROM wads WHERE id = ?1{live_only}"
    ))?;
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
/// Setting status to completed records a completion, when it was not already.
pub fn update_wad(conn: &Connection, wad_id: i64, update: &WadUpdate) -> Result<bool> {
    update.validate()?;
    if update.is_empty() {
        return Ok(false);
    }

    let recording_completion = update
        .fields
        .get("status")
        .is_some_and(|v| matches!(v, FieldValue::Text(Some(s)) if s == Status::Completed.as_str()));

    // If we're already inside a transaction (e.g. an import service wrapping
    // add_wad + update_wad atomically), run the body directly against the
    // current connection. Otherwise open a dedicated transaction.
    if conn.is_autocommit() {
        super::connection::with_transaction(conn, |tx| {
            update_wad_body(tx, wad_id, update, recording_completion)
        })
    } else {
        update_wad_body(conn, wad_id, update, recording_completion)
    }
}

fn update_wad_body(
    tx: &Connection,
    wad_id: i64,
    update: &WadUpdate,
    recording_completion: bool,
) -> Result<bool> {
    // Only auto-record a completion when status actually transitions *into*
    // Completed. An idempotent rewrite of an already-completed WAD (e.g. the
    // GUI edit dialog saving with every field, including unchanged status)
    // must not spawn duplicate `wad_completions` rows.
    let is_transition_to_completed = if recording_completion {
        let prev: Option<String> = tx
            .query_row("SELECT status FROM wads WHERE id = ?1", [wad_id], |row| {
                row.get(0)
            })
            .ok();
        prev.as_deref() != Some(Status::Completed.as_str())
    } else {
        false
    };

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

    set_parts.push("updated_at = ?".to_string());
    params.push(Box::new(Utc::now().to_rfc3339()));
    params.push(Box::new(wad_id));

    let sql = format!("UPDATE wads SET {} WHERE id = ?", set_parts.join(", "));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let count = tx.execute(&sql, param_refs.as_slice())?;
    let updated = count > 0;

    if updated && is_transition_to_completed {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The idgames WADs that have no idgames id, which were imported from the local index of
/// the archive at a moment nothing could connect their path to an id.
pub fn idgames_wads_without_id(conn: &Connection) -> Result<Vec<WadRecord>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {WAD_COLUMNS} FROM wads
         WHERE source_type = 'idgames'
           AND COALESCE(idgames_id, '') = ''
           AND COALESCE(source_id, '') = ''
           AND deleted_at IS NULL"
    ))?;
    let wads = statement
        .query_map([], WadRecord::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(wads)
}

/// Give an idgames WAD its id, which on such a row is its `source_id` too. `source_id` is
/// not something an update can change, since it is what makes a row a duplicate of another.
/// `false` if another WAD already has this id.
pub fn set_idgames_id(conn: &Connection, wad_id: i64, idgames_id: i64) -> Result<bool> {
    let id = idgames_id.to_string();
    let taken: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wads
         WHERE id != ?1 AND source_type = 'idgames' AND (source_id = ?2 OR idgames_id = ?2)",
        rusqlite::params![wad_id, id],
        |row| row.get(0),
    )?;
    if taken > 0 {
        return Ok(false);
    }
    conn.execute(
        "UPDATE wads SET idgames_id = ?2, source_id = ?2 WHERE id = ?1",
        rusqlite::params![wad_id, id],
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Availability;
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
        assert_eq!(wad.source_type, SourceType::Idgames);
        assert_eq!(wad.status, Status::Unplayed);
        assert_eq!(wad.tags, vec!["cacoward", "megawad"]);
    }

    #[test]
    fn test_new_wad_stores_idgames_id() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Scythe", SourceType::Idgames).idgames_id("18184"),
        )
        .unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.idgames_id.as_deref(), Some("18184"));
    }

    #[test]
    fn test_new_wad_trims_source_url() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Thread", SourceType::Doomworld)
                .source_url(" https://www.doomworld.com/vb/thread/149498\r\n"),
        )
        .unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(
            wad.source_url.as_deref(),
            Some("https://www.doomworld.com/vb/thread/149498")
        );
    }

    #[test]
    fn test_update_trims_source_url_and_only_source_url() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update = WadUpdate::new()
            .set_text(
                "source_url",
                Some("https://example.com/a.zip\r".to_string()),
            )
            .set_text("notes", Some("  kept as typed  ".to_string()));
        update_wad(&conn, id, &update).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.source_url.as_deref(), Some("https://example.com/a.zip"));
        assert_eq!(wad.notes.as_deref(), Some("  kept as typed  "));

        // Clearing is still clearing.
        update_wad(&conn, id, &WadUpdate::new().set_text("source_url", None)).unwrap();
        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert!(wad.source_url.is_none());
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
        assert_eq!(wad.status, Status::InProgress);
    }

    #[test]
    fn test_required_sourceport_family_roundtrip() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update =
            WadUpdate::new().set_text("required_sourceport_family", Some("dsda".to_string()));
        update_wad(&conn, id, &update).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.required_sourceport_family.as_deref(), Some("dsda"));
    }

    #[test]
    fn test_required_sourceport_family_can_be_cleared_to_null() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update =
            WadUpdate::new().set_text("required_sourceport_family", Some("dsda".to_string()));
        update_wad(&conn, id, &update).unwrap();
        let update = WadUpdate::new().set_text("required_sourceport_family", None);
        update_wad(&conn, id, &update).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert!(wad.required_sourceport_family.is_none());
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
    fn test_update_already_completed_does_not_duplicate_completion() {
        // Regression: saving an unchanged status (e.g. the GUI edit dialog
        // rewriting every field when the user tweaks the rating on an
        // already-completed WAD) must not spawn a duplicate completion row.
        let conn = setup();
        let id = add_test_wad(&conn);

        // First transition into Completed — one completion expected.
        update_wad(&conn, id, &WadUpdate::new().set_status(Status::Completed)).unwrap();

        // Now rewrite status=completed alongside an unrelated field. This is
        // exactly what the GUI save does when the WAD is already completed.
        let update = WadUpdate::new()
            .set_status(Status::Completed)
            .set_int("rating", Some(5));
        update_wad(&conn, id, &update).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wad_completions WHERE wad_id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "idempotent status rewrite must not add completion"
        );
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
    fn test_new_wad_defaults_to_unplayed() {
        let conn = setup();
        let id = add_wad(&conn, &NewWad::new("Test", SourceType::Local)).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.status, Status::Unplayed);
    }

    #[test]
    fn test_update_status() {
        let conn = setup();
        let id = add_test_wad(&conn);

        // Set to in-progress
        let update = WadUpdate::new().set_status(Status::InProgress);
        update_wad(&conn, id, &update).unwrap();
        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.status, Status::InProgress);

        // Set to completed
        let update = WadUpdate::new().set_status(Status::Completed);
        update_wad(&conn, id, &update).unwrap();
        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.status, Status::Completed);
    }

    #[test]
    fn test_availability_derived_after_add() {
        let conn = setup();

        // No cached_path, no source_url → unavailable
        let id1 = add_wad(&conn, &NewWad::new("No URL", SourceType::Local)).unwrap();
        let w1 = get_wad(&conn, id1, false).unwrap().unwrap();
        assert_eq!(w1.availability(), Availability::Unavailable);

        // With source_url → downloadable
        let id2 = add_wad(
            &conn,
            &NewWad::new("Has URL", SourceType::Idgames).source_url("https://example.com/wad.zip"),
        )
        .unwrap();
        let w2 = get_wad(&conn, id2, false).unwrap().unwrap();
        assert_eq!(w2.availability(), Availability::Downloadable);

        // With cached_path → cached
        let id3 = add_wad(
            &conn,
            &NewWad::new("Cached", SourceType::Local).cached_path("/tmp/test.wad"),
        )
        .unwrap();
        let w3 = get_wad(&conn, id3, false).unwrap().unwrap();
        assert_eq!(w3.availability(), Availability::Cached);
    }

    #[test]
    fn test_availability_follows_cached_path_changes() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Test", SourceType::Idgames).source_url("https://example.com/wad.zip"),
        )
        .unwrap();

        // Initially downloadable
        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.availability(), Availability::Downloadable);

        // Set cached_path → cached
        let update = WadUpdate::new().set_text("cached_path", Some("/tmp/test.wad".to_string()));
        update_wad(&conn, id, &update).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.availability(), Availability::Cached);

        // Clear cached_path → back to downloadable
        let update = WadUpdate::new().set_text("cached_path", None);
        update_wad(&conn, id, &update).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.availability(), Availability::Downloadable);
    }

    /// `presence` restores a row from `last_known_path`, so a row that has a
    /// file again must not keep one: evicting the new copy would otherwise
    /// bring back a link to wherever the old one used to be.
    #[test]
    fn test_setting_cached_path_forgets_last_known_path() {
        let conn = setup();
        let id = add_wad(&conn, &NewWad::new("Test", SourceType::Doomworld)).unwrap();
        conn.execute(
            "UPDATE wads SET last_known_path = '/mnt/nas/test.wad' WHERE id = ?1",
            [id],
        )
        .unwrap();
        let last_known =
            |conn: &Connection| get_wad(conn, id, false).unwrap().unwrap().last_known_path;

        // Clearing a path, or writing a blank one, is not finding the file.
        for blank in [None, Some(String::new())] {
            update_wad(&conn, id, &WadUpdate::new().set_text("cached_path", blank)).unwrap();
            assert_eq!(last_known(&conn).as_deref(), Some("/mnt/nas/test.wad"));
        }

        let found = WadUpdate::new().set_text("cached_path", Some("/keep/test.wad".to_string()));
        update_wad(&conn, id, &found).unwrap();
        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.cached_path.as_deref(), Some("/keep/test.wad"));
        assert_eq!(wad.last_known_path, None);
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

    #[test]
    fn test_only_idgames_wads_with_no_id_are_waiting_for_one() {
        let conn = setup();
        let waiting = add_wad(&conn, &NewWad::new("Waiting", SourceType::Idgames)).unwrap();
        add_wad(
            &conn,
            &NewWad::new("Has one", SourceType::Idgames)
                .source_id("1")
                .idgames_id("1"),
        )
        .unwrap();
        add_wad(&conn, &NewWad::new("From the wiki", SourceType::Doomwiki)).unwrap();
        let trashed = add_wad(&conn, &NewWad::new("Trashed", SourceType::Idgames)).unwrap();
        delete_wad(&conn, trashed, false).unwrap();

        let found: Vec<i64> = idgames_wads_without_id(&conn)
            .unwrap()
            .iter()
            .map(|wad| wad.id)
            .collect();
        assert_eq!(found, [waiting]);
    }

    #[test]
    fn test_setting_an_idgames_id_sets_what_duplicates_go_by_too() {
        let conn = setup();
        let id = add_wad(&conn, &NewWad::new("Sunlust", SourceType::Idgames)).unwrap();

        assert!(set_idgames_id(&conn, id, 18168).unwrap());

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.idgames_id.as_deref(), Some("18168"));
        assert_eq!(wad.source_id.as_deref(), Some("18168"));
        // Its own id is not another WAD's.
        assert!(set_idgames_id(&conn, id, 18168).unwrap());
    }

    #[test]
    fn test_an_id_another_wad_has_is_not_set() {
        let conn = setup();
        add_wad(
            &conn,
            &NewWad::new("Sunlust", SourceType::Idgames)
                .source_id("18168")
                .idgames_id("18168"),
        )
        .unwrap();
        let id = add_wad(&conn, &NewWad::new("The same", SourceType::Idgames)).unwrap();

        assert!(!set_idgames_id(&conn, id, 18168).unwrap());

        assert_eq!(get_wad(&conn, id, false).unwrap().unwrap().idgames_id, None);
    }
}
