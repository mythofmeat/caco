"""Database schema, migrations, and initialization."""

import sqlite3
from typing import Any

from caco.db._connection import get_connection


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

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT DEFAULT CURRENT_TIMESTAMP
);
"""

# Indexes that depend on migration-added columns — run after init_db() migrations
_POST_MIGRATION_INDEXES = """
CREATE INDEX IF NOT EXISTS idx_wads_deleted_at ON wads(deleted_at);
CREATE INDEX IF NOT EXISTS idx_wads_cached_path ON wads(cached_path);
CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(wad_id, started_at DESC);
"""


def init_db() -> None:
    """Initialize the database schema and run pending migrations."""
    with get_connection() as conn:
        conn.executescript(SCHEMA)

        # Determine which migrations have already been applied
        current_version = 0
        try:
            row = conn.execute("SELECT MAX(version) FROM schema_migrations").fetchone()
            if row and row[0] is not None:
                current_version = row[0]
        except sqlite3.OperationalError:
            # Table doesn't exist yet (shouldn't happen since SCHEMA creates it)
            pass

        # Run only pending migrations (they're idempotent, safe for first run)
        for version, name, fn in _MIGRATIONS:
            if version > current_version:
                fn(conn)
                conn.execute(
                    "INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (?, ?)",
                    (version, name),
                )

        # Indexes on migration-added columns (must run after migrations)
        conn.executescript(_POST_MIGRATION_INDEXES)


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


def _migrate_add_iwads_table(conn: sqlite3.Connection) -> None:
    """Create the iwads table for IWAD registry."""
    cursor = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='iwads'"
    )
    if not cursor.fetchone():
        conn.execute("""
            CREATE TABLE iwads (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                title TEXT,
                path TEXT NOT NULL,
                md5 TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
        """)


# Ordered migration registry — append new migrations here with incrementing version
_MIGRATIONS: list[tuple[int, str, Any]] = [
    (1, "add_custom_play_config", _migrate_add_custom_play_config),
    (2, "add_wad_completions", _migrate_add_wad_completions),
    (3, "rename_wishlist_to_toplay", _migrate_rename_wishlist_to_toplay),
    (4, "add_deleted_at", _migrate_add_deleted_at),
    (5, "add_version", _migrate_add_version),
    (6, "add_idgames_id", _migrate_add_idgames_id),
    (7, "drop_map_completions", _migrate_drop_map_completions),
    (8, "add_iwads_table", _migrate_add_iwads_table),
]
