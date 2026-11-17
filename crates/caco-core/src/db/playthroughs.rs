use chrono::Utc;
use rusqlite::Connection;

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
/// Also sets the WAD's `status` to `in-progress`.
pub fn start_playthrough(conn: &Connection, wad_id: i64) -> Result<i64> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO playthroughs (wad_id, started_at) VALUES (?1, ?2)",
        rusqlite::params![wad_id, now],
    )?;
    let pt_id = conn.last_insert_rowid();

    conn.execute(
        "UPDATE wads SET status = 'in-progress', updated_at = ?1
         WHERE id = ?2",
        rusqlite::params![now, wad_id],
    )?;

    Ok(pt_id)
}

/// Complete an active playthrough.
///
/// Also sets the WAD's `status` to `completed`.
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
        "UPDATE wads SET status = 'completed', updated_at = ?1
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
    let mut stmt =
        conn.prepare("SELECT * FROM playthroughs WHERE wad_id = ?1 ORDER BY started_at DESC")?;
    let rows = stmt
        .query_map([wad_id], PlaythroughRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::open_memory;
    use crate::db::models::{SourceType, Status};
    use crate::db::schema::init_db;
    use crate::db::wads::{NewWad, add_wad, get_wad};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn only_playthrough(conn: &Connection, wad_id: i64) -> PlaythroughRecord {
        let mut all = get_playthroughs(conn, wad_id).unwrap();
        assert_eq!(all.len(), 1);
        all.remove(0)
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

        let pt = only_playthrough(&conn, wad_id);
        assert_eq!(pt.wad_id, wad_id);
        assert!(pt.completed_at.is_none());

        // WAD should now be in-progress
        let wad = get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, Status::InProgress);
    }

    #[test]
    fn test_complete_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        let completed =
            complete_playthrough(&conn, pt_id, Some("{\"stats\":1}"), Some("GG")).unwrap();
        assert!(completed);

        let pt = only_playthrough(&conn, wad_id);
        assert!(pt.completed_at.is_some());
        assert_eq!(pt.stats_snapshot.as_deref(), Some("{\"stats\":1}"));
        assert_eq!(pt.notes.as_deref(), Some("GG"));

        // WAD should be completed
        let wad = get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, Status::Completed);

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
}
