//! A playthrough is one go at a WAD: it starts at the first map exit, or when the user
//! starts the WAD again, and ends when the WAD is finished or started again.
//!
//! Times beaten is counted from `wad_completions` and from nothing else (#84). A
//! completion can be a finish caco never saw, entered in Map Stats, so it cannot be a
//! playthrough with an end, and a playthrough can end without a finish: starting an
//! abandoned WAD again ends the one it was abandoned in. So a playthrough says when it
//! ended, and a completion says which playthrough it finished, if any. The log still calls
//! `ended_at` `completed_at`, the name it had when every playthrough that ended had been
//! finished.
//!
//! An abandoned WAD keeps its playthrough open, since playing it again carries on where it
//! was left. A completed one never has one open: [`record_completion`] ends it, and
//! [`reconcile`] ends it wherever that was missed.

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};

use super::models::Status;
use super::wads::{WadUpdate, update_wad};
use crate::{Error, Result};

// ---------------------------------------------------------------------------
// PlaythroughRecord
// ---------------------------------------------------------------------------

/// A single playthrough of a WAD.
#[derive(Debug, Clone)]
pub struct PlaythroughRecord {
    pub id: i64,
    pub wad_id: i64,
    pub started_at: String,
    /// When it was finished or replaced. Whether it was finished is whether a completion
    /// names it.
    pub ended_at: Option<String>,
    /// What the WAD's stats had got to when it ended.
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
            ended_at: row.get("ended_at")?,
            stats_snapshot: row.get("stats_snapshot")?,
            notes: row.get("notes")?,
            created_at: row.get("created_at")?,
        })
    }

    /// Whether this playthrough is still going.
    pub fn is_active(&self) -> bool {
        self.ended_at.is_none()
    }
}

// ---------------------------------------------------------------------------
// Starting and ending
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

/// Count a finish of a WAD, and end the playthrough it finished, in the transaction that
/// makes the WAD completed.
///
/// Only [`update_wad`] calls this, as a status turns to completed, so detection after a
/// session and the user's say-so leave the same records. They did not: detection went
/// through a function of its own that ended the playthrough, the user's say-so only added
/// the completion, and a WAD finished by hand could not be started again (#84).
pub(super) fn record_completion(tx: &Connection, wad_id: i64) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let finished = end_open(tx, wad_id, &now)?;
    tx.execute(
        "INSERT INTO wad_completions (wad_id, completed_at, stats_snapshot, playthrough_id) \
         SELECT id, ?2, stats_snapshot, ?3 FROM wads WHERE id = ?1",
        rusqlite::params![wad_id, now, finished],
    )?;
    Ok(())
}

/// Start a WAD again: end the playthrough it may still have open, and start another with
/// the WAD's stats put aside, so that what the last one did is not taken for progress in
/// this one. Returns the new playthrough.
///
/// Refused while the WAD is in progress, which would end a playthrough partway. What that
/// one got to stays on it.
pub fn start_over(conn: &Connection, wad_id: i64) -> Result<i64> {
    super::connection::with_transaction(conn, |tx| {
        let (title, status): (String, String) = tx
            .query_row(
                "SELECT title, status FROM wads WHERE id = ?1 AND deleted_at IS NULL",
                [wad_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(Error::WadNotFound(wad_id))?;
        if status == Status::InProgress.as_str() {
            return Err(Error::StillInProgress(title));
        }
        end_open(tx, wad_id, &Utc::now().to_rfc3339())?;
        let started = start_playthrough(tx, wad_id)?;
        update_wad(
            tx,
            wad_id,
            &WadUpdate::new().set_text("stats_snapshot", None),
        )?;
        Ok(started)
    })
}

/// End whatever playthrough of a WAD is still open, as of `at`, with the stats the WAD had
/// got to. Returns the one that ended. Only one is ever open, but should two be, neither is
/// left behind.
fn end_open(tx: &Connection, wad_id: i64, at: &str) -> Result<Option<i64>> {
    let newest = get_active_playthrough(tx, wad_id)?.map(|pt| pt.id);
    tx.execute(
        "UPDATE playthroughs SET ended_at = ?2, \
             stats_snapshot = (SELECT stats_snapshot FROM wads WHERE id = ?1) \
         WHERE wad_id = ?1 AND ended_at IS NULL",
        rusqlite::params![wad_id, at],
    )?;
    Ok(newest)
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Get the active (in-progress) playthrough for a WAD, if any.
pub fn get_active_playthrough(conn: &Connection, wad_id: i64) -> Result<Option<PlaythroughRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM playthroughs WHERE wad_id = ?1 AND ended_at IS NULL
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
// Putting right what was missed
// ---------------------------------------------------------------------------

/// Do what finishing a WAD should have done to its playthrough, wherever it was missed: by
/// a WAD finished by hand before #84, by an older build on another machine, which still
/// does that, or by two machines at once, one finishing the WAD as the other starts it
/// again. Returns how many rows it changed.
///
/// A playthrough a completed WAD has open ends as of the latest completion since it began,
/// which is linked to it, or if there is none, as of the last session played in it: the
/// latest, since one before it was followed by more of the same playthrough. A
/// completion recorded at the very moment a playthrough of its WAD ended, which is how
/// detection recorded one before completions named their playthrough, is linked to it.
///
/// An edit, so it is logged, where a migration would not be: a library rebuilt from the
/// logs, or one hearing from an older build after the upgrade, would otherwise be left as it
/// was. Every value it writes comes from what travels, so machines that each do it agree.
/// It reads first, and takes no write lock when there is nothing to do.
pub fn reconcile(conn: &Connection) -> Result<usize> {
    if open_on_completed(conn)?.is_empty() && unlinked_finishes(conn)?.is_empty() {
        return Ok(0);
    }
    super::connection::with_transaction(conn, |tx| {
        // Pairs first, so that a completion that finished one playthrough is not taken for
        // the end of another.
        let mut changed = 0;
        for (completion, playthrough) in unlinked_finishes(tx)? {
            changed += tx.execute(
                "UPDATE wad_completions SET playthrough_id = ?2 WHERE id = ?1 \
                 AND NOT EXISTS (SELECT 1 FROM wad_completions WHERE playthrough_id = ?2)",
                [completion, playthrough],
            )?;
        }
        for (playthrough, wad_id, started_at) in open_on_completed(tx)? {
            changed += end_as_finished(tx, playthrough, wad_id, &started_at)?;
        }
        Ok(changed)
    })
}

/// Playthroughs a completed WAD still has open, oldest first, as `(id, wad_id, started_at)`.
fn open_on_completed(conn: &Connection) -> Result<Vec<(i64, i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.wad_id, p.started_at FROM playthroughs p \
         JOIN wads w ON w.id = p.wad_id \
         WHERE p.ended_at IS NULL AND w.status = 'completed' \
         ORDER BY p.wad_id, julianday(p.started_at), p.uid",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Completions naming no playthrough that were recorded at the moment one of their WAD's
/// ended, and that playthrough, which no completion names yet: `(completion, playthrough)`.
fn unlinked_finishes(conn: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT id, playthrough FROM ( \
             SELECT c.id, c.uid, ( \
                 SELECT p.id FROM playthroughs p \
                 WHERE p.wad_id = c.wad_id AND p.ended_at = c.completed_at \
                   AND NOT EXISTS (SELECT 1 FROM wad_completions o WHERE o.playthrough_id = p.id) \
                 ORDER BY p.uid LIMIT 1 \
             ) AS playthrough \
             FROM wad_completions c WHERE c.playthrough_id IS NULL \
         ) WHERE playthrough IS NOT NULL ORDER BY uid",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// End a completed WAD's open playthrough as of the finish that should have ended it: the
/// one that made the WAD completed, which is the latest.
fn end_as_finished(
    tx: &Connection,
    playthrough: i64,
    wad_id: i64,
    started_at: &str,
) -> Result<usize> {
    let finish: Option<(i64, String, Option<String>)> = tx
        .query_row(
            "SELECT id, completed_at, stats_snapshot FROM wad_completions \
             WHERE wad_id = ?1 AND playthrough_id IS NULL \
               AND julianday(completed_at) >= julianday(?2) \
             ORDER BY julianday(completed_at) DESC, uid LIMIT 1",
            rusqlite::params![wad_id, started_at],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((completion, at, stats)) = finish else {
        // Completed, with nothing since it began to say when: two machines at once, or a
        // completion deleted in Map Stats.
        return Ok(tx.execute(
            "UPDATE playthroughs SET \
                 ended_at = COALESCE( \
                     (SELECT MAX(ended_at) FROM sessions WHERE playthrough_id = ?1), started_at), \
                 stats_snapshot = (SELECT stats_snapshot FROM wads WHERE id = ?2) \
             WHERE id = ?1",
            [playthrough, wad_id],
        )?);
    };
    tx.execute(
        "UPDATE playthroughs SET ended_at = ?2, stats_snapshot = ?3 WHERE id = ?1",
        rusqlite::params![playthrough, at, stats],
    )?;
    tx.execute(
        "UPDATE wad_completions SET playthrough_id = ?2 WHERE id = ?1",
        [completion, playthrough],
    )?;
    Ok(2)
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
    use crate::db::sessions::{add_wad_completion, get_wad_completions, start_session};
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

    fn set_status(conn: &Connection, wad_id: i64, status: Status) {
        update_wad(conn, wad_id, &WadUpdate::new().set_status(status)).unwrap();
    }

    fn set_stats(conn: &Connection, wad_id: i64, stats: &str) {
        let update = WadUpdate::new().set_text("stats_snapshot", Some(stats.to_string()));
        update_wad(conn, wad_id, &update).unwrap();
    }

    /// Which playthrough each of a WAD's completions names, newest first.
    fn completions_name(conn: &Connection, wad_id: i64) -> Vec<Option<i64>> {
        let mut stmt = conn
            .prepare(
                "SELECT playthrough_id FROM wad_completions WHERE wad_id = ?1 \
                 ORDER BY completed_at DESC",
            )
            .unwrap();
        stmt.query_map([wad_id], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// What a WAD finished by hand before #84 was left as: a completion, by itself.
    fn finished_the_old_way(conn: &Connection, wad_id: i64, at: &str) -> i64 {
        conn.execute(
            "UPDATE wads SET status = 'completed' WHERE id = ?1",
            [wad_id],
        )
        .unwrap();
        add_wad_completion(conn, wad_id, Some("{\"maps\":[]}"), None, Some(at)).unwrap()
    }

    #[test]
    fn test_start_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        assert!(pt_id > 0);

        let pt = only_playthrough(&conn, wad_id);
        assert_eq!(pt.wad_id, wad_id);
        assert!(pt.ended_at.is_none());

        // WAD should now be in-progress
        let wad = get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, Status::InProgress);
    }

    #[test]
    fn test_finishing_a_wad_ends_its_playthrough_however_it_was_said() {
        // Detection sets the status as the user does, so one test is both (#84).
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        set_stats(&conn, wad_id, "{\"stats\":1}");

        set_status(&conn, wad_id, Status::Completed);

        let pt = only_playthrough(&conn, wad_id);
        assert!(!pt.is_active());
        assert_eq!(pt.stats_snapshot.as_deref(), Some("{\"stats\":1}"));
        let completions = get_wad_completions(&conn, wad_id).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(
            pt.ended_at.as_deref(),
            Some(completions[0].completed_at.as_str())
        );
        assert_eq!(
            completions[0].stats_snapshot.as_deref(),
            Some("{\"stats\":1}")
        );
        assert_eq!(completions_name(&conn, wad_id), [Some(pt_id)]);
        assert_eq!(reconcile(&conn).unwrap(), 0, "nothing left to put right");
    }

    #[test]
    fn test_finishing_a_wad_never_played_counts_without_a_playthrough() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        set_status(&conn, wad_id, Status::Completed);

        assert!(get_playthroughs(&conn, wad_id).unwrap().is_empty());
        assert_eq!(completions_name(&conn, wad_id), [None]);
    }

    #[test]
    fn test_saying_completed_again_counts_nothing_more() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        start_playthrough(&conn, wad_id).unwrap();
        set_status(&conn, wad_id, Status::Completed);

        set_status(&conn, wad_id, Status::Completed);

        assert_eq!(get_wad_completions(&conn, wad_id).unwrap().len(), 1);
    }

    #[test]
    fn test_abandoning_a_wad_leaves_its_playthrough_to_come_back_to() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let pt_id = start_playthrough(&conn, wad_id).unwrap();

        set_status(&conn, wad_id, Status::Abandoned);

        assert_eq!(
            get_active_playthrough(&conn, wad_id).unwrap().unwrap().id,
            pt_id
        );
        assert!(get_wad_completions(&conn, wad_id).unwrap().is_empty());
        assert_eq!(reconcile(&conn).unwrap(), 0);
    }

    #[test]
    fn test_starting_over_after_finishing() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let first = start_playthrough(&conn, wad_id).unwrap();
        set_stats(&conn, wad_id, "{\"first\":1}");
        set_status(&conn, wad_id, Status::Completed);

        let second = start_over(&conn, wad_id).unwrap();

        let all = get_playthroughs(&conn, wad_id).unwrap();
        assert_eq!(
            all.iter().map(|pt| pt.id).collect::<Vec<_>>(),
            [second, first]
        );
        assert!(all[0].is_active());
        assert_eq!(all[1].stats_snapshot.as_deref(), Some("{\"first\":1}"));
        let wad = get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, Status::InProgress);
        assert_eq!(wad.stats_snapshot, None);
        assert_eq!(get_wad_completions(&conn, wad_id).unwrap().len(), 1);
    }

    #[test]
    fn test_starting_over_after_abandoning_ends_the_last_playthrough_unfinished() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let first = start_playthrough(&conn, wad_id).unwrap();
        set_stats(&conn, wad_id, "{\"partway\":1}");
        set_status(&conn, wad_id, Status::Abandoned);

        let second = start_over(&conn, wad_id).unwrap();

        assert_eq!(
            get_active_playthrough(&conn, wad_id).unwrap().unwrap().id,
            second
        );
        let all = get_playthroughs(&conn, wad_id).unwrap();
        let ended = all.iter().find(|pt| pt.id == first).unwrap();
        assert!(!ended.is_active());
        assert_eq!(ended.stats_snapshot.as_deref(), Some("{\"partway\":1}"));
        assert!(
            get_wad_completions(&conn, wad_id).unwrap().is_empty(),
            "giving up is not a finish"
        );
    }

    #[test]
    fn test_starting_over_is_refused_partway() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let pt_id = start_playthrough(&conn, wad_id).unwrap();

        let refused = start_over(&conn, wad_id);

        assert!(matches!(refused, Err(Error::StillInProgress(title)) if title == "Test WAD"));
        assert_eq!(only_playthrough(&conn, wad_id).id, pt_id);
        assert!(only_playthrough(&conn, wad_id).is_active());
    }

    #[test]
    fn test_a_wad_finished_by_hand_before_84_can_be_started_again() {
        // Cybernights, 2026-11-17: finished by hand, and its playthrough left open.
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        start_playthrough(&conn, wad_id).unwrap();
        finished_the_old_way(&conn, wad_id, "2099-11-17T12:41:07+00:00");

        assert!(start_over(&conn, wad_id).is_ok());
        assert_eq!(get_playthroughs(&conn, wad_id).unwrap().len(), 2);
    }

    #[test]
    fn test_reconcile_ends_what_a_finish_by_hand_left_open() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        // A finish from before it began, one it carried on after, and the one that made the
        // WAD completed, in the spelling SQLite's CURRENT_TIMESTAMP gave it.
        add_wad_completion(&conn, wad_id, None, None, Some("2000-01-01T00:00:00+00:00")).unwrap();
        add_wad_completion(&conn, wad_id, None, None, Some("2099-11-17T09:00:00+00:00")).unwrap();
        let finish = finished_the_old_way(&conn, wad_id, "2099-11-17 12:41:07");

        assert_eq!(reconcile(&conn).unwrap(), 2);

        let pt = only_playthrough(&conn, wad_id);
        assert_eq!(pt.ended_at.as_deref(), Some("2099-11-17 12:41:07"));
        assert_eq!(pt.stats_snapshot.as_deref(), Some("{\"maps\":[]}"));
        let linked: Option<i64> = conn
            .query_row(
                "SELECT playthrough_id FROM wad_completions WHERE id = ?1",
                [finish],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked, Some(pt_id));
        assert_eq!(reconcile(&conn).unwrap(), 0, "once is enough");
    }

    #[test]
    fn test_reconcile_with_no_finish_ends_at_the_last_session() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        let session = start_session(&conn, wad_id, None, None).unwrap();
        conn.execute(
            "UPDATE sessions SET playthrough_id = ?1, ended_at = '2099-01-02T03:04:05+00:00' \
             WHERE id = ?2",
            [pt_id, session],
        )
        .unwrap();
        conn.execute(
            "UPDATE wads SET status = 'completed' WHERE id = ?1",
            [wad_id],
        )
        .unwrap();

        assert_eq!(reconcile(&conn).unwrap(), 1);

        let pt = only_playthrough(&conn, wad_id);
        assert_eq!(pt.ended_at.as_deref(), Some("2099-01-02T03:04:05+00:00"));
        assert!(get_wad_completions(&conn, wad_id).unwrap().is_empty());
    }

    #[test]
    fn test_reconcile_links_a_finish_detection_recorded_before_84() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let pt_id = start_playthrough(&conn, wad_id).unwrap();
        let at = "2026-04-27T06:01:25.651865297+00:00";
        conn.execute(
            "UPDATE playthroughs SET ended_at = ?1 WHERE id = ?2",
            rusqlite::params![at, pt_id],
        )
        .unwrap();
        add_wad_completion(&conn, wad_id, None, None, Some(at)).unwrap();
        add_wad_completion(&conn, wad_id, None, None, Some("2026-01-01T00:00:00+00:00")).unwrap();

        assert_eq!(reconcile(&conn).unwrap(), 1);

        assert_eq!(completions_name(&conn, wad_id), [Some(pt_id), None]);
    }

    #[test]
    fn test_reconcile_leaves_an_abandoned_playthrough_open() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        start_playthrough(&conn, wad_id).unwrap();
        conn.execute(
            "UPDATE wads SET status = 'abandoned' WHERE id = ?1",
            [wad_id],
        )
        .unwrap();

        assert_eq!(reconcile(&conn).unwrap(), 0);
        assert!(only_playthrough(&conn, wad_id).is_active());
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

        // Finish it
        set_status(&conn, wad_id, Status::Completed);
        assert!(get_active_playthrough(&conn, wad_id).unwrap().is_none());
    }

    #[test]
    fn test_get_playthroughs() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        start_playthrough(&conn, wad_id).unwrap();
        set_status(&conn, wad_id, Status::Completed);
        start_over(&conn, wad_id).unwrap();

        let pts = get_playthroughs(&conn, wad_id).unwrap();
        assert_eq!(pts.len(), 2);
        // Most recent first
        assert!(pts[0].is_active());
        assert!(!pts[1].is_active());
    }
}
