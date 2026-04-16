use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Connection;

use super::connection::{SQLITE_MAX_VARS, attach_tags, batch_query_i64, batch_query_string};
use super::models::WadRecord;
use crate::Result;

/// Minimum duration in seconds for a session to count as a real play session.
/// Sessions shorter than this are still recorded but excluded from stats and listings.
const MIN_SESSION_SECONDS: i64 = 300;

// =============================================================================
// Play Sessions
// =============================================================================

/// Start a play session. Returns the session ID.
pub fn start_session(conn: &Connection, wad_id: i64, sourceport: Option<&str>) -> Result<i64> {
    conn.execute(
        "INSERT INTO sessions (wad_id, started_at, sourceport) VALUES (?1, ?2, ?3)",
        rusqlite::params![wad_id, Utc::now().to_rfc3339(), sourceport],
    )?;
    let session_id = conn.last_insert_rowid();
    if session_id <= 0 {
        return Err(crate::Error::Database(
            rusqlite::Error::StatementChangedRows(0),
        ));
    }
    Ok(session_id)
}

/// End a play session.
pub fn end_session(
    conn: &Connection,
    session_id: i64,
    notes: Option<&str>,
    exit_code: Option<i32>,
) -> Result<()> {
    let ended_at = Utc::now();

    let started_at: Option<String> = conn
        .query_row(
            "SELECT started_at FROM sessions WHERE id = ?",
            [session_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(start_str) = started_at {
        let started = chrono::DateTime::parse_from_rfc3339(&start_str).unwrap_or_else(|_| {
            // Fall back to parsing ISO format without timezone
            chrono::DateTime::parse_from_rfc3339(&format!("{start_str}+00:00"))
                .unwrap_or(ended_at.into())
        });
        let duration = (ended_at - started.with_timezone(&chrono::Utc)).num_seconds();

        conn.execute(
            "UPDATE sessions SET ended_at = ?, duration_seconds = ?, notes = ?, exit_code = ? WHERE id = ?",
            rusqlite::params![ended_at.to_rfc3339(), duration, notes, exit_code, session_id],
        )?;
    }

    Ok(())
}

/// Attach before/after stats snapshots to a session record.
pub fn update_session_stats(
    conn: &Connection,
    session_id: i64,
    stats_before: Option<&str>,
    stats_after: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET stats_before = ?, stats_after = ? WHERE id = ?",
        rusqlite::params![stats_before, stats_after, session_id],
    )?;
    Ok(())
}

/// Attach a recorded demo file path to a session record.
pub fn update_session_demo(conn: &Connection, session_id: i64, demo_file: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET demo_file = ? WHERE id = ?",
        rusqlite::params![demo_file, session_id],
    )?;
    Ok(())
}

/// Session record from the database.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: i64,
    pub wad_id: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub sourceport: Option<String>,
    pub notes: Option<String>,
    pub exit_code: Option<i32>,
    pub demo_file: Option<String>,
    pub stats_before: Option<String>,
    pub stats_after: Option<String>,
}

impl SessionRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            wad_id: row.get("wad_id")?,
            started_at: row.get("started_at")?,
            ended_at: row.get("ended_at")?,
            duration_seconds: row.get("duration_seconds")?,
            sourceport: row.get("sourceport")?,
            notes: row.get("notes")?,
            exit_code: row.get("exit_code")?,
            demo_file: row.get("demo_file")?,
            stats_before: row.get("stats_before")?,
            stats_after: row.get("stats_after")?,
        })
    }
}

/// Get all play sessions for a WAD (excludes short non-play sessions).
pub fn get_sessions(conn: &Connection, wad_id: i64) -> Result<Vec<SessionRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM sessions WHERE wad_id = ? \
         AND COALESCE(duration_seconds, 0) >= ? \
         ORDER BY started_at DESC",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![wad_id, MIN_SESSION_SECONDS],
            SessionRecord::from_row,
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get total playtime in seconds for a WAD.
pub fn get_total_playtime(conn: &Connection, wad_id: i64) -> Result<i64> {
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(duration_seconds), 0) FROM sessions \
         WHERE wad_id = ? AND COALESCE(duration_seconds, 0) >= ?",
        rusqlite::params![wad_id, MIN_SESSION_SECONDS],
        |row| row.get(0),
    )?;
    Ok(total)
}

// =============================================================================
// Batch Stats
// =============================================================================

/// Get total playtime for multiple WADs efficiently. Returns `{wad_id: seconds}`.
pub fn get_total_playtime_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, i64>> {
    let result = batch_query_i64(
        conn,
        wad_ids,
        &format!(
            "SELECT wad_id, COALESCE(SUM(duration_seconds), 0) as total \
             FROM sessions WHERE wad_id IN ({{placeholders}}) \
             AND COALESCE(duration_seconds, 0) >= {MIN_SESSION_SECONDS} GROUP BY wad_id"
        ),
        "total",
    )?;
    Ok(wad_ids
        .iter()
        .map(|&id| (id, *result.get(&id).unwrap_or(&0)))
        .collect())
}

/// Get the last played timestamp for a WAD.
pub fn get_last_played(conn: &Connection, wad_id: i64) -> Result<Option<String>> {
    let result: Option<String> = conn
        .query_row(
            "SELECT started_at FROM sessions WHERE wad_id = ? \
             AND COALESCE(duration_seconds, 0) >= ? \
             ORDER BY started_at DESC LIMIT 1",
            rusqlite::params![wad_id, MIN_SESSION_SECONDS],
            |row| row.get(0),
        )
        .ok();
    Ok(result)
}

/// Get last played timestamp for multiple WADs efficiently.
pub fn get_last_played_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, String>> {
    batch_query_string(
        conn,
        wad_ids,
        &format!(
            "SELECT wad_id, MAX(started_at) as last_played \
             FROM sessions WHERE wad_id IN ({{placeholders}}) \
             AND COALESCE(duration_seconds, 0) >= {MIN_SESSION_SECONDS} GROUP BY wad_id"
        ),
        "last_played",
    )
}

/// Get the most recently played WAD across the entire library.
pub fn get_most_recently_played(conn: &Connection) -> Result<Option<WadRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT wads.* FROM wads \
         JOIN sessions ON sessions.wad_id = wads.id \
         WHERE wads.deleted_at IS NULL \
         AND COALESCE(sessions.duration_seconds, 0) >= {MIN_SESSION_SECONDS} \
         ORDER BY sessions.started_at DESC \
         LIMIT 1"
    ))?;
    match stmt.query_row([], WadRecord::from_row) {
        Ok(mut wad) => {
            attach_tags(conn, &mut wad)?;
            Ok(Some(wad))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get session count for multiple WADs efficiently.
pub fn get_session_count_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, i64>> {
    batch_query_i64(
        conn,
        wad_ids,
        &format!(
            "SELECT wad_id, COUNT(*) as count \
             FROM sessions WHERE wad_id IN ({{placeholders}}) \
             AND COALESCE(duration_seconds, 0) >= {MIN_SESSION_SECONDS} GROUP BY wad_id"
        ),
        "count",
    )
}

/// Combined stats for a WAD (used in list views).
#[derive(Debug, Clone, Default)]
pub struct WadStats {
    pub playtime: i64,
    pub last_played: Option<String>,
    pub session_count: i64,
    pub times_beaten: i64,
}

/// Get all stats for multiple WADs in 2 queries on 1 connection.
///
/// Replaces 4 separate batch functions for list view loading.
pub fn get_wad_stats_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, WadStats>> {
    if wad_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut session_stats: HashMap<i64, (i64, Option<String>, i64)> = HashMap::new();
    let mut beaten_map: HashMap<i64, i64> = HashMap::new();

    for chunk in wad_ids.chunks(SQLITE_MAX_VARS) {
        let placeholders: String = (0..chunk.len())
            .map(|i| if i > 0 { ",?" } else { "?" })
            .collect();

        // Query 1: session aggregates (playtime + last_played + count)
        {
            let sql = format!(
                "SELECT wad_id, \
                     COALESCE(SUM(duration_seconds), 0) as playtime, \
                     MAX(started_at) as last_played, \
                     COUNT(*) as session_count \
                 FROM sessions WHERE wad_id IN ({placeholders}) \
                 AND COALESCE(duration_seconds, 0) >= {MIN_SESSION_SECONDS} \
                 GROUP BY wad_id"
            );
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|id| id as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>("wad_id")?,
                    row.get::<_, i64>("playtime")?,
                    row.get::<_, Option<String>>("last_played")?,
                    row.get::<_, i64>("session_count")?,
                ))
            })?;
            for row in rows {
                let (wad_id, playtime, last_played, session_count) = row?;
                session_stats.insert(wad_id, (playtime, last_played, session_count));
            }
        }

        // Query 2: completions (times_beaten)
        {
            let sql = format!(
                "SELECT wad_id, COUNT(*) as times_beaten \
                 FROM wad_completions WHERE wad_id IN ({placeholders}) GROUP BY wad_id"
            );
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|id| id as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>("wad_id")?,
                    row.get::<_, i64>("times_beaten")?,
                ))
            })?;
            for row in rows {
                let (wad_id, times_beaten) = row?;
                beaten_map.insert(wad_id, times_beaten);
            }
        }
    }

    let mut result = HashMap::new();
    for &wid in wad_ids {
        let (playtime, last_played, session_count) =
            session_stats.remove(&wid).unwrap_or((0, None, 0));
        result.insert(
            wid,
            WadStats {
                playtime,
                last_played,
                session_count,
                times_beaten: beaten_map.get(&wid).copied().unwrap_or(0),
            },
        );
    }
    Ok(result)
}

/// Get deletion-relevant stats for a single WAD.
pub fn get_wad_stats(conn: &Connection, wad_id: i64) -> Result<(i64, i64)> {
    let session_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE wad_id = ? \
         AND COALESCE(duration_seconds, 0) >= ?",
        rusqlite::params![wad_id, MIN_SESSION_SECONDS],
        |row| row.get(0),
    )?;
    let total_playtime: i64 = conn.query_row(
        "SELECT COALESCE(SUM(duration_seconds), 0) FROM sessions \
         WHERE wad_id = ? AND COALESCE(duration_seconds, 0) >= ?",
        rusqlite::params![wad_id, MIN_SESSION_SECONDS],
        |row| row.get(0),
    )?;
    Ok((session_count, total_playtime))
}

// =============================================================================
// WAD Completions (Times Beaten)
// =============================================================================

/// Completion record from the database.
#[derive(Debug, Clone)]
pub struct CompletionRecord {
    pub id: i64,
    pub wad_id: i64,
    pub completed_at: String,
    pub stats_snapshot: Option<String>,
    pub notes: Option<String>,
}

impl CompletionRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            wad_id: row.get("wad_id")?,
            completed_at: row.get("completed_at")?,
            stats_snapshot: row.get("stats_snapshot")?,
            notes: row.get("notes")?,
        })
    }
}

/// Record a WAD completion. Returns completion ID.
pub fn add_wad_completion(
    conn: &Connection,
    wad_id: i64,
    stats_snapshot: Option<&str>,
    notes: Option<&str>,
    completed_at: Option<&str>,
) -> Result<i64> {
    if let Some(ts) = completed_at {
        conn.execute(
            "INSERT INTO wad_completions (wad_id, completed_at, stats_snapshot, notes) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![wad_id, ts, stats_snapshot, notes],
        )?;
    } else {
        conn.execute(
            "INSERT INTO wad_completions (wad_id, stats_snapshot, notes) VALUES (?1, ?2, ?3)",
            rusqlite::params![wad_id, stats_snapshot, notes],
        )?;
    }
    let id = conn.last_insert_rowid();
    if id <= 0 {
        return Err(crate::Error::Database(
            rusqlite::Error::StatementChangedRows(0),
        ));
    }
    Ok(id)
}

/// Get all completion records for a WAD.
pub fn get_wad_completions(conn: &Connection, wad_id: i64) -> Result<Vec<CompletionRecord>> {
    let mut stmt =
        conn.prepare("SELECT * FROM wad_completions WHERE wad_id = ? ORDER BY completed_at DESC")?;
    let rows = stmt
        .query_map([wad_id], CompletionRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Update a completion record's stats_snapshot and/or notes.
pub fn update_wad_completion(
    conn: &Connection,
    completion_id: i64,
    stats_snapshot: Option<&str>,
    notes: Option<&str>,
) -> Result<bool> {
    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ss) = stats_snapshot {
        updates.push("stats_snapshot = ?");
        params.push(Box::new(ss.to_string()));
    }
    if let Some(n) = notes {
        updates.push("notes = ?");
        params.push(Box::new(n.to_string()));
    }
    if updates.is_empty() {
        return Ok(false);
    }

    params.push(Box::new(completion_id));
    let sql = format!(
        "UPDATE wad_completions SET {} WHERE id = ?",
        updates.join(", ")
    );
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let count = conn.execute(&sql, param_refs.as_slice())?;
    Ok(count > 0)
}

/// Delete a specific completion record. Returns true if deleted.
pub fn delete_wad_completion(conn: &Connection, completion_id: i64) -> Result<bool> {
    let count = conn.execute("DELETE FROM wad_completions WHERE id = ?", [completion_id])?;
    Ok(count > 0)
}

/// Delete a completion record by exact completed_at match.
pub fn delete_wad_completion_by_timestamp(
    conn: &Connection,
    wad_id: i64,
    timestamp: &str,
) -> Result<bool> {
    let count = conn.execute(
        "DELETE FROM wad_completions WHERE wad_id = ? AND completed_at = ?",
        rusqlite::params![wad_id, timestamp],
    )?;
    Ok(count > 0)
}

/// Find a completion by timestamp prefix match.
pub fn find_completion_by_timestamp(
    conn: &Connection,
    wad_id: i64,
    timestamp: &str,
) -> Result<Option<CompletionRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM wad_completions \
         WHERE wad_id = ? AND completed_at LIKE ? || '%' \
         ORDER BY completed_at DESC LIMIT 1",
    )?;
    match stmt.query_row(
        rusqlite::params![wad_id, timestamp],
        CompletionRecord::from_row,
    ) {
        Ok(rec) => Ok(Some(rec)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Set the completion count for a WAD to a specific number.
pub fn set_wad_completion_count(conn: &Connection, wad_id: i64, count: i64) -> Result<()> {
    let current: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wad_completions WHERE wad_id = ?",
        [wad_id],
        |row| row.get(0),
    )?;

    if count < current {
        let to_delete = current - count;
        conn.execute(
            "DELETE FROM wad_completions WHERE id IN (\
                 SELECT id FROM wad_completions WHERE wad_id = ? \
                 ORDER BY completed_at ASC LIMIT ?\
             )",
            rusqlite::params![wad_id, to_delete],
        )?;
    } else if count > current {
        let to_add = count - current;
        let mut stmt = conn.prepare("INSERT INTO wad_completions (wad_id, notes) VALUES (?, ?)")?;
        for _ in 0..to_add {
            stmt.execute(rusqlite::params![wad_id, "Manually added"])?;
        }
    }

    Ok(())
}

/// Get count of completions for a WAD.
pub fn get_times_beaten(conn: &Connection, wad_id: i64) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wad_completions WHERE wad_id = ?",
        [wad_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Get times beaten for multiple WADs efficiently.
pub fn get_times_beaten_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, i64>> {
    let result = batch_query_i64(
        conn,
        wad_ids,
        "SELECT wad_id, COUNT(*) as times_beaten \
         FROM wad_completions WHERE wad_id IN ({placeholders}) GROUP BY wad_id",
        "times_beaten",
    )?;
    Ok(wad_ids
        .iter()
        .map(|&id| (id, *result.get(&id).unwrap_or(&0)))
        .collect())
}

// =============================================================================
// Cache Management
// =============================================================================

/// Get all WADs with cached files (non-null cached_path, not deleted).
pub fn get_cached_wads(conn: &Connection) -> Result<Vec<WadRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM wads WHERE cached_path IS NOT NULL AND deleted_at IS NULL ORDER BY title",
    )?;
    let mut results: Vec<WadRecord> = stmt
        .query_map([], WadRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for wad in &mut results {
        attach_tags(conn, wad)?;
    }
    Ok(results)
}

/// Clear the cached_path for a WAD. Returns true if updated.
pub fn clear_cached_path(conn: &Connection, wad_id: i64) -> Result<bool> {
    let count = conn.execute("UPDATE wads SET cached_path = NULL WHERE id = ?", [wad_id])?;
    Ok(count > 0)
}

/// Clear cached_path for all WADs. Returns count of WADs updated.
pub fn clear_all_cached_paths(conn: &Connection) -> Result<usize> {
    let count = conn.execute(
        "UPDATE wads SET cached_path = NULL WHERE cached_path IS NOT NULL",
        [],
    )?;
    Ok(count)
}

/// Find a WAD by the filename portion of its cached_path.
pub fn get_wad_by_cached_filename(conn: &Connection, filename: &str) -> Result<Option<WadRecord>> {
    let mut stmt =
        conn.prepare("SELECT * FROM wads WHERE cached_path LIKE ? AND deleted_at IS NULL")?;
    match stmt.query_row(
        rusqlite::params![format!("%/{filename}")],
        WadRecord::from_row,
    ) {
        Ok(mut wad) => {
            attach_tags(conn, &mut wad)?;
            Ok(Some(wad))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

// =============================================================================
// Library Statistics
// =============================================================================

/// Library-wide statistics snapshot.
#[derive(Debug, Clone, Default)]
pub struct StatsSnapshot {
    pub total_wads: i64,
    pub total_sessions: i64,
    pub total_playtime: i64,
    pub wads_with_sessions: i64,
    pub wads_by_status: HashMap<String, i64>,
    pub played_wads: i64,
    pub completed_wads: i64,
    pub completion_rate: f64,
    pub total_completions: i64,
    pub activity: Vec<ActivityPeriod>,
}

/// A single time period of activity.
#[derive(Debug, Clone)]
pub struct ActivityPeriod {
    pub period: String,
    pub wad_count: i64,
    pub session_count: i64,
    pub total_playtime: i64,
}

/// Get a complete library statistics snapshot.
pub fn get_stats_snapshot(conn: &Connection, period: &str) -> Result<StatsSnapshot> {
    let stats = get_library_stats(conn)?;
    let completion = get_completion_rate(conn)?;
    let activity = get_wads_played_by_period(conn, period)?;

    Ok(StatsSnapshot {
        total_wads: stats.0,
        total_sessions: stats.1,
        total_playtime: stats.2,
        wads_with_sessions: stats.3,
        wads_by_status: stats.4,
        played_wads: completion.0,
        completed_wads: completion.1,
        completion_rate: completion.2,
        total_completions: completion.3,
        activity,
    })
}

/// Library overview stats tuple.
type LibraryOverview = (i64, i64, i64, i64, HashMap<String, i64>);

/// Get library-wide overview statistics.
///
/// Returns (total_wads, total_sessions, total_playtime, wads_with_sessions, wads_by_status).
fn get_library_stats(conn: &Connection) -> Result<LibraryOverview> {
    let total_wads: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wads WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;

    let total_sessions: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE COALESCE(duration_seconds, 0) >= ?",
        [MIN_SESSION_SECONDS],
        |row| row.get(0),
    )?;

    let total_playtime: i64 = conn.query_row(
        "SELECT COALESCE(SUM(duration_seconds), 0) FROM sessions \
         WHERE COALESCE(duration_seconds, 0) >= ?",
        [MIN_SESSION_SECONDS],
        |row| row.get(0),
    )?;

    let wads_with_sessions: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT wad_id) FROM sessions \
         WHERE COALESCE(duration_seconds, 0) >= ?",
        [MIN_SESSION_SECONDS],
        |row| row.get(0),
    )?;

    let mut wads_by_status = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT status, COUNT(*) as count FROM wads WHERE deleted_at IS NULL GROUP BY status",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (status, count) = row?;
        wads_by_status.insert(status, count);
    }

    Ok((
        total_wads,
        total_sessions,
        total_playtime,
        wads_with_sessions,
        wads_by_status,
    ))
}

/// Get activity grouped by time period.
pub fn get_wads_played_by_period(conn: &Connection, period: &str) -> Result<Vec<ActivityPeriod>> {
    let strftime = match period {
        "year" => "'%Y'",
        _ => "'%Y-%m'",
    };

    let sql = format!(
        "SELECT strftime({strftime}, started_at) as period, \
             COUNT(DISTINCT wad_id) as wad_count, \
             COUNT(*) as session_count, \
             COALESCE(SUM(duration_seconds), 0) as total_playtime \
         FROM sessions \
         WHERE COALESCE(duration_seconds, 0) >= {MIN_SESSION_SECONDS} \
         GROUP BY strftime({strftime}, started_at) \
         ORDER BY period DESC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ActivityPeriod {
                period: row.get("period")?,
                wad_count: row.get("wad_count")?,
                session_count: row.get("session_count")?,
                total_playtime: row.get("total_playtime")?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get completion statistics.
///
/// Returns (played_wads, completed_wads, completion_rate, total_completions).
fn get_completion_rate(conn: &Connection) -> Result<(i64, i64, f64, i64)> {
    let played_wads: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT wad_id) FROM sessions \
         WHERE COALESCE(duration_seconds, 0) >= ?",
        [MIN_SESSION_SECONDS],
        |row| row.get(0),
    )?;

    let completed_wads: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT wads.id) FROM wads \
         JOIN sessions ON sessions.wad_id = wads.id \
         WHERE wads.status = 'completed' AND wads.deleted_at IS NULL \
         AND COALESCE(sessions.duration_seconds, 0) >= ?",
        [MIN_SESSION_SECONDS],
        |row| row.get(0),
    )?;

    let total_completions: i64 =
        conn.query_row("SELECT COUNT(*) FROM wad_completions", [], |row| row.get(0))?;

    let completion_rate = if played_wads > 0 {
        completed_wads as f64 / played_wads as f64
    } else {
        0.0
    };

    Ok((
        played_wads,
        completed_wads,
        completion_rate,
        total_completions,
    ))
}

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

    fn add_test_wad(conn: &Connection) -> i64 {
        add_wad(
            conn,
            &NewWad::new("Test WAD", SourceType::Local).author("Test Author"),
        )
        .unwrap()
    }

    #[test]
    fn test_start_and_end_session() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let session_id = start_session(&conn, wad_id, Some("dsda-doom")).unwrap();
        assert!(session_id > 0);

        end_session(&conn, session_id, Some("Test notes"), None).unwrap();

        // Query directly to test session mechanics (get_sessions filters short sessions)
        let mut stmt = conn.prepare("SELECT * FROM sessions WHERE id = ?").unwrap();
        let session = stmt
            .query_row([session_id], SessionRecord::from_row)
            .unwrap();
        assert_eq!(session.sourceport.as_deref(), Some("dsda-doom"));
        assert_eq!(session.notes.as_deref(), Some("Test notes"));
        assert!(session.duration_seconds.is_some());
    }

    #[test]
    fn test_get_total_playtime() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        assert_eq!(get_total_playtime(&conn, wad_id).unwrap(), 0);

        // Insert a session with known duration
        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, ended_at, duration_seconds) \
             VALUES (?1, '2024-01-01T00:00:00', '2024-01-01T01:00:00', 3600)",
            [wad_id],
        )
        .unwrap();

        assert_eq!(get_total_playtime(&conn, wad_id).unwrap(), 3600);
    }

    #[test]
    fn test_batch_playtime() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_test_wad(&conn);

        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, duration_seconds) VALUES (?1, '2024-01-01', 1000)",
            [id1],
        ).unwrap();
        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, duration_seconds) VALUES (?1, '2024-01-02', 2000)",
            [id1],
        ).unwrap();

        let batch = get_total_playtime_batch(&conn, &[id1, id2]).unwrap();
        assert_eq!(batch[&id1], 3000);
        assert_eq!(batch[&id2], 0);
    }

    #[test]
    fn test_wad_completions() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let comp_id = add_wad_completion(&conn, wad_id, None, Some("First clear"), None).unwrap();
        assert!(comp_id > 0);

        let completions = get_wad_completions(&conn, wad_id).unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].notes.as_deref(), Some("First clear"));

        assert_eq!(get_times_beaten(&conn, wad_id).unwrap(), 1);
    }

    #[test]
    fn test_delete_completion() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let comp_id = add_wad_completion(&conn, wad_id, None, None, None).unwrap();
        assert!(delete_wad_completion(&conn, comp_id).unwrap());
        assert_eq!(get_times_beaten(&conn, wad_id).unwrap(), 0);
    }

    #[test]
    fn test_set_completion_count() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        set_wad_completion_count(&conn, wad_id, 3).unwrap();
        assert_eq!(get_times_beaten(&conn, wad_id).unwrap(), 3);

        set_wad_completion_count(&conn, wad_id, 1).unwrap();
        assert_eq!(get_times_beaten(&conn, wad_id).unwrap(), 1);

        set_wad_completion_count(&conn, wad_id, 0).unwrap();
        assert_eq!(get_times_beaten(&conn, wad_id).unwrap(), 0);
    }

    #[test]
    fn test_update_session_stats() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let session_id = start_session(&conn, wad_id, None).unwrap();

        update_session_stats(
            &conn,
            session_id,
            Some("{\"before\": true}"),
            Some("{\"after\": true}"),
        )
        .unwrap();

        let mut stmt = conn.prepare("SELECT * FROM sessions WHERE id = ?").unwrap();
        let session = stmt
            .query_row([session_id], SessionRecord::from_row)
            .unwrap();
        assert_eq!(session.stats_before.as_deref(), Some("{\"before\": true}"));
        assert_eq!(session.stats_after.as_deref(), Some("{\"after\": true}"));
    }

    #[test]
    fn test_update_session_demo() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let session_id = start_session(&conn, wad_id, None).unwrap();

        update_session_demo(&conn, session_id, "/path/to/demo.lmp").unwrap();

        let mut stmt = conn.prepare("SELECT * FROM sessions WHERE id = ?").unwrap();
        let session = stmt
            .query_row([session_id], SessionRecord::from_row)
            .unwrap();
        assert_eq!(session.demo_file.as_deref(), Some("/path/to/demo.lmp"));
    }

    #[test]
    fn test_times_beaten_batch() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_test_wad(&conn);

        add_wad_completion(&conn, id1, None, None, None).unwrap();
        add_wad_completion(&conn, id1, None, None, None).unwrap();

        let batch = get_times_beaten_batch(&conn, &[id1, id2]).unwrap();
        assert_eq!(batch[&id1], 2);
        assert_eq!(batch[&id2], 0);
    }

    #[test]
    fn test_wad_stats_batch() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_test_wad(&conn);

        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, duration_seconds) VALUES (?1, '2024-01-01', 1000)",
            [id1],
        ).unwrap();
        add_wad_completion(&conn, id1, None, None, None).unwrap();

        let batch = get_wad_stats_batch(&conn, &[id1, id2]).unwrap();
        assert_eq!(batch[&id1].playtime, 1000);
        assert_eq!(batch[&id1].session_count, 1);
        assert_eq!(batch[&id1].times_beaten, 1);
        assert!(batch[&id1].last_played.is_some());

        assert_eq!(batch[&id2].playtime, 0);
        assert_eq!(batch[&id2].session_count, 0);
        assert_eq!(batch[&id2].times_beaten, 0);
        assert!(batch[&id2].last_played.is_none());
    }

    #[test]
    fn test_cache_management() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Cached WAD", SourceType::Idgames).cached_path("/cache/test.wad"),
        )
        .unwrap();

        let cached = get_cached_wads(&conn).unwrap();
        assert_eq!(cached.len(), 1);

        assert!(clear_cached_path(&conn, id).unwrap());
        assert!(get_cached_wads(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_clear_all_cached_paths() {
        let conn = setup();
        add_wad(
            &conn,
            &NewWad::new("A", SourceType::Local).cached_path("/a"),
        )
        .unwrap();
        add_wad(
            &conn,
            &NewWad::new("B", SourceType::Local).cached_path("/b"),
        )
        .unwrap();

        let count = clear_all_cached_paths(&conn).unwrap();
        assert_eq!(count, 2);
        assert!(get_cached_wads(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_get_wad_by_cached_filename() {
        let conn = setup();
        add_wad(
            &conn,
            &NewWad::new("Test", SourceType::Local).cached_path("/cache/test.wad"),
        )
        .unwrap();

        let found = get_wad_by_cached_filename(&conn, "test.wad").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "Test");

        let not_found = get_wad_by_cached_filename(&conn, "other.wad").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn test_library_stats() {
        let conn = setup();
        let id = add_test_wad(&conn);

        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, duration_seconds) VALUES (?1, '2024-01-01', 1000)",
            [id],
        ).unwrap();

        let snapshot = get_stats_snapshot(&conn, "month").unwrap();
        assert_eq!(snapshot.total_wads, 1);
        assert_eq!(snapshot.total_sessions, 1);
        assert_eq!(snapshot.total_playtime, 1000);
        assert_eq!(snapshot.wads_with_sessions, 1);
        assert_eq!(snapshot.played_wads, 1);
        assert!(!snapshot.activity.is_empty());
    }

    #[test]
    fn test_find_completion_by_timestamp() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        add_wad_completion(&conn, wad_id, None, None, Some("2024-06-15T18:30:00")).unwrap();

        let found = find_completion_by_timestamp(&conn, wad_id, "2024-06-15").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().completed_at, "2024-06-15T18:30:00");

        let not_found = find_completion_by_timestamp(&conn, wad_id, "2024-07-01").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn test_most_recently_played() {
        let conn = setup();
        let id1 = add_test_wad(&conn);
        let id2 = add_wad(&conn, &NewWad::new("Second WAD", SourceType::Local)).unwrap();

        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, duration_seconds) VALUES (?1, '2024-01-01T00:00:00', 300)",
            [id1],
        ).unwrap();
        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, duration_seconds) VALUES (?1, '2024-06-01T00:00:00', 600)",
            [id2],
        ).unwrap();

        let recent = get_most_recently_played(&conn).unwrap();
        assert!(recent.is_some());
        assert_eq!(recent.unwrap().title, "Second WAD");
    }

    #[test]
    fn test_update_wad_completion() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let comp_id = add_wad_completion(&conn, wad_id, None, None, None).unwrap();

        assert!(
            update_wad_completion(&conn, comp_id, Some("{\"stats\": true}"), Some("Updated"))
                .unwrap()
        );

        let completions = get_wad_completions(&conn, wad_id).unwrap();
        assert_eq!(
            completions[0].stats_snapshot.as_deref(),
            Some("{\"stats\": true}")
        );
        assert_eq!(completions[0].notes.as_deref(), Some("Updated"));
    }

    #[test]
    fn test_session_exit_code() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let session_id = start_session(&conn, wad_id, None).unwrap();

        end_session(&conn, session_id, None, Some(139)).unwrap();

        let mut stmt = conn.prepare("SELECT * FROM sessions WHERE id = ?").unwrap();
        let session = stmt
            .query_row([session_id], SessionRecord::from_row)
            .unwrap();
        assert_eq!(session.exit_code, Some(139));
    }
}
