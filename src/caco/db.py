"""SQLite database for WAD library."""

import sqlite3
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any

DB_PATH = Path.home() / ".local" / "share" / "caco" / "library.db"


class Status(str, Enum):
    """Play status for a WAD."""
    WISHLIST = "wishlist"
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

CREATE INDEX IF NOT EXISTS idx_wads_status ON wads(status);
CREATE INDEX IF NOT EXISTS idx_wads_source_type ON wads(source_type);
CREATE INDEX IF NOT EXISTS idx_tags_wad_id ON tags(wad_id);
CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(tag);
CREATE INDEX IF NOT EXISTS idx_sessions_wad_id ON sessions(wad_id);
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
) -> list[dict[str, Any]]:
    """
    Search WADs with optional filters.

    Query supports beets-style field:value syntax:
        caco list id:1
        caco list title:scythe author:alm
        caco list "tnt evilution"
    """
    conditions = []
    params = []

    if query:
        filters, free_text = parse_query(query)

        # Handle field-specific filters
        if "id" in filters:
            try:
                conditions.append("id = ?")
                params.append(int(filters["id"]))
            except ValueError:
                pass

        if "title" in filters:
            conditions.append("title LIKE ?")
            params.append(f"%{filters['title']}%")

        if "author" in filters:
            conditions.append("author LIKE ?")
            params.append(f"%{filters['author']}%")

        if "year" in filters:
            try:
                conditions.append("year = ?")
                params.append(int(filters["year"]))
            except ValueError:
                pass

        if "tag" in filters:
            conditions.append("id IN (SELECT wad_id FROM tags WHERE tag LIKE ?)")
            params.append(f"%{filters['tag'].lower()}%")

        if "status" in filters:
            conditions.append("status = ?")
            params.append(filters["status"].lower())

        if "source" in filters:
            conditions.append("source_type = ?")
            params.append(filters["source"].lower())

        # Free text searches title, author, description
        for term in free_text:
            conditions.append("(title LIKE ? OR author LIKE ? OR description LIKE ?)")
            like = f"%{term}%"
            params.extend([like, like, like])

    # CLI option filters (override query filters if both present)
    if status:
        conditions.append("status = ?")
        params.append(status.value)

    if source_type:
        conditions.append("source_type = ?")
        params.append(source_type.value)

    if tag:
        conditions.append("id IN (SELECT wad_id FROM tags WHERE tag = ?)")
        params.append(tag.lower())

    where = " AND ".join(conditions) if conditions else "1=1"

    with get_connection() as conn:
        rows = conn.execute(
            f"SELECT * FROM wads WHERE {where} ORDER BY title", params
        ).fetchall()

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
    """Update a WAD's fields. Returns True if updated."""
    if not fields:
        return False

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
        return cursor.rowcount > 0


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
