use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Connection;

use super::connection::{SQLITE_MAX_VARS, attach_tags};
use super::models::{Status, WAD_COLUMNS, WadRecord};
use crate::Result;

/// Minimum duration in seconds for a session to count as a real play session.
/// Sessions shorter than this are still recorded but excluded from stats and listings.
const MIN_SESSION_SECONDS: i64 = 300;

// =============================================================================
// Play Sessions
// =============================================================================

/// Start a play session. Returns the session ID.
///
/// `content_sha256` is what the sourceport is about to load (see
/// `crate::identity`), so that the next launch can tell whether the file has
/// changed since. `None` when caco could not read the file to hash it.
pub fn start_session(
    conn: &Connection,
    wad_id: i64,
    sourceport: Option<&str>,
    content_sha256: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO sessions (wad_id, started_at, sourceport, content_sha256) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![wad_id, Utc::now().to_rfc3339(), sourceport, content_sha256],
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
    pub stats_before: Option<String>,
    pub stats_after: Option<String>,
    /// What the sourceport loaded, by `crate::identity`'s content hash.
    pub content_sha256: Option<String>,
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
            stats_before: row.get("stats_before")?,
            stats_after: row.get("stats_after")?,
            content_sha256: row.get("content_sha256")?,
        })
    }
}

fn session_has_map_progress(session: &SessionRecord, fallback_before: Option<&str>) -> bool {
    let Some(after_json) = session.stats_after.as_deref() else {
        return false;
    };
    let Ok(after) = crate::wad_stats::stats_from_json(after_json) else {
        return false;
    };
    let before = session
        .stats_before
        .as_deref()
        .or(fallback_before)
        .and_then(|json| crate::wad_stats::stats_from_json(json).ok());

    !crate::wad_stats::compute_stats_delta(before.as_ref(), &after)
        .maps_played
        .is_empty()
}

/// Get all play sessions for a WAD.
///
/// Excludes short launch/quit sessions, but keeps short sessions when stats
/// prove actual map progress. Fast ZDoom maps can legitimately complete under
/// `MIN_SESSION_SECONDS`, and hiding those makes session history look stale.
pub fn get_sessions(conn: &Connection, wad_id: i64) -> Result<Vec<SessionRecord>> {
    let mut stmt =
        conn.prepare("SELECT * FROM sessions WHERE wad_id = ? ORDER BY started_at DESC")?;
    let rows = stmt
        .query_map([wad_id], SessionRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(idx, session)| {
            let fallback_before = rows
                .get(idx + 1)
                .and_then(|prev| prev.stats_after.as_deref());
            if session.duration_seconds.unwrap_or(0) >= MIN_SESSION_SECONDS
                || session_has_map_progress(session, fallback_before)
            {
                Some(session.clone())
            } else {
                None
            }
        })
        .collect())
}

// =============================================================================
// Batch Stats
// =============================================================================

/// Combined stats for a WAD (used in list views).
#[derive(Debug, Clone, Default)]
pub struct WadStats {
    pub playtime: i64,
    pub last_played: Option<String>,
    pub session_count: i64,
    pub times_beaten: i64,
}

/// Get all stats for multiple WADs in 2 queries on 1 connection.
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

/// Update a completion record's fields.
///
/// Each `Option<Option<&str>>` argument follows the same tri-state convention:
/// `None` leaves the column untouched, `Some(None)` writes SQL NULL, and
/// `Some(Some(v))` writes the value. `completed_at` is `Option<&str>` because
/// NULL is not a valid value for that column.
pub fn update_wad_completion(
    conn: &Connection,
    completion_id: i64,
    stats_snapshot: Option<Option<&str>>,
    notes: Option<Option<&str>>,
    completed_at: Option<&str>,
) -> Result<bool> {
    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ss) = stats_snapshot {
        updates.push("stats_snapshot = ?");
        params.push(Box::new(ss.map(|s| s.to_string())));
    }
    if let Some(n) = notes {
        updates.push("notes = ?");
        params.push(Box::new(n.map(|s| s.to_string())));
    }
    if let Some(ts) = completed_at {
        updates.push("completed_at = ?");
        params.push(Box::new(ts.to_string()));
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

// =============================================================================
// Cache Management
// =============================================================================

/// Get all WADs with cached files (non-null cached_path, not deleted).
pub fn get_cached_wads(conn: &Connection) -> Result<Vec<WadRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {WAD_COLUMNS} FROM wads \
         WHERE cached_path IS NOT NULL AND deleted_at IS NULL ORDER BY title"
    ))?;
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

// =============================================================================
// Library Statistics
// =============================================================================

/// Library-wide statistics, for the Stats view.
///
/// One definition of each thing, and all of it about the library as it stands, so a WAD in
/// the Trash and its sessions don't count. "Completed" is a WAD's status and nothing else, the
/// same number the library's filter shows: this used to report a second figure, of completed
/// WADs that also had a recorded session, and a rate worked out from that, which disagreed
/// with the first for everything finished before caco was there to see it.
#[derive(Debug, Clone, Default)]
pub struct StatsSnapshot {
    pub total_wads: i64,
    pub total_sessions: i64,
    pub total_playtime: i64,
    pub wads_by_status: HashMap<String, i64>,
    /// How many times anything has been finished, counting playing it again.
    pub total_completions: i64,
    /// Newest month first.
    pub activity: Vec<ActivityPeriod>,
    /// `("2026-03", times finished)`, newest month first.
    pub completions_by_month: Vec<(String, i64)>,
    /// By playtime.
    pub top_authors: Vec<Tally>,
    /// By playtime, under an executable's name, wherever it was run from.
    pub sourceports: Vec<Tally>,
    /// By how many WADs use each.
    pub iwads: Vec<Tally>,
    /// By how many WADs use each.
    pub complevels: Vec<Tally>,
}

impl StatsSnapshot {
    /// How many WADs have this status.
    pub fn with_status(&self, status: Status) -> i64 {
        self.wads_by_status
            .get(status.as_str())
            .copied()
            .unwrap_or(0)
    }
}

/// A single time period of activity.
#[derive(Debug, Clone)]
pub struct ActivityPeriod {
    pub period: String,
    pub wad_count: i64,
    pub session_count: i64,
    pub total_playtime: i64,
}

/// One line of a breakdown: a name, and how much of whatever it is counting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    pub name: String,
    pub value: i64,
}

/// How many lines a breakdown has.
const TOP: usize = 5;

/// What a breakdown calls what has no value for it.
const NOT_SET: &str = "Not set";

/// Sessions that count, of WADs that count.
const COUNTED_SESSIONS: &str = "FROM sessions JOIN wads ON wads.id = sessions.wad_id \
     WHERE wads.deleted_at IS NULL AND COALESCE(sessions.duration_seconds, 0) >= ?1";

/// Get a complete library statistics snapshot.
pub fn get_stats_snapshot(conn: &Connection) -> Result<StatsSnapshot> {
    let total_wads = conn.query_row(
        "SELECT COUNT(*) FROM wads WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    let (total_sessions, total_playtime) = conn.query_row(
        &format!("SELECT COUNT(*), COALESCE(SUM(duration_seconds), 0) {COUNTED_SESSIONS}"),
        [MIN_SESSION_SECONDS],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let total_completions = conn.query_row(
        "SELECT COUNT(*) FROM wad_completions c JOIN wads ON wads.id = c.wad_id \
         WHERE wads.deleted_at IS NULL",
        [],
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

    Ok(StatsSnapshot {
        total_wads,
        total_sessions,
        total_playtime,
        wads_by_status,
        total_completions,
        activity: activity_by_month(conn)?,
        completions_by_month: completions_by_month(conn)?,
        top_authors: top_authors(conn)?,
        sourceports: sourceports(conn)?,
        iwads: iwads(conn)?,
        complevels: complevels(conn)?,
    })
}

/// Play by month, newest first. A month with nothing in it has no row.
fn activity_by_month(conn: &Connection) -> Result<Vec<ActivityPeriod>> {
    let sql = format!(
        "SELECT strftime('%Y-%m', started_at) as period, \
             COUNT(DISTINCT wad_id) as wad_count, \
             COUNT(*) as session_count, \
             COALESCE(SUM(duration_seconds), 0) as total_playtime \
         {COUNTED_SESSIONS} \
         GROUP BY period \
         ORDER BY period DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([MIN_SESSION_SECONDS], |row| {
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

/// How many times something was finished, by month, newest first.
fn completions_by_month(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m', c.completed_at) as period, COUNT(*) \
         FROM wad_completions c JOIN wads ON wads.id = c.wad_id \
         WHERE wads.deleted_at IS NULL AND period IS NOT NULL \
         GROUP BY period \
         ORDER BY period DESC",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Add up what shares a name, and keep the largest few. Largest first, and by name after
/// that, so the same library always reads the same.
fn top_of(rows: impl IntoIterator<Item = (String, i64)>) -> Vec<Tally> {
    let mut totals: HashMap<String, i64> = HashMap::new();
    for (name, value) in rows {
        *totals.entry(name).or_default() += value;
    }
    let mut tallies: Vec<Tally> = totals
        .into_iter()
        .map(|(name, value)| Tally { name, value })
        .collect();
    tallies.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.name.cmp(&b.name)));
    tallies.truncate(TOP);
    tallies
}

/// A file's name, wherever it is.
fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Who has been played the most. "Various" is what a community project says, and not anybody.
fn top_authors(conn: &Connection) -> Result<Vec<Tally>> {
    let sql = format!(
        "SELECT wads.author, SUM(sessions.duration_seconds) {COUNTED_SESSIONS} \
         AND TRIM(COALESCE(wads.author, '')) != '' AND LOWER(TRIM(wads.author)) != 'various' \
         GROUP BY wads.author"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([MIN_SESSION_SECONDS], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<Vec<(String, i64)>, _>>()?;
    Ok(top_of(rows))
}

/// What has been played with. A session records the executable it ran, so the same sourceport
/// is several paths, and a build of it in caco's own folder is another.
fn sourceports(conn: &Connection) -> Result<Vec<Tally>> {
    let sql = format!(
        "SELECT sessions.sourceport, SUM(sessions.duration_seconds) {COUNTED_SESSIONS} \
         AND TRIM(COALESCE(sessions.sourceport, '')) != '' \
         GROUP BY sessions.sourceport"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([MIN_SESSION_SECONDS], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(top_of(rows.into_iter().map(|(path, seconds)| {
        (base_name(&path).to_string(), seconds)
    })))
}

/// Which IWADs the library's WADs need. The same one can be spelled `doom2` or `DOOM2.WAD`.
fn iwads(conn: &Connection) -> Result<Vec<Tally>> {
    let mut stmt =
        conn.prepare("SELECT custom_iwad, COUNT(*) FROM wads WHERE deleted_at IS NULL GROUP BY 1")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(top_of(rows.into_iter().map(|(iwad, count)| {
        let name = iwad
            .as_deref()
            .map(|iwad| base_name(iwad.trim()).to_lowercase())
            .filter(|iwad| !iwad.is_empty())
            .map(|iwad| iwad.strip_suffix(".wad").unwrap_or(&iwad).to_string())
            .unwrap_or_else(|| NOT_SET.to_string());
        (name, count)
    })))
}

/// Which complevels the library's WADs are for, by the name a person knows them by.
fn complevels(conn: &Connection) -> Result<Vec<Tally>> {
    let mut stmt =
        conn.prepare("SELECT complevel, COUNT(*) FROM wads WHERE deleted_at IS NULL GROUP BY 1")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, Option<i32>>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(top_of(rows.into_iter().map(|(complevel, count)| {
        let name = match complevel {
            Some(_) => crate::complevel::complevel_name_string(complevel),
            None => NOT_SET.to_string(),
        };
        (name, count)
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::open_memory;
    use crate::db::models::SourceType;
    use crate::db::models::Status;
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

    fn stats_json(maps: &[(&str, i32)]) -> String {
        let maps: Vec<crate::wad_stats::MapStats> = maps
            .iter()
            .map(|(lump, exits)| crate::wad_stats::MapStats {
                lump: (*lump).to_string(),
                total_exits: *exits,
                best_skill: if *exits > 0 { 4 } else { 0 },
                best_time: if *exits > 0 { 3500 } else { -1 },
                time_secs: if *exits > 0 { 100.0 } else { -1.0 },
                kills: 0,
                total_kills: -1,
                items: 0,
                total_items: -1,
                secrets: 0,
                total_secrets: -1,
                episode: 0,
                map_num: 0,
                best_max_time: -1,
                best_nm_time: -1,
                cumulative_kills: 0,
                total_time_secs: -1.0,
            })
            .collect();
        let stats = crate::wad_stats::WadStats {
            format: "stats_txt".to_string(),
            maps,
            version: 1,
            header_total_kills: 0,
        };
        crate::wad_stats::stats_to_json(&stats).unwrap()
    }

    #[test]
    fn test_start_and_end_session() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let session_id = start_session(&conn, wad_id, Some("dsda-doom"), Some("3f2a9c1e")).unwrap();
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
        // What it played, which the next launch compares against.
        assert_eq!(session.content_sha256.as_deref(), Some("3f2a9c1e"));
    }

    #[test]
    fn test_get_wad_stats_counts_sessions_and_playtime() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        assert_eq!(get_wad_stats(&conn, wad_id).unwrap(), (0, 0));

        // Insert a session with known duration
        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, ended_at, duration_seconds) \
             VALUES (?1, '2024-01-01T00:00:00', '2024-01-01T01:00:00', 3600)",
            [wad_id],
        )
        .unwrap();

        assert_eq!(get_wad_stats(&conn, wad_id).unwrap(), (1, 3600));
    }

    #[test]
    fn test_get_sessions_excludes_short_session_without_progress() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, ended_at, duration_seconds) \
             VALUES (?1, '2024-01-01T00:00:00', '2024-01-01T00:01:00', 60)",
            [wad_id],
        )
        .unwrap();

        assert!(get_sessions(&conn, wad_id).unwrap().is_empty());
    }

    #[test]
    fn test_get_sessions_includes_short_session_with_map_progress() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let before = stats_json(&[("MAP01", 1)]);
        let after = stats_json(&[("MAP01", 1), ("MAP02", 1)]);

        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, ended_at, duration_seconds, stats_before, stats_after) \
             VALUES (?1, '2024-01-01T00:00:00', '2024-01-01T00:01:00', 60, ?2, ?3)",
            rusqlite::params![wad_id, before, after],
        )
        .unwrap();

        let sessions = get_sessions(&conn, wad_id).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].duration_seconds, Some(60));
    }

    #[test]
    fn test_get_sessions_uses_previous_after_for_short_missing_before() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let before = stats_json(&[("MAP01", 1), ("MAP02", 1), ("MAP03", 1), ("MAP04", 1)]);
        let after = stats_json(&[
            ("MAP01", 1),
            ("MAP02", 1),
            ("MAP03", 1),
            ("MAP04", 1),
            ("MAP05", 1),
        ]);

        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, ended_at, duration_seconds, stats_after) \
             VALUES (?1, '2024-01-01T00:00:00', '2024-01-01T00:45:00', 2700, ?2)",
            rusqlite::params![wad_id, before],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, ended_at, duration_seconds, stats_after) \
             VALUES (?1, '2024-01-02T00:00:00', '2024-01-02T00:01:00', 60, ?2)",
            rusqlite::params![wad_id, after],
        )
        .unwrap();

        let sessions = get_sessions(&conn, wad_id).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].started_at, "2024-01-02T00:00:00");
        assert_eq!(sessions[0].duration_seconds, Some(60));
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
    }

    #[test]
    fn test_delete_completion() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        let comp_id = add_wad_completion(&conn, wad_id, None, None, None).unwrap();
        assert!(delete_wad_completion(&conn, comp_id).unwrap());
        assert!(get_wad_completions(&conn, wad_id).unwrap().is_empty());
    }

    #[test]
    fn test_update_session_stats() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let session_id = start_session(&conn, wad_id, None, None).unwrap();

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

    /// Evicting from the cache used to leave `availability` reading `cached`,
    /// because `clear_cached_path` nulls the column with raw SQL and the old
    /// stored column was only maintained inside `update_wad`. Deriving it on
    /// read is what fixes this; the test pins the behaviour, not the mechanism.
    #[test]
    fn test_eviction_downgrades_availability() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Evicted", SourceType::Idgames)
                .source_url("https://example.com/w.zip")
                .cached_path("/cache/w.zip"),
        )
        .unwrap();
        assert_eq!(
            crate::db::get_wad(&conn, id, false)
                .unwrap()
                .unwrap()
                .availability(),
            crate::db::Availability::Cached
        );

        clear_cached_path(&conn, id).unwrap();

        assert_eq!(
            crate::db::get_wad(&conn, id, false)
                .unwrap()
                .unwrap()
                .availability(),
            crate::db::Availability::Downloadable
        );
    }

    #[test]
    fn test_library_stats() {
        let conn = setup();
        let id = add_test_wad(&conn);

        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, duration_seconds) VALUES (?1, '2024-01-01', 1000)",
            [id],
        ).unwrap();

        let snapshot = get_stats_snapshot(&conn).unwrap();
        assert_eq!(snapshot.total_wads, 1);
        assert_eq!(snapshot.total_sessions, 1);
        assert_eq!(snapshot.total_playtime, 1000);
        assert_eq!(snapshot.activity.len(), 1);
        assert_eq!(snapshot.activity[0].period, "2024-01");
    }

    /// The number the library's filter shows, whether or not caco saw it being played. These
    /// used to be two numbers, with a rate worked out from the wrong one.
    #[test]
    fn test_completed_means_the_status_and_nothing_else() {
        let conn = setup();
        let add = |title: &str, status: Status| {
            add_wad(&conn, &NewWad::new(title, SourceType::Local).status(status)).unwrap()
        };
        let played = add("Played and finished", Status::Completed);
        add("Finished before caco", Status::Completed);
        add("Not started", Status::Unplayed);
        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, duration_seconds) \
             VALUES (?1, '2024-01-01', 1000)",
            [played],
        )
        .unwrap();
        add_wad_completion(&conn, played, None, None, Some("2024-01-02T00:00:00+00:00")).unwrap();
        add_wad_completion(&conn, played, None, None, Some("2024-03-02T00:00:00+00:00")).unwrap();

        let snapshot = get_stats_snapshot(&conn).unwrap();

        assert_eq!(snapshot.with_status(Status::Completed), 2);
        assert_eq!(snapshot.with_status(Status::Abandoned), 0);
        assert_eq!(
            snapshot.total_completions, 2,
            "playing it again counts here"
        );
        assert_eq!(
            snapshot.completions_by_month,
            [("2024-03".to_string(), 1), ("2024-01".to_string(), 1)]
        );
    }

    #[test]
    fn test_what_is_in_the_trash_does_not_count() {
        let conn = setup();
        let kept = add_wad(&conn, &NewWad::new("Kept", SourceType::Local)).unwrap();
        let trashed = add_wad(
            &conn,
            &NewWad::new("Trashed", SourceType::Local).author("Somebody"),
        )
        .unwrap();
        for id in [kept, trashed] {
            conn.execute(
                "INSERT INTO sessions (wad_id, started_at, duration_seconds, sourceport) \
                 VALUES (?1, '2024-01-01', 1000, 'dsda-doom')",
                [id],
            )
            .unwrap();
            add_wad_completion(&conn, id, None, None, Some("2024-01-02T00:00:00+00:00")).unwrap();
        }
        crate::db::wads::delete_wad(&conn, trashed, false).unwrap();

        let snapshot = get_stats_snapshot(&conn).unwrap();

        assert_eq!(snapshot.total_wads, 1);
        assert_eq!(snapshot.total_sessions, 1);
        assert_eq!(snapshot.total_playtime, 1000);
        assert_eq!(snapshot.total_completions, 1);
        assert_eq!(snapshot.activity[0].wad_count, 1);
        assert_eq!(snapshot.completions_by_month[0].1, 1);
        assert_eq!(snapshot.sourceports[0].value, 1000);
        assert!(snapshot.top_authors.is_empty());
    }

    #[test]
    fn test_the_same_sourceport_is_one_line_wherever_it_was_run_from() {
        let conn = setup();
        let id = add_test_wad(&conn);
        for (path, seconds) in [
            ("/usr/bin/nyan-doom", 1000),
            (
                "/home/me/.cache/caco/sourceports/nyan-doom/v1/bin/nyan-doom",
                500,
            ),
            ("C:\\Doom\\uzdoom", 600),
        ] {
            conn.execute(
                "INSERT INTO sessions (wad_id, started_at, duration_seconds, sourceport) \
                 VALUES (?1, '2024-01-01', ?2, ?3)",
                rusqlite::params![id, seconds, path],
            )
            .unwrap();
        }

        let names: Vec<(String, i64)> = get_stats_snapshot(&conn)
            .unwrap()
            .sourceports
            .into_iter()
            .map(|tally| (tally.name, tally.value))
            .collect();
        assert_eq!(
            names,
            [("nyan-doom".to_string(), 1500), ("uzdoom".to_string(), 600)]
        );
    }

    #[test]
    fn test_iwads_and_complevels_read_as_one_thing_each() {
        let conn = setup();
        for (iwad, complevel) in [
            (Some("doom2"), Some(9)),
            (Some("DOOM2.WAD"), Some(9)),
            (Some("/iwads/doom2.wad"), Some(21)),
            (Some("tnt"), None),
            (None, None),
        ] {
            let id = add_test_wad(&conn);
            conn.execute(
                "UPDATE wads SET custom_iwad = ?1, complevel = ?2 WHERE id = ?3",
                rusqlite::params![iwad, complevel, id],
            )
            .unwrap();
        }

        let snapshot = get_stats_snapshot(&conn).unwrap();
        let tally = |name: &str, value| Tally {
            name: name.to_string(),
            value,
        };
        assert_eq!(
            snapshot.iwads,
            [tally("doom2", 3), tally(NOT_SET, 1), tally("tnt", 1)]
        );
        assert_eq!(
            snapshot.complevels,
            [tally("Boom", 2), tally(NOT_SET, 2), tally("MBF21", 1)]
        );
    }

    /// A community project is not an author, and only the top few get a line.
    #[test]
    fn test_most_played_authors_are_people_and_only_a_few_of_them() {
        let conn = setup();
        for (index, author) in ["A", "B", "C", "D", "E", "F", "Various", "various "]
            .iter()
            .enumerate()
        {
            let id = add_wad(
                &conn,
                &NewWad::new(format!("WAD {index}"), SourceType::Local).author(*author),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sessions (wad_id, started_at, duration_seconds) \
                 VALUES (?1, '2024-01-01', ?2)",
                rusqlite::params![id, 1000 + index as i64],
            )
            .unwrap();
        }

        let authors: Vec<String> = get_stats_snapshot(&conn)
            .unwrap()
            .top_authors
            .into_iter()
            .map(|tally| tally.name)
            .collect();
        assert_eq!(authors, ["F", "E", "D", "C", "B"]);
    }

    #[test]
    fn test_update_wad_completion() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let comp_id = add_wad_completion(&conn, wad_id, None, None, None).unwrap();

        assert!(
            update_wad_completion(
                &conn,
                comp_id,
                Some(Some("{\"stats\": true}")),
                Some(Some("Updated")),
                None,
            )
            .unwrap()
        );

        let completions = get_wad_completions(&conn, wad_id).unwrap();
        assert_eq!(
            completions[0].stats_snapshot.as_deref(),
            Some("{\"stats\": true}")
        );
        assert_eq!(completions[0].notes.as_deref(), Some("Updated"));

        // Clear stats snapshot; leave notes untouched; change date.
        assert!(
            update_wad_completion(
                &conn,
                comp_id,
                Some(None),
                None,
                Some("2025-01-02T03:04:05+00:00"),
            )
            .unwrap()
        );
        let completions = get_wad_completions(&conn, wad_id).unwrap();
        assert_eq!(completions[0].stats_snapshot, None);
        assert_eq!(completions[0].notes.as_deref(), Some("Updated"));
        assert_eq!(completions[0].completed_at, "2025-01-02T03:04:05+00:00");
    }

    #[test]
    fn test_session_exit_code() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let session_id = start_session(&conn, wad_id, None, None).unwrap();

        end_session(&conn, session_id, None, Some(139)).unwrap();

        let mut stmt = conn.prepare("SELECT * FROM sessions WHERE id = ?").unwrap();
        let session = stmt
            .query_row([session_id], SessionRecord::from_row)
            .unwrap();
        assert_eq!(session.exit_code, Some(139));
    }
}
