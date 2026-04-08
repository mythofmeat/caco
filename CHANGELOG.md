# Changelog

All notable changes to Caco are documented in this file.
Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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

[Unreleased]: http://localhost:3000/eshen/caco/compare/v2.1.0...main
[2.1.0]: http://localhost:3000/eshen/caco/compare/v2.0.0...v2.1.0
[2.0.0]: http://localhost:3000/eshen/caco/compare/v1.2.0...v2.0.0
[1.2.0]: http://localhost:3000/eshen/caco/compare/v1.1.0...v1.2.0
[1.1.0]: http://localhost:3000/eshen/caco/compare/v1.0.0...v1.1.0
[1.0.0]: http://localhost:3000/eshen/caco/src/tag/v1.0.0
