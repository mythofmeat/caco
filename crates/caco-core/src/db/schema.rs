use rusqlite::Connection;

use crate::Result;

/// Core schema SQL — creates tables and indexes if they don't exist.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS wads (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    author TEXT,
    year INTEGER,
    description TEXT,

    -- Play status
    status TEXT DEFAULT 'unplayed',
    rating INTEGER,
    notes TEXT,

    -- Source info
    source_type TEXT NOT NULL,
    source_id TEXT,
    source_url TEXT,
    idgames_id TEXT,

    -- File info (when downloaded/cached)
    filename TEXT,
    cached_path TEXT,

    -- Per-WAD play config (overrides global config)
    custom_iwad TEXT,
    custom_sourceport TEXT,
    required_sourceport_family TEXT,
    custom_args TEXT,

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
"#;

/// Post-migration indexes that depend on migration-added columns.
const POST_MIGRATION_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_wads_deleted_at ON wads(deleted_at);
CREATE INDEX IF NOT EXISTS idx_wads_cached_path ON wads(cached_path);
CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(wad_id, started_at DESC);
"#;

// ---------------------------------------------------------------------------
// Migration registry
// ---------------------------------------------------------------------------

/// A single migration entry: (version, name, function).
type Migration = (i64, &'static str, fn(&Connection) -> Result<()>);

static MIGRATIONS: &[Migration] = &[
    (1, "add_custom_play_config", migrate_add_custom_play_config),
    (2, "add_wad_completions", migrate_add_wad_completions),
    (
        3,
        "rename_wishlist_to_toplay",
        migrate_rename_wishlist_to_toplay,
    ),
    (4, "add_deleted_at", migrate_add_deleted_at),
    (5, "add_version", migrate_add_version),
    (6, "add_idgames_id", migrate_add_idgames_id),
    (7, "drop_map_completions", migrate_drop_map_completions),
    (8, "add_iwads_table", migrate_add_iwads_table),
    (9, "iwads_family_variant", migrate_iwads_family_variant),
    // Migration 10 (relocate_wad_cache) is a Python-side filesystem migration.
    // The Rust port skips it — the move was a one-time Python-era operation.
    (10, "relocate_wad_cache_noop", migrate_noop),
    (11, "add_stats_snapshot", migrate_add_stats_snapshot),
    // Migration 12 (fix_stale_cache_paths) was a Python-side fixup. Noop here.
    (12, "fix_stale_cache_paths_noop", migrate_noop),
    // Migration 13 (iwad_dir_restructure) is a Python-side filesystem migration.
    (13, "iwad_dir_restructure_noop", migrate_noop),
    (14, "add_companion_files", migrate_add_companion_files),
    (15, "add_session_stats", migrate_add_session_stats),
    (16, "add_demo_file", migrate_add_demo_file),
    (17, "add_id24_wads_table", migrate_add_id24_wads_table),
    (18, "add_custom_complevel", migrate_add_custom_complevel),
    (19, "add_complevel", migrate_add_complevel),
    (20, "add_session_exit_code", migrate_add_session_exit_code),
    (21, "add_custom_config", migrate_add_custom_config),
    (
        22,
        "merge_custom_complevel_to_complevel",
        migrate_merge_custom_complevel,
    ),
    (
        23,
        "add_companion_tables_and_gc_ignore",
        migrate_add_companion_tables_and_gc_ignore,
    ),
    // Python uses migration 24 for add_gc_ignore (split from Rust's migration 23).
    // Our new migrations start at 25 to avoid collisions with Python-migrated databases.
    (24, "add_gc_ignore_compat_noop", migrate_noop),
    (25, "add_three_axis_columns", migrate_add_three_axis_columns),
    (26, "add_playthroughs_table", migrate_add_playthroughs_table),
    (27, "add_smart_collections", migrate_add_smart_collections),
    (28, "add_wad_analysis_table", migrate_add_wad_analysis_table),
    (
        29,
        "fix_started_dropped_conflict",
        migrate_fix_started_dropped,
    ),
    (
        30,
        "fix_started_queued_conflict",
        migrate_fix_started_queued,
    ),
    (31, "add_zdoom_required", migrate_add_zdoom_required),
    (32, "consolidate_status_columns", migrate_consolidate_status),
    (33, "drop_custom_complevel", migrate_drop_custom_complevel),
    (34, "add_download_urls", migrate_add_download_urls),
    (
        35,
        "add_required_sourceport_family",
        migrate_add_required_sourceport_family,
    ),
    (36, "add_cacowards_table", migrate_add_cacowards_table),
    (
        37,
        "add_cacoward_supported_flag",
        migrate_add_cacoward_supported_flag,
    ),
    (38, "drop_availability", migrate_drop_availability),
    (
        39,
        "backfill_idgames_id_trim_source_url",
        migrate_backfill_idgames_id_trim_source_url,
    ),
    (40, "add_last_known_path", migrate_add_last_known_path),
    (41, "add_file_identity", migrate_add_file_identity),
    (
        42,
        "key_cacowards_on_dataset",
        migrate_key_cacowards_on_dataset,
    ),
    (43, "add_mirror_files", migrate_add_mirror_files),
    (44, "drop_dead_columns", migrate_drop_dead_columns),
];

// ---------------------------------------------------------------------------
// init_db
// ---------------------------------------------------------------------------

/// Initialize the database schema and run pending migrations.
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;

    // Determine which migrations have already been applied
    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // If any migrations are pending, snapshot the DB file to the backup dir
    // first. Transactions already make migrations atomic, but a file-level
    // backup provides a manual recovery story for logic-error migrations
    // that commit successfully yet leave user data in a bad state.
    let has_pending = MIGRATIONS.iter().any(|&(v, _, _)| v > current_version);
    if has_pending {
        backup_before_migration(conn, current_version);
    }

    // Run only pending migrations. Each migration + its version-record INSERT run in
    // one transaction so a failed migration rolls back and is not recorded as applied.
    for &(version, name, func) in MIGRATIONS {
        if version > current_version {
            super::connection::with_transaction(conn, |tx| {
                func(tx)?;
                tx.execute(
                    "INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (?1, ?2)",
                    rusqlite::params![version, name],
                )?;
                Ok(())
            })
            .map_err(|e| {
                crate::Error::MigrationFailed(format!("migration {version} ({name}): {e}"))
            })?;
        }
    }

    // Post-migration indexes
    conn.execute_batch(POST_MIGRATION_INDEXES)?;

    Ok(())
}

/// Copy the live DB file to `pre-migration-{from_version}.db` in
/// [`crate::config::migration_backup_dir`], beside the database itself.
/// Best-effort: logs a warning and continues on failure (a missing backup
/// should not prevent the user from running pending migrations).
fn backup_before_migration(conn: &Connection, from_version: i64) {
    let Some(db_path) = conn.path() else {
        return; // in-memory DB — nothing to back up
    };
    let db_path = std::path::Path::new(db_path);
    if !db_path.exists() {
        return;
    }
    let backup_dir = crate::config::migration_backup_dir(db_path);
    if let Err(e) = std::fs::create_dir_all(&backup_dir) {
        tracing::warn!("failed to create backup dir {backup_dir:?}: {e}");
        return;
    }
    let backup_path = backup_dir.join(format!("pre-migration-{from_version}.db"));
    match std::fs::copy(db_path, &backup_path) {
        Ok(_) => tracing::info!("created pre-migration backup at {backup_path:?}"),
        Err(e) => tracing::warn!("failed to create pre-migration backup at {backup_path:?}: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a column exists in a table.
fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql)?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(columns.contains(&column.to_string()))
}

/// Check if a table exists.
fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Add a column if it doesn't already exist.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    col_type: &str,
) -> Result<()> {
    if !has_column(conn, table, column)? {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {col_type}"
        ))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Migration functions
// ---------------------------------------------------------------------------

fn migrate_noop(_conn: &Connection) -> Result<()> {
    Ok(())
}

fn migrate_add_custom_play_config(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "custom_iwad", "TEXT")?;
    add_column_if_missing(conn, "wads", "custom_sourceport", "TEXT")?;
    add_column_if_missing(conn, "wads", "custom_args", "TEXT")?;
    Ok(())
}

fn migrate_add_wad_completions(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "wad_completions")? {
        conn.execute_batch(
            "CREATE TABLE wad_completions (
                id INTEGER PRIMARY KEY,
                wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
                completed_at TEXT DEFAULT CURRENT_TIMESTAMP,
                stats_snapshot TEXT,
                notes TEXT
            );
            CREATE INDEX idx_wad_completions_wad_id ON wad_completions(wad_id);",
        )?;
    }
    Ok(())
}

fn migrate_rename_wishlist_to_toplay(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE wads SET status = 'to-play' WHERE status = 'wishlist'",
        [],
    )?;
    Ok(())
}

fn migrate_add_deleted_at(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "deleted_at", "TEXT")
}

fn migrate_add_version(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "version", "TEXT")
}

fn migrate_add_idgames_id(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "idgames_id", "TEXT")
}

fn migrate_drop_map_completions(conn: &Connection) -> Result<()> {
    conn.execute_batch("DROP TABLE IF EXISTS map_completions")?;
    Ok(())
}

fn migrate_add_iwads_table(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "iwads")? {
        conn.execute_batch(
            "CREATE TABLE iwads (
                id INTEGER PRIMARY KEY,
                family TEXT NOT NULL,
                variant TEXT NOT NULL DEFAULT 'unknown',
                title TEXT,
                path TEXT NOT NULL,
                md5 TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(family, variant)
            )",
        )?;
    }
    Ok(())
}

fn migrate_iwads_family_variant(conn: &Connection) -> Result<()> {
    // Check if already migrated (has 'family' column) or was created fresh
    if has_column(conn, "iwads", "family")? {
        return Ok(());
    }
    // Old schema had 'name' column — this migration restructures to family/variant.
    // If 'name' column doesn't exist either, the table was created fresh by migration 8.
    if !has_column(conn, "iwads", "name")? {
        return Ok(());
    }
    // Read old rows, drop table, recreate with new schema, re-insert.
    // For the Rust port we don't import KNOWN_IWADS here — just set variant to "unknown".
    type OldIwadRow = (
        i64,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
    );
    let old_rows: Vec<OldIwadRow> = {
        let mut stmt = conn.prepare("SELECT id, name, title, path, md5, created_at FROM iwads")?;
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    conn.execute_batch("DROP TABLE iwads")?;
    conn.execute_batch(
        "CREATE TABLE iwads (
            id INTEGER PRIMARY KEY,
            family TEXT NOT NULL,
            variant TEXT NOT NULL DEFAULT 'unknown',
            title TEXT,
            path TEXT NOT NULL,
            md5 TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(family, variant)
        )",
    )?;

    let mut insert = conn.prepare(
        "INSERT OR IGNORE INTO iwads (family, variant, title, path, md5, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (_, family, title, path, md5, created_at) in &old_rows {
        insert.execute(rusqlite::params![
            family, "unknown", title, path, md5, created_at
        ])?;
    }

    Ok(())
}

fn migrate_add_stats_snapshot(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "stats_snapshot", "TEXT")
}

fn migrate_add_companion_files(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "companion_files", "TEXT")
}

fn migrate_add_session_stats(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "sessions", "stats_before", "TEXT")?;
    add_column_if_missing(conn, "sessions", "stats_after", "TEXT")?;
    Ok(())
}

fn migrate_add_demo_file(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "sessions", "demo_file", "TEXT")
}

fn migrate_add_id24_wads_table(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "id24_wads")? {
        conn.execute_batch(
            "CREATE TABLE id24_wads (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                version TEXT,
                title TEXT,
                path TEXT NOT NULL,
                md5 TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
        )?;
    }
    Ok(())
}

fn migrate_add_custom_complevel(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "custom_complevel", "TEXT")
}

fn migrate_add_complevel(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "complevel", "INTEGER")
}

fn migrate_add_session_exit_code(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "sessions", "exit_code", "INTEGER")
}

fn migrate_add_custom_config(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "custom_config", "TEXT")
}

fn migrate_merge_custom_complevel(conn: &Connection) -> Result<()> {
    if !has_column(conn, "wads", "custom_complevel")? {
        return Ok(());
    }
    conn.execute_batch(
        "UPDATE wads
         SET complevel = CAST(custom_complevel AS INTEGER)
         WHERE custom_complevel IS NOT NULL
           AND complevel IS NULL",
    )?;
    Ok(())
}

/// Drop the `availability` column.
///
/// It duplicated what `cached_path` + `source_url` already say, and nothing
/// kept the copy honest: `clear_cached_path` nulls the path with raw SQL, so
/// every cache eviction left a row still reading `cached`. `Availability` is
/// derived on read now, which is why there is nothing to backfill here.
fn migrate_drop_availability(conn: &Connection) -> Result<()> {
    if has_column(conn, "wads", "availability")? {
        conn.execute("ALTER TABLE wads DROP COLUMN availability", [])?;
    }
    Ok(())
}

/// Make `idgames_id` the answer to "is this on idgames" for every row, and strip
/// the whitespace some `source_url`s came in with, which the builders now refuse.
///
/// The idgames importer only ever wrote `source_id`, while every reader of
/// "this WAD's idgames id" looks at `idgames_id`, so the Cacoward auto-linker
/// could not match the very WADs it exists for. Trashed rows are included
/// because they can come back.
fn migrate_backfill_idgames_id_trim_source_url(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "UPDATE wads SET idgames_id = source_id
         WHERE source_type = 'idgames' AND COALESCE(idgames_id, '') = '';
         UPDATE wads SET source_url = TRIM(source_url, char(9, 10, 11, 12, 13, 32))
         WHERE source_url <> TRIM(source_url, char(9, 10, 11, 12, 13, 32));",
    )?;
    Ok(())
}

/// Add `last_known_path`: where a WAD's file was the last time caco found it.
///
/// A record of history rather than a copy of something derivable, so it does
/// not bring back the drift migration 38 removed — see `crate::presence`, its
/// only writer. Nothing is backfilled: the startup check that runs after
/// `init_db` finds the dangling paths, and finding them here would put a `stat`,
/// which blocks for as long as a dead network mount does, inside a migration.
fn migrate_add_last_known_path(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "last_known_path", "TEXT")
}

/// Record what each WAD's file is, not only where (#52): see `crate::identity`,
/// the only writer of every column here.
///
/// On `wads`, the file's size, SHA-256 and SHA-1, the modification time it was
/// hashed at (which, with the size, says whether it has been replaced since),
/// and the hash of what a sourceport loads from it. `wad_files` lists the
/// loadable files inside an archive, and each session records what it loaded.
///
/// Nothing is backfilled here, for migration 40's reason and then some: the
/// startup check that runs after `init_db` reads every file through, which
/// would hold a migration for as long as a library takes to read.
fn migrate_add_file_identity(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "file_size", "INTEGER")?;
    add_column_if_missing(conn, "wads", "file_sha256", "TEXT")?;
    add_column_if_missing(conn, "wads", "file_sha1", "TEXT")?;
    add_column_if_missing(conn, "wads", "file_mtime", "INTEGER")?;
    add_column_if_missing(conn, "wads", "content_sha256", "TEXT")?;
    add_column_if_missing(conn, "sessions", "content_sha256", "TEXT")?;
    if !table_exists(conn, "wad_files")? {
        conn.execute_batch(
            "CREATE TABLE wad_files (
                wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
                inner_path TEXT NOT NULL,
                size INTEGER NOT NULL,
                sha256 TEXT NOT NULL,
                sha1 TEXT NOT NULL,
                PRIMARY KEY (wad_id, inner_path)
            );",
        )?;
    }
    Ok(())
}

fn migrate_drop_custom_complevel(conn: &Connection) -> Result<()> {
    if has_column(conn, "wads", "custom_complevel")? {
        conn.execute("ALTER TABLE wads DROP COLUMN custom_complevel", [])?;
    }
    Ok(())
}

fn migrate_add_companion_tables_and_gc_ignore(conn: &Connection) -> Result<()> {
    // Companion files registry (MD5-deduplicated storage)
    if !table_exists(conn, "companion_files_registry")? {
        conn.execute_batch(
            "CREATE TABLE companion_files_registry (
                id INTEGER PRIMARY KEY,
                md5 TEXT NOT NULL UNIQUE,
                filename TEXT NOT NULL,
                path TEXT NOT NULL,
                size INTEGER NOT NULL DEFAULT 0
            )",
        )?;
    }

    // WAD ↔ companion junction table
    if !table_exists(conn, "wad_companions")? {
        conn.execute_batch(
            "CREATE TABLE wad_companions (
                wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
                companion_id INTEGER NOT NULL REFERENCES companion_files_registry(id) ON DELETE CASCADE,
                enabled INTEGER NOT NULL DEFAULT 1,
                load_order INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (wad_id, companion_id)
            );
            CREATE INDEX idx_wad_companions_wad_id ON wad_companions(wad_id);
            CREATE INDEX idx_wad_companions_companion_id ON wad_companions(companion_id);",
        )?;
    }

    // gc_ignore column on wads table
    add_column_if_missing(conn, "wads", "gc_ignore", "INTEGER DEFAULT 0")?;

    Ok(())
}

fn migrate_add_three_axis_columns(conn: &Connection) -> Result<()> {
    // Add three new axis columns
    add_column_if_missing(conn, "wads", "play_state", "TEXT DEFAULT 'unplayed'")?;
    add_column_if_missing(conn, "wads", "intent", "TEXT DEFAULT 'inbox'")?;
    add_column_if_missing(conn, "wads", "availability", "TEXT DEFAULT 'unavailable'")?;

    // Backfill play_state and intent from existing status
    conn.execute_batch(
        "UPDATE wads SET play_state = 'unplayed', intent = 'queued'  WHERE status = 'to-play';
         UPDATE wads SET play_state = 'unplayed', intent = 'shelved' WHERE status = 'backlog';
         UPDATE wads SET play_state = 'started',  intent = 'queued'  WHERE status = 'playing';
         UPDATE wads SET play_state = 'completed', intent = 'shelved' WHERE status = 'finished';
         UPDATE wads SET play_state = 'unplayed', intent = 'dropped' WHERE status = 'abandoned';
         UPDATE wads SET play_state = 'unplayed', intent = 'shelved' WHERE status = 'awaiting-update';",
    )?;

    // Backfill availability from cached_path and source_url
    conn.execute_batch(
        "UPDATE wads SET availability = 'cached'       WHERE cached_path IS NOT NULL;
         UPDATE wads SET availability = 'downloadable' WHERE cached_path IS NULL AND source_url IS NOT NULL;
         UPDATE wads SET availability = 'unavailable'  WHERE cached_path IS NULL AND source_url IS NULL;",
    )?;

    // Add awaiting-update tag to WADs that had that status
    conn.execute(
        "INSERT OR IGNORE INTO tags (wad_id, tag)
         SELECT id, 'awaiting-update' FROM wads WHERE status = 'awaiting-update'",
        [],
    )?;

    Ok(())
}

fn migrate_add_smart_collections(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "smart_collections")? {
        conn.execute_batch(
            "CREATE TABLE smart_collections (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                query TEXT NOT NULL,
                sort_by TEXT,
                sort_desc INTEGER DEFAULT 1,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );",
        )?;
    }
    Ok(())
}

fn migrate_add_wad_analysis_table(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "wad_analysis")? {
        conn.execute_batch(
            "CREATE TABLE wad_analysis (
                wad_id INTEGER PRIMARY KEY REFERENCES wads(id) ON DELETE CASCADE,
                total_maps INTEGER NOT NULL DEFAULT 0,
                required_maps INTEGER NOT NULL DEFAULT 0,
                secret_maps TEXT,
                terminal_map TEXT,
                has_umapinfo INTEGER DEFAULT 0,
                analysis_json TEXT,
                expected_map_count INTEGER,
                analyzed_at TEXT DEFAULT CURRENT_TIMESTAMP
            );",
        )?;
    }
    Ok(())
}

fn migrate_add_playthroughs_table(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "playthroughs")? {
        conn.execute_batch(
            "CREATE TABLE playthroughs (
                id INTEGER PRIMARY KEY,
                wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
                started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                completed_at TEXT,
                stats_snapshot TEXT,
                notes TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX idx_playthroughs_wad_id ON playthroughs(wad_id);
            CREATE INDEX idx_playthroughs_completed ON playthroughs(wad_id, completed_at);",
        )?;
    }

    // Add playthrough_id column to sessions
    add_column_if_missing(
        conn,
        "sessions",
        "playthrough_id",
        "INTEGER REFERENCES playthroughs(id)",
    )?;

    // Synthesize playthroughs from existing wad_completions
    // For each completion, create a completed playthrough.
    conn.execute_batch(
        "INSERT INTO playthroughs (wad_id, started_at, completed_at, stats_snapshot, notes, created_at)
         SELECT wad_id, completed_at, completed_at, stats_snapshot, notes, completed_at
         FROM wad_completions
         ORDER BY completed_at ASC;",
    )?;

    // For WADs currently 'playing' with sessions but no completions, create an active playthrough.
    conn.execute_batch(
        "INSERT INTO playthroughs (wad_id, started_at)
         SELECT DISTINCT s.wad_id, MIN(s.started_at)
         FROM sessions s
         JOIN wads w ON w.id = s.wad_id
         WHERE w.status = 'playing'
           AND w.id NOT IN (SELECT DISTINCT wad_id FROM wad_completions)
         GROUP BY s.wad_id;",
    )?;

    // Associate sessions with their nearest playthrough (best-effort by wad_id + chronological order).
    // For each session, find the playthrough for the same wad that started before or at the session start.
    conn.execute(
        "UPDATE sessions SET playthrough_id = (
            SELECT p.id FROM playthroughs p
            WHERE p.wad_id = sessions.wad_id
              AND p.started_at <= sessions.started_at
            ORDER BY p.started_at DESC
            LIMIT 1
        )
        WHERE playthrough_id IS NULL",
        [],
    )?;

    Ok(())
}

fn migrate_fix_started_dropped(conn: &Connection) -> Result<()> {
    // started + dropped is contradictory: "playing" and "abandoned" at once.
    // Normalize to unplayed + dropped (the base abandoned state).
    conn.execute(
        "UPDATE wads SET play_state = 'unplayed'
         WHERE play_state = 'started' AND intent = 'dropped'",
        [],
    )?;
    Ok(())
}

fn migrate_fix_started_queued(conn: &Connection) -> Result<()> {
    // started + queued is contradictory: playing and queued are mutually exclusive.
    // Move playing WADs out of the queue to shelved.
    conn.execute(
        "UPDATE wads SET intent = 'shelved'
         WHERE play_state = 'started' AND intent = 'queued'",
        [],
    )?;
    Ok(())
}

fn migrate_add_zdoom_required(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "zdoom_required", "INTEGER")
}

/// Add `download_urls` — a JSON-encoded array of candidate download URLs
/// scraped from Doomworld threads (or anywhere else we find them). Lets the
/// player / cache retry against the next URL when a host goes down, and gives
/// the user somewhere to copy from if auto-download isn't wired up.
fn migrate_add_download_urls(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "download_urls", "TEXT")
}

fn migrate_add_required_sourceport_family(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "wads", "required_sourceport_family", "TEXT")
}

/// Cacowards: yearly "best WAD" awards from Doomworld, scraped from the Doom
/// Wiki. The table stands alone (entries exist whether or not the user owns
/// the WAD) so the completion-rate view can compute "x of N runners-up beaten."
/// `wad_id` is best-effort auto-linked from `idgames_url`; `manual_override`
/// pins a link so subsequent enrichment scrapes don't clobber a user's choice.
fn migrate_add_cacowards_table(conn: &Connection) -> Result<()> {
    if !table_exists(conn, "cacowards")? {
        conn.execute_batch(
            "CREATE TABLE cacowards (
                id INTEGER PRIMARY KEY,
                year INTEGER NOT NULL,
                category TEXT NOT NULL,
                rank INTEGER,
                wad_title TEXT NOT NULL,
                wad_author TEXT,
                idgames_url TEXT,
                doomwiki_url TEXT,
                blurb TEXT,
                wad_id INTEGER REFERENCES wads(id) ON DELETE SET NULL,
                manual_override INTEGER NOT NULL DEFAULT 0,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(year, category, wad_title)
            );
            CREATE INDEX idx_cacowards_wad_id ON cacowards(wad_id);
            CREATE INDEX idx_cacowards_year ON cacowards(year);",
        )?;
    }
    Ok(())
}

/// Cacoward entries the user has flagged as "not playable from caco" —
/// usually because they require an IWAD or sourceport family caco doesn't
/// yet wire up (Doom 64, Hedon, sometimes Heretic/Hexen depending on setup).
/// `supported = 0` rows still display in the magazine view but are excluded
/// from completion totals so the year's progress bar reflects only entries
/// the user can actually play.
fn migrate_add_cacoward_supported_flag(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "cacowards", "supported", "INTEGER NOT NULL DEFAULT 1")
}

/// Key the table on the curated dataset's `key` instead of
/// `UNIQUE(year, category, wad_title)`, under which a corrected title turned a
/// pinned row into a duplicate (#53).
///
/// A rebuild, because SQLite cannot drop a table's UNIQUE constraint. A row gets
/// the key its year, category and title would have been curated under, which is
/// how it meets its entry when the dataset is first written: what the user did to
/// it (the link, its pin, an unsupported flag) comes across, and the dataset
/// overwrites the rest. `supported = 1` was only ever the default, so it comes
/// across as no opinion, and a `0` is the user's.
///
/// The old identifier columns are deliberately left behind, and `blurb` with them,
/// which nothing ever wrote. The dataset's identifiers arriving in the now-empty
/// columns is what makes the first sync consider each row for linking once, which
/// is the only chance a row has if its WAD was imported before linking ran at import.
fn migrate_key_cacowards_on_dataset(conn: &Connection) -> Result<()> {
    if has_column(conn, "cacowards", "key")? {
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TABLE cacowards_keyed (
            id INTEGER PRIMARY KEY,
            key TEXT NOT NULL UNIQUE,
            year INTEGER NOT NULL,
            category TEXT NOT NULL,
            page_order INTEGER NOT NULL,
            wad_title TEXT NOT NULL,
            wad_author TEXT,
            idgames_id INTEGER,
            doomworld_id INTEGER,
            doomwiki_page TEXT,
            sha256 TEXT,
            wad_id INTEGER REFERENCES wads(id) ON DELETE SET NULL,
            manual_override INTEGER NOT NULL DEFAULT 0,
            supported_default INTEGER NOT NULL DEFAULT 1,
            supported_override INTEGER,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        );",
    )?;

    struct Row {
        id: i64,
        year: i64,
        category: String,
        rank: Option<i64>,
        title: String,
    }
    let rows = {
        let mut stmt =
            conn.prepare("SELECT id, year, category, rank, wad_title FROM cacowards ORDER BY id")?;
        stmt.query_map([], |row| {
            Ok(Row {
                id: row.get(0)?,
                year: row.get(1)?,
                category: row.get(2)?,
                rank: row.get(3)?,
                title: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    let mut taken = std::collections::HashSet::new();
    for row in rows {
        let mut key = crate::cacowards::entry_key(row.year, &row.category, &row.title);
        // The old constraint let two titles that differ only in punctuation
        // through, and the key cannot tell those apart. The first keeps the key.
        if !taken.insert(key.clone()) {
            key = format!("{key}-{}", row.id);
            taken.insert(key.clone());
        }
        conn.execute(
            "INSERT INTO cacowards_keyed
                 (id, key, year, category, page_order, wad_title, wad_author, wad_id,
                  manual_override, supported_override, created_at, updated_at)
             SELECT id, ?2, year, category, ?3, wad_title, wad_author, wad_id, manual_override,
                    CASE supported WHEN 0 THEN 0 END, created_at, updated_at
             FROM cacowards WHERE id = ?1",
            rusqlite::params![row.id, key, row.rank.unwrap_or(9999)],
        )?;
    }

    conn.execute_batch(
        "DROP TABLE cacowards;
         ALTER TABLE cacowards_keyed RENAME TO cacowards;
         CREATE INDEX idx_cacowards_wad_id ON cacowards(wad_id);
         CREATE INDEX idx_cacowards_year ON cacowards(year);",
    )?;
    Ok(())
}

/// Add the tables each mirror's index is synced into: see `crate::db::mirrors`,
/// which also says why they are not a flag on `wads`.
///
/// An index per column the match compares, so that it stays one probe per kind of
/// evidence on a query that runs for every row of the library. The primary key
/// leads with the mirror, which the hash lookups do not have.
fn migrate_add_mirror_files(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mirror_files (
            mirror TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            size INTEGER NOT NULL,
            filename TEXT NOT NULL,
            stored_as TEXT,
            content_sha256 TEXT NOT NULL,
            idgames_id TEXT,
            doomworld_topic INTEGER,
            doomwiki_page TEXT,
            added TEXT,
            PRIMARY KEY (mirror, sha256)
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS idx_mirror_files_sha256 ON mirror_files(sha256);
         CREATE INDEX IF NOT EXISTS idx_mirror_files_content ON mirror_files(content_sha256);
         CREATE INDEX IF NOT EXISTS idx_mirror_files_idgames ON mirror_files(idgames_id);
         CREATE INDEX IF NOT EXISTS idx_mirror_files_topic ON mirror_files(doomworld_topic);
         CREATE INDEX IF NOT EXISTS idx_mirror_files_page ON mirror_files(doomwiki_page);

         CREATE TABLE IF NOT EXISTS mirror_file_cacowards (
            mirror TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            cacoward_key TEXT NOT NULL,
            PRIMARY KEY (mirror, sha256, cacoward_key),
            FOREIGN KEY (mirror, sha256) REFERENCES mirror_files(mirror, sha256)
                ON DELETE CASCADE
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS idx_mirror_file_cacowards_key
            ON mirror_file_cacowards(cacoward_key);",
    )?;
    Ok(())
}

/// Drop two columns nothing has written since the Python port: `wads.companion_files`,
/// superseded by `wad_companions`, and `sessions.demo_file`, which demos never used,
/// since they are found on disk.
fn migrate_drop_dead_columns(conn: &Connection) -> Result<()> {
    if has_column(conn, "wads", "companion_files")? {
        conn.execute("ALTER TABLE wads DROP COLUMN companion_files", [])?;
    }
    if has_column(conn, "sessions", "demo_file")? {
        conn.execute("ALTER TABLE sessions DROP COLUMN demo_file", [])?;
    }
    Ok(())
}

fn migrate_consolidate_status(conn: &Connection) -> Result<()> {
    // Step 1: Map old status values to new ones
    conn.execute_batch(
        "UPDATE wads SET status = 'unplayed'    WHERE status IN ('backlog', 'to-play', 'awaiting-update');
         UPDATE wads SET status = 'in-progress' WHERE status = 'playing';
         UPDATE wads SET status = 'completed'   WHERE status = 'finished';",
    )?;
    // 'abandoned' stays as 'abandoned' — no change needed

    // Step 2: Override based on play_state/intent where more accurate
    if has_column(conn, "wads", "play_state")? {
        conn.execute_batch(
            "UPDATE wads SET status = 'in-progress' WHERE play_state = 'started' AND status != 'abandoned';
             UPDATE wads SET status = 'completed'   WHERE play_state = 'completed' AND status != 'abandoned';",
        )?;
    }
    if has_column(conn, "wads", "intent")? {
        conn.execute(
            "UPDATE wads SET status = 'abandoned' WHERE intent = 'dropped'",
            [],
        )?;
    }

    // Step 3: Drop indexes and the now-unused columns
    if has_column(conn, "wads", "play_state")? {
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_wads_play_state;
             ALTER TABLE wads DROP COLUMN play_state",
        )?;
    }
    if has_column(conn, "wads", "intent")? {
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_wads_intent;
             ALTER TABLE wads DROP COLUMN intent",
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::open_memory;

    #[test]
    fn test_init_db_fresh() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        // Verify tables exist
        assert!(table_exists(&conn, "wads").unwrap());
        assert!(table_exists(&conn, "tags").unwrap());
        assert!(table_exists(&conn, "sessions").unwrap());
        assert!(table_exists(&conn, "wad_completions").unwrap());
        assert!(table_exists(&conn, "schema_migrations").unwrap());
        assert!(table_exists(&conn, "iwads").unwrap());
        assert!(table_exists(&conn, "id24_wads").unwrap());
        assert!(table_exists(&conn, "companion_files_registry").unwrap());
        assert!(table_exists(&conn, "wad_companions").unwrap());
        assert!(table_exists(&conn, "playthroughs").unwrap());
        assert!(table_exists(&conn, "smart_collections").unwrap());
        assert!(table_exists(&conn, "wad_analysis").unwrap());
        assert!(table_exists(&conn, "cacowards").unwrap());
        assert!(table_exists(&conn, "mirror_files").unwrap());
        assert!(table_exists(&conn, "mirror_file_cacowards").unwrap());
    }

    #[test]
    fn test_dead_columns_are_gone() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        assert!(!has_column(&conn, "wads", "companion_files").unwrap());
        assert!(!has_column(&conn, "sessions", "demo_file").unwrap());
    }

    #[test]
    fn test_init_db_idempotent() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        // Running again should be a no-op
        init_db(&conn).unwrap();

        // All migrations should be recorded
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, MIGRATIONS.len() as i64);
    }

    #[test]
    fn test_all_wad_columns_exist() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        // Check all expected columns after migrations
        let expected_columns = [
            "id",
            "title",
            "author",
            "year",
            "description",
            "status",
            "rating",
            "notes",
            "source_type",
            "source_id",
            "source_url",
            "idgames_id",
            "filename",
            "cached_path",
            "last_known_path",
            "file_size",
            "file_sha256",
            "file_sha1",
            "file_mtime",
            "content_sha256",
            "custom_iwad",
            "custom_sourceport",
            "required_sourceport_family",
            "custom_args",
            "created_at",
            "updated_at",
            "deleted_at",
            "version",
            "stats_snapshot",
            "complevel",
            "custom_config",
            "gc_ignore",
            "zdoom_required",
            "download_urls",
        ];
        for col in &expected_columns {
            assert!(
                has_column(&conn, "wads", col).unwrap(),
                "missing column: {col}"
            );
        }
    }

    #[test]
    fn test_migration_add_required_sourceport_family() {
        let conn = open_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE wads (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT DEFAULT 'unplayed',
                source_type TEXT NOT NULL,
                source_id TEXT,
                source_url TEXT,
                idgames_id TEXT,
                cached_path TEXT,
                deleted_at TEXT
            );
            CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO schema_migrations (version, name)
            VALUES (34, 'add_download_urls');",
        )
        .unwrap();

        init_db(&conn).unwrap();

        assert!(
            has_column(&conn, "wads", "required_sourceport_family").unwrap(),
            "migration should add required_sourceport_family"
        );
    }

    #[test]
    fn test_migration_backfills_idgames_id_and_trims_source_url() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO wads (id, title, source_type, source_id, idgames_id, source_url, deleted_at) VALUES
                (1, 'unset', 'idgames', '21511', NULL, 'https://www.doomworld.com/idgames/ne2', NULL),
                (2, 'empty', 'idgames', '21233', '', NULL, NULL),
                (3, 'already set', 'idgames', '100', '200', NULL, NULL),
                (4, 'trashed', 'idgames', '19951', NULL, NULL, '2026-01-01'),
                (5, 'no id to copy', 'idgames', NULL, NULL, NULL, NULL),
                (6, 'other source', 'doomworld', '149498', NULL,
                    'https://www.doomworld.com/vb/thread/149498' || char(13), NULL),
                (7, 'both ends', 'url', NULL, NULL,
                    char(32, 9) || 'https://example.com/a b.zip' || char(13, 10), NULL);
             -- `init_db` resumes after the highest recorded version, so every
             -- later one has to go too for 39 to run again.
             DELETE FROM schema_migrations WHERE version >= 39;",
        )
        .unwrap();

        init_db(&conn).unwrap();

        let column = |id: i64, name: &str| -> Option<String> {
            conn.query_row(
                &format!("SELECT {name} FROM wads WHERE id = ?1"),
                [id],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(column(1, "idgames_id").as_deref(), Some("21511"));
        assert_eq!(column(2, "idgames_id").as_deref(), Some("21233"));
        assert_eq!(column(3, "idgames_id").as_deref(), Some("200"));
        assert_eq!(column(4, "idgames_id").as_deref(), Some("19951"));
        assert_eq!(column(5, "idgames_id"), None);
        // A Doomworld thread id is not an idgames id.
        assert_eq!(column(6, "idgames_id"), None);

        assert_eq!(
            column(1, "source_url").as_deref(),
            Some("https://www.doomworld.com/idgames/ne2")
        );
        assert_eq!(
            column(6, "source_url").as_deref(),
            Some("https://www.doomworld.com/vb/thread/149498")
        );
        assert_eq!(
            column(7, "source_url").as_deref(),
            Some("https://example.com/a b.zip")
        );
        assert_eq!(column(2, "source_url"), None);
    }

    /// A library at schema 41, whose `cacowards` table a scrape filled.
    fn library_before_the_dataset() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute_batch(
            "DROP TABLE cacowards;
             CREATE TABLE cacowards (
                id INTEGER PRIMARY KEY,
                year INTEGER NOT NULL,
                category TEXT NOT NULL,
                rank INTEGER,
                wad_title TEXT NOT NULL,
                wad_author TEXT,
                idgames_url TEXT,
                doomwiki_url TEXT,
                blurb TEXT,
                wad_id INTEGER REFERENCES wads(id) ON DELETE SET NULL,
                manual_override INTEGER NOT NULL DEFAULT 0,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                supported INTEGER NOT NULL DEFAULT 1,
                UNIQUE(year, category, wad_title)
             );
             DELETE FROM schema_migrations WHERE version >= 42;
             INSERT INTO wads (id, title, source_type) VALUES (1, 'Lonesome Road | Chapter 1', 'doomworld');
             INSERT INTO cacowards
                 (id, year, category, rank, wad_title, wad_author, idgames_url, doomwiki_url,
                  wad_id, manual_override, supported) VALUES
                 (10, 2025, 'winner', 8, 'Lonesome Road', 'tourniquet', NULL,
                  'https://doomwiki.org/wiki/Lonesome_Road', 1, 1, 1),
                 (11, 2023, 'mordeth', 1, 'TNT2 MAP26', NULL, NULL, NULL, NULL, 0, 0),
                 (12, 2023, 'winner', 2, 'Piña Colada', NULL,
                  'https://www.doomworld.com/idgames/?id=20917', NULL, 1, 0, 1),
                 (13, 2023, 'winner', NULL, 'Unranked', NULL, NULL, NULL, NULL, 0, 1),
                 (14, 2023, 'winner', 3, 'Same Name!', NULL, NULL, NULL, NULL, 0, 1),
                 (15, 2023, 'winner', 4, 'Same Name?', NULL, NULL, NULL, NULL, 1, 1);",
        )
        .unwrap();
        conn
    }

    fn keyed_row(conn: &Connection, id: i64) -> crate::db::cacowards::CacowardRecord {
        crate::db::cacowards::get_cacoward(conn, id)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn test_migration_keys_rows_the_way_the_dataset_does() {
        let conn = library_before_the_dataset();
        init_db(&conn).unwrap();

        assert_eq!(keyed_row(&conn, 10).key, "2025-winner-lonesome-road");
        assert_eq!(keyed_row(&conn, 11).key, "2023-mordeth-tnt2-map26");
        assert_eq!(keyed_row(&conn, 12).key, "2023-winner-pina-colada");
    }

    #[test]
    fn test_migration_keeps_what_the_user_did_to_a_row() {
        let conn = library_before_the_dataset();
        init_db(&conn).unwrap();

        let pinned = keyed_row(&conn, 10);
        assert_eq!(pinned.wad_id, Some(1));
        assert!(pinned.manual_override);
        assert_eq!(pinned.wad_title, "Lonesome Road");
        assert_eq!(pinned.page_order, 8);
        assert_eq!(pinned.wad_author.as_deref(), Some("tourniquet"));

        // An auto-link is kept, and is still not a pin.
        let auto_linked = keyed_row(&conn, 12);
        assert_eq!(auto_linked.wad_id, Some(1));
        assert!(!auto_linked.manual_override);
    }

    #[test]
    fn test_migration_reads_supported_as_the_users_word_only_when_it_was_off() {
        let conn = library_before_the_dataset();
        init_db(&conn).unwrap();

        let flagged = keyed_row(&conn, 11);
        assert_eq!(flagged.supported_override, Some(false));
        assert!(!flagged.supported);

        // 1 was the column's default, so it says nothing the data may not overrule.
        let unflagged = keyed_row(&conn, 10);
        assert_eq!(unflagged.supported_override, None);
        assert!(unflagged.supported);
    }

    #[test]
    fn test_migration_leaves_identifiers_for_the_dataset_to_supply() {
        // Their arrival is what makes the first sync consider each row for linking.
        let conn = library_before_the_dataset();
        init_db(&conn).unwrap();

        let row = keyed_row(&conn, 12);
        assert_eq!(row.idgames_id, None);
        assert_eq!(row.doomwiki_page, None);
        assert!(!has_column(&conn, "cacowards", "blurb").unwrap());
        assert!(!has_column(&conn, "cacowards", "idgames_url").unwrap());
    }

    #[test]
    fn test_migration_survives_titles_the_key_cannot_tell_apart() {
        let conn = library_before_the_dataset();
        init_db(&conn).unwrap();

        assert_eq!(keyed_row(&conn, 14).key, "2023-winner-same-name");
        assert_eq!(keyed_row(&conn, 15).key, "2023-winner-same-name-15");
        assert!(keyed_row(&conn, 15).manual_override);
    }

    #[test]
    fn test_migration_gives_a_row_with_no_rank_a_place_at_the_end() {
        let conn = library_before_the_dataset();
        init_db(&conn).unwrap();
        assert_eq!(keyed_row(&conn, 13).page_order, 9999);
    }

    #[test]
    fn test_migration_keeps_the_foreign_key_and_indexes() {
        let conn = library_before_the_dataset();
        init_db(&conn).unwrap();

        conn.execute("DELETE FROM wads WHERE id = 1", []).unwrap();
        assert_eq!(keyed_row(&conn, 10).wad_id, None);

        let indexes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name IN ('idx_cacowards_wad_id', 'idx_cacowards_year')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 2);
    }

    #[test]
    fn test_a_snapshot_goes_beside_the_database_it_is_of() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_connection(&dir.path().join("library.db")).unwrap();

        init_db(&conn).unwrap();

        assert!(dir.path().join("backups/pre-migration-0.db").exists());
    }

    #[test]
    fn test_each_schema_version_keeps_its_own_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_connection(&dir.path().join("library.db")).unwrap();
        init_db(&conn).unwrap();

        let latest = MIGRATIONS.last().unwrap().0;
        conn.execute(
            "DELETE FROM schema_migrations WHERE version >= ?1",
            [latest],
        )
        .unwrap();
        init_db(&conn).unwrap();

        let backups = dir.path().join("backups");
        assert!(backups.join("pre-migration-0.db").exists());
        assert!(
            backups
                .join(format!("pre-migration-{}.db", latest - 1))
                .exists()
        );
    }

    #[test]
    fn test_key_is_unique_and_title_no_longer_is() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        let insert = |key: &str| {
            conn.execute(
                "INSERT INTO cacowards (key, year, category, page_order, wad_title)
                 VALUES (?1, 2023, 'winner', 1, 'Same Title')",
                [key],
            )
        };
        insert("a").unwrap();
        insert("b").unwrap();
        assert!(insert("a").is_err());
    }

    #[test]
    fn test_session_columns_exist() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        let expected = [
            "id",
            "wad_id",
            "started_at",
            "ended_at",
            "duration_seconds",
            "sourceport",
            "notes",
            "stats_before",
            "stats_after",
            "exit_code",
            "playthrough_id",
            "content_sha256",
        ];
        for col in &expected {
            assert!(
                has_column(&conn, "sessions", col).unwrap(),
                "missing session column: {col}"
            );
        }
    }

    #[test]
    fn test_iwads_table_schema() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        assert!(has_column(&conn, "iwads", "family").unwrap());
        assert!(has_column(&conn, "iwads", "variant").unwrap());
        assert!(has_column(&conn, "iwads", "title").unwrap());
        assert!(has_column(&conn, "iwads", "path").unwrap());
        assert!(has_column(&conn, "iwads", "md5").unwrap());
    }

    #[test]
    fn test_id24_wads_table_schema() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        assert!(has_column(&conn, "id24_wads", "name").unwrap());
        assert!(has_column(&conn, "id24_wads", "version").unwrap());
        assert!(has_column(&conn, "id24_wads", "title").unwrap());
        assert!(has_column(&conn, "id24_wads", "path").unwrap());
        assert!(has_column(&conn, "id24_wads", "md5").unwrap());
    }

    #[test]
    fn test_companion_files_registry_schema() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        assert!(has_column(&conn, "companion_files_registry", "id").unwrap());
        assert!(has_column(&conn, "companion_files_registry", "md5").unwrap());
        assert!(has_column(&conn, "companion_files_registry", "filename").unwrap());
        assert!(has_column(&conn, "companion_files_registry", "path").unwrap());
        assert!(has_column(&conn, "companion_files_registry", "size").unwrap());
    }

    #[test]
    fn test_wad_companions_schema() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        assert!(has_column(&conn, "wad_companions", "wad_id").unwrap());
        assert!(has_column(&conn, "wad_companions", "companion_id").unwrap());
        assert!(has_column(&conn, "wad_companions", "enabled").unwrap());
        assert!(has_column(&conn, "wad_companions", "load_order").unwrap());
    }

    #[test]
    fn test_wad_files_schema() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        for column in ["wad_id", "inner_path", "size", "sha256", "sha1"] {
            assert!(
                has_column(&conn, "wad_files", column).unwrap(),
                "missing wad_files column: {column}"
            );
        }

        // The rows describe a WAD's file, so they go with the WAD.
        conn.execute_batch(
            "INSERT INTO wads (id, title, source_type) VALUES (1, 'W', 'local');
             INSERT INTO wad_files (wad_id, inner_path, size, sha256, sha1)
             VALUES (1, 'w.wad', 4, 'a', 'b');
             DELETE FROM wads WHERE id = 1;",
        )
        .unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM wad_files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn test_wad_analysis_schema() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        assert!(has_column(&conn, "wad_analysis", "wad_id").unwrap());
        assert!(has_column(&conn, "wad_analysis", "total_maps").unwrap());
        assert!(has_column(&conn, "wad_analysis", "required_maps").unwrap());
        assert!(has_column(&conn, "wad_analysis", "secret_maps").unwrap());
        assert!(has_column(&conn, "wad_analysis", "terminal_map").unwrap());
        assert!(has_column(&conn, "wad_analysis", "has_umapinfo").unwrap());
        assert!(has_column(&conn, "wad_analysis", "analysis_json").unwrap());
        assert!(has_column(&conn, "wad_analysis", "expected_map_count").unwrap());
        assert!(has_column(&conn, "wad_analysis", "analyzed_at").unwrap());
    }
}
