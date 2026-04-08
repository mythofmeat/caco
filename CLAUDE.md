# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Caco is a personal Doom WAD library manager inspired by `beets`. It tracks WADs you want to play, have played, or are playing, with metadata from multiple sources (idgames, Doomwiki, Doomworld forums, manual entry). Key features:

- SQLite database for WAD metadata and play history
- Import from idgames archive, Doom Wiki, Doomworld forums, URLs, or local files
- Automatic playtime tracking via sourceport wrapper
- Tag-based organization and beets-style query syntax
- On-demand downloading (WADs are cached, not stored permanently)
- LLM-powered metadata extraction (optional, for Doomworld imports)
- Completion tracking with per-map stats import/export and auto-tracking
- Companion file management with MD5 deduplication
- IWAD/id24 registry with auto-detection from WAD file contents
- Sourceport config profile management
- Garbage collection for completed/abandoned WAD data
- Three interfaces: CLI, TUI (ratatui), and GUI (egui)

## Dual Implementation

The project has two implementations sharing the same SQLite database and config:

- **Rust** (`crates/`) — active development target, at full CLI feature parity with Python, 632 tests
- **Python** (`src/caco/`) — original implementation, feature-complete including stats watchers

The Rust CLI is the primary development focus. Both implementations read/write the same `~/.local/share/caco/library.db` and `~/.config/caco/config.toml`.

## Commands (Rust)

```bash
# Build
cargo build --workspace
cargo build --release -p caco-cli    # Release CLI binary

# Run CLI
cargo run -p caco-cli -- <command>

# Run GUI
cargo run -p caco-gui

# Run TUI
cargo run -p caco-tui

# Tests
cargo test --workspace               # 632 tests
cargo clippy --workspace -- -D warnings  # Lint (warnings = errors)

# Examples
cargo run -p caco-cli -- ls
cargo run -p caco-cli -- ls -o plain
cargo run -p caco-cli -- info 1 -o json
```

## Commands (Python)

```bash
uv sync
uv sync --extra test
uv run caco <command>
uv run pytest tests/ -v
```

## Rust Architecture

```
crates/
├── caco-core/          # Core library (DB, config, detection, player, services)
│   └── src/
│       ├── lib.rs              # Re-exports all modules
│       ├── error.rs            # Error/Result types (thiserror)
│       ├── config.rs           # TOML config, ensure_config_keys(), resolve_iwad()
│       ├── player.rs           # Sourceport launcher + playtime tracking + companion injection
│       ├── companion_service.rs # MD5 dedup, managed storage, orphan cleanup
│       ├── resource_service.rs  # IWAD/id24 registration (identify + copy + DB)
│       ├── sourceports.rs      # Family registry (dsda/zdoom/chocolate/woof/eternity/helion/uzdoom)
│       ├── complevel.rs        # Shared names, aliases, parse_complevel()
│       ├── complevel_detect.rs # Auto-detect from WAD lumps (COMPLVL, UMAPINFO, DEHACKED)
│       ├── iwad_detect.rs      # Auto-detect IWAD from PNAMES/map lumps
│       ├── wad_stats.rs        # Per-map stats parser (stats.txt + levelstat.txt), progress display
│       ├── saves.rs            # Save file discovery, backup/restore
│       ├── demos.rs            # Demo file management
│       ├── titlepic.rs         # TITLEPIC extraction from WAD files
│       ├── utils.rs            # Shared utilities
│       └── db/
│           ├── mod.rs          # Re-exports, open_connection(), init_db()
│           ├── schema.rs       # Schema SQL, migrations (23+)
│           ├── models.rs       # Status enum (Unplayed/InProgress/Completed/Abandoned), WadRecord, NewWad, WadUpdate builders
│           ├── connection.rs   # Connection helpers, tag helpers, batch chunking
│           ├── query.rs        # Query parser, search_wads(), find_duplicate()
│           ├── wads.rs         # WAD CRUD (add/get/update/delete), tag add/remove
│           ├── sessions.rs     # Sessions, completions, batch stats, StatsSnapshot
│           ├── iwads.rs        # IWAD registry: family/variant, priority resolution
│           ├── id24.rs         # id24 WAD registry: known hashes, identification
│           └── companions.rs   # Companion registry: junction table, orphan detection, batch query
├── caco-sources/       # API clients and import service
│   └── src/
│       ├── lib.rs              # Re-exports
│       ├── error.rs            # SourceError type
│       ├── http.rs             # Shared HTTP client (reqwest blocking)
│       ├── import_service.rs   # Centralized import for all sources + auto-enrichment
│       ├── json_import.rs      # Offline JSON import (idgames/Doomwiki saved responses)
│       ├── idgames/            # idgames API client + models
│       ├── doomwiki/           # Doom Wiki client + wikitext parser
│       └── doomworld/          # Doomworld forum client + HTML/LLM parser
├── caco-cli/           # CLI (clap)
│   └── src/
│       ├── main.rs             # Entry point, Cli struct, DB init
│       ├── output.rs           # Shared output formatting (table, plain, JSON)
│       ├── parsing.rs          # Sort extraction, modify parsing, ModifyAction
│       ├── picker.rs           # Interactive fzf-style picker
│       ├── resolve.rs          # WAD resolution helpers
│       └── commands/
│           ├── mod.rs          # Commands enum (all subcommands)
│           ├── ls.rs           # ls (--iwad, --id24, -o plain/json)
│           ├── info.rs         # info (--levelstats, --live, -o plain/json)
│           ├── modify.rs       # modify (field=value, beaten±N, --add-file, --remove-file)
│           ├── import.rs       # import (auto-detect source, --idgames/--doomwiki/--doomworld/--url/--local)
│           ├── play.rs         # play (--iwad, --record, --config, --complevel)
│           ├── trash.rs        # trash (--restore, --list, --iwad, --id24)
│           ├── random.rs       # random (--info)
│           ├── companion.rs    # companion (add/rm/enable/disable/ls)
│           ├── gc.rs           # gc (--dry-run, --keep-*, --orphans-only, --ignore/--unignore)
│           ├── enrich.rs       # enrich (--complevel, --dry-run)
│           ├── stats.rs        # stats (--period, --limit, --plain)
│           ├── cache.rs        # cache (list/clear/prune)
│           ├── saves.rs        # saves (list/backup/restore/clean/backups)
│           ├── demos.rs        # demos (list/play/clean)
│           ├── profile.rs      # profile (ls/create/edit/cp/rm/path)
│           ├── config.rs       # config (--edit)
│           └── completions.rs  # completions (fish/bash/zsh)
├── caco-tui/           # TUI (ratatui + crossterm)
│   └── src/
│       ├── main.rs, app.rs     # Entry point, main event loop
│       ├── event.rs, input.rs  # Event handling, key bindings
│       ├── message.rs          # Message enum for screen communication
│       ├── theme.rs            # Status colors, display config
│       ├── screens/            # tabbed_library, wad_detail, wad_edit, sessions,
│       │                       # confirm_delete, stats, wad_stats, cache, resources
│       └── widgets/            # wad_table, wad_info, filter_input, sort_select,
│                               # library_pane, import_pane, search_pane, form_pane, status_bar
├── caco-gui/           # GUI (egui/eframe)
│   └── src/
│       ├── main.rs, app.rs     # Entry point, CacoApp with tab state
│       ├── state.rs            # Shared app state, selection, refresh
│       ├── message.rs          # Message passing between panels
│       ├── theme.rs            # Doom-inspired dark theme, status colors
│       ├── thumbnails.rs       # TITLEPIC extraction + caching
│       ├── wiki_scraper.rs     # Doom Wiki thumbnail scraper
│       ├── workers.rs          # Background threads for search/import/play
│       ├── panels/
│       │   ├── library.rs      # Main library view (table + detail + filter)
│       │   ├── detail.rs       # Right sidebar: metadata, stats, actions
│       │   ├── wad_table.rs    # WAD list with sorting
│       │   ├── wad_grid.rs     # Grid view with WAD cards
│       │   ├── filter_bar.rs   # Search input with debounce
│       │   └── sort_controls.rs # Sort dropdown + direction
│       ├── dialogs/
│       │   ├── edit.rs         # WAD metadata editing
│       │   ├── delete.rs       # Delete confirmation
│       │   ├── sessions.rs     # Session history
│       │   ├── stats.rs        # Library statistics
│       │   ├── cache.rs        # Cache management
│       │   └── resources.rs    # IWAD/id24 management
│       └── import/
│           ├── mod.rs          # Import tab with source selector
│           ├── search_panel.rs # idgames/Doomwiki search
│           ├── form_panel.rs   # URL/local/Doomworld forms
│           ├── state.rs        # Import state management
│           └── workers.rs      # Async import operations
```

## Rust Dependencies

```toml
# Core
rusqlite = "0.34"       # SQLite (bundled)
serde / serde_json      # Serialization
toml = "0.8"            # Config parsing
chrono = "0.4"          # Date/time
thiserror = "2"         # Error types
dirs = "6"              # XDG paths
regex = "1"             # Pattern matching
md-5 = "0.10"           # MD5 hashing (companion dedup)
zip = "2"               # ZIP archive handling
image = "0.25"          # Image processing (thumbnails)

# CLI
clap = "4"              # Argument parsing (derive)
comfy-table = "7"       # Table output
indicatif = "0.17"      # Progress bars

# HTTP
reqwest = "0.12"        # HTTP client (blocking)

# TUI
ratatui = "0.29"        # Terminal UI
crossterm = "0.28"      # Terminal backend

# GUI
eframe = "0.31"         # egui framework
egui = "0.31"           # Immediate-mode GUI
```

## Rust Key Patterns

- **Builder pattern for DB writes**: `NewWad::builder()` and `WadUpdate::builder()` for type-safe WAD creation/updates
- **Batch stats**: `get_total_playtime_batch()`, `get_last_played_batch()`, etc. — same N+1 avoidance as Python
- **Query parser**: Same beets-style syntax as Python — field queries, OR, negation, status shortcuts, globs
- **Companion system**: `companion_files_registry` + `wad_companions` junction table; `companion_service.rs` handles MD5 dedup + managed storage at `~/.local/share/caco/companions/{md5[:12]}_{filename}`
- **GC**: `gc.rs` handles completed/abandoned cleanup with interactive y/n/i prompts; `gc_ignore` column for exclusion; orphan detection for data dirs, backups, and companions
- **Import service**: `import_service.rs` centralizes duplicate checking for all sources; auto-enriches with Doom Wiki metadata; JSON import fallback for Cloudflare-blocked APIs
- **Player**: `player.rs` wraps sourceport execution; injects companion files, data dir args, complevel args, config profile; returns `PlayResult` with crash detection
- **Detection**: `iwad_detect.rs` (PNAMES + map lumps), `complevel_detect.rs` (COMPLVL > UMAPINFO > DEHACKED > map lumps); both handle ZIP-wrapped WADs
- **GUI**: egui immediate-mode rendering; `CacoApp` holds all state; background workers for search/import/play via `std::thread` + `std::sync::mpsc`

## Python Architecture

```
src/caco/
├── cli/            # Click-based CLI
│   ├── __init__.py, parsing.py, library.py, import_cmds.py, play_cmd.py
│   ├── cache.py, config_cmd.py, stats.py, saves_cmd.py, demos_cmd.py
│   ├── profile_cmd.py, companion_cmd.py, gc_cmd.py, complete.py
│   └── _completion_scripts.py
├── db/             # SQLite (sqlite3.Row)
│   ├── _models.py, _connection.py, _schema.py, _query.py
│   ├── _wads.py, _sessions.py, _iwads.py, _id24.py, _companions.py
├── services/       # import_service.py, resource_service.py, companion_service.py
├── sources/        # base.py, idgames.py, doomwiki.py, doomworld.py
├── tui/            # Textual-based TUI
├── gui/            # PySide6/Qt6 GUI
├── watchers/       # Stats watchers (helion.py, uzdoom.py) — Python-only feature
├── config.py, player.py, sourceports.py
├── complevel.py, complevel_detect.py, iwad_detect.py
├── saves.py, demos.py, wad_stats.py, utils.py, stats_watcher.py
└── tests/          # pytest (~200 tests)
```

## Data Locations

- Database: `~/.local/share/caco/library.db`
- Config: `~/.config/caco/config.toml`
- Managed IWADs: `~/.local/share/caco/iwads/{variant}/{family}.wad`
- Managed id24 WADs: `~/.local/share/caco/id24/{name}.wad`
- WAD cache: `~/.local/share/caco/wads/`
- WAD data: `~/.local/share/caco/data/` (per-WAD saves, stats, configs)
- Companion files: `~/.local/share/caco/companions/{md5[:12]}_{filename}`
- Sourceport configs: `~/.local/share/caco/sourceports/{exe}/{profile}.cfg`
- Backups: `~/.local/share/caco/backups/`
- Thumbnails cache: `~/.cache/caco/thumbnails/`

## Feature Parity Status

| Feature | Python | Rust CLI | Rust GUI |
|---------|--------|----------|----------|
| Core library (ls, info, modify, play) | Yes | Yes | Yes |
| Import (5 sources + auto-enrich) | Yes | Yes | Yes |
| Companion files | Yes | Yes | No |
| GC command | Yes | Yes | N/A (CLI-only) |
| Enrich command | Yes | Yes | N/A (CLI-only) |
| JSON import fallback | Yes | Yes | No |
| Stats watchers (Helion/UZDoom) | Yes | No | No |
| WAD Stats dialog | Yes | — | No |
| WAD Unavailable dialog | Yes | — | No |
| Edit: source info + companions tabs | Yes | — | No |
| Clickable source links | Yes | — | No |
| Map progress in detail panel | Yes | — | No |
| Context menus | Yes | — | No |
| Keyboard shortcuts (vim, Ctrl+F) | Yes | — | Partial |
| Thumbnail display in detail panel | Yes | — | Partial |

## Shared Behavior (Both Implementations)

**Query syntax** (beets-style, used by ls/play/modify/trash/etc.):
- Fields: `id:`, `title:`, `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`, `iwad:`, `complevel:`, `config:`
- OR: `"status:in-progress , status:unplayed"` (comma with spaces)
- Negation: `^status:completed`
- Status shortcuts: `u` (unplayed), `p`/`ip` (in-progress), `c`/`f`/`done` (completed), `a`/`d` (abandoned)
- Glob patterns: `tag:caco*`
- Free text searches title, author, description

**Status enum**: `unplayed`, `in-progress`, `completed`, `abandoned`

**Companion files**: `companion_files_registry` (id, md5 UNIQUE, filename, path, size) + `wad_companions` junction (wad_id, companion_id, enabled, load_order); DEH/BEX auto-detected; `-deh` for non-zdoom, `-file` for zdoom; orphan policy via `companion_orphan_cleanup` config

**IWAD detection**: PNAMES lump analysis (TNT-only 197 patches / Plutonia-only 78 patches), map lump fallback (ExMy→doom, MAPxx→doom2); self-contained WADs don't trigger detection

**Complevel detection hierarchy**: COMPLVL lump (id24 byte or text) > UMAPINFO→21 > DEHACKED+MBF→11 > DEHACKED+ExMy→2 > DEHACKED+MAPxx→4 > ExMy→2 > MAPxx→4

**Sourceport families**: dsda/zdoom/chocolate/woof/eternity/helion/uzdoom; each maps to data dir args, save dir args, complevel args, config args

**Per-WAD config columns**: `custom_iwad`, `custom_sourceport`, `custom_args` (JSON), `complevel` (INT), `custom_config` (TEXT)

**DB migrations**: Run on `init_db()`; numbered sequentially; both implementations share the same schema

## CLI Commands Reference

```
caco ls [query] [--iwad|--id24] [-o plain|json]
caco info <query> [--levelstats] [-o plain|json]
caco modify <query> [field=value...] [beaten±N] [--add-file|--remove-file]
caco import <source> [--idgames|--doomwiki|--doomworld|--url|--local]
caco play <query> [-p PORT] [-c COMPLEVEL] [-C CONFIG] [--iwad] [--record]
caco trash <query> [--restore|--list] [--iwad FAMILY|--id24 NAME]
caco random [query] [--info]
caco companion add|rm|enable|disable|ls
caco gc [--dry-run] [-y] [--keep-saves|--keep-demos|--keep-data|--keep-cache|--keep-companions] [--orphans-only] [--ignore|--unignore]
caco enrich [query] [--complevel] [--dry-run]
caco stats [--period month|year] [--limit N] [--plain]
caco sessions <query> [--plain]
caco cache list|clear|prune
caco saves list|backup|restore|clean|backups
caco demos list|play|clean
caco profile ls|create|edit|cp|rm|path
caco config [--edit]
caco completions [fish|bash|zsh]
```

## Shell Completions

- Hand-crafted scripts for fish, bash, zsh in `completions/`
- Rust CLI: `caco completions [shell]` generates scripts; clap_complete for static completions
- Dynamic data via hidden `caco _complete <context>` (wads, tags, iwads, statuses, sort-fields, sourceports, modify-fields, query-fields)

## Git Instructions

- Commit working changes to git
- Update README.md, CLAUDE.md to document changes and features
- Rust quality gates: `cargo check --workspace && cargo clippy --workspace -- -D warnings && cargo test --workspace`
