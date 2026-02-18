"""SQLite database for WAD library."""

import shlex
import sqlite3
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from types import MappingProxyType
from typing import Any

from caco.config import get_db_path


class Status(str, Enum):
    """Play status for a WAD."""
    TO_PLAY = "to-play"
    BACKLOG = "backlog"
    PLAYING = "playing"
    FINISHED = "finished"
    ABANDONED = "abandoned"
    AWAITING_UPDATE = "awaiting-update"


class SourceType(str, Enum):
    """Where the WAD can be obtained from."""
    IDGAMES = "idgames"
    DOOMWIKI = "doomwiki"
    DOOMWORLD = "doomworld"
    URL = "url"
    LOCAL = "local"


# =============================================================================
# Query Parser Data Structures
# =============================================================================


@dataclass
class QueryTerm:
    """A single query term (field:value or free text)."""
    field: str | None  # None for free-text search
    value: str
    negated: bool = False

    def __repr__(self) -> str:
        neg = "-" if self.negated else ""
        if self.field:
            return f"{neg}{self.field}:{self.value}"
        return f"{neg}{self.value}"


@dataclass
class AndGroup:
    """A group of terms joined by AND (implicit)."""
    terms: list[QueryTerm] = field(default_factory=list)


@dataclass
class ParsedQuery:
    """Complete parsed query with OR groups.

    Structure: (term1 AND term2) OR (term3 AND term4)
    Each AndGroup is OR-ed together.
    """
    or_groups: list[AndGroup] = field(default_factory=list)

    def is_empty(self) -> bool:
        return not self.or_groups or all(not g.terms for g in self.or_groups)


# Status shortcuts for query parsing (moved from cli.py)
STATUS_SHORTCUTS: MappingProxyType[str, str] = MappingProxyType({
    "t": "to-play", "toplay": "to-play", "tp": "to-play",
    "b": "backlog", "back": "backlog",
    "p": "playing", "play": "playing",
    "f": "finished", "fin": "finished", "done": "finished",
    "a": "abandoned", "drop": "abandoned", "dropped": "abandoned",
    "w": "awaiting-update", "waiting": "awaiting-update", "wip": "awaiting-update",
    "au": "awaiting-update", "await": "awaiting-update",
})

# OR separator for query syntax (space-comma-space)
OR_SEPARATOR = " , "


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
    idgames_id TEXT,     -- idgames file ID for downloading (any source)

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
CREATE INDEX IF NOT EXISTS idx_wad_completions_wad_id ON wad_completions(wad_id);
"""


def get_connection() -> sqlite3.Connection:
    """Get a database connection, creating the database if needed."""
    db_path = get_db_path()
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys = ON")
    return conn


def _fetch_tags(conn: sqlite3.Connection, wad_id: int) -> list[str]:
    """Fetch tags for a single WAD."""
    rows = conn.execute(
        "SELECT tag FROM tags WHERE wad_id = ?", (wad_id,)
    ).fetchall()
    return [r["tag"] for r in rows]


def _attach_tags(conn: sqlite3.Connection, wad: dict) -> dict:
    """Attach tags to a WAD dict in-place and return it."""
    wad["tags"] = _fetch_tags(conn, wad["id"])
    return wad


def _fetch_tags_batch(conn: sqlite3.Connection, wad_ids: list[int]) -> dict[int, list[str]]:
    """Fetch tags for multiple WADs efficiently. Returns {wad_id: [tags]}."""
    if not wad_ids:
        return {}
    placeholders = ",".join("?" * len(wad_ids))
    rows = conn.execute(
        f"SELECT wad_id, tag FROM tags WHERE wad_id IN ({placeholders}) ORDER BY tag",
        wad_ids,
    ).fetchall()
    result: dict[int, list[str]] = {}
    for r in rows:
        result.setdefault(r["wad_id"], []).append(r["tag"])
    return result


def init_db() -> None:
    """Initialize the database schema."""
    with get_connection() as conn:
        conn.executescript(SCHEMA)
        # Migrations for existing databases
        _migrate_add_custom_play_config(conn)
        _migrate_add_wad_completions(conn)
        _migrate_rename_wishlist_to_toplay(conn)
        _migrate_add_deleted_at(conn)
        _migrate_add_version(conn)
        _migrate_add_idgames_id(conn)
        _migrate_drop_map_completions(conn)


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


def _migrate_add_deleted_at(conn: sqlite3.Connection) -> None:
    """Add deleted_at column for soft delete support."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "deleted_at" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN deleted_at TEXT")


def _migrate_add_version(conn: sqlite3.Connection) -> None:
    """Add version column for non-idgames releases."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "version" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN version TEXT")


def _migrate_add_idgames_id(conn: sqlite3.Connection) -> None:
    """Add idgames_id column for cross-source downloading."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "idgames_id" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN idgames_id TEXT")


def _migrate_drop_map_completions(conn: sqlite3.Connection) -> None:
    """Drop the map_completions table (feature removed)."""
    conn.execute("DROP TABLE IF EXISTS map_completions")


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
    cached_path: str | None = None,
    status: Status = Status.BACKLOG,
    tags: list[str] | None = None,
    version: str | None = None,
) -> int:
    """Add a WAD to the library. Returns the new WAD ID."""
    with get_connection() as conn:
        cursor = conn.execute(
            """
            INSERT INTO wads (title, author, year, description, source_type,
                              source_id, source_url, filename, cached_path, status, version)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (title, author, year, description, source_type.value,
             source_id, source_url, filename, cached_path, status.value, version),
        )
        wad_id = cursor.lastrowid
        if not wad_id or wad_id <= 0:
            raise RuntimeError(f"Failed to get valid WAD ID after insert (got {wad_id})")

        if tags:
            for tag in tags:
                conn.execute(
                    "INSERT OR IGNORE INTO tags (wad_id, tag) VALUES (?, ?)",
                    (wad_id, tag.lower()),
                )

        return wad_id


def get_wad(wad_id: int, include_deleted: bool = False) -> dict[str, Any] | None:
    """Get a WAD by ID.

    Args:
        wad_id: WAD ID to fetch
        include_deleted: If True, also return deleted WADs
    """
    with get_connection() as conn:
        if include_deleted:
            row = conn.execute("SELECT * FROM wads WHERE id = ?", (wad_id,)).fetchone()
        else:
            row = conn.execute(
                "SELECT * FROM wads WHERE id = ? AND deleted_at IS NULL",
                (wad_id,)
            ).fetchone()
        if row:
            return _attach_tags(conn, dict(row))
        return None


def _glob_to_like(pattern: str) -> str:
    """Convert a glob pattern to SQL LIKE pattern.

    Handles:
    - * → % (match any characters)
    - ? → _ (match single character)
    - Escapes existing % and _ in the pattern
    """
    # Check if it's a glob pattern
    if "*" not in pattern and "?" not in pattern:
        # Not a glob, return as-is for exact match
        return pattern

    # Escape existing SQL wildcards
    result = pattern.replace("%", r"\%").replace("_", r"\_")
    # Convert glob to LIKE
    result = result.replace("*", "%").replace("?", "_")
    return result


def _is_glob_pattern(pattern: str) -> bool:
    """Check if a string contains glob wildcards."""
    return "*" in pattern or "?" in pattern


def _split_or_groups(query: str) -> list[str]:
    """Split query by OR_SEPARATOR respecting quoted strings."""
    sep_len = len(OR_SEPARATOR)
    parts = []
    current: list[str] = []
    i = 0
    in_quotes = False
    quote_char = None

    while i < len(query):
        char = query[i]

        # Handle quote state
        if char in '"\'':
            if not in_quotes:
                in_quotes = True
                quote_char = char
            elif char == quote_char:
                in_quotes = False
                quote_char = None
            current.append(char)
            i += 1
            continue

        # Check for OR_SEPARATOR pattern (not inside quotes)
        if not in_quotes and i + sep_len <= len(query):
            if query[i:i+sep_len] == OR_SEPARATOR:
                parts.append("".join(current).strip())
                current = []
                i += sep_len
                continue

        current.append(char)
        i += 1

    # Add final part
    if current:
        parts.append("".join(current).strip())

    return [p for p in parts if p]


def _parse_and_group(group_str: str) -> list[QueryTerm]:
    """Parse a single AND group into terms."""
    terms = []

    try:
        tokens = shlex.split(group_str)
    except ValueError:
        tokens = group_str.split()

    for token in tokens:
        negated = False

        # Check for negation prefix (- or ^ like beets)
        # ^ is useful when - would be interpreted as a CLI option
        if (token.startswith("-") or token.startswith("^")) and len(token) > 1:
            negated = True
            token = token[1:]

        # Check for field:value pattern
        if ":" in token:
            field, _, value = token.partition(":")
            field = field.lower()

            # Normalize field aliases
            if field == "name":
                field = "title"

            terms.append(QueryTerm(field=field, value=value, negated=negated))
        else:
            # Free text term
            terms.append(QueryTerm(field=None, value=token, negated=negated))

    return terms


def parse_query(query: str) -> ParsedQuery:
    """
    Parse beets-style query into structured form.

    Syntax:
        - Field queries: field:value, field:"quoted value"
        - Free text: word (searches title/author/description)
        - Negation: -field:value, -word
        - OR groups: term1 term2 , term3 term4
          (comma surrounded by spaces creates OR boundary)
        - Field aliases: name: -> title:

    Examples:
        status:playing author:alm          -> AND(status=playing, author=alm)
        status:playing , status:to-play    -> OR(status=playing, status=to-play)
        -status:finished -tag:cacoward*    -> AND(NOT status=finished, NOT tag=cacoward*)
        "ancient aliens" , scythe          -> OR(free_text="ancient aliens", free_text=scythe)

    Returns:
        ParsedQuery with or_groups containing AndGroups of QueryTerms.
    """
    if not query or not query.strip():
        return ParsedQuery(or_groups=[])

    # Split by " , " (comma with surrounding spaces) for OR groups
    or_parts = _split_or_groups(query)

    or_groups = []
    for part in or_parts:
        terms = _parse_and_group(part)
        if terms:
            or_groups.append(AndGroup(terms=terms))

    return ParsedQuery(or_groups=or_groups)


def _normalize_status(value: str) -> str:
    """Normalize status value, expanding shortcuts."""
    lower = value.lower()
    return STATUS_SHORTCUTS.get(lower, lower)


def _build_term_sql(term: QueryTerm) -> tuple[str, list[Any]]:
    """Build SQL clause for a single QueryTerm."""
    clause = ""
    params: list[Any] = []

    if term.field is None:
        # Free text search
        clause = "(wads.title LIKE ? OR wads.author LIKE ? OR wads.description LIKE ?)"
        like = f"%{term.value}%"
        params = [like, like, like]

    elif term.field == "id":
        try:
            clause = "wads.id = ?"
            params = [int(term.value)]
        except ValueError:
            return "", []

    elif term.field == "title":
        clause = "wads.title LIKE ?"
        params = [f"%{term.value}%"]

    elif term.field == "author":
        clause = "wads.author LIKE ?"
        params = [f"%{term.value}%"]

    elif term.field == "year":
        try:
            clause = "wads.year = ?"
            params = [int(term.value)]
        except ValueError:
            return "", []

    elif term.field == "filename":
        clause = "wads.filename LIKE ?"
        params = [f"%{term.value}%"]

    elif term.field == "status":
        clause = "wads.status = ?"
        params = [_normalize_status(term.value)]

    elif term.field == "source":
        clause = "wads.source_type = ?"
        params = [term.value.lower()]

    elif term.field == "tag":
        tag_pattern = term.value.lower()
        if _is_glob_pattern(tag_pattern):
            like_pattern = _glob_to_like(tag_pattern)
            clause = "wads.id IN (SELECT wad_id FROM tags WHERE tag LIKE ? ESCAPE '\\')"
            params = [like_pattern]
        else:
            # Substring match for non-glob
            clause = "wads.id IN (SELECT wad_id FROM tags WHERE tag LIKE ?)"
            params = [f"%{tag_pattern}%"]

    else:
        # Unknown field - treat as free text
        clause = "(wads.title LIKE ? OR wads.author LIKE ? OR wads.description LIKE ?)"
        like = f"%{term.value}%"
        params = [like, like, like]

    # Apply negation
    if term.negated and clause:
        clause = f"NOT ({clause})"

    return clause, params


def _build_query_sql(parsed: ParsedQuery) -> tuple[str, list[Any]]:
    """Build SQL WHERE clause from ParsedQuery."""
    if parsed.is_empty():
        return "", []

    or_clauses = []
    all_params: list[Any] = []

    for and_group in parsed.or_groups:
        and_clauses = []
        group_params: list[Any] = []

        for term in and_group.terms:
            clause, term_params = _build_term_sql(term)
            if clause:
                and_clauses.append(clause)
                group_params.extend(term_params)

        if and_clauses:
            or_clauses.append(f"({' AND '.join(and_clauses)})")
            all_params.extend(group_params)

    if not or_clauses:
        return "", []

    return " OR ".join(or_clauses), all_params


def search_wads(
    query: str | None = None,
    sort_by: str | None = None,
    sort_desc: bool = True,
    include_deleted: bool = False,
) -> list[dict[str, Any]]:
    """
    Search WADs with beets-style query syntax.

    Query supports:
        - Field queries: status:playing, author:romero, tag:megawad
        - Negation: -status:finished, -tag:cacoward*
        - OR groups: status:playing , status:to-play
        - Free text: scythe (searches title/author/description)
        - Glob patterns: tag:caco* (matches cacoward, etc.)
        - Status shortcuts: status:p (playing), status:f (finished), etc.

    Sort fields: playtime, rating, created, title, author, last_played, year

    Args:
        query: Beets-style query string
        sort_by: Field to sort by
        sort_desc: Sort descending (default True)
        include_deleted: If True, only show deleted WADs. If False (default),
                        exclude deleted WADs.
    """
    # Validate sort field before use in SQL construction
    allowed_sort_fields = {"id", "playtime", "rating", "created", "title", "author", "last_played", "year", "random"}
    if sort_by and sort_by not in allowed_sort_fields:
        raise ValueError(f"Invalid sort field: {sort_by}")

    conditions = []
    params: list[Any] = []

    # Filter by deleted status
    if include_deleted:
        conditions.append("wads.deleted_at IS NOT NULL")
    else:
        conditions.append("wads.deleted_at IS NULL")

    if query:
        parsed = parse_query(query)
        if not parsed.is_empty():
            query_sql, query_params = _build_query_sql(parsed)
            if query_sql:
                conditions.append(f"({query_sql})")
                params.extend(query_params)

    # SAFETY: conditions built by _build_query_sql() which uses parameterized queries
    where = " AND ".join(conditions) if conditions else "1=1"

    # Determine sort order
    direction = "DESC" if sort_desc else "ASC"
    reverse_dir = "ASC" if sort_desc else "DESC"  # For text fields where default should be opposite
    # For nullable fields: DESC = NULLS LAST (best first), ASC = NULLS FIRST (worst/empty first)
    nulls = "NULLS LAST" if sort_desc else "NULLS FIRST"
    reverse_nulls = "NULLS FIRST" if sort_desc else "NULLS LAST"

    # Map sort field to SQL expression (all values are hardcoded, not user-controlled)
    sort_map = {
        "id": f"wads.id {reverse_dir}",  # ID default ascending
        "playtime": f"COALESCE(SUM(sessions.duration_seconds), 0) {direction}",
        "rating": f"wads.rating {direction} {nulls}",
        "created": f"wads.created_at {direction}",
        "title": f"LOWER(wads.title) {reverse_dir}",  # Title default ascending (A-Z)
        "author": f"LOWER(wads.author) {reverse_dir} {reverse_nulls}",  # Author default ascending
        "last_played": f"MAX(sessions.started_at) {direction} {nulls}",
        "year": f"wads.year {direction} {nulls}",
        "random": "RANDOM()",
    }

    if sort_by and sort_by in sort_map:
        order_by = sort_map[sort_by]
        use_group_by = sort_by in ("playtime", "last_played")
    else:
        # Default sort: ID ascending (simplest, most predictable)
        order_by = "wads.id ASC"
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

        results = [dict(row) for row in rows]

        # Batch-fetch tags for all results
        if results:
            wad_ids = [w["id"] for w in results]
            tags_by_wad = _fetch_tags_batch(conn, wad_ids)
            for wad in results:
                wad["tags"] = tags_by_wad.get(wad["id"], [])

        return results


def update_wad(wad_id: int, **fields) -> bool:
    """Update a WAD's fields. Returns True if updated.

    If status is set to 'finished', automatically records a completion.
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
        add_wad_completion(wad_id)

    return updated


def delete_wad(wad_id: int, purge: bool = False) -> bool:
    """Delete a WAD (soft delete by default).

    Args:
        wad_id: WAD ID to delete
        purge: If True, permanently delete. If False (default), soft delete.

    Returns True if deleted/trashed.
    """
    with get_connection() as conn:
        if purge:
            cursor = conn.execute("DELETE FROM wads WHERE id = ?", (wad_id,))
        else:
            cursor = conn.execute(
                "UPDATE wads SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL",
                (datetime.now().isoformat(), wad_id),
            )
        return cursor.rowcount > 0


def restore_wad(wad_id: int) -> bool:
    """Restore a soft-deleted WAD. Returns True if restored."""
    with get_connection() as conn:
        cursor = conn.execute(
            "UPDATE wads SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL",
            (wad_id,),
        )
        return cursor.rowcount > 0


def purge_all_deleted() -> int:
    """Permanently delete all soft-deleted WADs. Returns count of purged WADs."""
    with get_connection() as conn:
        cursor = conn.execute("DELETE FROM wads WHERE deleted_at IS NOT NULL")
        return cursor.rowcount


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


def _batch_query(
    wad_ids: list[int],
    query_template: str,
    result_column: str = "result",
) -> dict[int, Any]:
    """Generic batch query helper for aggregation queries.

    Args:
        wad_ids: List of WAD IDs to query
        query_template: SQL with {placeholders} format string for IN clause
        result_column: Column name to extract from results

    Returns:
        Dict mapping wad_id to result value
    """
    if not wad_ids:
        return {}

    with get_connection() as conn:
        placeholders = ",".join("?" * len(wad_ids))
        query = query_template.format(placeholders=placeholders)
        rows = conn.execute(query, wad_ids).fetchall()
        return {row["wad_id"]: row[result_column] for row in rows}


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


def get_all_tags() -> list[str]:
    """Get all unique tags."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT DISTINCT tag FROM tags ORDER BY tag"
        ).fetchall()
        return [row["tag"] for row in rows]


def get_tag_counts() -> list[tuple[str, int]]:
    """Get all tags with their WAD counts (excluding deleted WADs)."""
    with get_connection() as conn:
        rows = conn.execute(
            """
            SELECT t.tag, COUNT(*) as count
            FROM tags t
            JOIN wads w ON w.id = t.wad_id
            WHERE w.deleted_at IS NULL
            GROUP BY t.tag
            ORDER BY t.tag
            """
        ).fetchall()
        return [(row["tag"], row["count"]) for row in rows]


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
    2. doomwiki: exact match on source_id (wiki page ID)
    3. doomworld: exact match on source_id (thread ID)
    4. URL/local: exact match on source_url
    5. Fallback: normalized filename + author match

    Returns the existing WAD dict if found, or None.
    """
    with get_connection() as conn:
        # Strategy 1-3: Match by source_type + source_id (idgames, doomwiki, doomworld)
        if source_id and source_type in (SourceType.IDGAMES, SourceType.DOOMWIKI, SourceType.DOOMWORLD):
            row = conn.execute(
                "SELECT * FROM wads WHERE source_type = ? AND source_id = ?",
                (source_type.value, source_id),
            ).fetchone()
            if row:
                return _attach_tags(conn, dict(row))

        # Strategy 4: Match by source_url (for URL and local)
        if source_url and source_type in (SourceType.URL, SourceType.LOCAL):
            row = conn.execute(
                "SELECT * FROM wads WHERE source_type = ? AND source_url = ?",
                (source_type.value, source_url),
            ).fetchone()
            if row:
                return _attach_tags(conn, dict(row))

        # Strategy 5: Fuzzy match on normalized filename + author
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
                return _attach_tags(conn, dict(row))

        return None


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
            # Add placeholder completions
            to_add = count - current
            for _ in range(to_add):
                conn.execute(
                    "INSERT INTO wad_completions (wad_id, notes) VALUES (?, ?)",
                    (wad_id, "Manually added"),
                )


def get_times_beaten(wad_id: int) -> int:
    """Get count of completions for a WAD."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT COUNT(*) as count FROM wad_completions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()
        return row["count"]


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


def get_wads_played_by_period(period: str = "month") -> list[dict[str, Any]]:
    """Get activity grouped by time period.

    Args:
        period: "month" for YYYY-MM grouping, "year" for YYYY grouping

    Returns:
        List of dicts with keys: period, wad_count, session_count, total_playtime
        Ordered by period descending (most recent first).
    """
    # Map period to strftime format
    if period == "year":
        fmt = "%Y"
    else:
        fmt = "%Y-%m"

    with get_connection() as conn:
        rows = conn.execute(
            f"""
            SELECT
                strftime('{fmt}', started_at) as period,
                COUNT(DISTINCT wad_id) as wad_count,
                COUNT(*) as session_count,
                COALESCE(SUM(duration_seconds), 0) as total_playtime
            FROM sessions
            GROUP BY strftime('{fmt}', started_at)
            ORDER BY period DESC
            """
        ).fetchall()
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
