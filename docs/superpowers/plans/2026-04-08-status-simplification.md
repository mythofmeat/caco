# Status Simplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the three-axis organization model (Status + PlayState + Intent) with a single `status` field using four values: `unplayed`, `in-progress`, `completed`, `abandoned`.

**Architecture:** The old `Status` enum (6 values), `PlayState` enum (3 values), and `Intent` enum (4 values) collapse into one `Status` enum with 4 values. The DB drops the `play_state` and `intent` columns, keeping only `status`. All dual-write sync code is deleted. The `Availability` enum stays unchanged (orthogonal, system-managed). The `wad_completions` table continues tracking completion count.

**Tech Stack:** Rust, rusqlite, clap, ratatui, egui

---

## File Map

### Modified files:
- `crates/caco-core/src/db/models.rs` — Replace Status/PlayState/Intent enums with new Status enum
- `crates/caco-core/src/db/wads.rs` — Delete sync functions, simplify NewWad/WadUpdate
- `crates/caco-core/src/db/query.rs` — Remove intent:/play: handlers, update status: handler
- `crates/caco-core/src/db/schema.rs` — New migration to consolidate columns
- `crates/caco-core/src/db/mod.rs` — Update re-exports
- `crates/caco-core/src/db/sessions.rs` — Update `wads_by_status` query, `StatsSnapshot`
- `crates/caco-core/src/db/playthroughs.rs` — Update `start_playthrough`, `complete_playthrough`, `derive_play_state` to use new status values
- `crates/caco-core/src/error.rs` — Remove `InvalidPlayState`, `InvalidIntent`
- `crates/caco-core/src/config.rs` — Update `ListConfig.default_status`
- `crates/caco-cli/src/commands/modify.rs` — Remove intent/play_state field handling
- `crates/caco-cli/src/commands/completions.rs` — Remove `play-states`, `intents` contexts
- `crates/caco-cli/src/commands/gc.rs` — Update test helpers and queries
- `crates/caco-cli/src/output.rs` — Use new Status enum directly
- `crates/caco-cli/src/parsing.rs` — Remove `play`, `play_state`, `intent` from MODIFY_FIELDS
- `crates/caco-tui/src/theme.rs` — Delete old status/intent/play_state color functions, replace with new status colors
- `crates/caco-tui/src/screens/tabbed_library.rs` — Update tab queries
- `crates/caco-tui/src/screens/wad_edit.rs` — Replace StatusCycle with new 4-value cycle
- `crates/caco-tui/src/widgets/library_pane.rs` — Update status mode keybindings
- `crates/caco-gui/src/theme.rs` — Replace unified_status with direct status functions
- `crates/caco-gui/src/state.rs` — Replace unified_status with status
- `crates/caco-gui/src/dialogs/edit.rs` — Replace unified_status picker with status picker
- `crates/caco-gui/src/dialogs/stats.rs` — Update status breakdown display
- `crates/caco-gui/src/panels/detail.rs` — Update status pill
- `crates/caco-gui/src/panels/wad_table.rs` — Update status column
- `crates/caco-gui/src/panels/wad_grid.rs` — Update status display
- `crates/caco-gui/src/app.rs` — Update status bar references
- `crates/caco-gui/src/persist.rs` — Update persisted status_filter values

---

## Task 1: New Status Enum (caco-core/src/db/models.rs)

**Files:**
- Modify: `crates/caco-core/src/db/models.rs`
- Modify: `crates/caco-core/src/error.rs`

This is the foundation — everything else depends on it.

- [ ] **Step 1: Replace the Status enum**

Replace the old Status enum (lines 12-89) with the new one. Delete the PlayState enum (lines 92-154) and Intent enum (lines 157-224) entirely.

```rust
// ---------------------------------------------------------------------------
// Status enum
// ---------------------------------------------------------------------------

/// Play status for a WAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    Unplayed,
    InProgress,
    Completed,
    Abandoned,
}

impl Status {
    pub const ALL: &[Status] = &[
        Status::Unplayed,
        Status::InProgress,
        Status::Completed,
        Status::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Unplayed => "unplayed",
            Status::InProgress => "in-progress",
            Status::Completed => "completed",
            Status::Abandoned => "abandoned",
        }
    }

    /// Parse a status string, supporting shortcuts.
    pub fn parse(s: &str) -> Option<Status> {
        if let Ok(st) = s.parse::<Status>() {
            return Some(st);
        }
        STATUS_SHORTCUTS
            .get(s.to_lowercase().as_str())
            .and_then(|full| full.parse().ok())
    }

    /// Display name (e.g., "In Progress").
    pub fn display_name(self) -> &'static str {
        match self {
            Status::Unplayed => "Unplayed",
            Status::InProgress => "In Progress",
            Status::Completed => "Completed",
            Status::Abandoned => "Abandoned",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Status {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unplayed" => Ok(Status::Unplayed),
            "in-progress" => Ok(Status::InProgress),
            "completed" => Ok(Status::Completed),
            "abandoned" => Ok(Status::Abandoned),
            _ => Err(crate::Error::InvalidStatus(s.to_string())),
        }
    }
}
```

- [ ] **Step 2: Replace STATUS_SHORTCUTS, delete PLAY_STATE_SHORTCUTS and INTENT_SHORTCUTS**

Replace the `STATUS_SHORTCUTS` map (lines 472-494) and delete `PLAY_STATE_SHORTCUTS` (lines 496-505) and `INTENT_SHORTCUTS` (lines 507-516):

```rust
/// Status shortcuts for query parsing.
pub static STATUS_SHORTCUTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("u", "unplayed"),
        ("ip", "in-progress"),
        ("inp", "in-progress"),
        ("playing", "in-progress"),
        ("p", "in-progress"),
        ("c", "completed"),
        ("done", "completed"),
        ("finished", "completed"),
        ("f", "completed"),
        ("a", "abandoned"),
        ("dropped", "abandoned"),
        ("d", "abandoned"),
    ])
});
```

- [ ] **Step 3: Update ALLOWED_UPDATE_FIELDS**

Remove `play_state` and `intent` from the set (lines 522-553). Keep `status` and `availability`.

```rust
pub static ALLOWED_UPDATE_FIELDS: LazyLock<std::collections::HashSet<&'static str>> =
    LazyLock::new(|| {
        [
            "title",
            "author",
            "year",
            "description",
            "status",
            "rating",
            "notes",
            "source_url",
            "filename",
            "cached_path",
            "custom_iwad",
            "custom_sourceport",
            "custom_args",
            "companion_files",
            "custom_config",
            "version",
            "complevel",
            "zdoom_required",
            "idgames_id",
            "deleted_at",
            "stats_snapshot",
            "gc_ignore",
            "availability",
        ]
        .into_iter()
        .collect()
    });
```

- [ ] **Step 4: Replace metadata maps**

Delete `STATUS_METADATA`, `PLAY_STATE_METADATA`, `INTENT_METADATA` (lines 555-590). Replace with a single map:

```rust
/// Status metadata entry: (display_name, hex_color, rich_color, css_class).
pub type StatusMeta = (&'static str, &'static str, &'static str, &'static str);

/// Canonical status metadata.
pub static STATUS_METADATA: LazyLock<HashMap<&'static str, StatusMeta>> =
    LazyLock::new(|| {
        HashMap::from([
            ("unplayed",    ("Unplayed",    "#3366cc", "dodger_blue1", "status-unplayed")),
            ("in-progress", ("In Progress", "#33cc33", "green1",       "status-in-progress")),
            ("completed",   ("Completed",   "#808080", "grey50",       "status-completed")),
            ("abandoned",   ("Abandoned",   "#cc3333", "red",          "status-abandoned")),
        ])
    });
```

- [ ] **Step 5: Update WadRecord**

Remove `play_state` and `intent` fields from the struct (lines 334-366) and `from_row` (lines 373-407). Remove `play_state_enum()`, `intent_enum()` helpers (lines 415-427).

In WadRecord struct, remove:
```rust
    pub play_state: String,
    pub intent: String,
```

In `from_row`, remove:
```rust
            play_state: row.get::<_, Option<String>>("play_state")?.unwrap_or_else(|| "unplayed".to_string()),
            intent: row.get::<_, Option<String>>("intent")?.unwrap_or_else(|| "inbox".to_string()),
```

Remove methods `play_state_enum()` and `intent_enum()`.

- [ ] **Step 6: Update tests**

Replace all tests (lines 592-773). Delete PlayState, Intent, and old Status tests. Add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_as_str_roundtrip() {
        for &st in Status::ALL {
            let s = st.as_str();
            let parsed: Status = s.parse().unwrap();
            assert_eq!(parsed, st);
        }
    }

    #[test]
    fn test_status_shortcuts() {
        assert_eq!(Status::parse("u"), Some(Status::Unplayed));
        assert_eq!(Status::parse("ip"), Some(Status::InProgress));
        assert_eq!(Status::parse("p"), Some(Status::InProgress));
        assert_eq!(Status::parse("playing"), Some(Status::InProgress));
        assert_eq!(Status::parse("c"), Some(Status::Completed));
        assert_eq!(Status::parse("done"), Some(Status::Completed));
        assert_eq!(Status::parse("f"), Some(Status::Completed));
        assert_eq!(Status::parse("finished"), Some(Status::Completed));
        assert_eq!(Status::parse("a"), Some(Status::Abandoned));
        assert_eq!(Status::parse("d"), Some(Status::Abandoned));
        assert_eq!(Status::parse("dropped"), Some(Status::Abandoned));
    }

    #[test]
    fn test_status_display() {
        assert_eq!(Status::Unplayed.to_string(), "unplayed");
        assert_eq!(Status::InProgress.to_string(), "in-progress");
        assert_eq!(Status::Completed.to_string(), "completed");
        assert_eq!(Status::Abandoned.to_string(), "abandoned");
    }

    #[test]
    fn test_status_display_name() {
        assert_eq!(Status::Unplayed.display_name(), "Unplayed");
        assert_eq!(Status::InProgress.display_name(), "In Progress");
    }

    #[test]
    fn test_invalid_status() {
        assert!("invalid".parse::<Status>().is_err());
        assert!(Status::parse("invalid").is_none());
    }

    #[test]
    fn test_source_type_roundtrip() {
        for st in [
            SourceType::Idgames,
            SourceType::Doomwiki,
            SourceType::Doomworld,
            SourceType::Url,
            SourceType::Local,
        ] {
            let s = st.as_str();
            let parsed: SourceType = s.parse().unwrap();
            assert_eq!(parsed, st);
        }
    }

    #[test]
    fn test_invalid_source_type() {
        assert!("invalid".parse::<SourceType>().is_err());
    }

    #[test]
    fn test_parsed_query_empty() {
        let q = ParsedQuery::default();
        assert!(q.is_empty());

        let q2 = ParsedQuery {
            or_groups: vec![AndGroup { terms: vec![] }],
        };
        assert!(q2.is_empty());
    }

    #[test]
    fn test_allowed_update_fields() {
        assert!(ALLOWED_UPDATE_FIELDS.contains("title"));
        assert!(ALLOWED_UPDATE_FIELDS.contains("status"));
        assert!(ALLOWED_UPDATE_FIELDS.contains("availability"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("play_state"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("intent"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("id"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("created_at"));
    }

    // Availability tests unchanged — keep existing tests
    #[test]
    fn test_availability_as_str_roundtrip() {
        for &a in Availability::ALL {
            let s = a.as_str();
            let parsed: Availability = s.parse().unwrap();
            assert_eq!(parsed, a);
        }
    }

    #[test]
    fn test_availability_display() {
        assert_eq!(Availability::Cached.to_string(), "cached");
        assert_eq!(Availability::Unavailable.to_string(), "unavailable");
    }

    #[test]
    fn test_invalid_availability() {
        assert!("invalid".parse::<Availability>().is_err());
    }
}
```

- [ ] **Step 7: Update error.rs**

In `crates/caco-core/src/error.rs`, remove `InvalidPlayState` and `InvalidIntent` variants (lines 33-36).

- [ ] **Step 8: Run tests to verify compilation**

Run: `cargo test --workspace --no-run 2>&1 | head -50`
Expected: Compilation errors in downstream crates (expected — we'll fix those in subsequent tasks). Models tests should compile.

- [ ] **Step 9: Commit**

```bash
git add crates/caco-core/src/db/models.rs crates/caco-core/src/error.rs
git commit -m "refactor: replace Status/PlayState/Intent with single 4-value Status enum"
```

---

## Task 2: Simplify wads.rs (delete sync layer, update builders)

**Files:**
- Modify: `crates/caco-core/src/db/wads.rs`

- [ ] **Step 1: Delete sync functions and simplify imports**

Remove `sync_status_to_axes`, `sync_axes_to_status`, and the `Intent`/`PlayState` imports (lines 1-50). Keep `compute_availability`. Update imports:

```rust
use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Connection;

use super::connection::attach_tags;
use super::models::{
    Availability, WadRecord, ALLOWED_UPDATE_FIELDS, SourceType, Status,
};
use crate::Result;
```

- [ ] **Step 2: Simplify NewWad builder**

Remove `play_state` and `intent` fields. Remove the `play_state()` and `intent()` builder methods. Simplify the `status()` method to just set status directly:

```rust
pub struct NewWad {
    pub title: String,
    pub source_type: SourceType,
    pub author: Option<String>,
    pub year: Option<i32>,
    pub description: Option<String>,
    pub source_id: Option<String>,
    pub source_url: Option<String>,
    pub filename: Option<String>,
    pub cached_path: Option<String>,
    pub status: Status,
    pub version: Option<String>,
    pub tags: Vec<String>,
}

impl NewWad {
    pub fn new(title: impl Into<String>, source_type: SourceType) -> Self {
        Self {
            title: title.into(),
            source_type,
            author: None,
            year: None,
            description: None,
            source_id: None,
            source_url: None,
            filename: None,
            cached_path: None,
            status: Status::Unplayed,
            version: None,
            tags: Vec::new(),
        }
    }

    // Keep all existing builder methods (author, year, description, etc.)
    // Simplify status:
    pub fn status(mut self, v: Status) -> Self {
        self.status = v;
        self
    }

    // Delete play_state() and intent() methods entirely
}
```

- [ ] **Step 3: Simplify WadUpdate builder**

Replace `set_status`, `set_play_state`, `set_intent` with a single `set_status`:

```rust
    /// Set the status field (convenience).
    pub fn set_status(self, status: Status) -> crate::Result<Self> {
        self.set_text("status", Some(status.as_str().to_string()))
    }
```

Delete `set_play_state()` and `set_intent()` methods entirely.

Update the completion detection in `update_wad` (line 356-358) to check for `completed` instead of `finished`:

```rust
    let recording_completion = update.record_completion
        && update.fields.get("status").is_some_and(|v| {
            matches!(v, FieldValue::Text(Some(s)) if s == Status::Completed.as_str())
        });
```

- [ ] **Step 4: Update add_wad INSERT statement**

Remove `play_state` and `intent` from the INSERT:

```rust
pub fn add_wad(conn: &Connection, wad: &NewWad) -> Result<i64> {
    let avail = compute_availability(
        wad.cached_path.as_deref(),
        wad.source_url.as_deref(),
    );
    conn.execute(
        "INSERT INTO wads (title, author, year, description, source_type,
                          source_id, source_url, filename, cached_path, status, version,
                          availability)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            wad.title,
            wad.author,
            wad.year,
            wad.description,
            wad.source_type.as_str(),
            wad.source_id,
            wad.source_url,
            wad.filename,
            wad.cached_path,
            wad.status.as_str(),
            wad.version,
            avail.as_str(),
        ],
    )?;
    // ... rest unchanged
}
```

- [ ] **Step 5: Rewrite tests**

Delete all three-axis and dual-write tests (lines 767-1028). Replace with simpler status tests:

```rust
    #[test]
    fn test_new_wad_defaults_to_unplayed() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Test", SourceType::Local),
        )
        .unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.status, "unplayed");
    }

    #[test]
    fn test_update_status() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update = WadUpdate::new().set_status(Status::InProgress).unwrap();
        update_wad(&conn, id, &update).unwrap();

        let wad = get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(wad.status, "in-progress");
    }

    #[test]
    fn test_update_status_completed_records_completion() {
        let conn = setup();
        let id = add_test_wad(&conn);

        let update = WadUpdate::new().set_status(Status::Completed).unwrap();
        update_wad(&conn, id, &update).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wad_completions WHERE wad_id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
```

Keep the availability auto-compute tests and tag tests unchanged. Update `test_add_and_get_wad` to expect `"unplayed"` instead of `"backlog"`.

- [ ] **Step 6: Run tests**

Run: `cargo test -p caco-core -- db::wads 2>&1 | tail -20`
Expected: wads tests pass (some other crate tests may still fail)

- [ ] **Step 7: Commit**

```bash
git add crates/caco-core/src/db/wads.rs
git commit -m "refactor: delete dual-write sync layer, simplify NewWad/WadUpdate"
```

---

## Task 3: Update query parser (caco-core/src/db/query.rs)

**Files:**
- Modify: `crates/caco-core/src/db/query.rs`

- [ ] **Step 1: Remove normalize_play_state and normalize_intent functions**

Delete `normalize_play_state()` (lines 178-185) and `normalize_intent()` (lines 187-194). Keep `normalize_status()`.

- [ ] **Step 2: Update build_term_sql**

Remove the `play`/`play_state` handler (lines 246-249) and `intent` handler (lines 251-254). Keep the `status` handler but point it at the `status` column (it already does — no change needed there).

Also add `play` and `play_state` as aliases for `status` for backward compat:

```rust
        Some("status") | Some("play") | Some("play_state") => {
            let normalized = normalize_status(&term.value);
            ("wads.status = ?".into(), vec![Box::new(normalized)])
        }
```

Remove the `intent` handler entirely (no backward compat — it's a new concept that never shipped to users).

- [ ] **Step 3: Update imports**

Remove `INTENT_SHORTCUTS` and `PLAY_STATE_SHORTCUTS` from imports (line 5):

```rust
use super::models::{
    AndGroup, ParsedQuery, QueryTerm, SourceType, WadRecord, STATUS_SHORTCUTS,
};
```

- [ ] **Step 4: Update tests**

Update `add_test_wads` to use new Status values:

```rust
    fn add_test_wads(conn: &Connection) {
        add_wad(
            conn,
            &NewWad::new("Scythe", SourceType::Idgames)
                .author("Erik Alm")
                .year(2003)
                .source_id("12345")
                .tags(vec!["megawad".into(), "cacoward".into()]),
        )
        .unwrap();

        add_wad(
            conn,
            &NewWad::new("Ancient Aliens", SourceType::Idgames)
                .author("skillsaw")
                .year(2016)
                .status(Status::InProgress)
                .tags(vec!["megawad".into(), "cacoward".into()]),
        )
        .unwrap();

        add_wad(
            conn,
            &NewWad::new("Sunlust", SourceType::Doomwiki)
                .author("Ribbiks & Dannebubinga")
                .year(2015)
                .status(Status::Completed)
                .tags(vec!["megawad".into(), "slaughter".into()]),
        )
        .unwrap();
    }
```

Update test assertions:
- `test_search_wads_by_status`: query `"status:in-progress"`, expect "Ancient Aliens"
- `test_search_wads_negation`: query `"-status:completed"`, expect 2 results
- `test_search_wads_or`: query `"status:in-progress , status:completed"`, expect 2 results
- `test_search_wads_status_shortcut`: query `"status:p"`, expect "Ancient Aliens"
- `test_normalize_status`: update expected values

Delete `test_normalize_intent` if it exists.

- [ ] **Step 5: Run tests**

Run: `cargo test -p caco-core -- db::query 2>&1 | tail -20`
Expected: All query tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/caco-core/src/db/query.rs
git commit -m "refactor: simplify query parser to single status field"
```

---

## Task 4: Schema migration (caco-core/src/db/schema.rs)

**Files:**
- Modify: `crates/caco-core/src/db/schema.rs`

- [ ] **Step 1: Add migration 32: consolidate_status_columns**

Add to the MIGRATIONS array (after line 133):

```rust
    (32, "consolidate_status_columns", migrate_consolidate_status),
```

Add the migration function:

```rust
fn migrate_consolidate_status(conn: &Connection) -> Result<()> {
    // Step 1: Map old status values to new ones
    conn.execute_batch(
        "UPDATE wads SET status = 'unplayed'    WHERE status = 'backlog';
         UPDATE wads SET status = 'unplayed'    WHERE status = 'to-play';
         UPDATE wads SET status = 'in-progress' WHERE status = 'playing';
         UPDATE wads SET status = 'completed'   WHERE status = 'finished';
         UPDATE wads SET status = 'abandoned'   WHERE status IN ('abandoned');
         UPDATE wads SET status = 'unplayed'    WHERE status = 'awaiting-update';",
    )?;

    // Step 2: Also handle WADs that were using the three-axis play_state as canonical
    // Override status based on play_state where it's more accurate
    if has_column(conn, "wads", "play_state")? {
        conn.execute_batch(
            "UPDATE wads SET status = 'in-progress' WHERE play_state = 'started' AND status NOT IN ('abandoned');
             UPDATE wads SET status = 'completed'   WHERE play_state = 'completed' AND status NOT IN ('abandoned');",
        )?;
        // Handle intent=dropped → abandoned (regardless of play_state)
        if has_column(conn, "wads", "intent")? {
            conn.execute(
                "UPDATE wads SET status = 'abandoned' WHERE intent = 'dropped'",
                [],
            )?;
        }
    }

    // Note: We do NOT drop play_state/intent columns here.
    // SQLite doesn't support DROP COLUMN before 3.35.0, and even when it does,
    // it's easier to just ignore them. WadRecord::from_row will skip them.
    // The columns remain as dead weight but cause no harm.

    Ok(())
}
```

- [ ] **Step 2: Update POST_MIGRATION_INDEXES**

Remove `idx_wads_play_state` and `idx_wads_intent` from POST_MIGRATION_INDEXES (lines 85-86):

```rust
const POST_MIGRATION_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_wads_deleted_at ON wads(deleted_at);
CREATE INDEX IF NOT EXISTS idx_wads_cached_path ON wads(cached_path);
CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(wad_id, started_at DESC);
"#;
```

- [ ] **Step 3: Update the core SCHEMA default**

Change the default status in the CREATE TABLE (line 15):

```sql
    status TEXT DEFAULT 'unplayed',
```

- [ ] **Step 4: Update WadRecord::from_row to handle missing columns**

In `models.rs`, the `from_row` already has the `play_state` and `intent` reads — we removed those fields in Task 1. But the columns still exist in the DB. The `SELECT *` will include them, but since `from_row` no longer reads them, they'll just be ignored. No action needed here — just confirming.

- [ ] **Step 5: Update schema tests**

In `test_all_wad_columns_exist` (line 619-638), remove `play_state` and `intent` from expected columns. They'll still physically exist in the DB but we don't care. Actually, keep them in the test since the columns do still exist — just remove from the "expected" list if we want to be strict. Better: leave them in since the migration doesn't drop them.

- [ ] **Step 6: Run tests**

Run: `cargo test -p caco-core -- db::schema 2>&1 | tail -20`
Expected: Schema tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/caco-core/src/db/schema.rs
git commit -m "feat: add migration 32 to consolidate status columns"
```

---

## Task 5: Update mod.rs re-exports and sessions.rs

**Files:**
- Modify: `crates/caco-core/src/db/mod.rs`
- Modify: `crates/caco-core/src/db/sessions.rs`

- [ ] **Step 1: Update mod.rs re-exports**

Remove `Intent`, `PlayState`, `INTENT_METADATA`, `INTENT_SHORTCUTS`, `PLAY_STATE_METADATA`, `PLAY_STATE_SHORTCUTS` from the models re-export (line 46-48):

```rust
pub use models::{
    AndGroup, Availability, ParsedQuery, QueryTerm, SourceType, Status,
    StatusMeta, WadRecord, ALLOWED_UPDATE_FIELDS, OR_SEPARATOR,
    STATUS_METADATA, STATUS_SHORTCUTS,
};
```

Remove `normalize_intent`, `normalize_play_state` from the query re-export (line 50-53):

```rust
pub use query::{
    find_duplicate, normalize_status, parse_query, search_wads,
};
```

- [ ] **Step 2: Update sessions.rs — StatsSnapshot.wads_by_status**

The `get_library_stats` function (line 677-689) queries `GROUP BY status`. This still works — the status column now has the new values. Update the `get_completion_rate` function (line 729+) to use `status = 'completed'` instead of `status = 'finished'`:

Find and update in sessions.rs:
```rust
    let finished_wads: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wads WHERE deleted_at IS NULL AND status = 'completed'",
```

Also rename `finished_wads` → `completed_wads` in `StatsSnapshot` and related code for consistency:

```rust
pub struct StatsSnapshot {
    pub total_wads: i64,
    pub total_sessions: i64,
    pub total_playtime: i64,
    pub wads_with_sessions: i64,
    pub wads_by_status: HashMap<String, i64>,
    pub played_wads: i64,
    pub completed_wads: i64,   // renamed from finished_wads
    pub completion_rate: f64,
    pub total_completions: i64,
    pub activity: Vec<ActivityPeriod>,
}
```

Update `get_stats_snapshot` to use `completed_wads` field name.

- [ ] **Step 3: Run core tests**

Run: `cargo test -p caco-core 2>&1 | tail -30`
Expected: All caco-core tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/caco-core/src/db/mod.rs crates/caco-core/src/db/sessions.rs
git commit -m "refactor: update re-exports and stats to use new status values"
```

---

## Task 6: Update playthroughs.rs

**Files:**
- Modify: `crates/caco-core/src/db/playthroughs.rs`

- [ ] **Step 1: Update start_playthrough**

Replace the raw SQL (lines 63-73) that updates `play_state`, `intent`, and `status`:

```rust
    conn.execute(
        "UPDATE wads SET status = 'in-progress', updated_at = ?1
         WHERE id = ?2 AND status NOT IN ('abandoned')",
        rusqlite::params![now, wad_id],
    )?;
```

Note: We don't auto-un-abandon when starting a playthrough. If someone explicitly abandoned something and then plays it, they need to manually un-abandon first. (This matches the user's spec: abandoned is manual-only.)

Actually, re-reading the user's spec: "if i abandon something and want to retry it, its status should depend on whether it had completed levels or not." So starting a playthrough on an abandoned WAD SHOULD un-abandon:

```rust
    // Un-abandon if abandoned (starting a play implies intent to continue)
    conn.execute(
        "UPDATE wads SET status = 'in-progress', updated_at = ?1
         WHERE id = ?2",
        rusqlite::params![now, wad_id],
    )?;
```

- [ ] **Step 2: Update complete_playthrough**

Replace the raw SQL (lines 106-109):

```rust
    conn.execute(
        "UPDATE wads SET status = 'completed', updated_at = ?1
         WHERE id = ?2",
        rusqlite::params![now, wad_id],
    )?;
```

- [ ] **Step 3: Update derive_play_state**

Rename to `derive_status` and return `Status` instead of `PlayState`:

```rust
/// Derive the status from playthrough records.
pub fn derive_status(conn: &Connection, wad_id: i64) -> Result<Status> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM playthroughs WHERE wad_id = ?1",
        [wad_id],
        |row| row.get(0),
    )?;

    if total == 0 {
        return Ok(Status::Unplayed);
    }

    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM playthroughs WHERE wad_id = ?1 AND completed_at IS NULL",
        [wad_id],
        |row| row.get(0),
    )?;

    if active > 0 {
        return Ok(Status::InProgress);
    }

    Ok(Status::Completed)
}
```

Update imports to use `Status` instead of `PlayState`.

- [ ] **Step 4: Update tests**

Update `test_derive_play_state` → `test_derive_status`:

```rust
    #[test]
    fn test_derive_status() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        assert_eq!(derive_status(&conn, wad_id).unwrap(), Status::Unplayed);

        let pt = start_playthrough(&conn, wad_id).unwrap();
        assert_eq!(derive_status(&conn, wad_id).unwrap(), Status::InProgress);

        complete_playthrough(&conn, pt, None, None).unwrap();
        assert_eq!(derive_status(&conn, wad_id).unwrap(), Status::Completed);

        start_playthrough(&conn, wad_id).unwrap();
        assert_eq!(derive_status(&conn, wad_id).unwrap(), Status::InProgress);
    }
```

Update `test_start_playthrough_syncs_status` to check for `"in-progress"` instead of `"playing"`.
Update `test_complete_playthrough_syncs_status` to check for `"completed"` instead of `"finished"`.

- [ ] **Step 5: Update mod.rs re-export**

In `crates/caco-core/src/db/mod.rs`, update the playthroughs re-export: rename `derive_play_state` → `derive_status`.

- [ ] **Step 6: Run tests**

Run: `cargo test -p caco-core 2>&1 | tail -30`
Expected: All caco-core tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/caco-core/src/db/playthroughs.rs crates/caco-core/src/db/mod.rs
git commit -m "refactor: update playthroughs to use new Status enum"
```

---

## Task 7: Update CLI (caco-cli)

**Files:**
- Modify: `crates/caco-cli/src/commands/modify.rs`
- Modify: `crates/caco-cli/src/commands/completions.rs`
- Modify: `crates/caco-cli/src/output.rs`
- Modify: `crates/caco-cli/src/parsing.rs`
- Modify: `crates/caco-cli/src/commands/gc.rs`

- [ ] **Step 1: Update modify.rs**

Remove `Intent` and `PlayState` from imports (line 9):

```rust
use caco_core::db::{self, Status, WadRecord, WadUpdate};
```

In `apply_field_update` (lines 339-413):
- Remove the `"play" | "play_state"` match arm (lines 367-372)
- Remove the `"intent"` match arm (lines 374-379)
- Simplify the `"status"` arm — just call `update.set_status()`:

```rust
        "status" => {
            let status = Status::parse(value)
                .ok_or_else(|| format!("Invalid status: {value}"))?;
            update.set_status(status).map_err(|e| e.to_string())
        }
```

In the `ClearField` handler (lines 163-181), remove:
```rust
                    "play" | "play_state" => "play_state",
                    "intent" => "intent",
```

Update help text (lines 15-32) to reflect new status values.

- [ ] **Step 2: Update completions.rs**

Remove the `"play-states"` and `"intents"` completion contexts (lines 124-133).
Remove `"play:"` and `"intent:"` from `"query-fields"` (line 168).

- [ ] **Step 3: Update output.rs**

The output already uses `Status::parse(&wad.status)` which will work with the new enum. The `render_stats_table` function (lines 579-589) hard-codes old status names:

```rust
    println!("  Status breakdown:");
    let status_order = ["unplayed", "in-progress", "completed", "abandoned"];
    for status in &status_order {
        let count = snapshot.wads_by_status.get(*status).copied().unwrap_or(0);
        if count > 0 {
            let display = Status::parse(status)
                .map(|s| s.display_name().to_string())
                .unwrap_or_else(|| (*status).to_string());
            println!("    {display:<18} {count}");
        }
    }
```

Also update the plain stats output (lines 622-626):
```rust
    let status_order = ["unplayed", "in-progress", "completed", "abandoned"];
```

Update the `finished_wads` → `completed_wads` references in the stats display:
```rust
    println!("  Completed:      {} / {} played ({:.0}%)",
        snapshot.completed_wads,
```

- [ ] **Step 4: Update parsing.rs**

Remove `"play"`, `"play_state"`, `"intent"` from `MODIFY_FIELDS` (lines 75-79):

```rust
pub const MODIFY_FIELDS: &[&str] = &[
    "title", "author", "year", "description", "status", "rating", "notes",
    "iwad", "sourceport", "args", "complevel", "config", "idgames-id", "version",
];
```

- [ ] **Step 5: Update gc.rs tests**

The gc.rs tests use `add_wad_with_status(&conn, name, Status::Finished)` etc. Update all occurrences:
- `Status::Finished` → `Status::Completed`
- `Status::Playing` → `Status::InProgress`
- `Status::ToPlay` → `Status::Unplayed`
- `Status::Backlog` → `Status::Unplayed`
- `Status::AwaitingUpdate` → `Status::Unplayed`
- `Status::Abandoned` → `Status::Abandoned` (unchanged)

Also update the GC query on line 231:
```rust
    let query = "status:abandoned , status:completed";
```

(Previously `"intent:dropped , play:completed"`)

- [ ] **Step 6: Run CLI tests**

Run: `cargo test -p caco-cli 2>&1 | tail -30`
Expected: All CLI tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/caco-cli/src/
git commit -m "refactor: update CLI commands for new Status enum"
```

---

## Task 8: Update TUI (caco-tui)

**Files:**
- Modify: `crates/caco-tui/src/theme.rs`
- Modify: `crates/caco-tui/src/screens/tabbed_library.rs`
- Modify: `crates/caco-tui/src/screens/wad_edit.rs`
- Modify: `crates/caco-tui/src/widgets/library_pane.rs`

- [ ] **Step 1: Simplify theme.rs**

Delete `play_state_color`, `play_state_style`, `play_state_display`, `intent_color`, `intent_style`, `intent_display` functions (lines 83-122).

Replace `status_color` (lines 6-16) with the new values:

```rust
use caco_core::db::models::Status;

/// Map a status string to its ratatui Color.
pub fn status_color(status: &str) -> Color {
    match status {
        "unplayed" => Color::Rgb(0x33, 0x66, 0xcc),     // blue
        "in-progress" => Color::Rgb(0x33, 0xcc, 0x33),  // green
        "completed" => Color::Rgb(0x80, 0x80, 0x80),    // gray
        "abandoned" => Color::Rgb(0xcc, 0x33, 0x33),    // red
        _ => Color::Reset,
    }
}
```

Remove `use caco_core::db::models::{Intent, PlayState, Status};` → `use caco_core::db::models::Status;`

- [ ] **Step 2: Update tabbed_library.rs tabs**

Replace the TABS constant (lines 19-27):

```rust
const TABS: &[(&str, &str, Option<&str>)] = &[
    ("all",         "All",         None),
    ("unplayed",    "Unplayed",    Some("status:unplayed")),
    ("in-progress", "In Progress", Some("status:in-progress")),
    ("completed",   "Completed",   Some("status:completed")),
    ("abandoned",   "Abandoned",   Some("status:abandoned")),
    ("import",      "Import",      None),
];
```

- [ ] **Step 3: Update wad_edit.rs**

Replace `cycle_status` (lines 257-269) with new values:

```rust
    fn cycle_status(&mut self, forward: bool) {
        let statuses = ["unplayed", "in-progress", "completed", "abandoned"];
        if let Some(field) = self.fields.iter_mut().find(|f| f.name == "status") {
            let current = field.input.value().to_string();
            let idx = statuses.iter().position(|s| *s == current).unwrap_or(0);
            let new_idx = if forward {
                (idx + 1) % statuses.len()
            } else {
                if idx == 0 { statuses.len() - 1 } else { idx - 1 }
            };
            field.input.set_value(statuses[new_idx]);
        }
    }
```

The `save()` method (line 166-179) already calls `Status::parse` and `set_status` — this will work with the new enum.

- [ ] **Step 4: Update library_pane.rs status mode**

Replace `handle_status_mode_key` (lines 248-279):

```rust
    fn handle_status_mode_key(
        &mut self,
        key: KeyEvent,
        conn: &Connection,
    ) -> Option<AppMessage> {
        self.status_mode = false;

        let status = match key.code {
            KeyCode::Char('u') => "unplayed",
            KeyCode::Char('p') => "in-progress",
            KeyCode::Char('c') => "completed",
            KeyCode::Char('a') => "abandoned",
            KeyCode::Esc => return None,
            _ => return None,
        };

        if let Some(id) = self.table.selected_wad_id() {
            if let Ok(update) = db::wads::WadUpdate::new()
                .set_status(Status::parse(status).unwrap_or(Status::Unplayed))
            {
                let _ = wads::update_wad(conn, id, &update);
                self.table.update_row(conn, id);
                return Some(AppMessage::Notify(
                    format!("Status → {}", crate::theme::status_display(status)),
                    Severity::Info,
                ));
            }
        }
        None
    }
```

Remove `use caco_core::db::models::Status;` if redundant (it's already imported).

- [ ] **Step 5: Run TUI tests**

Run: `cargo test -p caco-tui 2>&1 | tail -20`
Expected: TUI tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/caco-tui/src/
git commit -m "refactor: update TUI for new Status enum"
```

---

## Task 9: Update GUI (caco-gui)

**Files:**
- Modify: `crates/caco-gui/src/theme.rs`
- Modify: `crates/caco-gui/src/state.rs`
- Modify: `crates/caco-gui/src/dialogs/edit.rs`
- Modify: `crates/caco-gui/src/dialogs/stats.rs`
- Modify: `crates/caco-gui/src/panels/detail.rs`
- Modify: `crates/caco-gui/src/panels/wad_table.rs`
- Modify: `crates/caco-gui/src/panels/wad_grid.rs`
- Modify: `crates/caco-gui/src/app.rs`
- Modify: `crates/caco-gui/src/persist.rs`

- [ ] **Step 1: Replace unified_status system in theme.rs**

Delete the entire `UNIFIED_STATUSES`, `unified_status()`, `unified_status_query()`, `unified_status_color()`, `unified_status_bg()`, `unified_status_display()`, `unified_status_pill()` block (lines 37-112).

Replace with direct status functions:

```rust
/// All status values in display order.
pub const STATUSES: &[&str] = &["unplayed", "in-progress", "completed", "abandoned"];

pub fn status_color(status: &str) -> Color32 {
    match status {
        "unplayed" => Color32::from_rgb(0x33, 0x66, 0xcc),
        "in-progress" => Color32::from_rgb(0x33, 0xcc, 0x33),
        "completed" => Color32::from_rgb(0x80, 0x80, 0x80),
        "abandoned" => Color32::from_rgb(0xcc, 0x33, 0x33),
        _ => TEXT_PRIMARY,
    }
}

pub fn status_bg(status: &str) -> Color32 {
    match status {
        "unplayed" => Color32::from_rgb(0x0d, 0x14, 0x2a),
        "in-progress" => Color32::from_rgb(0x0d, 0x2a, 0x0d),
        "completed" => Color32::from_rgb(0x1a, 0x1a, 0x1a),
        "abandoned" => Color32::from_rgb(0x2a, 0x0d, 0x0d),
        _ => BG_MEDIUM,
    }
}

pub fn status_display(status: &str) -> &str {
    match status {
        "unplayed" => "Unplayed",
        "in-progress" => "In Progress",
        "completed" => "Completed",
        "abandoned" => "Abandoned",
        _ => status,
    }
}

pub fn status_query(status: &str) -> &'static str {
    match status {
        "unplayed" => "status:unplayed",
        "in-progress" => "status:in-progress",
        "completed" => "status:completed",
        "abandoned" => "status:abandoned",
        _ => "",
    }
}

/// Render a status value as a colored pill badge.
pub fn status_pill(ui: &mut egui::Ui, status: &str) {
    let color = status_color(status);
    let label = status_display(status);
    let bg = status_bg(status);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.colored_label(color, egui::RichText::new(label).small().strong());
        });
}
```

- [ ] **Step 2: Update state.rs**

Replace all `unified_status` references with direct `status` field usage:

In `refresh_status_counts` (line 211+):
```rust
    pub fn refresh_status_counts(&mut self, conn: &Connection) {
        self.status_counts.clear();
        if let Ok(wads) = db::search_wads(conn, None, None, true, false, 0) {
            for wad in &wads {
                *self.status_counts.entry(wad.status.clone()).or_insert(0) += 1;
            }
        }
    }
```

In `build_query` (line 246+), replace `unified_status_query` with `status_query`:
```rust
        if let Some(ref sf) = self.status_filter {
            let qf = crate::theme::status_query(sf);
```

- [ ] **Step 3: Update edit.rs**

Remove `Intent` and `PlayState` imports (line 4). Replace `unified_status` field with `status`:

In field initialization (line 136):
```rust
            status: wad.status.clone(),
```

In the status picker (lines 435-466), replace `UNIFIED_STATUSES` with `STATUSES`:
```rust
            for &status in crate::theme::STATUSES {
                let color = crate::theme::status_color(status);
                let bg = crate::theme::status_bg(status);
                let is_selected = self.status == status;
                // ... rest similar but using status_display
            }
```

In the save method (lines 862-882), replace the mapping with a direct `set_status`:
```rust
        let status = Status::parse(&self.status).unwrap_or(Status::Unplayed);
        update = update.set_status(status).unwrap();
```

- [ ] **Step 4: Update remaining GUI files**

In `panels/detail.rs`: replace `unified_status_pill(ui, &wad.play_state, &wad.intent)` with `status_pill(ui, &wad.status)`.

In `panels/wad_table.rs` and `panels/wad_grid.rs`: replace any `unified_status()` calls with direct `wad.status` access.

In `dialogs/stats.rs`: update status breakdown display to use new status names.

In `app.rs`: update any `unified_status` or status bar references.

In `persist.rs`: update persisted status_filter values if they reference old unified status names.

- [ ] **Step 5: Build GUI**

Run: `cargo build -p caco-gui 2>&1 | tail -30`
Expected: Compiles successfully.

- [ ] **Step 6: Commit**

```bash
git add crates/caco-gui/src/
git commit -m "refactor: update GUI for new Status enum"
```

---

## Task 10: Full verification and cleanup

**Files:**
- Modify: `crates/caco-sources/src/import_service.rs` (if it references Status/Intent/PlayState)
- Modify: `CLAUDE.md`
- Modify: `docs/DESIGN-reorg.md`

- [ ] **Step 1: Check caco-sources for references**

Search for `Status`, `Intent`, `PlayState` usage in `crates/caco-sources/`. Update any references (likely in import_service.rs where NewWad is constructed).

- [ ] **Step 2: Run full test suite**

Run: `cargo test --workspace 2>&1 | tail -30`
Expected: All tests pass.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -30`
Expected: No warnings.

- [ ] **Step 4: Run type check**

Run: `cargo check --workspace 2>&1 | tail -10`
Expected: Clean.

- [ ] **Step 5: Update CLAUDE.md**

Update the Status enum documentation, query syntax, CLI commands reference, and Feature Parity table to reflect the new single-axis model.

- [ ] **Step 6: Update or remove docs/DESIGN-reorg.md**

Mark the design doc as completed/superseded.

- [ ] **Step 7: Final commit**

```bash
git add .
git commit -m "docs: update CLAUDE.md and design doc for status simplification"
```
