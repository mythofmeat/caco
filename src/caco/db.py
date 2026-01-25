"""SQLite database for WAD library."""

import sqlite3
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any

DB_PATH = Path.home() / ".local" / "share" / "caco" / "library.db"


class Status(str, Enum):
    """Play status for a WAD."""
    TO_PLAY = "to-play"
    BACKLOG = "backlog"
    PLAYING = "playing"
    FINISHED = "finished"
    ABANDONED = "abandoned"


class SourceType(str, Enum):
    """Where the WAD can be obtained from."""
    IDGAMES = "idgames"
    DOOMWIKI = "doomwiki"
    DOOMWORLD = "doomworld"
    URL = "url"
    LOCAL = "local"


SCHEMA = """
CREATE TABLE IF NOT EXISTS wads (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    author TEXT,
    year INTEGER,
    description TEXT,

    -- Play status
    status TEXT DEFAULT 'backlog',
    rating INTEGER,  -- 1-5 stars
    notes TEXT,

    -- Source info
    source_type TEXT NOT NULL,
    source_id TEXT,      -- e.g., idgames file ID
    source_url TEXT,     -- download URL or forum thread

    -- File info (when downloaded/cached)
    filename TEXT,
    cached_path TEXT,    -- local path if cached

    -- Per-WAD play config (overrides global config)
    custom_iwad TEXT,
    custom_sourceport TEXT,
    custom_args TEXT,    -- JSON array of extra arguments

    -- Metadata
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    wad_id INTEGER NOT NULL,
    tag TEXT NOT NULL,
    FOREIGN KEY (wad_id) REFERENCES wads(id) ON DELETE CASCADE,
    UNIQUE(wad_id, tag)
);

CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY,
    wad_id INTEGER NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    duration_seconds INTEGER,
    sourceport TEXT,
    notes TEXT,
    FOREIGN KEY (wad_id) REFERENCES wads(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS map_completions (
    id INTEGER PRIMARY KEY,
    wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
    map_name TEXT NOT NULL,
    skill INTEGER,
    completed_at TEXT DEFAULT CURRENT_TIMESTAMP,
    notes TEXT,
    UNIQUE(wad_id, map_name, skill)
);

CREATE TABLE IF NOT EXISTS wad_completions (
    id INTEGER PRIMARY KEY,
    wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
    completed_at TEXT DEFAULT CURRENT_TIMESTAMP,
    stats_snapshot TEXT,
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_wads_status ON wads(status);
CREATE INDEX IF NOT EXISTS idx_wads_source_type ON wads(source_type);
CREATE INDEX IF NOT EXISTS idx_tags_wad_id ON tags(wad_id);
CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(tag);
CREATE INDEX IF NOT EXISTS idx_sessions_wad_id ON sessions(wad_id);
CREATE INDEX IF NOT EXISTS idx_map_completions_wad_id ON map_completions(wad_id);
CREATE INDEX IF NOT EXISTS idx_wad_completions_wad_id ON wad_completions(wad_id);
"""


def get_connection() -> sqlite3.Connection:
    """Get a database connection, creating the database if needed."""
    DB_PATH.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys = ON")
    return conn


def init_db() -> None:
    """Initialize the database schema."""
    with get_connection() as conn:
        conn.executescript(SCHEMA)
        # Migrations for existing databases
        _migrate_add_custom_play_config(conn)
        _migrate_add_map_completions(conn)
        _migrate_add_wad_completions(conn)
        _migrate_rename_wishlist_to_toplay(conn)


def _migrate_add_custom_play_config(conn: sqlite3.Connection) -> None:
    """Add custom_iwad, custom_sourceport, custom_args columns if missing."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "custom_iwad" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN custom_iwad TEXT")
    if "custom_sourceport" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN custom_sourceport TEXT")
    if "custom_args" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN custom_args TEXT")


def _migrate_add_map_completions(conn: sqlite3.Connection) -> None:
    """Create map_completions table if missing."""
    cursor = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='map_completions'"
    )
    if not cursor.fetchone():
        conn.execute("""
            CREATE TABLE map_completions (
                id INTEGER PRIMARY KEY,
                wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
                map_name TEXT NOT NULL,
                skill INTEGER,
                completed_at TEXT DEFAULT CURRENT_TIMESTAMP,
                notes TEXT,
                UNIQUE(wad_id, map_name, skill)
            )
        """)
        conn.execute("CREATE INDEX idx_map_completions_wad_id ON map_completions(wad_id)")


def _migrate_add_wad_completions(conn: sqlite3.Connection) -> None:
    """Create wad_completions table if missing."""
    cursor = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='wad_completions'"
    )
    if not cursor.fetchone():
        conn.execute("""
            CREATE TABLE wad_completions (
                id INTEGER PRIMARY KEY,
                wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
                completed_at TEXT DEFAULT CURRENT_TIMESTAMP,
                stats_snapshot TEXT,
                notes TEXT
            )
        """)
        conn.execute("CREATE INDEX idx_wad_completions_wad_id ON wad_completions(wad_id)")


def _migrate_rename_wishlist_to_toplay(conn: sqlite3.Connection) -> None:
    """Rename 'wishlist' status to 'to-play' in existing data."""
    conn.execute("UPDATE wads SET status = 'to-play' WHERE status = 'wishlist'")


def add_wad(
    title: str,
    source_type: SourceType,
    *,
    author: str | None = None,
    year: int | None = None,
    description: str | None = None,
    source_id: str | None = None,
    source_url: str | None = None,
    filename: str | None = None,
    status: Status = Status.BACKLOG,
    tags: list[str] | None = None,
) -> int:
    """Add a WAD to the library. Returns the new WAD ID."""
    with get_connection() as conn:
        cursor = conn.execute(
            """
            INSERT INTO wads (title, author, year, description, source_type,
                              source_id, source_url, filename, status)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (title, author, year, description, source_type.value,
             source_id, source_url, filename, status.value),
        )
        wad_id = cursor.lastrowid

        if tags:
            for tag in tags:
                conn.execute(
                    "INSERT OR IGNORE INTO tags (wad_id, tag) VALUES (?, ?)",
                    (wad_id, tag.lower()),
                )

        return wad_id


def get_wad(wad_id: int) -> dict[str, Any] | None:
    """Get a WAD by ID."""
    with get_connection() as conn:
        row = conn.execute("SELECT * FROM wads WHERE id = ?", (wad_id,)).fetchone()
        if row:
            wad = dict(row)
            # Fetch tags
            tags = conn.execute(
                "SELECT tag FROM tags WHERE wad_id = ?", (wad_id,)
            ).fetchall()
            wad["tags"] = [t["tag"] for t in tags]
            return wad
        return None


def parse_query(query: str) -> tuple[dict[str, str], list[str]]:
    """
    Parse beets-style query into field filters and free text.

    Supports:
        id:123          - exact ID match
        title:foo       - title contains 'foo'
        name:foo        - alias for title
        author:bar      - author contains 'bar'
        year:2020       - exact year match
        tag:megawad     - has tag
        status:playing  - status match
        source:idgames  - source type match
        foo bar         - free text search (title/author/description)

    Returns:
        (field_filters, free_text_terms)
    """
    import shlex

    filters: dict[str, str] = {}
    free_text: list[str] = []

    try:
        tokens = shlex.split(query)
    except ValueError:
        tokens = query.split()

    for token in tokens:
        if ":" in token:
            field, _, value = token.partition(":")
            field = field.lower()
            # Normalize field names
            if field == "name":
                field = "title"
            filters[field] = value
        else:
            free_text.append(token)

    return filters, free_text


def search_wads(
    query: str | None = None,
    status: Status | None = None,
    source_type: SourceType | None = None,
    tag: str | None = None,
    sort_by: str | None = None,
    sort_desc: bool = True,
) -> list[dict[str, Any]]:
    """
    Search WADs with optional filters.

    Query supports beets-style field:value syntax:
        caco list id:1
        caco list title:scythe author:alm
        caco list "tnt evilution"

    Sort fields: playtime, rating, created, title, author, last_played, year
    """
    conditions = []
    params = []

    if query:
        filters, free_text = parse_query(query)

        # Handle field-specific filters
        if "id" in filters:
            try:
                conditions.append("wads.id = ?")
                params.append(int(filters["id"]))
            except ValueError:
                pass

        if "title" in filters:
            conditions.append("wads.title LIKE ?")
            params.append(f"%{filters['title']}%")

        if "author" in filters:
            conditions.append("wads.author LIKE ?")
            params.append(f"%{filters['author']}%")

        if "year" in filters:
            try:
                conditions.append("wads.year = ?")
                params.append(int(filters["year"]))
            except ValueError:
                pass

        if "tag" in filters:
            conditions.append("wads.id IN (SELECT wad_id FROM tags WHERE tag LIKE ?)")
            params.append(f"%{filters['tag'].lower()}%")

        if "status" in filters:
            conditions.append("wads.status = ?")
            params.append(filters["status"].lower())

        if "source" in filters:
            conditions.append("wads.source_type = ?")
            params.append(filters["source"].lower())

        if "filename" in filters:
            conditions.append("wads.filename LIKE ?")
            params.append(f"%{filters['filename']}%")

        # Free text searches title, author, description
        for term in free_text:
            conditions.append("(wads.title LIKE ? OR wads.author LIKE ? OR wads.description LIKE ?)")
            like = f"%{term}%"
            params.extend([like, like, like])

    # CLI option filters (override query filters if both present)
    if status:
        conditions.append("wads.status = ?")
        params.append(status.value)

    if source_type:
        conditions.append("wads.source_type = ?")
        params.append(source_type.value)

    if tag:
        conditions.append("wads.id IN (SELECT wad_id FROM tags WHERE tag = ?)")
        params.append(tag.lower())

    where = " AND ".join(conditions) if conditions else "1=1"

    # Determine sort order
    direction = "DESC" if sort_desc else "ASC"
    reverse_dir = "ASC" if sort_desc else "DESC"  # For text fields where default should be opposite

    # Map sort field to SQL expression
    sort_map = {
        "playtime": f"COALESCE(SUM(sessions.duration_seconds), 0) {direction}",
        "rating": f"wads.rating {direction} NULLS LAST",
        "created": f"wads.created_at {direction}",
        "title": f"LOWER(wads.title) {reverse_dir}",  # Title default ascending (A-Z)
        "author": f"LOWER(wads.author) {reverse_dir} NULLS LAST",  # Author default ascending
        "last_played": f"MAX(sessions.started_at) {direction} NULLS LAST",
        "year": f"wads.year {direction} NULLS LAST",
    }

    if sort_by and sort_by in sort_map:
        order_by = sort_map[sort_by]
        use_group_by = sort_by in ("playtime", "last_played")
    else:
        # Default sort: status priority, then last played/created
        order_by = """
            CASE wads.status
                WHEN 'playing' THEN 1
                WHEN 'backlog' THEN 2
                WHEN 'to-play' THEN 3
                WHEN 'abandoned' THEN 4
                WHEN 'finished' THEN 5
                ELSE 6
            END,
            CASE wads.status
                WHEN 'playing' THEN (SELECT MAX(started_at) FROM sessions WHERE sessions.wad_id = wads.id)
                WHEN 'abandoned' THEN (SELECT MAX(started_at) FROM sessions WHERE sessions.wad_id = wads.id)
                WHEN 'finished' THEN (SELECT MAX(started_at) FROM sessions WHERE sessions.wad_id = wads.id)
                ELSE NULL
            END DESC NULLS LAST,
            wads.created_at DESC
        """
        use_group_by = False

    with get_connection() as conn:
        if use_group_by:
            # For playtime/last_played, need to JOIN with sessions
            sql = f"""
                SELECT wads.*
                FROM wads
                LEFT JOIN sessions ON sessions.wad_id = wads.id
                WHERE {where}
                GROUP BY wads.id
                ORDER BY {order_by}
            """
        else:
            sql = f"SELECT wads.* FROM wads WHERE {where} ORDER BY {order_by}"

        rows = conn.execute(sql, params).fetchall()

        results = []
        for row in rows:
            wad = dict(row)
            tags = conn.execute(
                "SELECT tag FROM tags WHERE wad_id = ?", (wad["id"],)
            ).fetchall()
            wad["tags"] = [t["tag"] for t in tags]
            results.append(wad)

        return results


def update_wad(wad_id: int, **fields) -> bool:
    """Update a WAD's fields. Returns True if updated.

    If status is set to 'finished', automatically records a completion
    with a snapshot of the stats.txt file (if available).
    """
    if not fields:
        return False

    # Check if setting status to finished (before enum conversion)
    recording_completion = False
    status_value = fields.get("status")
    if status_value:
        if isinstance(status_value, Status):
            recording_completion = status_value == Status.FINISHED
        else:
            recording_completion = status_value == Status.FINISHED.value

    # Convert enums to values
    for key, value in fields.items():
        if isinstance(value, Enum):
            fields[key] = value.value

    fields["updated_at"] = datetime.now().isoformat()

    set_clause = ", ".join(f"{k} = ?" for k in fields.keys())

    with get_connection() as conn:
        cursor = conn.execute(
            f"UPDATE wads SET {set_clause} WHERE id = ?",
            list(fields.values()) + [wad_id],
        )
        updated = cursor.rowcount > 0

    # Record completion if status was set to 'finished'
    if updated and recording_completion:
        # Get stats snapshot if available (late import to avoid circular dependency)
        from caco.player import get_stats_path

        wad = get_wad(wad_id)
        stats_content = None
        if wad:
            stats_path = get_stats_path(wad)
            if stats_path and stats_path.exists():
                try:
                    stats_content = stats_path.read_text()
                except OSError:
                    pass
        add_wad_completion(wad_id, stats_snapshot=stats_content)

    return updated


def delete_wad(wad_id: int) -> bool:
    """Delete a WAD. Returns True if deleted."""
    with get_connection() as conn:
        cursor = conn.execute("DELETE FROM wads WHERE id = ?", (wad_id,))
        return cursor.rowcount > 0


def add_tag(wad_id: int, tag: str) -> bool:
    """Add a tag to a WAD. Returns True if added."""
    with get_connection() as conn:
        try:
            conn.execute(
                "INSERT INTO tags (wad_id, tag) VALUES (?, ?)",
                (wad_id, tag.lower()),
            )
            return True
        except sqlite3.IntegrityError:
            return False


def remove_tag(wad_id: int, tag: str) -> bool:
    """Remove a tag from a WAD. Returns True if removed."""
    with get_connection() as conn:
        cursor = conn.execute(
            "DELETE FROM tags WHERE wad_id = ? AND tag = ?",
            (wad_id, tag.lower()),
        )
        return cursor.rowcount > 0


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
        return cursor.lastrowid


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
        return row["total"]


def get_last_played(wad_id: int) -> str | None:
    """Get the last played timestamp for a WAD."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT started_at FROM sessions WHERE wad_id = ? ORDER BY started_at DESC LIMIT 1",
            (wad_id,),
        ).fetchone()
        return row["started_at"] if row else None


def get_all_tags() -> list[str]:
    """Get all unique tags."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT DISTINCT tag FROM tags ORDER BY tag"
        ).fetchall()
        return [row["tag"] for row in rows]


def find_duplicate(
    source_type: SourceType,
    source_id: str | None = None,
    source_url: str | None = None,
    filename: str | None = None,
    author: str | None = None,
) -> dict[str, Any] | None:
    """
    Find a potential duplicate WAD in the library.

    Detection strategy (in priority order):
    1. idgames: exact match on source_id
    2. URL/local: exact match on source_url
    3. Fallback: normalized filename + author match

    Returns the existing WAD dict if found, or None.
    """
    with get_connection() as conn:
        # Strategy 1: Match by source_id (for idgames)
        if source_type == SourceType.IDGAMES and source_id:
            row = conn.execute(
                "SELECT * FROM wads WHERE source_type = ? AND source_id = ?",
                (source_type.value, source_id),
            ).fetchone()
            if row:
                wad = dict(row)
                tags = conn.execute(
                    "SELECT tag FROM tags WHERE wad_id = ?", (wad["id"],)
                ).fetchall()
                wad["tags"] = [t["tag"] for t in tags]
                return wad

        # Strategy 2: Match by source_url (for URL and local)
        if source_url and source_type in (SourceType.URL, SourceType.LOCAL):
            row = conn.execute(
                "SELECT * FROM wads WHERE source_type = ? AND source_url = ?",
                (source_type.value, source_url),
            ).fetchone()
            if row:
                wad = dict(row)
                tags = conn.execute(
                    "SELECT tag FROM tags WHERE wad_id = ?", (wad["id"],)
                ).fetchall()
                wad["tags"] = [t["tag"] for t in tags]
                return wad

        # Strategy 3: Fuzzy match on normalized filename + author
        if filename:
            # Normalize filename: lowercase, strip extension
            normalized = filename.lower()
            for ext in (".zip", ".wad", ".pk3", ".pk7"):
                if normalized.endswith(ext):
                    normalized = normalized[: -len(ext)]
                    break

            # Build query: filename LIKE pattern, optionally with author
            if author:
                row = conn.execute(
                    """
                    SELECT * FROM wads
                    WHERE LOWER(filename) LIKE ?
                    AND LOWER(author) LIKE ?
                    """,
                    (f"%{normalized}%", f"%{author.lower()}%"),
                ).fetchone()
            else:
                row = conn.execute(
                    "SELECT * FROM wads WHERE LOWER(filename) LIKE ?",
                    (f"%{normalized}%",),
                ).fetchone()

            if row:
                wad = dict(row)
                tags = conn.execute(
                    "SELECT tag FROM tags WHERE wad_id = ?", (wad["id"],)
                ).fetchall()
                wad["tags"] = [t["tag"] for t in tags]
                return wad

        return None


# =============================================================================
# Map Completions
# =============================================================================


def add_map_completion(
    wad_id: int,
    map_name: str,
    skill: int | None = None,
    notes: str | None = None,
) -> int:
    """Add a map completion record. Returns the completion ID."""
    with get_connection() as conn:
        cursor = conn.execute(
            """
            INSERT INTO map_completions (wad_id, map_name, skill, notes)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(wad_id, map_name, skill) DO UPDATE SET
                completed_at = CURRENT_TIMESTAMP,
                notes = COALESCE(excluded.notes, notes)
            """,
            (wad_id, map_name.upper(), skill, notes),
        )
        return cursor.lastrowid


def remove_map_completion(wad_id: int, map_name: str, skill: int | None = None) -> bool:
    """Remove a map completion. Returns True if removed."""
    with get_connection() as conn:
        if skill is not None:
            cursor = conn.execute(
                "DELETE FROM map_completions WHERE wad_id = ? AND map_name = ? AND skill = ?",
                (wad_id, map_name.upper(), skill),
            )
        else:
            # Remove all skill levels for this map
            cursor = conn.execute(
                "DELETE FROM map_completions WHERE wad_id = ? AND map_name = ?",
                (wad_id, map_name.upper()),
            )
        return cursor.rowcount > 0


def get_map_completions(wad_id: int) -> list[dict[str, Any]]:
    """Get all map completions for a WAD."""
    with get_connection() as conn:
        rows = conn.execute(
            """
            SELECT * FROM map_completions
            WHERE wad_id = ?
            ORDER BY map_name, skill
            """,
            (wad_id,),
        ).fetchall()
        return [dict(row) for row in rows]


def get_completed_maps(wad_id: int) -> list[str]:
    """Get list of unique completed map names for a WAD."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT DISTINCT map_name FROM map_completions WHERE wad_id = ? ORDER BY map_name",
            (wad_id,),
        ).fetchall()
        return [row["map_name"] for row in rows]


def sync_map_completions(wad_id: int, completions: list[tuple[str, int]]) -> int:
    """
    Sync map completions from stats file.

    Args:
        wad_id: WAD database ID
        completions: List of (map_name, skill) tuples

    Returns:
        Number of new completions added
    """
    added = 0
    with get_connection() as conn:
        for map_name, skill in completions:
            # Check if already exists
            existing = conn.execute(
                "SELECT id FROM map_completions WHERE wad_id = ? AND map_name = ? AND skill = ?",
                (wad_id, map_name.upper(), skill),
            ).fetchone()

            if not existing:
                conn.execute(
                    "INSERT INTO map_completions (wad_id, map_name, skill) VALUES (?, ?, ?)",
                    (wad_id, map_name.upper(), skill),
                )
                added += 1

    return added


def get_map_completion_stats(wad_id: int) -> dict[str, Any]:
    """Get map completion statistics for a WAD."""
    with get_connection() as conn:
        # Count unique maps completed
        row = conn.execute(
            "SELECT COUNT(DISTINCT map_name) as count FROM map_completions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()
        unique_maps = row["count"]

        # Get highest skill completed for each map
        rows = conn.execute(
            """
            SELECT map_name, MAX(skill) as max_skill
            FROM map_completions
            WHERE wad_id = ?
            GROUP BY map_name
            ORDER BY map_name
            """,
            (wad_id,),
        ).fetchall()

        return {
            "unique_maps": unique_maps,
            "by_map": {row["map_name"]: row["max_skill"] for row in rows},
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
        return cursor.lastrowid


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


def get_times_beaten(wad_id: int) -> int:
    """Get count of completions for a WAD."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT COUNT(*) as count FROM wad_completions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()
        return row["count"]


def get_maps_completed_batch(wad_ids: list[int]) -> dict[int, int]:
    """Get maps completed count for multiple WADs efficiently."""
    if not wad_ids:
        return {}

    with get_connection() as conn:
        placeholders = ",".join("?" * len(wad_ids))
        rows = conn.execute(
            f"""
            SELECT wad_id, COUNT(DISTINCT map_name) as count
            FROM map_completions
            WHERE wad_id IN ({placeholders})
            GROUP BY wad_id
            """,
            wad_ids,
        ).fetchall()
        return {row["wad_id"]: row["count"] for row in rows}


def get_times_beaten_batch(wad_ids: list[int]) -> dict[int, int]:
    """Get times beaten for multiple WADs efficiently."""
    if not wad_ids:
        return {}

    with get_connection() as conn:
        placeholders = ",".join("?" * len(wad_ids))
        rows = conn.execute(
            f"""
            SELECT wad_id, COUNT(*) as count
            FROM wad_completions
            WHERE wad_id IN ({placeholders})
            GROUP BY wad_id
            """,
            wad_ids,
        ).fetchall()
        return {row["wad_id"]: row["count"] for row in rows}
