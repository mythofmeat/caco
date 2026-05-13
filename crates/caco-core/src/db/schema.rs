use rusqlite::Connection;

use crate::Result;

/// Core schema SQL — creates tables and indexes if they don't exist.
pub const SCHEMA: &str = r#"
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

/// Copy the live DB file to `{backup_dir}/pre-migration-{from_version}.db`.
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
    let backup_dir = crate::config::backup_dir();
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
            "custom_iwad",
            "custom_sourceport",
            "required_sourceport_family",
            "custom_args",
            "created_at",
            "updated_at",
            "deleted_at",
            "version",
            "stats_snapshot",
            "companion_files",
            "complevel",
            "custom_config",
            "gc_ignore",
            "availability",
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
            "demo_file",
            "exit_code",
            "playthrough_id",
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
