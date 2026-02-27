"""Sessions, completions, batch stats, cache management, and library statistics."""

from dataclasses import dataclass, field
from datetime import datetime
from typing import Any

from caco.db._connection import get_connection, _attach_tags, _batch_query, _SQLITE_MAX_VARS


# =============================================================================
# Play Sessions
# =============================================================================


def start_session(wad_id: int, sourceport: str | None = None) -> int:
    """Start a play session. Returns the session ID."""
    with get_connection() as conn:
        cursor = conn.execute(
            """
            INSERT INTO sessions (wad_id, started_at, sourceport)
            VALUES (?, ?, ?)
            """,
            (wad_id, datetime.now().isoformat(), sourceport),
        )
        session_id = cursor.lastrowid
        if not session_id or session_id <= 0:
            raise RuntimeError(f"Failed to get valid session ID after insert (got {session_id})")
        return session_id


def end_session(session_id: int, notes: str | None = None) -> None:
    """End a play session."""
    ended_at = datetime.now()

    with get_connection() as conn:
        # Get start time
        row = conn.execute(
            "SELECT started_at FROM sessions WHERE id = ?", (session_id,)
        ).fetchone()

        if row:
            started_at = datetime.fromisoformat(row["started_at"])
            duration = int((ended_at - started_at).total_seconds())

            conn.execute(
                """
                UPDATE sessions SET ended_at = ?, duration_seconds = ?, notes = ?
                WHERE id = ?
                """,
                (ended_at.isoformat(), duration, notes, session_id),
            )


def update_session_stats(
    session_id: int,
    stats_before: str | None,
    stats_after: str | None,
) -> None:
    """Attach before/after stats snapshots to a session record.

    Called after end_session() to store the stats diff data without
    modifying the critical session lifecycle code.
    """
    with get_connection() as conn:
        conn.execute(
            """
            UPDATE sessions SET stats_before = ?, stats_after = ?
            WHERE id = ?
            """,
            (stats_before, stats_after, session_id),
        )


def get_sessions(wad_id: int) -> list[dict[str, Any]]:
    """Get all play sessions for a WAD."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT * FROM sessions WHERE wad_id = ? ORDER BY started_at DESC",
            (wad_id,),
        ).fetchall()
        return [dict(row) for row in rows]


def get_total_playtime(wad_id: int) -> int:
    """Get total playtime in seconds for a WAD."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT COALESCE(SUM(duration_seconds), 0) as total FROM sessions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()
        total: int = row["total"]
        return total


# =============================================================================
# Batch Stats
# =============================================================================


def get_total_playtime_batch(wad_ids: list[int]) -> dict[int, int]:
    """Get total playtime for multiple WADs efficiently. Returns {wad_id: seconds}."""
    result = _batch_query(
        wad_ids,
        """
        SELECT wad_id, COALESCE(SUM(duration_seconds), 0) as total
        FROM sessions
        WHERE wad_id IN ({placeholders})
        GROUP BY wad_id
        """,
        "total",
    )
    # Fill in zeros for WADs with no sessions
    return {wid: result.get(wid, 0) for wid in wad_ids}


def get_last_played(wad_id: int) -> str | None:
    """Get the last played timestamp for a WAD."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT started_at FROM sessions WHERE wad_id = ? ORDER BY started_at DESC LIMIT 1",
            (wad_id,),
        ).fetchone()
        return row["started_at"] if row else None


def get_last_played_batch(wad_ids: list[int]) -> dict[int, str]:
    """Get last played timestamp for multiple WADs efficiently. Returns {wad_id: timestamp}."""
    return _batch_query(
        wad_ids,
        """
        SELECT wad_id, MAX(started_at) as last_played
        FROM sessions
        WHERE wad_id IN ({placeholders})
        GROUP BY wad_id
        """,
        "last_played",
    )


def get_most_recently_played() -> dict[str, Any] | None:
    """Get the most recently played WAD across the entire library."""
    with get_connection() as conn:
        row = conn.execute(
            """
            SELECT wads.* FROM wads
            JOIN sessions ON sessions.wad_id = wads.id
            WHERE wads.deleted_at IS NULL
            ORDER BY sessions.started_at DESC
            LIMIT 1
            """
        ).fetchone()
        if row:
            return _attach_tags(conn, dict(row))
        return None


def get_session_count_batch(wad_ids: list[int]) -> dict[int, int]:
    """Get session count for multiple WADs efficiently. Returns {wad_id: count}."""
    return _batch_query(
        wad_ids,
        """
        SELECT wad_id, COUNT(*) as count
        FROM sessions
        WHERE wad_id IN ({placeholders})
        GROUP BY wad_id
        """,
        "count",
    )


def get_wad_stats_batch(wad_ids: list[int]) -> dict[int, dict[str, Any]]:
    """Get all stats for multiple WADs in 2 queries on 1 connection.

    Returns {wad_id: {playtime, last_played, session_count, times_beaten}}.
    Replaces 4 separate batch functions for list view loading.
    """
    if not wad_ids:
        return {}

    session_stats: dict[int, dict[str, Any]] = {}
    beaten_map: dict[int, int] = {}

    with get_connection() as conn:
        for i in range(0, len(wad_ids), _SQLITE_MAX_VARS):
            chunk = wad_ids[i:i + _SQLITE_MAX_VARS]
            placeholders = ",".join("?" * len(chunk))

            # Query 1: session aggregates (playtime + last_played + count)
            rows = conn.execute(
                f"""
                SELECT wad_id,
                    COALESCE(SUM(duration_seconds), 0) as playtime,
                    MAX(started_at) as last_played,
                    COUNT(*) as session_count
                FROM sessions
                WHERE wad_id IN ({placeholders})
                GROUP BY wad_id
                """,
                chunk,
            ).fetchall()
            for r in rows:
                session_stats[r["wad_id"]] = {
                    "playtime": r["playtime"],
                    "last_played": r["last_played"],
                    "session_count": r["session_count"],
                }

            # Query 2: completions (times_beaten)
            rows = conn.execute(
                f"""
                SELECT wad_id, COUNT(*) as times_beaten
                FROM wad_completions
                WHERE wad_id IN ({placeholders})
                GROUP BY wad_id
                """,
                chunk,
            ).fetchall()
            for r in rows:
                beaten_map[r["wad_id"]] = r["times_beaten"]

    # Build result with defaults for WADs with no data
    result = {}
    for wid in wad_ids:
        ss = session_stats.get(wid, {})
        result[wid] = {
            "playtime": ss.get("playtime", 0),
            "last_played": ss.get("last_played"),
            "session_count": ss.get("session_count", 0),
            "times_beaten": beaten_map.get(wid, 0),
        }
    return result


def get_wad_stats(wad_id: int) -> dict[str, Any]:
    """Get deletion-relevant stats for a WAD.

    Returns:
        Dict with keys:
        - session_count: number of play sessions
        - total_playtime: total playtime in seconds
    """
    with get_connection() as conn:
        # Session count
        row = conn.execute(
            "SELECT COUNT(*) as count FROM sessions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()
        session_count = row["count"]

        # Total playtime
        row = conn.execute(
            "SELECT COALESCE(SUM(duration_seconds), 0) as total FROM sessions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()
        total_playtime = row["total"]

        return {
            "session_count": session_count,
            "total_playtime": total_playtime,
        }


# =============================================================================
# WAD Completions (Times Beaten)
# =============================================================================


def add_wad_completion(
    wad_id: int,
    stats_snapshot: str | None = None,
    notes: str | None = None,
) -> int:
    """Record a WAD completion. Returns completion ID."""
    with get_connection() as conn:
        cursor = conn.execute(
            """
            INSERT INTO wad_completions (wad_id, stats_snapshot, notes)
            VALUES (?, ?, ?)
            """,
            (wad_id, stats_snapshot, notes),
        )
        completion_id = cursor.lastrowid
        if not completion_id or completion_id <= 0:
            raise RuntimeError(f"Failed to get valid completion ID after insert (got {completion_id})")
        return completion_id


def get_wad_completions(wad_id: int) -> list[dict[str, Any]]:
    """Get all completion records for a WAD."""
    with get_connection() as conn:
        rows = conn.execute(
            """
            SELECT * FROM wad_completions
            WHERE wad_id = ?
            ORDER BY completed_at DESC
            """,
            (wad_id,),
        ).fetchall()
        return [dict(row) for row in rows]


def update_wad_completion(
    completion_id: int,
    stats_snapshot: str | None = None,
    notes: str | None = None,
) -> bool:
    """Update a completion record's stats_snapshot and/or notes.

    Only updates fields that are not None. Returns True if the record existed.
    """
    updates = []
    params: list[Any] = []
    if stats_snapshot is not None:
        updates.append("stats_snapshot = ?")
        params.append(stats_snapshot)
    if notes is not None:
        updates.append("notes = ?")
        params.append(notes)
    if not updates:
        return False
    params.append(completion_id)
    with get_connection() as conn:
        cursor = conn.execute(
            f"UPDATE wad_completions SET {', '.join(updates)} WHERE id = ?",
            params,
        )
        return cursor.rowcount > 0


def delete_wad_completion(completion_id: int) -> bool:
    """Delete a specific completion record. Returns True if deleted."""
    with get_connection() as conn:
        cursor = conn.execute(
            "DELETE FROM wad_completions WHERE id = ?",
            (completion_id,),
        )
        return cursor.rowcount > 0


def set_wad_completion_count(wad_id: int, count: int) -> None:
    """Set the completion count for a WAD to a specific number.

    If count is less than current, removes oldest completions.
    If count is more than current, adds placeholder completions.
    """
    with get_connection() as conn:
        current = conn.execute(
            "SELECT COUNT(*) as cnt FROM wad_completions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()["cnt"]

        if count < current:
            # Delete oldest completions to reach target count
            to_delete = current - count
            conn.execute(
                """
                DELETE FROM wad_completions
                WHERE id IN (
                    SELECT id FROM wad_completions
                    WHERE wad_id = ?
                    ORDER BY completed_at ASC
                    LIMIT ?
                )
                """,
                (wad_id, to_delete),
            )
        elif count > current:
            # Add placeholder completions in bulk
            to_add = count - current
            conn.executemany(
                "INSERT INTO wad_completions (wad_id, notes) VALUES (?, ?)",
                [(wad_id, "Manually added")] * to_add,
            )


def get_times_beaten(wad_id: int) -> int:
    """Get count of completions for a WAD."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT COUNT(*) as count FROM wad_completions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()
        count: int = row["count"]
        return count


def get_times_beaten_batch(wad_ids: list[int]) -> dict[int, int]:
    """Get times beaten for multiple WADs efficiently."""
    result = _batch_query(
        wad_ids,
        """
        SELECT wad_id, COUNT(*) as times_beaten
        FROM wad_completions
        WHERE wad_id IN ({placeholders})
        GROUP BY wad_id
        """,
        "times_beaten",
    )
    return {wid: result.get(wid, 0) for wid in wad_ids}


# =============================================================================
# Cache Management
# =============================================================================


def get_cached_wads() -> list[dict[str, Any]]:
    """Get all WADs with cached files (non-null cached_path, not deleted)."""
    with get_connection() as conn:
        rows = conn.execute(
            """
            SELECT * FROM wads
            WHERE cached_path IS NOT NULL
            AND deleted_at IS NULL
            ORDER BY title
            """
        ).fetchall()
        return [dict(row) for row in rows]


def clear_cached_path(wad_id: int) -> bool:
    """Clear the cached_path for a WAD. Returns True if updated."""
    with get_connection() as conn:
        cursor = conn.execute(
            "UPDATE wads SET cached_path = NULL WHERE id = ?",
            (wad_id,),
        )
        return cursor.rowcount > 0


def clear_all_cached_paths() -> int:
    """Clear cached_path for all WADs. Returns count of WADs updated."""
    with get_connection() as conn:
        cursor = conn.execute(
            "UPDATE wads SET cached_path = NULL WHERE cached_path IS NOT NULL"
        )
        return cursor.rowcount


def get_wad_by_cached_filename(filename: str) -> dict[str, Any] | None:
    """Find a WAD by the filename portion of its cached_path.

    Used to detect orphaned files (files in cache dir not tracked in DB).
    """
    with get_connection() as conn:
        # Match against the filename part of cached_path (ends with /filename)
        row = conn.execute(
            """
            SELECT * FROM wads
            WHERE cached_path LIKE ?
            AND deleted_at IS NULL
            """,
            (f"%/{filename}",),
        ).fetchone()
        if row:
            return _attach_tags(conn, dict(row))
        return None


# =============================================================================
# Library Statistics
# =============================================================================


@dataclass
class StatsSnapshot:
    """Library-wide statistics bundled into a single object.

    Replaces 3 separate DB calls (get_library_stats, get_completion_rate,
    get_wads_played_by_period) with one `get_stats_snapshot()` call.
    """
    # Overview
    total_wads: int = 0
    total_sessions: int = 0
    total_playtime: int = 0
    wads_with_sessions: int = 0
    wads_by_status: dict[str, int] = field(default_factory=dict)
    # Completion
    played_wads: int = 0
    finished_wads: int = 0
    completion_rate: float = 0.0
    total_completions: int = 0
    # Activity
    activity: list[dict[str, Any]] = field(default_factory=list)


def get_stats_snapshot(period: str = "month") -> StatsSnapshot:
    """Get a complete library statistics snapshot in a single call.

    Combines get_library_stats(), get_completion_rate(), and
    get_wads_played_by_period() into one snapshot object.
    """
    stats = get_library_stats()
    completion = get_completion_rate()
    activity = get_wads_played_by_period(period)
    return StatsSnapshot(
        total_wads=stats["total_wads"],
        total_sessions=stats["total_sessions"],
        total_playtime=stats["total_playtime"],
        wads_with_sessions=stats["wads_with_sessions"],
        wads_by_status=stats["wads_by_status"],
        played_wads=completion["played_wads"],
        finished_wads=completion["finished_wads"],
        completion_rate=completion["completion_rate"],
        total_completions=completion["total_completions"],
        activity=activity,
    )


def get_library_stats() -> dict[str, Any]:
    """Get library-wide overview statistics.

    Returns:
        Dict with keys:
        - total_wads: COUNT of non-deleted WADs
        - total_sessions: COUNT of all play sessions
        - total_playtime: SUM of duration_seconds from sessions
        - wads_with_sessions: COUNT of distinct WADs with at least one session
        - wads_by_status: dict mapping status -> count
    """
    with get_connection() as conn:
        # Total WADs (non-deleted)
        row = conn.execute(
            "SELECT COUNT(*) as count FROM wads WHERE deleted_at IS NULL"
        ).fetchone()
        total_wads = row["count"]

        # Total sessions
        row = conn.execute("SELECT COUNT(*) as count FROM sessions").fetchone()
        total_sessions = row["count"]

        # Total playtime
        row = conn.execute(
            "SELECT COALESCE(SUM(duration_seconds), 0) as total FROM sessions"
        ).fetchone()
        total_playtime = row["total"]

        # WADs with at least one session
        row = conn.execute(
            "SELECT COUNT(DISTINCT wad_id) as count FROM sessions"
        ).fetchone()
        wads_with_sessions = row["count"]

        # WADs by status
        rows = conn.execute(
            """
            SELECT status, COUNT(*) as count
            FROM wads
            WHERE deleted_at IS NULL
            GROUP BY status
            """
        ).fetchall()
        wads_by_status = {row["status"]: row["count"] for row in rows}

        return {
            "total_wads": total_wads,
            "total_sessions": total_sessions,
            "total_playtime": total_playtime,
            "wads_with_sessions": wads_with_sessions,
            "wads_by_status": wads_by_status,
        }


_PERIOD_QUERIES: dict[str, str] = {
    "year": """
        SELECT
            strftime('%Y', started_at) as period,
            COUNT(DISTINCT wad_id) as wad_count,
            COUNT(*) as session_count,
            COALESCE(SUM(duration_seconds), 0) as total_playtime
        FROM sessions
        GROUP BY strftime('%Y', started_at)
        ORDER BY period DESC
    """,
    "month": """
        SELECT
            strftime('%Y-%m', started_at) as period,
            COUNT(DISTINCT wad_id) as wad_count,
            COUNT(*) as session_count,
            COALESCE(SUM(duration_seconds), 0) as total_playtime
        FROM sessions
        GROUP BY strftime('%Y-%m', started_at)
        ORDER BY period DESC
    """,
}


def get_wads_played_by_period(period: str = "month") -> list[dict[str, Any]]:
    """Get activity grouped by time period.

    Args:
        period: "month" for YYYY-MM grouping, "year" for YYYY grouping

    Returns:
        List of dicts with keys: period, wad_count, session_count, total_playtime
        Ordered by period descending (most recent first).
    """
    query = _PERIOD_QUERIES.get(period, _PERIOD_QUERIES["month"])
    with get_connection() as conn:
        rows = conn.execute(query).fetchall()
        return [dict(row) for row in rows]


def get_completion_rate() -> dict[str, Any]:
    """Get completion statistics.

    Returns:
        Dict with keys:
        - played_wads: WADs with at least one session
        - finished_wads: WADs with status='finished' that have been played (have sessions)
        - completion_rate: finished/played as float (0.0 if no played WADs)
        - total_completions: COUNT from wad_completions (includes replays)
    """
    with get_connection() as conn:
        # WADs with at least one session
        row = conn.execute(
            "SELECT COUNT(DISTINCT wad_id) as count FROM sessions"
        ).fetchone()
        played_wads = row["count"]

        # WADs with status='finished' that also have at least one session (played AND finished)
        row = conn.execute(
            """
            SELECT COUNT(DISTINCT wads.id) as count
            FROM wads
            JOIN sessions ON sessions.wad_id = wads.id
            WHERE wads.status = 'finished' AND wads.deleted_at IS NULL
            """
        ).fetchone()
        finished_wads = row["count"]

        # Total completions (including replays)
        row = conn.execute(
            "SELECT COUNT(*) as count FROM wad_completions"
        ).fetchone()
        total_completions = row["count"]

        # Calculate completion rate (avoid division by zero)
        completion_rate = finished_wads / played_wads if played_wads > 0 else 0.0

        return {
            "played_wads": played_wads,
            "finished_wads": finished_wads,
            "completion_rate": completion_rate,
            "total_completions": total_completions,
        }
