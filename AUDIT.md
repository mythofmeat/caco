# Code Quality Audit

Comprehensive audit of the Rust codebase, organized by priority.

## HIGH PRIORITY

### 1. `player::play()` is 324 lines
**File:** `crates/caco-core/src/player.rs:66-390`

Handles WAD resolution, sourceport selection, IWAD detection, complevel detection, companion loading, demo recording, session tracking, and stats collection. Should be broken into 5-6 focused functions.

### 2. `Box::leak()` memory leak
**File:** `crates/caco-cli/src/commands/demos.rs:151`
```rust
Some(Box::leak(iwad.into_boxed_str()) as &str)
```
Intentionally leaking memory to get `'static` lifetime. Should use proper lifetime management or pass an owned `String`.

### 3. 15+ consecutive `.unwrap()` on WadUpdate builder
**Files:**
- `crates/caco-gui/src/dialogs/edit.rs:875-920`
- `crates/caco-tui/src/screens/wad_edit.rs:179-224`

Both build `WadUpdate` with identical patterns and identical unwrap chains. Extract builder logic to shared function in `caco-core`; either make builder infallible or propagate errors.

### 4. TUI theme duplicates core display logic
**File:** `crates/caco-tui/src/theme.rs:22-139`

Reimplements `status_display()`, `play_state_display()`, and `intent_display()` — all already exist as `Status::display_name()`, `PlayState::display_name()`, and `Intent::display_name()` in `caco-core::db::models`. Should import from core.

### 5. TUI suppresses 7 clippy lints globally
**File:** `crates/caco-tui/src/lib.rs:1-8`
```rust
#![allow(clippy::collapsible_if, clippy::collapsible_else_if, ...)]
```
Fix the underlying issues rather than suppressing.

---

## MEDIUM PRIORITY — Duplicated Code

### 6. Recursive directory traversal — 3 near-identical implementations
- `crates/caco-core/src/player.rs:531-548` (`collect_stats_files_recursive`)
- `crates/caco-core/src/saves.rs:50-92` (`collect_save_files_recursive`)
- `crates/caco-core/src/saves.rs:139-150` (`collect_all_files_recursive`)

Extract generic `walk_dir_recursive()` helper in `utils.rs`.

### 7. Timestamp conversion — repeated 4+ times
Same `SystemTime → UNIX_EPOCH → chrono → to_rfc3339()` pattern in:
- `crates/caco-core/src/saves.rs` (3 occurrences)
- `crates/caco-core/src/demos.rs` (1 occurrence)

Create `utils::system_time_to_rfc3339()` helper.

### 8. `get_data_dir()` duplicated in CLI
- `crates/caco-cli/src/commands/saves.rs:77-82`
- `crates/caco-cli/src/commands/demos.rs:61-66`

Nearly identical WAD resolution + data dir lookup. Extract to shared utility.

### 9. Confirmation prompt pattern — duplicated 5+ times in CLI
`eprint!() → flush() → read_line() → starts_with('y')` in:
- `trash.rs` (3 times), `saves.rs`, `profile.rs`

Extract `confirm(prompt: &str) -> bool` helper.

### 10. Import result handling — duplicated 4 times
**File:** `crates/caco-cli/src/commands/import.rs`
Lines 252-264, 373-385, 647-659, 726-738 all follow identical pattern: check `is_duplicate` → print success → print error. Extract helper.

### 11. idgames download logic duplicated within same file
- `crates/caco-sources/src/idgames/client.rs:223-273` (`download`)
- `crates/caco-sources/src/idgames/client.rs:282-350` (`download_direct`)

Both implement atomic file download with `.partial` extension, progress callbacks, and cleanup. Extract shared helper.

### 12. Progress bar setup duplicated in play.rs
**File:** `crates/caco-cli/src/commands/play.rs:211-226` and `252-267`
Two identical progress bar setup sequences.

### 13. GUI/TUI share heavy duplication across 6+ areas

| Area | GUI file | TUI file | Duplicated |
|------|----------|----------|------------|
| Delete dialog | `dialogs/delete.rs` | `screens/confirm_delete.rs` | State struct + DB queries |
| Stats display | `dialogs/stats.rs` | `screens/stats.rs` | Data fetching + section org |
| Cache mgmt | `dialogs/cache.rs` | `screens/cache.rs` | `CacheEntry` struct + load logic |
| Sort fields | `state.rs:91-99` | `widgets/sort_select.rs:8-16` | Identical `SORT_FIELDS` array |
| Search panel | `import/search_panel.rs` | `widgets/search_pane.rs` | Search state + navigation |
| Rating stars | `theme.rs:119-128` | `theme.rs:142-147` | Nearly identical function |

Data model and business logic (not rendering) should move to `caco-core`.

### 14. Regex compiled on every call
**File:** `crates/caco-sources/src/import_service.rs:92`
```rust
let re = Regex::new(r"[^a-z0-9\s]").unwrap();
```
Should use `LazyLock`.

---

## MEDIUM PRIORITY — Error Handling

### 15. Silent error suppression with `let _`
- `crates/caco-core/src/player.rs:209,212,222,234,256` — `create_dir_all()` and `File::create()` errors ignored
- `crates/caco-cli/src/commands/trash.rs:165,196` — file removal errors silently ignored
- `crates/caco-cli/src/commands/cache.rs:193,228` — same pattern

At minimum, log these errors.

### 16. GUI delete dialog confirms even on error
**File:** `crates/caco-gui/src/dialogs/delete.rs:90-96`
```rust
Err(e) => {
    eprintln!("delete failed: {e}");
    result = DeleteResult::Confirmed;  // still confirms
}
```

### 17. Unsafe unwraps in json_import.rs
**File:** `crates/caco-sources/src/json_import.rs:68-77`
`.unwrap()` calls after manual `.is_some()` / `.is_array()` checks. Use `if let` / pattern matching.

### 18. `enrich.rs` uses `.unwrap()` on WadUpdate operations
**File:** `crates/caco-cli/src/commands/enrich.rs:146-209` — 5 locations. Use `?` operator.

---

## LOW PRIORITY — Code Smells

### 19. `modify.rs` apply_modifications is 197 lines
Field-to-column mapping duplicated in two places (lines 133-151 and 309-329).

### 20. `caco-gui/src/app.rs` is 1347 lines
8 sequential dialog render-match blocks (lines 386-500) that could use trait-based approach.

### 21. `caco-gui/src/dialogs/edit.rs` is 1008 lines
Single file handling all edit tabs. Should split by tab.

### 22. `AppState` has 25+ fields mixing concerns
**File:** `crates/caco-gui/src/state.rs` — UI state, business logic, and config mixed. Group with newtypes.

### 23. Status order arrays duplicated
`["to-play", "backlog", ...]` appears in `output.rs` (2 times) and `screens/stats.rs` (1 time, different order). Extract to core constant.

### 24. Doomwiki WAF detection has redundant logic
**File:** `crates/caco-sources/src/doomwiki/client.rs:41-60` — checks status and header twice. Simplify.

---

## WELL-DESIGNED (no action needed)

- Config loading via `OnceLock` — properly centralized
- DB connection handling — consistent pragmas across all entry points
- Query/filter parser — single source of truth in `caco-core`
- Complevel formatting — well centralized with `LazyLock`
- No `unwrap()` in production core code (only tests and config defaults)
- Good builder patterns (`NewWad`, `WadUpdate`)
- Comprehensive test coverage (632 tests)
