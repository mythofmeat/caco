# Architecture Review Report: Caco

## Executive Summary

Caco is a well-structured personal tool that has grown organically into a three-interface application (CLI, TUI, GUI) with multiple external data source integrations. The overall architecture is sound for a personal-use project, with thoughtful patterns like batch stat queries, source adapter abstractions, and a beets-inspired query parser. However, the codebase exhibits several architectural concerns that will compound as the application continues to grow. The most significant issues are (1) the absence of a service/use-case layer, causing all three UI frontends to independently call `db.py` functions with duplicated business logic, (2) excessive connection churn in the database layer, and (3) a monolithic `db.py` that combines schema, queries, migrations, parsing, and domain logic in a single 1361-line file.

---

## Findings

### Finding 1: No Service/Use-Case Layer Between UIs and Data Access

**Severity: High**

All three frontends (CLI, TUI, GUI) import `from caco import db` directly and call raw database functions. Business logic that should be centralized is scattered across the presentation layers. For example, "play a WAD" involves downloading, cache management, session tracking, and sourceport launching -- this logic lives in `player.py`, but other operations like "import a WAD with duplicate detection" or "update a WAD with auto-completion recording" are duplicated in the CLI import_cmds, TUI import panes, and GUI import panes independently.

**Affected files:**
- `src/caco/cli/__init__.py` (`_check_and_import_entry`)
- `src/caco/cli/import_cmds.py` (`_do_auto_import` and helpers)
- `src/caco/tui/widgets/library_pane.py` (direct `db.update_wad`, `db.delete_wad`, `db.add_wad_completion` calls)
- `src/caco/gui/main_window.py` (`db.delete_wad` directly)
- `src/caco/gui/dialogs/edit_dialog.py` (save logic with tag sync)

**Recommended fix:** Introduce a `service.py` (or `services/` package) layer that encapsulates business operations. All three UIs would call service functions, and `db.py` would be a pure data-access layer.

---

### Finding 2: Excessive Database Connection Churn

**Severity: High**

Every individual function in `db.py` calls `get_connection()`, which calls `load_config()` (parses `config.toml` from disk), computes `get_db_path()`, and opens a new `sqlite3.connect()`. The `get_connection()` call appears 34 times in `db.py`. In the batch stat pattern used by both TUI and GUI, a single `load_wads()` call triggers at minimum 5 separate connection open/close cycles.

**Recommended fix:** Introduce connection caching (thread-local or module-level singleton) and cache `load_config()` result with mtime-based invalidation.

---

### Finding 3: db.py Is a Monolithic Module Combining Multiple Concerns

**Severity: Medium**

At 1361 lines, `db.py` combines: schema definition, migration logic (7 migration functions), enum definitions, query parser data structures and logic, all CRUD operations, session management, completion tracking, cache queries, and library statistics.

**Recommended fix:** Split into focused modules: `db/models.py`, `db/query_parser.py`, `db/migrations.py`, `db/connection.py`, `db/wads.py`, `db/sessions.py`, `db/completions.py`, `db/stats.py`, `db/__init__.py` (re-exports).

---

### Finding 4: update_wad Constructs Column Names from Caller-Provided Keys

**Severity: Medium**

The `update_wad` function builds a SQL `SET` clause using dict keys provided by callers without validation. While all current callers pass hardcoded field names, the pattern is structurally vulnerable.

**Recommended fix:** Add a `ALLOWED_UPDATE_FIELDS` frozenset whitelist.

---

### Finding 5: Config Is Re-Parsed from Disk on Every Access

**Severity: Medium**

`load_config()` reads and parses `config.toml` from disk on every call. Every single database operation triggers a TOML file read.

**Recommended fix:** Cache the config dict with mtime-based invalidation.

---

### Finding 6: save_config Has a Naive TOML Serializer That Drops Nested Sections

**Severity: Medium**

The `save_config` function only handles top-level keys. Nested dict sections (`[tui]`, `[gui]`, `[list]`) are silently dropped.

**Recommended fix:** Use `tomlkit` for round-trip TOML writing, or add dict handling.

---

### Finding 7: Duplicated Display Formatting Logic Across TUI and GUI

**Severity: Medium**

Both TUI and GUI independently implement status color mapping, rating star rendering, playtime formatting, last-played date truncation, tag truncation, and beaten count display.

**Recommended fix:** Extract framework-agnostic formatting into a shared module.

---

### Finding 8: Batch Stats Pattern Is Duplicated Across TUI and GUI

**Severity: Medium**

The TUI `WadTable.load_wads()` and GUI `WadTableModel.load()` implement nearly identical batch-loading patterns (4 batch queries + stats dict assembly).

**Recommended fix:** Move the batch-fetch-and-assemble pattern into a `search_wads_with_stats()` function in `db.py`.

---

### Finding 9: GUI MainWindow Reaches Into LibraryTab Private Members

**Severity: Medium**

`MainWindow` directly accesses `_sort`, `_toggle_view`, `_list_view`, `_model`, `_splitter`, `_is_grid_view` on `LibraryTab`.

**Recommended fix:** Expose public methods on `LibraryTab`.

---

### Finding 10: Migration System Lacks Version Tracking

**Severity: Low**

Migrations run every time the database is initialized, each independently checking whether it needs to run.

**Recommended fix:** Add a `schema_version` table and track migration version.

---

### Finding 11: Source Adapters Import db.add_wad Directly

**Severity: Low**

Source adapters in `src/caco/sources/` perform database writes directly instead of returning data for the caller to persist.

**Recommended fix:** Have adapters return a domain dict instead of writing directly.

---

### Finding 12: Test Coverage Is Narrow

**Severity: Low**

Only 5 unit test files exist. No tests for CLI, source adapters, import logic, config module, or integration tests.

---

### Finding 13: player.py Couples Rich Console Output with Core Logic

**Severity: Low**

The `play()` function accepts both a `Console` parameter and a `progress_callback`, creating a dual progress-reporting interface.

**Recommended fix:** Use only the callback pattern.

---

### Finding 14: Data Flows Through Untyped Dicts Throughout the Codebase

**Severity: Low**

WAD data flows as `dict[str, Any]` everywhere with no structural definition of expected keys.

**Recommended fix:** Define a `WadRecord` dataclass or `TypedDict`.

---

## What Works Well

1. **Source adapter pattern** with `BaseHttpClient` and context managers
2. **Beets-style query parser** (`ParsedQuery`/`AndGroup`/`QueryTerm`)
3. **Batch stat queries** to avoid N+1 problems
4. **Soft-delete with trash/restore**
5. **Per-WAD config overrides** with fallback to global config
6. **GUI shared model** between list and grid views
7. **Click CLI structure** with submodule registration pattern

---

## Prioritized Action Items

1. Cache config reads (Finding 5) - ~30 min
2. Fix save_config nested sections (Finding 6) - ~1-2 hours
3. Cache/reuse db connections (Finding 2) - ~1 hour
4. Add field whitelist to update_wad (Finding 4) - ~15 min
5. Extract search_wads_with_stats (Finding 8) - ~1 hour
6. Expose public API on LibraryTab (Finding 9) - ~1-2 hours
7. Extract shared formatting functions (Finding 7) - ~2 hours
8. Split db.py (Finding 3) - ~3-4 hours
9. Introduce service layer (Finding 1) - ~1-2 days
10. Add migration version tracking (Finding 10) - ~1 hour
11. Expand test coverage (Finding 12) - ongoing
12. Introduce WadRecord dataclass (Finding 14) - ~1 day
13. Decouple source adapters from db (Finding 11) - ~2 hours
14. Remove Rich dependency from player.py (Finding 13) - ~30 min
