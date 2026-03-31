use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Connection;

use super::connection::batch_query_i64;
use super::models::PlayState;
use crate::Result;

// ---------------------------------------------------------------------------
// PlaythroughRecord
// ---------------------------------------------------------------------------

/// A single playthrough of a WAD.
#[derive(Debug, Clone)]
pub struct PlaythroughRecord {
    pub id: i64,
    pub wad_id: i64,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub stats_snapshot: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
}

impl PlaythroughRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            wad_id: row.get("wad_id")?,
            started_at: row.get("started_at")?,
            completed_at: row.get("completed_at")?,
            stats_snapshot: row.get("stats_snapshot")?,
            notes: row.get("notes")?,
            created_at: row.get("created_at")?,
        })
    }

    /// Whether this playthrough is still in progress.
    pub fn is_active(&self) -> bool {
        self.completed_at.is_none()
    }
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

/// Start a new playthrough. Returns the playthrough ID.
///
/// Also sets the WAD's `play_state` to `started` and syncs the `status` column.
pub fn start_playthrough(conn: &Connection, wad_id: i64) -> Result<i64> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO playthroughs (wad_id, started_at) VALUES (?1, ?2)",
        rusqlite::params![wad_id, now],
    )?;
    let pt_id = conn.last_insert_rowid();

    // Update WAD play_state to started + sync status.
    // Playing and queued are mutually exclusive: leave the queue when playing starts.
    // Also un-drop if abandoned (playing un-abandons).
    conn.execute(
        "UPDATE wads SET play_state = 'started',
                         intent = CASE intent
                             WHEN 'dropped' THEN 'shelved'
                             WHEN 'queued' THEN 'shelved'
                             ELSE intent END,
                         status = 'playing',
                         updated_at = ?1
         WHERE id = ?2",
        rusqlite::params![now, wad_id],
    )?;

    Ok(pt_id)
}

/// Complete an active playthrough.
///
/// Also sets the WAD's `play_state` to `completed` and syncs the `status` column.
pub fn complete_playthrough(
    conn: &Connection,
    playthrough_id: i64,
    stats_snapshot: Option<&str>,
    notes: Option<&str>,
) -> Result<bool> {
    let now = Utc::now().to_rfc3339();

    let count = conn.execute(
        "UPDATE playthroughs SET completed_at = ?1, stats_snapshot = ?2, notes = ?3
         WHERE id = ?4 AND completed_at IS NULL",
        rusqlite::params![now, stats_snapshot, notes, playthrough_id],
    )?;

    if count == 0 {
        return Ok(false);
    }

    // Get the wad_id to update play_state
    let wad_id: i64 = conn.query_row(
        "SELECT wad_id FROM playthroughs WHERE id = ?1",
        [playthrough_id],
        |row| row.get(0),
    )?;

    conn.execute(
        "UPDATE wads SET play_state = 'completed', status = 'finished', updated_at = ?1
         WHERE id = ?2",
        rusqlite::params![now, wad_id],
    )?;

    // Also create a wad_completions record for backward compat during transition
    conn.execute(
        "INSERT INTO wad_completions (wad_id, completed_at, stats_snapshot, notes)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![wad_id, now, stats_snapshot, notes],
    )?;

    Ok(true)
}

/// Get the active (in-progress) playthrough for a WAD, if any.
pub fn get_active_playthrough(conn: &Connection, wad_id: i64) -> Result<Option<PlaythroughRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM playthroughs WHERE wad_id = ?1 AND completed_at IS NULL
         ORDER BY started_at DESC LIMIT 1",
    )?;
    match stmt.query_row([wad_id], PlaythroughRecord::from_row) {
        Ok(pt) => Ok(Some(pt)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all playthroughs for a WAD, ordered by most recent first.
pub fn get_playthroughs(conn: &Connection, wad_id: i64) -> Result<Vec<PlaythroughRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM playthroughs WHERE wad_id = ?1 ORDER BY started_at DESC",
    )?;
    let rows = stmt
        .query_map([wad_id], PlaythroughRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get a single playthrough by ID.
pub fn get_playthrough(conn: &Connection, id: i64) -> Result<Option<PlaythroughRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM playthroughs WHERE id = ?1")?;
    match stmt.query_row([id], PlaythroughRecord::from_row) {
        Ok(pt) => Ok(Some(pt)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Delete a playthrough. Returns true if deleted.
pub fn delete_playthrough(conn: &Connection, id: i64) -> Result<bool> {
    let count = conn.execute("DELETE FROM playthroughs WHERE id = ?1", [id])?;
    Ok(count > 0)
}

/// Get or create an active playthrough for a WAD.
///
/// If an active playthrough exists, returns its ID. Otherwise starts a new one.
pub fn ensure_playthrough(conn: &Connection, wad_id: i64) -> Result<i64> {
    if let Some(pt) = get_active_playthrough(conn, wad_id)? {
        return Ok(pt.id);
    }
    start_playthrough(conn, wad_id)
}

/// Count completed playthroughs for a WAD.
pub fn get_times_completed(conn: &Connection, wad_id: i64) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM playthroughs WHERE wad_id = ?1 AND completed_at IS NOT NULL",
        [wad_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Count completed playthroughs for multiple WADs efficiently.
pub fn get_times_completed_batch(
    conn: &Connection,
    wad_ids: &[i64],
) -> Result<HashMap<i64, i64>> {
    let result = batch_query_i64(
        conn,
        wad_ids,
        "SELECT wad_id, COUNT(*) as times_completed \
         FROM playthroughs WHERE completed_at IS NOT NULL \
         AND wad_id IN ({placeholders}) GROUP BY wad_id",
        "times_completed",
    )?;
    Ok(wad_ids
        .iter()
        .map(|&id| (id, *result.get(&id).unwrap_or(&0)))
        .collect())
}

/// Derive the play state for a WAD from its playthrough records.
pub fn derive_play_state(conn: &Connection, wad_id: i64) -> Result<PlayState> {
    // Check for any playthrough at all
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM playthroughs WHERE wad_id = ?1",
        [wad_id],
        |row| row.get(0),
    )?;

    if total == 0 {
        return Ok(PlayState::Unplayed);
    }

    // Check if there's an active (incomplete) playthrough
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM playthroughs WHERE wad_id = ?1 AND completed_at IS NULL",
        [wad_id],
        |row| row.get(0),
    )?;

    if active > 0 {
        return Ok(PlayState::Started);
    }

    // All playthroughs are completed
    Ok(PlayState::Completed)
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
    use crate::db::sessions::start_session;
    use crate::db::wads::{add_wad, get_wad, NewWad};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection) -> i64 {
        add_wad(
            conn,
            &NewWad::new("Test WAD", SourceType::Local).author("Test Author"),
        )
        .unwrap()
    }

    #[test]
    fn test_start_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        assert!(pt_id > 0);

        let pt = get_playthrough(&conn, pt_id).unwrap().unwrap();
        assert_eq!(pt.wad_id, wad_id);
        assert!(pt.completed_at.is_none());

        // WAD should now be play_state=started
        let wad = get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.play_state, "started");
        assert_eq!(wad.status, "playing");
    }

    #[test]
    fn test_complete_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        let completed = complete_playthrough(&conn, pt_id, Some("{\"stats\":1}"), Some("GG")).unwrap();
        assert!(completed);

        let pt = get_playthrough(&conn, pt_id).unwrap().unwrap();
        assert!(pt.completed_at.is_some());
        assert_eq!(pt.stats_snapshot.as_deref(), Some("{\"stats\":1}"));
        assert_eq!(pt.notes.as_deref(), Some("GG"));

        // WAD should be play_state=completed
        let wad = get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.play_state, "completed");
        assert_eq!(wad.status, "finished");

        // Should also have created a backward-compat wad_completions record
        let comp_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wad_completions WHERE wad_id = ?1",
                [wad_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(comp_count, 1);
    }

    #[test]
    fn test_complete_already_completed_returns_false() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        assert!(complete_playthrough(&conn, pt_id, None, None).unwrap());
        // Completing again should return false
        assert!(!complete_playthrough(&conn, pt_id, None, None).unwrap());
    }

    #[test]
    fn test_get_active_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        // No active playthrough initially
        assert!(get_active_playthrough(&conn, wad_id).unwrap().is_none());

        // Start one
        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        let active = get_active_playthrough(&conn, wad_id).unwrap().unwrap();
        assert_eq!(active.id, pt_id);

        // Complete it
        complete_playthrough(&conn, pt_id, None, None).unwrap();
        assert!(get_active_playthrough(&conn, wad_id).unwrap().is_none());
    }

    #[test]
    fn test_get_playthroughs() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let pt1 = start_playthrough(&conn, wad_id).unwrap();
        complete_playthrough(&conn, pt1, None, None).unwrap();
        let _pt2 = start_playthrough(&conn, wad_id).unwrap();

        let pts = get_playthroughs(&conn, wad_id).unwrap();
        assert_eq!(pts.len(), 2);
        // Most recent first
        assert!(pts[0].is_active());
        assert!(!pts[1].is_active());
    }

    #[test]
    fn test_delete_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let pt_id = start_playthrough(&conn, wad_id).unwrap();

        assert!(delete_playthrough(&conn, pt_id).unwrap());
        assert!(get_playthrough(&conn, pt_id).unwrap().is_none());
        assert!(!delete_playthrough(&conn, pt_id).unwrap());
    }

    #[test]
    fn test_ensure_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        // First call creates a new playthrough
        let pt1 = ensure_playthrough(&conn, wad_id).unwrap();
        // Second call returns the same one
        let pt2 = ensure_playthrough(&conn, wad_id).unwrap();
        assert_eq!(pt1, pt2);

        // Complete it, then ensure creates a new one
        complete_playthrough(&conn, pt1, None, None).unwrap();
        let pt3 = ensure_playthrough(&conn, wad_id).unwrap();
        assert_ne!(pt1, pt3);
    }

    #[test]
    fn test_times_completed() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        assert_eq!(get_times_completed(&conn, wad_id).unwrap(), 0);

        let pt1 = start_playthrough(&conn, wad_id).unwrap();
        complete_playthrough(&conn, pt1, None, None).unwrap();
        assert_eq!(get_times_completed(&conn, wad_id).unwrap(), 1);

        let pt2 = start_playthrough(&conn, wad_id).unwrap();
        complete_playthrough(&conn, pt2, None, None).unwrap();
        assert_eq!(get_times_completed(&conn, wad_id).unwrap(), 2);
    }

    #[test]
    fn test_times_completed_batch() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_test_wad(&conn);

        let pt = start_playthrough(&conn, id1).unwrap();
        complete_playthrough(&conn, pt, None, None).unwrap();

        let batch = get_times_completed_batch(&conn, &[id1, id2]).unwrap();
        assert_eq!(batch[&id1], 1);
        assert_eq!(batch[&id2], 0);
    }

    #[test]
    fn test_derive_play_state() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        // No playthroughs → unplayed
        assert_eq!(derive_play_state(&conn, wad_id).unwrap(), PlayState::Unplayed);

        // Active playthrough → started
        let pt = start_playthrough(&conn, wad_id).unwrap();
        assert_eq!(derive_play_state(&conn, wad_id).unwrap(), PlayState::Started);

        // Completed → completed
        complete_playthrough(&conn, pt, None, None).unwrap();
        assert_eq!(derive_play_state(&conn, wad_id).unwrap(), PlayState::Completed);

        // New active playthrough → started again
        start_playthrough(&conn, wad_id).unwrap();
        assert_eq!(derive_play_state(&conn, wad_id).unwrap(), PlayState::Started);
    }

    #[test]
    fn test_session_links_to_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let pt_id = start_playthrough(&conn, wad_id).unwrap();

        // Start a session — playthrough_id column exists on sessions table
        let session_id = start_session(&conn, wad_id, None).unwrap();

        // We can manually link it
        conn.execute(
            "UPDATE sessions SET playthrough_id = ?1 WHERE id = ?2",
            rusqlite::params![pt_id, session_id],
        )
        .unwrap();

        let linked: i64 = conn
            .query_row(
                "SELECT playthrough_id FROM sessions WHERE id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked, pt_id);
    }
}
