"""Database schema, migrations, and initialization."""

import shutil
import sqlite3
from pathlib import Path
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
                family TEXT NOT NULL,
                variant TEXT NOT NULL DEFAULT 'unknown',
                title TEXT,
                path TEXT NOT NULL,
                md5 TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(family, variant)
            )
        """)


def _migrate_iwads_family_variant(conn: sqlite3.Connection) -> None:
    """Restructure iwads table: name -> family + variant columns.

    Migrates existing rows by mapping name -> family and detecting variant
    from MD5.  If the table already has the new schema (family column),
    this is a no-op.
    """
    cursor = conn.execute("PRAGMA table_info(iwads)")
    columns = {row[1] for row in cursor.fetchall()}

    # Already migrated (or created fresh with migration #8 new schema)
    if "family" in columns:
        return

    # Old schema has 'name' column — need to migrate
    if "name" not in columns:
        return

    from caco.db._iwads import KNOWN_IWADS

    # Read existing rows
    old_rows = conn.execute("SELECT id, name, title, path, md5, created_at FROM iwads").fetchall()

    # Drop and recreate with new schema
    conn.execute("DROP TABLE iwads")
    conn.execute("""
        CREATE TABLE iwads (
            id INTEGER PRIMARY KEY,
            family TEXT NOT NULL,
            variant TEXT NOT NULL DEFAULT 'unknown',
            title TEXT,
            path TEXT NOT NULL,
            md5 TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(family, variant)
        )
    """)

    # Re-insert with family/variant
    for row in old_rows:
        family = row[1]  # old 'name' becomes family
        md5 = row[4]
        variant = "unknown"

        # Try to detect variant from MD5
        if md5 and md5 in KNOWN_IWADS:
            _, detected_variant, _ = KNOWN_IWADS[md5]
            variant = detected_variant

        conn.execute(
            "INSERT OR IGNORE INTO iwads (family, variant, title, path, md5, created_at) "
            "VALUES (?, ?, ?, ?, ?, ?)",
            (family, variant, row[2], row[3], md5, row[5]),
        )


def _migrate_relocate_wad_cache(conn: sqlite3.Connection) -> None:
    """Move WAD cache from ~/.cache/caco/wads/ to ~/.local/share/caco/wads/.

    Updates cached_path in the database for all relocated files.
    Skips if the user has a custom cache_dir that isn't the old or new default.
    """
    old_default = Path.home() / ".cache" / "caco" / "wads"
    new_default = Path.home() / ".local" / "share" / "caco" / "wads"

    # Skip if old directory doesn't exist
    if not old_default.is_dir():
        return

    # Skip if user has a custom cache_dir that isn't old or new default
    from caco.config import load_config
    config = load_config()
    user_cache_dir = config.get("cache_dir", "")
    if user_cache_dir:
        user_path = Path(user_cache_dir).expanduser()
        if user_path != old_default and user_path != new_default:
            return

    # Ensure destination exists
    new_default.mkdir(parents=True, exist_ok=True)

    # Move files and update DB paths
    for entry in old_default.iterdir():
        if not entry.is_file():
            continue
        dest = new_default / entry.name
        if not dest.exists():
            shutil.move(str(entry), str(dest))

        # Update cached_path in DB
        old_path_str = str(entry)
        new_path_str = str(dest)
        conn.execute(
            "UPDATE wads SET cached_path = ? WHERE cached_path = ?",
            (new_path_str, old_path_str),
        )

    # Try to clean up old empty directories
    try:
        old_default.rmdir()
    except OSError:
        pass
    try:
        old_default.parent.rmdir()
    except OSError:
        pass


def _migrate_add_stats_snapshot(conn: sqlite3.Connection) -> None:
    """Add stats_snapshot column to wads for live stats tracking."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "stats_snapshot" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN stats_snapshot TEXT")


def _migrate_fix_stale_cache_paths(conn: sqlite3.Connection) -> None:
    """Fix cached_path values still pointing to old ~/.cache/caco/wads/ location.

    Migration 10 moved the files but the DB UPDATE may not have persisted.
    This does a bulk string replacement as a safety net.
    """
    old_prefix = str(Path.home() / ".cache" / "caco" / "wads")
    new_prefix = str(Path.home() / ".local" / "share" / "caco" / "wads")

    conn.execute(
        "UPDATE wads SET cached_path = REPLACE(cached_path, ?, ?) "
        "WHERE cached_path LIKE ?",
        (old_prefix, new_prefix, old_prefix + "%"),
    )


def _migrate_iwad_dir_restructure(conn: sqlite3.Connection) -> None:
    """Restructure managed IWAD paths from {family}_{variant}.wad to {variant}/{family}.wad.

    Moves files on disk and updates paths in the database.  Only touches
    files inside the managed IWAD directory; user-managed IWADs are skipped.
    """
    from caco.config import get_iwad_dir

    iwad_dir = get_iwad_dir()
    resolved_iwad_dir = iwad_dir.resolve()

    rows = conn.execute("SELECT id, family, variant, path FROM iwads").fetchall()
    for row in rows:
        old_path = Path(row[3])
        try:
            resolved_old = old_path.resolve()
        except OSError:
            continue

        # Only migrate files inside the managed IWAD directory
        if not resolved_old.is_relative_to(resolved_iwad_dir):
            continue

        new_path = iwad_dir / row[2] / f"{row[1]}.wad"  # {variant}/{family}.wad

        # Skip if already at the new location
        if old_path == new_path:
            continue

        # Create variant subdirectory and move file
        new_path.parent.mkdir(parents=True, exist_ok=True)
        try:
            if old_path.exists():
                shutil.move(str(old_path), str(new_path))
        except OSError:
            pass

        # Update DB path regardless (so future operations use new convention)
        conn.execute(
            "UPDATE iwads SET path = ? WHERE id = ?",
            (str(new_path), row[0]),
        )

    # Clean up old empty files at iwad_dir root (there shouldn't be dirs to clean)
    # No-op if nothing was moved


def _migrate_add_companion_files(conn: sqlite3.Connection) -> None:
    """Add companion_files column for multi-file WAD support."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "companion_files" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN companion_files TEXT")


def _migrate_add_session_stats(conn: sqlite3.Connection) -> None:
    """Add stats_before and stats_after columns to sessions for per-session map tracking."""
    cursor = conn.execute("PRAGMA table_info(sessions)")
    columns = {row[1] for row in cursor.fetchall()}
    if "stats_before" not in columns:
        conn.execute("ALTER TABLE sessions ADD COLUMN stats_before TEXT")
    if "stats_after" not in columns:
        conn.execute("ALTER TABLE sessions ADD COLUMN stats_after TEXT")


def _migrate_add_demo_file(conn: sqlite3.Connection) -> None:
    """Add demo_file column to sessions for linking recorded demos."""
    cursor = conn.execute("PRAGMA table_info(sessions)")
    columns = {row[1] for row in cursor.fetchall()}
    if "demo_file" not in columns:
        conn.execute("ALTER TABLE sessions ADD COLUMN demo_file TEXT")


def _migrate_add_id24_wads_table(conn: sqlite3.Connection) -> None:
    """Create the id24_wads table for id24 content registry."""
    cursor = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='id24_wads'"
    )
    if not cursor.fetchone():
        conn.execute("""
            CREATE TABLE id24_wads (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                version TEXT,
                title TEXT,
                path TEXT NOT NULL,
                md5 TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
        """)


def _migrate_add_custom_complevel(conn: sqlite3.Connection) -> None:
    """Add custom_complevel column for per-WAD complevel setting."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "custom_complevel" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN custom_complevel TEXT")


def _migrate_add_complevel(conn: sqlite3.Connection) -> None:
    """Add complevel column for compatibility level tracking."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "complevel" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN complevel INTEGER")


def _migrate_add_session_exit_code(conn: sqlite3.Connection) -> None:
    """Add exit_code column to sessions for crash detection."""
    cursor = conn.execute("PRAGMA table_info(sessions)")
    columns = {row[1] for row in cursor.fetchall()}
    if "exit_code" not in columns:
        conn.execute("ALTER TABLE sessions ADD COLUMN exit_code INTEGER")


def _migrate_merge_custom_complevel(conn: sqlite3.Connection) -> None:
    """Copy non-null custom_complevel values to complevel (as int) where complevel IS NULL.

    After this migration, only the complevel INTEGER column is used.
    The custom_complevel TEXT column stays in SQLite (can't drop columns) but is dead.
    """
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "custom_complevel" not in columns:
        return  # Nothing to migrate

    conn.execute("""
        UPDATE wads
        SET complevel = CAST(custom_complevel AS INTEGER)
        WHERE custom_complevel IS NOT NULL
          AND complevel IS NULL
    """)


def _migrate_add_custom_config_column(conn: sqlite3.Connection) -> None:
    """Add custom_config column for sourceport config profile name."""
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "custom_config" not in columns:
        conn.execute("ALTER TABLE wads ADD COLUMN custom_config TEXT")


def _migrate_companion_files_tables(conn: sqlite3.Connection) -> None:
    """Create companion_files and wad_companions tables, migrate existing JSON data.

    Moves from the old companion_files TEXT column (JSON array of absolute paths)
    to a deduplicated registry with a junction table.
    """
    import json

    # Create companion_files registry table
    cursor = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='companion_files_registry'"
    )
    if not cursor.fetchone():
        conn.execute("""
            CREATE TABLE companion_files_registry (
                id INTEGER PRIMARY KEY,
                md5 TEXT UNIQUE,
                filename TEXT NOT NULL,
                path TEXT,
                size INTEGER,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
        """)

    # Create wad_companions junction table
    cursor = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='wad_companions'"
    )
    if not cursor.fetchone():
        conn.execute("""
            CREATE TABLE wad_companions (
                wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
                companion_id INTEGER NOT NULL REFERENCES companion_files_registry(id) ON DELETE CASCADE,
                enabled INTEGER DEFAULT 1,
                load_order INTEGER DEFAULT 0,
                PRIMARY KEY (wad_id, companion_id)
            )
        """)
        conn.execute("CREATE INDEX IF NOT EXISTS idx_wad_companions_wad_id ON wad_companions(wad_id)")
        conn.execute("CREATE INDEX IF NOT EXISTS idx_wad_companions_companion_id ON wad_companions(companion_id)")

    # Migrate existing companion_files JSON data
    cursor = conn.execute("PRAGMA table_info(wads)")
    columns = {row[1] for row in cursor.fetchall()}
    if "companion_files" not in columns:
        return

    rows = conn.execute(
        "SELECT id, companion_files FROM wads WHERE companion_files IS NOT NULL"
    ).fetchall()

    if not rows:
        return

    from caco.config import get_companion_dir
    from caco.utils import compute_md5

    companion_dir = get_companion_dir()
    companion_dir.mkdir(parents=True, exist_ok=True)

    for row in rows:
        wad_id = row[0]
        try:
            files = json.loads(row[1])
        except (json.JSONDecodeError, TypeError):
            continue
        if not isinstance(files, list):
            continue

        for load_order, file_path in enumerate(files):
            file_path_obj = Path(file_path)
            filename = file_path_obj.name

            if file_path_obj.exists():
                md5 = compute_md5(file_path_obj)
                size = file_path_obj.stat().st_size

                # Check if already registered (dedup)
                existing = conn.execute(
                    "SELECT id FROM companion_files_registry WHERE md5 = ?", (md5,)
                ).fetchone()

                if existing:
                    companion_id = existing[0]
                else:
                    # Copy to managed dir
                    managed_name = f"{md5[:12]}_{filename}"
                    managed_path = companion_dir / managed_name
                    if not managed_path.exists():
                        shutil.copy2(file_path, str(managed_path))

                    cursor = conn.execute(
                        "INSERT INTO companion_files_registry (md5, filename, path, size) "
                        "VALUES (?, ?, ?, ?)",
                        (md5, filename, str(managed_path), size),
                    )
                    companion_id = cursor.lastrowid
            else:
                # File missing — register with NULL md5/path
                cursor = conn.execute(
                    "INSERT INTO companion_files_registry (md5, filename, path, size) "
                    "VALUES (NULL, ?, NULL, NULL)",
                    (filename,),
                )
                companion_id = cursor.lastrowid

            # Link to WAD
            conn.execute(
                "INSERT OR IGNORE INTO wad_companions (wad_id, companion_id, enabled, load_order) "
                "VALUES (?, ?, 1, ?)",
                (wad_id, companion_id, load_order),
            )


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
    (9, "iwads_family_variant", _migrate_iwads_family_variant),
    (10, "relocate_wad_cache", _migrate_relocate_wad_cache),
    (11, "add_stats_snapshot", _migrate_add_stats_snapshot),
    (12, "fix_stale_cache_paths", _migrate_fix_stale_cache_paths),
    (13, "iwad_dir_restructure", _migrate_iwad_dir_restructure),
    (14, "add_companion_files", _migrate_add_companion_files),
    (15, "add_session_stats", _migrate_add_session_stats),
    (16, "add_demo_file", _migrate_add_demo_file),
    (17, "add_id24_wads_table", _migrate_add_id24_wads_table),
    (18, "add_custom_complevel", _migrate_add_custom_complevel),
    (19, "add_complevel", _migrate_add_complevel),
    (20, "add_session_exit_code", _migrate_add_session_exit_code),
    (21, "add_custom_config", _migrate_add_custom_config_column),
    (22, "merge_custom_complevel_to_complevel", _migrate_merge_custom_complevel),
    (23, "companion_files_tables", _migrate_companion_files_tables),
]
