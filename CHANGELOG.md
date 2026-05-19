# Changelog

All notable changes to Caco are documented in this file.
Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [3.2.2] - 2026-05-19

### Added

- Manage completions & stats from CLI and GUI
- **db**: Snapshot DB before migrations; document recovery
- **core**: Add start_new_playthrough for replay flow
- **db**: Drop play_state and intent columns in migration 32
- **completions**: Add profile/saves/demos/sessions completions, fix Helion exe case
- **sourceports**: Add Helion family with .ini config extension
- **stats**: Add ZDoom-family stats collection via custom PK3 mod
- **sourceports**: Add uzdoom to zdoom family and wiki detection
- **detect**: Auto-detect ZDoom-family sourceport requirement
- **analysis**: Add PK3 support with MAPINFO-driven completion detection
- **play**: Auto-detect WAD completion and enforce playing/queued exclusivity
- **core**: Add tri-axis status model with dual-write sync
- Global path override flags and env vars for CLI

### Changed

- Stats reporter: Rust-side header-fallback for save-load regression
- Stats reporter: WorldLoaded-side fallback for uzdoom regression
- Add `supported` flag on cacowards, exclude unsupported from totals
- Add cacoward: query filter and ID helpers
- Wire Cacowards into the CLI (enrich + stats)
- Add cacowards table and CRUD module
- Detect Boss Brain exits in WAD analysis
- Fix GUI link retry and sourceport family clearing
- Fix WAD completion exit detection
- Show short sessions with map progress
- Fix session map deltas from DB snapshots
- Fix ZDoom stats session tracking
- **completion**: Split classifier and verdict into pure layers
- Add sourceport family metadata
- Overhaul Doomworld import metadata; drop LLM path
- Address code review findings from prior feature batch
- **core**: Make config reloadable via ArcSwap<Config>
- **db**: Type WadRecord status/availability/source_type as enums
- **db**: WadUpdate accumulates errors instead of short-circuiting
- Validate custom_args on write, not just at play time
- **db**: Drop custom_complevel column, aggregate status counts
- Stream downloads, bound thumbnail pool, harden zip extract, add CI
- Wrap player/import in transactions, surface silent errors
- **db**: Add transaction helper, wrap migrations + update_wad
- **playthroughs**: Use simplified Status enum
- **core**: Update mod.rs re-exports and sessions.rs for status simplification
- **query**: Update query parser for simplified Status enum
- **core**: Add migration 32 to consolidate status columns
- **core**: Simplify wads.rs — delete sync layer, update builders
- **core**: Simplify Status to 4-variant enum, remove PlayState/Intent
- Deduplicate code and fix quality issues across all crates
- Deduplicate code, fix bugs, and improve error handling across crates
- Expand detection, companion system, sourceports, utils, wad stats
- Update .gitignore: add Rust target/, reorganize by category

### Fixed

- **completion**: Unify GUI and verdict on the classifier's Required set
- Resolve sourceport family TODOs
- **completion**: Require real terminal progress
- **detect**: Verify map markers are followed by map data lumps
- **detect**: Only count IWAD-required patches reachable from used textures
- Auto-heal wad completion stats after sourceport hangs
- **gui**: Grid progress bar pinned at 100% for levelstat WADs
- **db**: Don't duplicate completion on idempotent status rewrite
- **core**: Complete zdoom auto-completion + repair drifted status
- **core**: Require real exit to count a new map as played
- **core**: Detect session progress via compute_stats_delta
- **gui**: Properly AND status filter with collection/user query
- **core**: Only mark WAD in-progress after actual level progress
- **completion**: Auto-complete when terminal exited despite orphan map slots
- Zdoom family uses .ini config, not .cfg
- Auto-download without idgames_id, complevel 77 bug, playthrough init
- **db**: Drop indexes before columns in migration 32
- Update player.rs play_state reference to status
- **analysis**: Raise ZIP entry cap to 1 GiB for large megaWADs
- **cli**: Preserve spaces in query args like tag:"multi word"
- **analysis**: Invalidate stale cached analyses via version tracking
- **analysis**: Detect Boom generalized exit linedefs
- **helion**: Pass -config arg and use wall-clock playtime
- **detect**: Check all WADs in PK3 archives, not just maps/ directory
- **detect**: Rewrite zdoom detection to only check UDMF map format
- **detect**: Remove DECORATE and GLDEFS from zdoom detection
- **completion**: Guard against false Complete when required=0
- **db**: Filter out short sessions (<5min) from stats and listings
- **db**: Prevent started+dropped (playing+abandoned) state conflict
- Find and merge all stats files in WAD data directories
- Pre-create dsda nested save dir before launching sourceport
- Use Doom PLAYPAL instead of grayscale for TITLEPIC rendering

## [3.0.0] — 2026-04-16

### BREAKING CHANGES

- **CLI**: The `--plain` flag has been removed from `cache list`, `saves list`, `saves backups`, `demos list`, and `stats`. Use `-o plain` (or `-o json` / `-o table`) instead. `--plain` remains on `info --levelstats`, `sessions`, and `companion ls` where it is a content-format selector, not a list-output switch.
- **Sourceport profiles**: zdoom-family ports (gzdoom, uzdoom, lzdoom, vkdoom, qzdoom, zdoom) now write their config as `.ini`, not `.cfg`, matching what the ports actually read. Existing `.cfg` profiles for zdoom ports are orphaned — rename to `.ini` if they contain real content.

### Added

- **MCP server (`caco-mcp`)**: new workspace crate and `caco-mcp-server` binary exposing 17 CLI commands (`caco_ls`, `caco_info`, `caco_modify`, `caco_trash`, `caco_random`, `caco_import`, `caco_cache`, `caco_stats`, `caco_sessions`, `caco_saves`, `caco_demos`, `caco_collection`, `caco_companion`, `caco_profile`, `caco_enrich`, `caco_gc`, `caco_config`) and 7 read-only DB introspection tools (`inspect_schema_version`, `inspect_wad`, `inspect_sessions`, `inspect_companions`, `inspect_iwads`, `inspect_id24`, `run_sql`) over the Model Context Protocol. All operations run against a sandboxed copy of the user's library; a hard safety guard refuses any path that resolves to the real `CACO_HOME` / `XDG_DATA_HOME/caco`. `run_sql` opens the DB read-only, requires `Statement::readonly()`, and rejects multi-statement input.
- **Import**: Doom Wiki pages that link to their idgames archive entry (via `{{ig|...}}` templates) now auto-populate `idgames_id` on the imported WAD, making them downloadable via `caco play` without manual follow-up.
- **Config reload**: Ctrl+R in the TUI and GUI re-reads `~/.config/caco/config.toml` without a restart. Subsequent reads see new values; in-memory state captured at startup (default sort, window geometry) still requires a restart.
- **DB migration backups**: Before running any pending migration, the live database is snapshotted to `~/.local/share/caco/backups/pre-migration-<N>.db`. README gains a "Recovering from a bad migration" section documenting the recovery path.
- **CI**: New `.gitea/workflows/ci.yml` runs fmt + clippy + tests on every push and PR. `package.yml` remains the tag-only release pipeline.
- **Bundled font**: NotoSansSymbols2 is now bundled so the completed-WAD ✓ badge and other dingbats render instead of showing tofu.
- **Icon**: GUI window now embeds `assets/caco.png` so the title bar and taskbar show the proper icon. `install.sh` places the icon at standard `256x256` plus a scalable fallback and runs `gtk-update-icon-cache`.

### Fixed

- **Completion tracking**: WADs with DEHACKED-patched exit handling (e.g. Pina Colada 2) now auto-complete when the terminal map is exited, even when static analysis incorrectly marks orphan map slots as required.
- **Doom Wiki imports**: `caco import https://doomwiki.org/wiki/<title>` now extracts the title from the URL and fetches the page directly, instead of doing a failed search with the entire URL as a query.
- **idgames URL routing**: `doomworld.com/idgames/?id=N` URLs now route to the idgames importer instead of being handed to the Doomworld forum client, which rejected them. The JSON fallback path is fixed similarly.
- **GUI Unavailable dialog**: Download failures (API blocked, no stored path, direct download failed, idgames fetch failed) now surface the Link dialog instead of a plain toast, so the user can relink or open the source URL.
- **MCP sandbox isolation**: `reset_sandbox` now strips `db_path`, `cache_dir`, `data_dir`, `iwad_dir`, `sourceport_dir`, and `iwad_dirs` from the copied config. Previously absolute path fields in the user's config could let a sandboxed caco invocation reach the real library despite env-var isolation.
- **GUI dingbats**: Completed-WAD ✓ badge rendered as tofu; NotoSansSymbols2 is now bundled to cover dingbats, geometric shapes, and misc symbols.
- **P0 correctness bugs from counter-review**: AWS WAF challenge detection used `||` where `&&` was intended; `Screen::on_search_complete` hook added so TUI search results actually reach the screen; `gc` output now uses UTF-8-safe truncation; `ls` plain/JSON renderers now include completions and companions (scripted consumers were silently losing data).
- **Transactions**: Player pre-session block (ensure_playthrough + start_session + id update) is now atomic; each import path wraps add_wad + auto-link/enrich in one transaction so partial imports roll back cleanly; migrations wrap body + `schema_migrations` insert so failed migrations roll back atomically and aren't recorded as applied.
- **custom_args validation**: TUI and GUI edit paths now normalize and validate `custom_args` on write, matching the CLI. Previously malformed JSON was silently stored and only surfaced at launch time.

### Changed

- **Output flags**: `cache list`, `saves list`, `saves backups`, `demos list`, and `stats` now take `-o plain|json|table` (see Breaking Changes). Each command emits a JSON representation alongside its table and plain formats, matching `ls`, `info`, and `collection`.
- **GUI file pickers**: `rfd::FileDialog::pick_file` / `save_file` now run on short-lived worker threads so the egui update loop no longer freezes while the OS picker is open. Every dialog site (Link, Edit companions, Resources IWAD/id24, WAD stats Import/Export, local import Browse) polls a receiver each frame and gates the button on receiver-empty so a second picker can't spawn.
- **DB types**: `WadRecord.status` / `availability` / `source_type` are now typed enums instead of `String`, eliminating per-site `Status::parse` calls and typos that compiled.
- **DB: WadUpdate error accumulation**: Setters now return `Self` and collect rejected field names into a `Vec<String>`; `update_wad` validates up front and returns `Error::InvalidFields` listing every bad field at once, instead of short-circuiting with bare `None`.
- **DB schema**: Migration 33 drops the now-unused `custom_complevel` column (migration 22 had already merged its data into `complevel`).
- **GUI status counts**: Sidebar refresh now uses a single `GROUP BY` aggregate query instead of materialising every WAD row to count in Rust.
- **GUI grid rendering**: Cards outside the scroll viewport now skip painter ops and event handling, cutting per-frame work from O(total_wads) to O(visible). `allocate_exact_size` still runs so scrollbar extents and keyboard-navigation indices stay consistent.
- **GUI action dispatch**: `CacoApp` now dispatches every queued `ActionRequest` per frame instead of only the first; rapid clicks are no longer silently dropped.
- **GUI config snapshot**: Global config is now an `ArcSwap<Config>` snapshot rather than `OnceLock<Config>`, so `load_config()` returns `Arc<Config>` from a lock-free snapshot and `reload_config()` atomically publishes new values.
- **GUI filter debounce**: The 150ms filter debounce is now a testable `FilterQuery` struct with a pure `poll()` method, replacing the three loose `AppState` fields. Six unit tests cover the timing cases.
- **ImportService**: Now a stateful struct holding `Arc<DoomwikiClient>` and `Arc<IdgamesClient>` so repeated auto-enrich / auto-link passes reuse the same `reqwest::Client` and TLS state instead of rebuilding per call.
- **Downloads**: idgames downloads now stream via `Response::copy_to` instead of buffering the entire response. Memory stays flat for large WADs.
- **Thumbnails**: Per-request `thread::spawn` replaced with a bounded mpsc worker pool capped at `min(available_parallelism, 4)`.
- **ZIP extraction**: Defence-in-depth canonicalization check on save extract paths in addition to the existing `enclosed_name` guard.
- **Dev builds**: Now link with `mold` via clang for faster iteration. Requires clang and mold (both standard on Arch).
- **GUI code structure**: `app.rs` split from 1427 lines into `app/help.rs`, `app/status_bar.rs`, `app/section_header.rs`, `app/hero.rs`, `app/sidebar.rs`, `app/topbar.rs`, and `app/dialogs.rs` submodules.
- **Tests**: +28 new unit tests covering TUI filter_input/library_pane, GUI thumbnails, and GUI import state. Workspace test count now 1299.

## [2.2.1] — 2026-04-10

### Changed
- Gitea workflow adjustments

## [2.2.0] — 2026-04-08

### Added
- GUI: Completion badge overlay on grid cards showing progress status
- GUI: "Start New Playthrough" context menu option for replaying WADs
- CLI: `--new-playthrough` flag on play command for starting fresh playthroughs
- CLI: Borderless table output with dimmed headers and truncated author names

### Fixed
- Clippy warnings from replay and badge changes

### Changed
- Removed dead detail panel code and unused `BeatenAdd`/`BeatenRemove` messages from GUI

## [2.1.0] — 2026-04-08

### Added
- GUI: Multi-select filter pills replacing sidebar status dropdown, with grid progress bars
- GUI: Random sort order, collection context menu, collection icon fix
- GUI: Required vs secret map progress display in hero card
- CLI: Improved modify UX for tag removal with clearer prompts
- Analysis: Boom generalized exit linedef detection
- Analysis: Version-tracked cached analyses with automatic stale invalidation

### Fixed
- Auto-download WADs without `idgames_id`, complevel 77 bug, playthrough initialization
- DB: Drop indexes before columns in migration 32 to avoid SQLite errors
- CLI: Preserve spaces in query args like `tag:"multi word"`
- Analysis: Raise ZIP entry size cap to 1 GiB for large megaWADs
- Helion: Pass `-config` arg correctly and use wall-clock playtime

### Changed
- **Status model simplification**: Consolidated tri-axis intent/play_state into a single 4-value Status enum (unplayed/in-progress/completed/abandoned) via migration 32
- Arch packaging: Split into separate `caco`, `caco-gui`, `caco-tui` packages
- Codebase-wide deduplication and quality fixes across all crates

## [2.0.0] - 2026-04-01

Major release adding the tri-axis status model, automatic completion detection,
PK3 support, ZDoom/Helion sourceport families, and collections.

### Added
- **Tri-axis status model**: Split single status into intent (to-play/backlog/awaiting-update) and play state (queued/playing/finished/abandoned) with dual-write sync for backwards compatibility
- **Auto-completion detection**: Detect WAD completion after play sessions and enforce playing/queued exclusivity
- **PK3 support**: MAPINFO-driven completion detection for PK3 archives
- **ZDoom requirement detection**: Auto-detect ZDoom-family sourceport requirement from UDMF map format
- **ZDoom stats collection**: Custom PK3 mod for collecting per-map stats from ZDoom-family ports
- **Helion sourceport family**: Full integration with `.ini` config extension
- **UZDoom sourceport**: Added to ZDoom family with Doom Wiki detection
- **Collections**: Named collections dialog in GUI, replacing saved searches
- Tri-axis UI across all interfaces: intent/play_state pills and tabs in GUI, tab colors in TUI, modify/import/gc support in CLI
- Shell completions for profile, saves, demos, and sessions commands

### Fixed
- Detect all WADs inside PK3 archives, not just maps/ directory
- Guard against false completion when `required=0`
- Read `source_id` as idgames ID fallback when `idgames_id` is unset
- GC: Collect all completed WADs in a single batch prompt
- Filter out short sessions (<5 min) from stats and listings
- Prevent started+dropped (playing+abandoned) state conflict
- GUI: Remove gradient overlay on real thumbnails, smooth placeholder gradient

### Changed
- Enrich only reports `zdoom_required=true` as a visible change
- GUI: Unify intent + play_state into single mutually exclusive status display

## [1.2.0] - 2026-03-27

### Added
- GUI redesign: Warm launcher aesthetic with cover flow card grid
- Auto-download idgames WADs on play from GUI

### Fixed
- Pre-create dsda nested save directory before launching sourceport
- Missing GUI icons and status bar hint removal

## [1.1.0] - 2026-03-27

### Fixed
- Restore desktop entry and application icon
- PKGBUILD: Use `options=(!lto)` instead of manual CFLAGS stripping for bundled SQLite

## [1.0.0] - 2026-03-26

First Rust release — complete rewrite from the original Python implementation
with full CLI feature parity and a native egui GUI.

### Highlights
- **Rust CLI** (clap) with all commands: ls, info, modify, import, play, trash, random, companion, gc, enrich, stats, sessions, cache, saves, demos, profile, config, completions
- **Rust GUI** (egui/eframe): Library table + grid views, detail panel, import tab, edit/delete/sessions/stats/cache/resources dialogs
- **Rust TUI** (ratatui): Tabbed library, WAD detail, edit, sessions, stats, cache, resources screens
- 632 tests across all crates
- Arch Linux PKGBUILD packaging

### Core Library (caco-core)
- SQLite database with 23+ migrations, shared with Python implementation
- Builder pattern for WAD creation/updates (`NewWad::builder()`, `WadUpdate::builder()`)
- Beets-style query parser: field queries, OR, negation, status shortcuts, globs
- Sourceport family registry: dsda, zdoom, chocolate, woof, eternity
- Companion file system with MD5 deduplication and managed storage
- IWAD detection from PNAMES lump analysis and map lump fallback
- Complevel detection hierarchy: COMPLVL > UMAPINFO > DEHACKED > map lumps
- IWAD/id24 resource management with auto-detection
- Per-WAD data directories, save/demo management, stats tracking
- Sourceport config profile management
- Garbage collection for finished/abandoned WAD data

### Import Sources (caco-sources)
- idgames API, Doom Wiki, Doomworld forum, URL, and local file import
- JSON import fallback for Cloudflare-blocked APIs
- Auto-enrichment with Doom Wiki metadata on import

### GUI Features
- Context menus, menu bar with keyboard accelerators, native file picker
- WAD Stats dialog with import/export and completion comparison
- Tabbed Edit dialog with Source info and Companions tabs
- WAD Unavailable / Link dialog for missing WAD files
- Clickable source links and map progress in detail panel
- Grid view with responsive cards, rating stars, thumbnails, tooltips
- Arrow key + vim keybindings, keyboard shortcuts help dialog
- GUI state persistence across sessions
- TITLEPIC rendering using Doom PLAYPAL palette

### Infrastructure
- Build infrastructure: workspace metadata, release profile, git hash
- Global path override flags and environment variables for CLI

## Pre-1.0 — Python Era (January–March 2026)

The original Python implementation. Key milestones:

- **2026-01-24**: Initial scaffold — idgames import, beets-style queries, config, fish completions
- **2026-01-30**: CLI UX improvements, cache management, tag completions
- **2026-02-01**: Doom Wiki and Doomworld import sources
- **2026-02-05**: Per-map statistics, session tracking, playtime tracking
- **2026-02-10**: IWAD management with MD5 identification, family/variant rework
- **2026-02-15**: Per-WAD data directories, sourceport family registry, auto stats tracking
- **2026-02-20**: Companion files with MD5 dedup, complevel detection, auto-enrichment
- **2026-02-25**: Save/demo management, sourceport profiles, crash detection, bash/zsh completions
- **2026-03-02**: Companion file redesign — relational model with managed storage
- **2026-03-05**: PySide6 GUI with dark theme, grid/list views, thumbnails
- **2026-03-08**: Textual TUI with tabs, sort, edit, idgames search
- **2026-03-10**: Comprehensive test suite, mypy integration, garbage collection
- **2026-03-18**: JSON import fallback, Cloudflare bypass, offline support

[Unreleased]: http://localhost:3000/eshen/caco/compare/v3.0.0...main
[3.0.0]: http://localhost:3000/eshen/caco/compare/v2.2.1...v3.0.0
[2.2.1]: http://localhost:3000/eshen/caco/compare/v2.2.0...v2.2.1
[2.2.0]: http://localhost:3000/eshen/caco/compare/v2.1.0...v2.2.0
[2.1.0]: http://localhost:3000/eshen/caco/compare/v2.0.0...v2.1.0
[2.0.0]: http://localhost:3000/eshen/caco/compare/v1.2.0...v2.0.0
[1.2.0]: http://localhost:3000/eshen/caco/compare/v1.1.0...v1.2.0
[1.1.0]: http://localhost:3000/eshen/caco/compare/v1.0.0...v1.1.0
[1.0.0]: http://localhost:3000/eshen/caco/src/tag/v1.0.0
