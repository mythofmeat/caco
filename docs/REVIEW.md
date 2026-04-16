# Caco Codebase Critical Review

**Date:** 2026-04-16
**Scope:** Full codebase — all 6 Rust crates, project infrastructure, documentation
**Lines reviewed:** ~15,000+ across 100+ source files

---

## Executive Summary

Caco is a well-conceived Doom WAD library manager with solid domain modeling, a clean crate structure, and thoughtful UX patterns. The query parser, migration system, builder patterns, and batch query infrastructure are well-designed. However, the review uncovered **12 CRITICAL**, **24 HIGH**, **49 MEDIUM**, and **44 LOW** severity findings across the codebase.

The most urgent issues are:

1. **Security vulnerabilities** — ZIP path traversal in backup restore, command argument injection via DB fields, SSRF risk in URL validation
2. **Data integrity risks** — Missing transactions on multi-step DB operations, broken TUI import flow, silent error swallowing
3. **Stale documentation** — CLAUDE.md describes a Python implementation that doesn't exist in the repository
4. **Zero CI/CD** — No automated quality gates for a 49K LOC project with 632 tests

---

## Findings by Severity

### CRITICAL (12)

| # | Component | Finding | File:Line |
|---|-----------|---------|-----------|
| C1 | caco-core | **Path traversal in ZIP restore.** `restore_backup()` extracts ZIP entries via `data_dir.join(enclosed)` without verifying the resolved path stays within `data_dir`. A malicious backup ZIP with entries like `../../.bashrc` writes files outside the target directory. | `saves.rs:185` |
| C2 | caco-core | **Command argument injection via DB fields.** `play()` passes `custom_args` (JSON array from DB), `source_url`, `custom_iwad` directly as `Command::args()`. A compromised DB could inject arbitrary sourceport flags (e.g., `--exec`). No validation or sanitization. | `player.rs:156-344` |
| C3 | caco-core | **No transactions for multi-step DB operations.** `play()` performs writes across sessions, playthroughs, WAD updates, and completions without a transaction. A crash mid-session leaves the database inconsistent. Same issue in `update_wad()`. | `player.rs:93-428`, `wads.rs:241-333` |
| C4 | caco-sources | **AWS WAF detection logic inverted.** `is_aws_waf_challenged()` uses `\|\|` (OR) when it should use `&&` (AND). This returns `true` for **any** 403 response, masking real errors and preventing retry. | `http.rs:74-81` |
| C5 | caco-sources | **Entire download loaded into memory.** `response.bytes()` reads the full response body into heap. WAD files can be 100+ MB. Multiple concurrent downloads create unbounded memory pressure. | `idgames/client.rs:239` |
| C6 | caco-sources | **No transaction safety across import operations.** All import methods perform multiple DB writes without transactions. A failure at step 2 or 3 leaves a partially-initialized WAD record. | `import_service.rs:98-318` |
| C7 | caco-tui | **TUI import is completely broken.** `SearchComplete` messages are received but never forwarded to `ImportPaneState`. `set_bg_sender` is never called. Import pane uses a dummy channel that immediately drops the receiver. | `app.rs:257-263`, `tabbed_library.rs:76-78` |
| C8 | caco-gui | **File dialogs block the UI thread.** `rfd::FileDialog::pick_file()` called synchronously from the egui update loop. Freezes the entire GUI on platforms without native async file dialogs. | `edit.rs:767`, `link.rs:100`, `wad_stats.rs:210` |
| C9 | caco-gui | **Thumbnail threads have no concurrency limit.** Every visible WAD without a thumbnail spawns a new thread. Scrolling a 1000+ WAD grid creates unbounded HTTP requests and threads. No rate limiting, no thread pool, no cancellation. | `thumbnails.rs:79` |
| C10 | caco-tui/gui | **Zero tests in both TUI and GUI crates.** ~6,500 lines of UI code with no `#[test]` blocks. Untested: query building, debounce, tag delta computation, form validation, navigation, state persistence, thumbnail state machine. | Both crates |
| C11 | infrastructure | **No CI/CD pipeline.** No `.github/workflows/`, no Makefile, no pre-commit hooks, no release automation. A 49K LOC project with 632 tests has zero automated quality gates. | — |
| C12 | documentation | **Python implementation does not exist.** CLAUDE.md extensively documents a dual Rust/Python implementation with `src/caco/` paths. No `src/` directory exists. All Python Architecture, Python Commands, and feature parity sections are fiction. | `CLAUDE.md` |

### HIGH (24)

| # | Component | Finding | File:Line |
|---|-----------|---------|-----------|
| H1 | caco-core | **WadRecord uses String instead of enums.** `status`, `availability`, `source_type` stored as `String` despite proper enums existing. Consumers must call `_enum()` conversions. Scattered string comparisons throughout. | `models.rs:195-199` |
| H2 | caco-core | **Duplicated helper functions.** `read_lump_text()` in both `iwad_detect.rs` and `complevel_detect.rs`. `build_wad()` test helper reimplemented in 5 files. | Multiple |
| H3 | caco-core | **`sanitize_name` can panic on multi-byte UTF-8.** Slices at `name[..name.len().min(max_len)]` — if the slice point falls inside a multi-byte character, runtime panic. | `utils.rs:134` |
| H4 | caco-core | **Global `OnceLock<Config>` freezes config.** Config loaded once per process lifetime. GUI/TUI config changes invisible until restart. | `config.rs:261-292` |
| H5 | caco-core | **Silent error suppression with `let _ =`.** 9+ locations in `player.rs` silently ignore errors on critical operations: session-playthrough links, stats snapshots, demo file links, WAD updates. | `player.rs` (9 locations) |
| H6 | caco-core | **`get_companion_orphan_cleanup` returns String instead of enum.** Validates 3-value option but returns raw string. | `config.rs:447-453` |
| H7 | caco-sources | **New HTTP client per enrichment call.** `auto_enrich_doomwiki` creates a new `DoomwikiClient` (with fresh connection pool) for every imported WAD. 50 imports = 50 destroyed connection pools. | `import_service.rs:438` |
| H8 | caco-sources | **Config re-read from disk on every import.** `load_config()` reads and parses `~/.config/caco/config.toml` per WAD during batch imports. | `import_service.rs:432-433` |
| H9 | caco-sources | **Silent error swallowing in enrichment.** `auto_enrich_doomwiki` discards all errors including DB write failures. 6 `let _ = db::update_wad(...)` across the file. | `import_service.rs:506-507` and 5 others |
| H10 | caco-sources | **SSRF risk in URL validation.** `doomworld/client.rs` uses `url.contains("doomworld.com/forum/topic/")` — a simple string check that can be bypassed. Should parse URL and verify host. | `doomworld/client.rs:39-48` |
| H11 | caco-cli | **UTF-8 panic in `gc::truncate`.** Uses `s.len()` (byte count) then slices `&s[..max - 1]` on byte boundaries. Multi-byte UTF-8 characters cause runtime panic. Correct implementation exists in `output.rs`. | `gc.rs:791-795` |
| H12 | caco-cli | **`info` silently drops data in plain/JSON output.** `render_wad_info_plain` and `render_wad_info_json` ignore `completions` and `companions` entirely. Only table format shows them. Data loss for scripted JSON consumers. | `output.rs:314-366` |
| H13 | caco-cli | **Duplicated field-to-column mapping.** Same mapping maintained in 3 separate locations (`modify.rs` × 2, `parsing.rs`). One already out of sync (`parsing.rs` missing `"version"`). | `modify.rs:172-188,369-385`, `parsing.rs:70-79` |
| H14 | caco-cli | **Missing `-o`/`--output` on 7 commands.** `stats`, `sessions`, `cache list`, `saves list`, `demos list`, `companion ls`, `saves backups` use `--plain` boolean instead of standard `-o plain/json`. | Multiple commands |
| H15 | caco-cli | **`enrich` makes N+1 wiki API calls.** Each WAD needing complevel triggers a Doom Wiki search with no rate limiting, caching, or progress indication. | `enrich.rs:70-75` |
| H16 | caco-cli | **`play` silently consumes trailing sourceport args.** `trailing_var_arg` captures everything as query, so `caco play scythe -warp 1` treats `-warp 1` as query terms. | `play.rs:40-43` |
| H17 | caco-tui | **`play_wad` blocks the event loop.** `player::play()` runs synchronously on the main thread. If it hangs, the TUI is stuck. | `app.rs:315-363` |
| H18 | caco-gui | **Grid view renders all rows.** Iterates all WADs (including off-screen) every frame at 60fps. 1000+ WADs = 1000+ cards with gradients painted every frame. | `wad_grid.rs:86-363` |
| H19 | caco-gui | **`app.rs` is a 1427-line god file.** Contains App struct, all message dispatch, sidebar, topbar, hero (200+ lines of progress bar logic), status bar, help dialog, about dialog, and the entire update loop. | `app.rs` |
| H20 | caco-gui | **Only first action dispatched per frame.** `actions.into_iter().next()` processes only one action. Multiple simultaneous clicks lose user actions silently. | `app.rs:676-678` |
| H21 | infrastructure | **Migrations not wrapped in transactions.** Failed migration leaves DB in inconsistent state between versions. | `schema.rs:173-183` |
| H22 | infrastructure | **CLAUDE.md is severely stale.** References Python-only features, commands, architecture that don't exist. | `CLAUDE.md` |
| H23 | infrastructure | **`config.example.toml` missing keys.** Missing: `auto_detect_complevel`, `auto_doomwiki_enrich`, `companion_orphan_cleanup`, `zdoom_sourceport`, `[llm]` section, `link_mode`. | `config.example.toml` |
| H24 | caco-mcp | **`run_sql` multi-statement check bypassable.** The `has_trailing_statement` function misses `;` inside string literals. Defense relies on rusqlite `readonly()` which is valid but fragile. | `introspect.rs:326-329` |

### MEDIUM (49)

<details>
<summary>Click to expand MEDIUM findings (49)</summary>

| # | Component | Finding | File:Line |
|---|-----------|---------|-----------|
| M1 | caco-core | `WadUpdate` builder consumes `self`, loses partial state on error | `wads.rs:138-153` |
| M2 | caco-core | `find_duplicate` fuzzy match has false positive risk (`doom` matches `freedoom1.wad`) | `query.rs:525-560` |
| M3 | caco-core | N+1 query in `get_cached_wads` — calls `attach_tags()` inside a loop | `sessions.rs:543-554` |
| M4 | caco-core | Floating-point precision in `get_cache_max_size` | `config.rs:506-513` |
| M5 | caco-core | Missing partial index for session duration filtering (`duration_seconds >= 300`) | `schema.rs:66-71` |
| M6 | caco-core | `get_wad_by_cached_filename` LIKE doesn't escape `%` or `_` | `sessions.rs:572-586` |
| M7 | caco-core | `compute_availability` treats empty string `Some("")` as available | `wads.rs:11-19` |
| M8 | caco-core | `update_wad_completion` builds SQL from strings without column allowlist | `sessions.rs:414-443` |
| M9 | caco-core | `parse_and_group` negation strips only single character — incorrect for multi-byte | `query.rs:112-116` |
| M10 | caco-core | `find_completion_by_timestamp` LIKE pattern not escaped | `sessions.rs:471-472` |
| M11 | caco-core | Config has 20+ `String` fields that should be `PathBuf` | `config.rs:95-130` |
| M12 | caco-core | `ensure_config_keys` silently ignores write errors | `config.rs:339` |
| M13 | caco-core | `glob_to_like` doesn't escape `%`/`_` for non-glob inputs | `query.rs:9-15` |
| M14 | caco-core | `add_wad` doesn't validate tag contents (length, reserved chars) | `wads.rs:206-211` |
| M15 | caco-core | `start_session` checks impossible condition (`last_insert_rowid <= 0`) | `sessions.rs:25-28` |
| M16 | caco-sources | Regex-based HTML parsing in Doomworld parser is inherently fragile | `doomworld/parser.rs` |
| M17 | caco-sources | `WikitextParser` recompiles 7 regexes on every instantiation | `doomwiki/parser.rs:26-34` |
| M18 | caco-sources | `extract_first_paragraph` brace counter can go negative | `doomwiki/parser.rs:258-261` |
| M19 | caco-sources | `template_re` cannot match nested templates | `doomwiki/parser.rs:31` |
| M20 | caco-sources | `normalize_list` clones JSON values unnecessarily | `idgames/client.rs:341-355` |
| M21 | caco-sources | `parse_file_list` silently discards deserialization errors | `idgames/client.rs:373-382` |
| M22 | caco-sources | JSON import duplicates idgames parsing logic | `json_import.rs:59-91` |
| M23 | caco-sources | No input sanitization on import titles (no length limits) | `import_service.rs:259,325,342,393` |
| M24 | caco-sources | `extract_complevel` has ordering-dependent matching and accepts invalid levels (e.g., 99) | `doomworld/parser.rs:73-86` |
| M25 | caco-cli | `ls.rs` N+1 IWAD queries for preferred resolution | `ls.rs:50-57` |
| M26 | caco-cli | `looks_like_id_range` accepts space-only strings, returns all WADs | `resolve.rs:87-89` |
| M27 | caco-cli | Sort extraction can consume legitimate query terms (`caco ls rating`) | `parsing.rs:48-54` |
| M28 | caco-cli | Duplicated `truncate` function (one buggy, one correct) | `gc.rs:790-795` vs `output.rs:745-752` |
| M29 | caco-cli | Inconsistent `-y` semantics: "auto-select first" vs "skip confirmation" | Multiple commands |
| M30 | caco-cli | Session JSON output renders table instead of JSON | `output.rs:554` |
| M31 | caco-cli | Config load error silently discarded in main | `main.rs:65` |
| M32 | caco-cli | `random` uses magic sort string `"random"` not in `SORT_FIELDS` | `random.rs:25` |
| M33 | caco-cli | `glob_matches` only supports single `*` wildcard | `modify.rs:448-461` |
| M34 | caco-cli | No `--output json` for `stats` command | `stats.rs` |
| M35 | caco-cli | Demo playback bypasses `player::play` infrastructure (no session tracking) | `demos.rs:100-185` |
| M36 | caco-tui | TextInput unconditionally returns `true` from `handle_key`, swallowing Tab/Ctrl+C | `input.rs:30-32` |
| M37 | caco-tui | No page-up/page-down in library pane (hardcoded page size of 20) | `library_pane.rs:130-137` |
| M38 | caco-tui | `on_resume` refreshes all 5 panes unnecessarily | `tabbed_library.rs:188-193` |
| M39 | caco-gui | Notification expiry happens during render (side-effect in read path) | `app.rs:1322-1329` |
| M40 | caco-gui | Duplicated search/import worker logic between TUI and GUI | Multiple |
| M41 | caco-gui | Duplicated types: `SearchResultEntry`, `SearchSourceData`, status helpers, `format_size`, `opt_str`, `rating_stars` between TUI and GUI | Multiple |
| M42 | caco-gui | Edit dialog Delete WAD button is a TODO stub | `edit.rs:349` |
| M43 | caco-gui | `refresh_status_counts` does full table scan instead of `GROUP BY` query | `state.rs:207-216` |
| M44 | infrastructure | No integration test suite — all tests are unit tests within source files | — |
| M45 | infrastructure | No shared test utilities across crates | — |
| M46 | infrastructure | All MCP errors map to `internal_error` — no semantic distinction | `caco-mcp/error.rs:42-44` |
| M47 | infrastructure | Missing indexes on `wads(title)`, `wads(created_at)`, `wads(rating)` | `schema.rs` |
| M48 | infrastructure | Residual `custom_complevel` column after migration 22 (never dropped) | `schema.rs:416-427` |
| M49 | infrastructure | Shell completions missing `--record`, `-c`, `-C` flags for `play`; missing `--id24` for `ls` | `completions/` |

</details>

### LOW (44)

<details>
<summary>Click to expand LOW findings (44)</summary>

| # | Component | Finding |
|---|-----------|---------|
| L1 | caco-core | `home_dir().expect()` panics with no recovery in sandboxed environments |
| L2 | caco-core | Config warnings printed to stderr, invisible to GUI/TUI |
| L3 | caco-core | Inconsistent `Option<String>` vs `Option<&str>` in public API |
| L4 | caco-core | `search_wads` sort direction logic counterintuitive, undocumented |
| L5 | caco-core | Duplicated test setup pattern across test modules |
| L6 | caco-core | `system_time_to_rfc3339` uses `timestamp_opt` without handling ambiguity |
| L7 | caco-core | Magic number `300` for `MIN_SESSION_SECONDS` not configurable |
| L8 | caco-sources | `build_client` uses `expect()` instead of returning `Result` |
| L9 | caco-sources | Inconsistent user agent strings across clients |
| L10 | caco-sources | Serde utility functions misplaced in `http.rs` |
| L11 | caco-sources | `ImportResult` struct allows invalid states (should be enum) |
| L12 | caco-sources | Hardcoded mirror list with no health checking |
| L13 | caco-sources | `get_thread_by_id` accepts negative IDs |
| L14 | caco-sources | HTML-to-text produces multiple intermediate String allocations |
| L15 | caco-sources | `auto_link_iwad` reads WAD from DB twice |
| L16 | caco-sources | No `PartialEq` derive on `SourceError` |
| L17 | caco-sources | `WikitextParser::parse_template_params` lowercases all param names |
| L18 | caco-sources | `LlmError` not integrated with `SourceError` |
| L19 | caco-cli | `join_query_args` doesn't escape quotes within values |
| L20 | caco-cli | `ResolveMode::Error` variant has `#[allow(dead_code)]` — never used |
| L21 | caco-cli | `format_timestamp` fallback produces wrong dates silently |
| L22 | caco-cli | fzf stdin write errors silently ignored |
| L23 | caco-cli | `profile::run()` takes `&Connection` but most operations don't need it |
| L24 | caco-cli | `try_register_iwad`/`try_register_id24` swallow errors |
| L25 | caco-cli | Hardcoded `include_str!` relative paths for completion scripts |
| L26 | caco-cli | `use` statements placed after function definitions in `play.rs` |
| L27 | caco-cli | No integration tests for command dispatch |
| L28 | caco-cli | `_complete wads` loads ALL WADs into memory for shell completion |
| L29 | caco-tui | Unused `WadInfoState::current_wad_id` — set but never read |
| L30 | caco-tui | Hardcoded 500ms gg timeout |
| L31 | caco-tui | No wrap/scroll for long text in `wad_info` |
| L32 | caco-tui | `WadEditScreen::g_pressed` field unused |
| L33 | caco-gui | Dead code: `compact = false` hardcoded in `wad_table.rs` |
| L34 | caco-gui | `persist.rs` save failures silently ignored |
| L35 | caco-gui | Status bar notification truncates long errors |
| L36 | caco-gui | Wiki scraper has no caching — 3 HTTP requests per thumbnail |
| L37 | caco-gui | `resources.rs` — misleading dead import warning (actually used) |
| L38 | infrastructure | `caco-cli` directly depends on `rusqlite`/`toml`/`dirs`/`chrono` available through core |
| L39 | infrastructure | `caco-mcp` uses both `thiserror` and `anyhow` |
| L40 | infrastructure | `byteorder` dependency purpose unclear |
| L41 | infrastructure | `fs_extra` used only for directory copy — could use stdlib |
| L42 | infrastructure | `caco-mcp` references design doc at `docs/superpowers/` that doesn't exist |
| L43 | infrastructure | `inspect_schema_version` queries `user_version` pragma but migrations use `schema_migrations` table |
| L44 | infrastructure | `caco-sources` has `rusqlite` in both `[dependencies]` and `[dev-dependencies]` |

</details>

---

## Architectural Observations

### What's Working Well

1. **Crate separation** — Clean 6-crate architecture with proper dependency flow: core → sources → cli/tui/gui/mcp
2. **Builder patterns** — `NewWad::builder()` and `WadUpdate::builder()` provide type-safe DB writes
3. **Batch queries** — `get_total_playtime_batch()`, `get_last_played_batch()`, `fetch_tags_batch()` avoid N+1 patterns
4. **Query parser** — Beets-style syntax with OR, negation, status shortcuts, and glob support is well-implemented
5. **Migration system** — Idempotent migrations with version tracking and column existence guards
6. **MCP sandbox** — Isolated environment with path validation, read-only SQL, and config sanitization
7. **Detection logic** — IWAD (PNAMES + map lumps) and complevel (COMPLVL > UMAPINFO > DEHACKED) detection is thorough
8. **Test coverage in core** — 632 tests with good edge case coverage in core and CLI data modules

### Key Architectural Issues

1. **No service layer** — CLI commands import `caco_core::db::*` directly and perform raw SQL. The `companion_service.rs` and `resource_service.rs` suggest a service pattern was started but not completed. Commands like `gc`, `enrich`, `import` duplicate business logic.

2. **Shared state between TUI and GUI** — Both UI crates independently define the same types (`SearchResultEntry`, `SearchSourceData`), theme helpers, worker functions, and format utilities. A `caco-ui-common` crate or moving shared types into `caco-sources` would eliminate ~500 lines of duplication.

3. **Stateless ImportService** — `pub struct ImportService;` is a unit struct that creates HTTP clients and reads config on every call. It should own its clients and configuration as fields.

4. **Stringly-typed database models** — `WadRecord` stores `status`, `availability`, `source_type` as `String` despite having proper enums. This forces runtime conversions and prevents compile-time guarantees.

5. **Config immutability** — `OnceLock<Config>` means long-running processes (TUI, GUI) can't see config changes without restart.

---

## Recommendations (Priority Order)

### P0 — Fix Immediately

| # | Action | Impact |
|---|--------|--------|
| 1 | Fix ZIP path traversal in `saves.rs:185` — canonicalize paths and verify containment | Security |
| 2 | Fix AWS WAF logic bug — change `\|\|` to `&&` in `http.rs:74` | Correctness |
| 3 | Fix UTF-8 panic in `gc::truncate` — replace with `output::truncate_str` | Crash prevention |
| 4 | Wrap `play()` DB operations in a transaction | Data integrity |
| 5 | Wrap import DB operations in a transaction | Data integrity |
| 6 | Fix TUI import flow — wire `set_bg_sender` and forward `SearchComplete` | Broken feature |
| 7 | Stream downloads instead of `response.bytes()` | OOM prevention |

### P1 — Fix Soon

| # | Action | Impact |
|---|--------|--------|
| 8 | Add CI pipeline (`cargo test`, `cargo clippy`, `cargo fmt --check`) | Quality gates |
| 9 | Remove stale Python documentation from CLAUDE.md | Truthfulness |
| 10 | Validate command args from DB in `player.rs` (allowlist/blocklist) | Security |
| 11 | Validate URLs properly (parse + host check) in Doomworld client | SSRF prevention |
| 12 | Add thread pool for GUI thumbnails (bounded channel or rayon) | Resource management |
| 13 | Add virtual scrolling to GUI grid view | Performance |
| 14 | Refactor `ImportService` to own HTTP clients and config | Architecture |
| 15 | Fix `info` command dropping completions/companions in plain/JSON | Data loss |

### P2 — Plan and Implement

| # | Action | Impact |
|---|--------|--------|
| 16 | Convert `WadRecord` string fields to enums | Type safety |
| 17 | Extract shared UI types into common crate/module | Deduplication |
| 18 | Decompose `app.rs` (1427 lines) into focused modules | Maintainability |
| 19 | Add database indexes on `title`, `created_at`, `rating` | Performance |
| 20 | Replace `OnceLock<Config>` with reloadable config for TUI/GUI | UX |
| 21 | Add basic tests for TUI and GUI (pure logic functions first) | Coverage |
| 22 | Standardize `--output` flag across all commands | Consistency |
| 23 | Add `--output json` to `stats` command | Scriptability |
| 24 | Wrap each migration in a transaction | Robustness |
| 25 | Update `config.example.toml` with all available keys | Documentation |

---

## Positive Patterns Worth Highlighting

- **Idempotent migrations** with `IF NOT EXISTS` guards — makes schema evolution safe
- **Picker fallback chain** (fzf → numbered menu → first match) — good UX degradation
- **Dry-run support** in `gc`, `modify`, `cache`, `trash` — safe preview before destructive operations
- **Companion file deduplication** via MD5 hashing — prevents storage waste
- **Per-WAD config** (custom IWAD, sourceport, args, complevel) — flexible without global state
- **Sourceport family abstraction** — maps 7 families to correct args, saves, configs
- **Progress bars** with indicatif for downloads and long operations
- **Atomic download** with mirror fallback in idgames client

---

*This review was conducted by examining all source files across 6 Rust crates. Findings reference specific file:line locations for easy navigation. Each finding includes enough context to understand the issue without further investigation.*
