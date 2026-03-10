# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Caco is a personal Doom WAD library manager inspired by `beets`. It tracks WADs you want to play, have played, or are playing, with metadata from multiple sources (idgames, Doomwiki, Doomworld forums, manual entry). Key features:

- SQLite database for WAD metadata and play history
- Import from idgames archive, Doom Wiki, Doomworld forums, URLs, or local files
- Automatic playtime tracking via sourceport wrapper
- Tag-based organization
- On-demand downloading (WADs are cached, not stored permanently)
- LLM-powered metadata extraction (optional, for Doomworld imports)
- Completion tracking (times beaten per WAD) with per-map stats import/export and auto-tracking
- Soft-delete with trash/restore lifecycle
- IWAD registry with family/variant model, MD5-based identification, priority resolution, and auto-scan
- Managed IWAD storage: `iwads/{variant}/{family}.wad` (canonical filenames for sourceport compatibility)
- Auto-detect required IWAD from WAD file contents (PNAMES analysis + map lump names)
- Play IWADs directly (`caco play --iwad doom2`) without a PWAD in the library
- Auto-detect installed sourceports with helpful error messages
- Sourceport crash detection (exit code tracking, warnings, session history indicators)
- Sourceport config profile management (managed `.cfg` files, per-WAD overrides, `caco profile` command group)
- Auto-update config file with missing keys on load
- id24 WAD registry: managed storage, MD5-based identification, auto-detect on import

## Commands

```bash
# Sync dependencies (uses uv)
uv sync
uv sync --extra test  # include test dependencies

# Run CLI
uv run caco <command>

# Example: list library
uv run caco ls

# Example: test with plain output
uv run caco ls -o plain

# Example: JSON output (for scripting)
uv run caco ls -o json
uv run caco info 1 -o json

# Run tests
uv run pytest tests/ -v
```

## Architecture

```
src/caco/
├── cli/            # Click-based CLI (split into submodules)
│   ├── __init__.py     # cli group, shared helpers, command aliases, JSON/plain renderers
│   ├── parsing.py      # Argument classification: sort extraction, modify parsing, ModifyAction
│   ├── library.py      # ls, info, modify, trash, random, enrich
│   ├── import_cmds.py  # unified import command with source flags
│   ├── play_cmd.py     # play command (--first, --iwad)
│   ├── cache.py        # cache list/clear/prune
│   ├── config_cmd.py   # config, completions commands
│   ├── stats.py        # library stats command + per-map stats helpers
│   ├── saves_cmd.py    # saves list/backup/restore/clean/backups
│   ├── demos_cmd.py   # demos list/play/clean
│   ├── complete.py     # hidden _complete command for shell completions
│   ├── profile_cmd.py  # profile command group (ls, create, edit, cp, rm, path)
│   ├── companion_cmd.py # companion command group (add, rm, enable, disable, ls)
│   ├── gc_cmd.py        # garbage collection command (finished/abandoned WADs, orphans)
│   └── _completion_scripts.py  # Embedded fish/bash/zsh completion scripts
├── complevel.py    # Shared complevel names, aliases, parse_complevel()
├── complevel_detect.py # Auto-detect complevel from WAD lumps (COMPLVL, UMAPINFO, DEHACKED)
├── iwad_detect.py  # Auto-detect IWAD family from WAD file PNAMES/map lumps
├── saves.py        # Save game management: find, backup, restore, clean save files
├── demos.py        # Demo file management: find, clean, generate names
├── sourceports.py  # Sourceport family registry (exe→CLI flags for data/save/complevel/config)
├── stats_watcher.py # Stats watcher framework: ABC, registry, thread runner for active stats collection
├── utils.py        # Shared utilities (coerce_str, BaseHttpClient, CacoSourceError, extract_year, parse_wad_directory)
├── wad_stats.py    # Per-map stats parser/formatter (stats.txt + levelstat.txt)
├── watchers/       # Sourceport-specific stats watcher implementations
│   ├── __init__.py
│   ├── helion.py   # Helion watcher: polls levelstat.txt, enriches skill from .hsg saves
│   └── uzdoom.py   # UZDoom watcher: injects ZScript pk3 mod, parses console log for stats
├── db/             # SQLite database package
│   ├── __init__.py     # Re-exports all public symbols (backward compat)
│   ├── _models.py      # Enums (Status, SourceType), WadRecord TypedDict, constants
│   ├── _connection.py  # get_connection(), tag helpers, batch query chunking
│   ├── _schema.py      # Schema SQL, migrations, init_db()
│   ├── _query.py       # Query parser, search_wads(), find_duplicate()
│   ├── _wads.py        # WAD CRUD (add/get/update/delete), tag add/remove
│   ├── _sessions.py    # Sessions, completions, batch stats, cache, StatsSnapshot
│   ├── _iwads.py       # IWAD registry: family/variant model, priority resolution, CRUD
│   ├── _id24.py        # id24 WAD registry: known hashes, identification, CRUD
│   └── _companions.py  # Companion file registry: managed storage, MD5 dedup, junction table CRUD
├── config.py       # TOML config in ~/.config/caco/; IWAD_DIR, get_iwad_dir(), SOURCEPORT_DIR, get_sourceport_dir(), get_profile_path(), list_profiles()
├── player.py       # Sourceport launcher + playtime tracking
├── idgames/        # idgames API client
│   ├── client.py   # HTTP client (inherits BaseHttpClient)
│   └── models.py   # Pydantic models (FileEntry, etc.)
├── doomwiki/       # Doom Wiki API client
│   ├── client.py   # HTTP client (inherits BaseHttpClient)
│   ├── models.py   # Pydantic models (WikiEntry, SearchResult)
│   └── parser.py   # Wikitext parser for {{Wad}} infobox template
├── doomworld/      # Doomworld forum client
│   ├── client.py   # HTTP client (inherits BaseHttpClient)
│   ├── models.py   # Pydantic models (ForumThread)
│   ├── parser.py   # HTML/JSON-LD parser + regex extraction
│   └── llm.py      # LLM backends for smart metadata extraction
├── tui/            # Textual-based TUI (caco --tui)
│   ├── app.py      # Main Textual App class
│   ├── theme.py    # Centralized status colors/display config
│   ├── styles.tcss # Textual CSS styles
│   ├── screens/    # Screen classes
│   │   ├── tabbed_library.py  # Main tabbed interface (entry point)
│   │   ├── wad_detail.py  # WAD detail view
│   │   ├── wad_edit.py    # WAD metadata edit form
│   │   ├── sessions.py    # Session history
│   │   ├── confirm_delete.py # Delete confirmation modal
│   │   ├── stats.py       # Library statistics screen
│   │   ├── wad_stats.py   # Per-map stats screen (stats.txt/levelstat.txt)
│   │   ├── cache.py       # Cache management screen
│   │   └── resources.py   # Combined IWAD/id24 management screen (tabbed)
│   └── widgets/    # Widget classes
│       ├── base_search_pane.py # Abstract base for search panes
│       ├── wad_table.py   # DataTable for WAD list (with vim bindings, batch stats)
│       ├── wad_info.py    # Info panel widget (accepts pre-fetched stats + wad dict)
│       ├── filter_input.py # Search/filter input
│       ├── sort_select.py  # Sort dropdown widget (ID, Title, Author, Playtime, Last Played, Year, Rating)
│       ├── library_pane.py # Reusable library view (table + panel + filter + delete/beaten/trash/stats/cache)
│       ├── import_pane.py  # Import container with source selector
│       ├── idgames_pane.py # idgames search (extends BaseSearchPane)
│       ├── doomwiki_pane.py # Doom Wiki search (extends BaseSearchPane)
│       ├── doomworld_pane.py # Doomworld forum URL import
│       ├── url_pane.py     # Manual URL import form
│       └── local_pane.py   # Local file import form
├── gui/            # PySide6-based GUI (caco --gui)
│   ├── __init__.py      # CacoGuiApp entry point
│   ├── app.py           # QApplication setup, dark palette, stylesheet
│   ├── main_window.py   # QMainWindow: tab bar, toolbar, status bar, geometry save/restore
│   ├── theme.py         # Doom palette colors, QSS stylesheet, status color mappings
│   ├── constants.py     # Column definitions, sort fields, status tabs
│   ├── models/
│   │   └── wad_model.py     # QAbstractTableModel wrapping db.search_wads() + batch stats
│   ├── views/
│   │   ├── list_view.py     # QTableView with context menu, vim keys
│   │   ├── grid_view.py     # QListView (IconMode) with WadCardDelegate for cards
│   │   ├── detail_panel.py  # Right sidebar: thumbnail, metadata, stats, action buttons
│   │   ├── filter_bar.py    # QLineEdit with 300ms debounce
│   │   └── sort_controls.py # QComboBox + asc/desc toggle
│   ├── tabs/
│   │   ├── library_tab.py   # Composite: filter + sort + list/grid + detail panel
│   │   └── import_tab.py    # QTabWidget with 5 source panes
│   ├── import_panes/
│   │   ├── idgames_pane.py  # idgames search + import
│   │   ├── doomwiki_pane.py # Doom Wiki search + import
│   │   ├── doomworld_pane.py # Doomworld forum URL import
│   │   ├── url_pane.py      # Manual URL form
│   │   └── local_pane.py    # File picker + form
│   ├── dialogs/
│   │   ├── edit_dialog.py    # WAD metadata editing form
│   │   ├── delete_dialog.py  # Confirmation dialog with WAD stats
│   │   ├── link_dialog.py    # WadUnavailableDialog: open source page, link local file
│   │   ├── sessions_dialog.py # Session history table
│   │   ├── stats_dialog.py   # Library statistics overview
│   │   ├── wad_stats_dialog.py # Per-map stats table with import/export
│   │   ├── cache_dialog.py     # Cache management
│   │   └── resources_dialog.py # Combined IWAD/id24 management (tabbed)
│   ├── workers/
│   │   ├── play_worker.py      # QThread for sourceport launch
│   │   ├── search_worker.py    # QRunnable for API searches
│   │   ├── import_worker.py    # QRunnable for import operations
│   │   └── thumbnail_worker.py # Re-export of ThumbnailLoader
│   └── thumbnails/
│       ├── extractor.py  # TITLEPIC extraction from WAD files + Doom patch decoder
│       ├── scraper.py    # Doom Wiki image scraping via MediaWiki API
│       ├── cache.py      # Thumbnail filesystem cache (~/.cache/caco/thumbnails/)
│       └── loader.py     # Async QThreadPool-based thumbnail loader
├── services/
│   ├── __init__.py
│   ├── import_service.py  # Centralized duplicate-check-and-import for all 5 source types
│   ├── resource_service.py  # Shared IWAD/id24 registration (identify + copy + DB insert)
│   └── companion_service.py  # Companion file register/unregister with MD5 dedup + managed storage
├── sources/
│   ├── base.py     # BaseSource mixin (shared context-manager lifecycle)
│   ├── idgames.py  # idgames archive adapter (extends BaseSource)
│   ├── doomwiki.py # Doom Wiki adapter (extends BaseSource)
│   └── doomworld.py # Doomworld forum adapter (extends BaseSource)
└── tests/          # pytest test suite
    ├── conftest.py     # Shared fixtures (in-memory DB, make_wad factory, tmp_config, populated_db)
    └── unit/           # Unit tests (utils, query parser, db, sessions, config, parsers, CLI, models, player, iwad_detect, complevel, complevel_detect, sourceports)
```

**Data locations:**
- Database: `~/.local/share/caco/library.db` (configurable via `db_path`)
- Config: `~/.config/caco/config.toml`
- Managed IWADs: `~/.local/share/caco/iwads/{variant}/{family}.wad`
- Managed id24 WADs: `~/.local/share/caco/id24/{name}.wad`
- WAD cache: `~/.local/share/caco/wads/`
- WAD data: `~/.local/share/caco/data/` (per-WAD saves, stats, configs; configurable via `data_dir`)
- Companion files: `~/.local/share/caco/companions/{md5[:12]}_{filename}` (managed, deduplicated)
- Sourceport configs: `~/.local/share/caco/sourceports/{exe}/{profile}.cfg` (configurable via `sourceport_dir`)

**Key patterns:**
- `db/` package uses raw sqlite3 with `sqlite3.Row` for dict-like access; tag helpers (`_fetch_tags`, `_attach_tags`, `_fetch_tags_batch`) and batch query functions (`get_total_playtime_batch`, `get_last_played_batch`, `get_times_beaten_batch`, `get_session_count_batch`) reduce N+1 queries; `__init__.py` re-exports everything so `from caco import db` and `from caco.db import Status` both work
- TUI widgets use batch stats: `WadTable.load_wads()` batch-fetches all stats; `get_wad_stats()` and `get_wad_by_id()` expose cached data to `WadInfoPanel`; `update_row()` handles incremental cell updates
- Status colors/display centralized in `tui/theme.py` (`STATUS_CONFIG` dict with `get_status_display/color/css_class` helpers)
- Source adapters inherit `BaseSource` from `sources/base.py` for shared context-manager lifecycle; clients inherit `BaseHttpClient` from `utils.py`; errors inherit `CacoSourceError`
- CLI uses Click's decorator registration pattern: each `cli/*.py` submodule imports `cli` from `caco.cli` and registers commands; `__init__.py` imports all submodules at bottom to trigger registration
- `player.py` wraps sourceport execution to track session start/end times; returns `PlayResult(duration, exit_code)` with `.crashed` property; decoupled from Rich — uses `ProgressCallback` for download progress; CLI creates Rich progress wrapper in `play_cmd.py`
- `ImportService` in `services/import_service.py` centralizes duplicate-check-and-import for all 5 source types; used by CLI, TUI, and GUI; auto-enriches non-Doomwiki imports with Doom Wiki metadata (`_auto_enrich_doomwiki`) — fills missing author/year, appends wiki description, auto-links IWAD; config-gated via `auto_doomwiki_enrich` (default: true); failures silently logged
- `WadInfoPanel` and `DetailPanel` accept optional pre-fetched `wad` dict to avoid DB re-fetch on selection
- Status enum: `to-play`, `backlog`, `playing`, `finished`, `abandoned`, `awaiting-update`
- Import command uses flag-based source selection: `caco import <source> [--idgames|--doomwiki|--doomworld|--local|--url URL]`
- Query syntax (beets-style):
  - Fields: `id:`, `title:`, `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`, `iwad:`, `complevel:`, `config:`
  - OR queries: `"status:playing , status:to-play"` (comma with spaces — spaces required!)
  - Negation: `^status:finished` (prefer `^` prefix, `-` also works but may conflict with CLI flags)
  - Status shortcuts: `status:p` (playing), `status:f` (finished), etc.
  - Glob patterns: `tag:caco*` (matches cacoward, etc.)
  - Free text searches title, author, and description
  - Multiple terms are joined with implicit AND
- Per-WAD config: `custom_iwad`, `custom_sourceport`, `custom_args` (JSON array), `complevel` (INTEGER), `custom_config` (TEXT) columns in wads table; companion files managed via junction table (see below)
- Companion files: `companion_files_registry` table (id, md5 UNIQUE, filename, path, size) + `wad_companions` junction table (wad_id, companion_id, enabled, load_order); managed storage at `~/.local/share/caco/companions/` with MD5-based deduplication; filename format: `{md5[:12]}_{original_filename}`; DEH/BEX files auto-detected by extension and loaded with `-deh` (non-zdoom) or `-file` (zdoom); managed via `caco companion add/rm/enable/disable/ls` or `caco modify --add-file`/`--remove-file`; `uses_deh_flag()` in `sourceports.py` determines correct flag per family; `companion_orphan_cleanup` config controls behavior when last WAD unlinks a companion ("delete", "keep", "ask"); `db/_companions.py` has CRUD functions; `services/companion_service.py` handles register/unregister with dedup + managed storage; old `companion_files` JSON column migrated to new tables in migration #23
- Crash detection: `exit_code` INTEGER column on sessions table; `PlayResult` dataclass in `player.py` with `crashed` property (non-zero exit code); CLI/TUI/GUI warn on crash; session history views show "Crash (N)" indicator
- Auto stats tracking: `stats_snapshot` TEXT column on `wads` table stores live per-map stats JSON; auto-read from data dir after play sessions; auto-archived to completion on `modify beaten+N` or `modify status=finished`; `auto_stats` config (default: true); live stats shown as "Current (live)" entry in Map Stats dialog (GUI) and screen (TUI)
- Per-session map tracking: `stats_before`/`stats_after` TEXT columns on `sessions` table store stats snapshots captured before/after each play session; `compute_stats_delta()` in `wad_stats.py` diffs these to determine which maps were played; `_read_stats_snapshot()` in `player.py` reads stats file to JSON; `caco sessions` command displays session history with maps played per session
- IWAD resolution: `iwad_dirs` config allows short names (e.g., `doom2` instead of full path); `resolve_iwad()` in `config.py` checks DB registry (with priority resolution) then searches dirs for exact name or name + `.wad`; `IWAD_DIR` / `get_iwad_dir()` provides the managed IWAD directory path (`~/.local/share/caco/iwads/`)
- Cross-source downloading: `idgames_id` column allows any WAD to download via idgames API (set with `caco modify id:N idgames-id=XXXXX`)
- Soft-delete: `deleted_at` column; `caco trash` moves to trash, `caco trash --restore` recovers, `caco trash --list` shows trash
- `modify --link PATH`: copies/moves a local file to cache and updates `cached_path`/`filename` for metadata-only entries (e.g., Doomwiki imports); behavior controlled by `link_mode` config ("move" or "copy")
- `version` column tracks WAD version strings for non-idgames releases
- Database migrations run on `init_db()`: add columns, create tables, rename statuses
- IWAD registry: `iwads` table with family/variant model; `KNOWN_IWADS` (MD5→(family, variant, title)), `KNOWN_IWAD_FILENAMES` (filename→(family, variant, title)), `IWAD_ALIASES` (free text→family), `DEFAULT_IWAD_PRIORITY` (family→variant order), `FAMILY_FALLBACKS` (family→fallback families) in `db/_iwads.py`; `get_iwad(family)` does priority resolution; `managed_iwad_filename()` returns `{variant}/{family}.wad` path for managed IWADs (canonical filenames for sourceport compatibility); `remove_iwad_with_paths()` returns removed paths for managed file cleanup; `resolve_iwad()` checks DB registry before `iwad_dirs`; Doom Wiki imports auto-link to registered IWADs via `ImportService._auto_link_iwad()`
- IWAD priority: `get_iwad_priority(family)` checks config `[iwad_priority]` section first, then `DEFAULT_IWAD_PRIORITY`; freedoom is cross-family fallback via `FAMILY_FALLBACKS`
- Sourceport families: `sourceports.py` maps executable basenames to CLI flags; `SOURCEPORT_FAMILIES` dict with dsda/zdoom/chocolate/woof/eternity/helion/uzdoom families; `identify_sourceport_family()` strips path and matches basename; `get_family_name()` exposes family name string (e.g., "helion"); `get_data_dir_args()` returns `-data`/`-save` or `-savedir` args; for dsda family, `-save` points to nested stats dir (`{exe}_data/{iwad}/{wad_stem}/`) via `get_dsda_save_dir()` when `iwad` and `wad_path` are provided; `get_config_args()` returns `-config <path>` for dsda-family ports; Helion uses `+complevel` with string names (vanilla/boom/mbf/mbf21) instead of ints — `get_complevel_args()` converts via `complevel_to_helion_name()`, returns `[]` for unsupported complevels
- Stats watcher framework: `stats_watcher.py` provides `StatsWatcher` ABC (start/stop/collect) + registry (`register_watcher`/`get_watcher`) + `run_watcher_thread()` daemon thread runner; `get_watcher()` returns `None` for families without a registered watcher (existing passive stats reading unchanged); lazy registration via `_ensure_watchers_registered()` imports `caco.watchers.helion` on first call; `player.py` starts watcher after `Popen`, stops after `proc.wait()`, writes collected stats to `{wad_data_dir}/levelstat.txt` before `_auto_track_stats()` picks it up
- Helion watcher: `watchers/helion.py` implements `HelionWatcher(StatsWatcher)` — polls `~/.config/Helion/levelstat.txt` mtime every 1s, parses changes via `wad_stats.parse_stats_text()`, accumulates with `merge_stats()`; on `stop()`, enriches `best_skill` from most recent `.hsg` save file (ZIP containing `world.json` with 0-indexed `Skill` field, +1 for 1-indexed `best_skill`); `_get_helion_config_dir()` respects `$XDG_CONFIG_HOME` on Linux, uses `~/Saved Games/Helion/` on Windows
- UZDoom watcher: `watchers/uzdoom.py` implements `UZDoomWatcher(StatsWatcher)` — injects a ZScript `.pk3` mod (`CacoStatsReporter` EventHandler) that outputs `CACOSTATS|` prefixed lines via `Console.Printf`; uses `+logfile` to redirect console output to a log file in wad_data_dir; polls log file for `CACOSTATS|{map}|{skill}|{maptime}|{kills}/{total}|{items}/{total}|{secrets}/{total}` lines; skill is 0-indexed gameskill CVar (converted to 1-indexed); `.pk3` auto-created at `~/.local/share/caco/mods/caco_stats_reporter.pk3`; `extra_args()` returns `["-file", pk3_path, "+logfile", log_path]`
- Config profiles: `SOURCEPORT_DIR = DB_DIR / "sourceports"`; `get_sourceport_dir()` / `get_profile_path(sourceport, profile)` / `list_profiles()` in `config.py`; auto-created `default.cfg` on first play for dsda-family ports; resolution: CLI `--config` > WAD `custom_config` > `"default"`; `caco profile` command group (ls, create, edit, cp, rm, path); `config:` query field searches `custom_config` column
- Per-WAD data dirs: `player.py` injects data dir args when `manage_data_dirs=True` (default); `get_wad_data_dir(id, title)` returns `{data_dir}/{id}_{sanitized_title}/`; `find_wad_data_dir(id)` finds existing dir by ID prefix (handles title renames); `_sanitize_dirname()` lowercases, replaces non-alnum with hyphens, truncates to 64 chars
- IWAD auto-detection: `iwad_detect.py` inspects PWAD file PNAMES lump for TNT-only (197) / Plutonia-only (78) patches, then falls back to map lump names (ExMy→doom, MAPxx→doom2); self-contained WADs (patches provided as lumps) don't trigger detection; result persisted to `custom_iwad` on first play; `auto_detect_iwad` config (default: true); `parse_wad_directory()` shared between `iwad_detect.py` and `gui/thumbnails/extractor.py` via `utils.py`
- Complevel: single `complevel INTEGER` column; `complevel.py` has shared names/aliases/parser + `HELION_COMPLEVEL_NAMES`/`complevel_to_helion_name()` for Helion's string complevels; `complevel_detect.py` auto-detects from WAD lumps — self-contained detection hierarchy: COMPLVL lump (id24 single byte or text string like "mbf21"), UMAPINFO→21, DEHACKED with MBF codepointers→11, ExMy-only→2; `sourceports.py` `get_complevel_args()` injects `-complevel N` for dsda and woof families, `+complevel NAME` for helion; `player.py` auto-detects on first play via single `detect_complevel()` call and stores to `complevel` column; CLI `play --complevel`/`-c` overrides; `complevel:` query field supports ints and aliases (vanilla/boom/mbf/mbf21); `modify complevel=boom` sets; `auto_detect_complevel` config (default: true); `_load_wad_data()` shared helper handles ZIP-wrapped WADs; Doomworld imports store extracted complevel; Doomwiki imports auto-link from `port` field; `caco enrich --complevel` backfills existing WADs
- id24 resource auto-loading: `_get_id24_resource_args()` in `player.py` checks for COMPLVL lump directly (via `detect_complvl()` — matches only 1-byte id24 lumps, not text-based) and injects `id24res.wad` for id24 WADs; also loads id1-specific resources (id1-res, id1-tex, id1-weap, id1-mus) when playing id1.wad
- Direct IWAD play: `caco play --iwad doom2` launches an IWAD directly via `play_iwad()` in `player.py`; no session tracking, no WAD record — just a clean sourceport launch; supports `-p`/`--sourceport` and extra args
- Sourceport detection: `detect_sourceports()` in `sourceports.py` uses `shutil.which()` to find installed sourceports from `SOURCEPORT_FAMILIES`; play command error message lists detected ports when no sourceport is configured
- Config auto-update: `ensure_config_keys()` in `config.py` runs on `load_config()` — compares existing config file against `DEFAULT_CONFIG` and section defaults (`[tui]`, `[gui]`, `[list]`); adds missing keys with default values; only runs if config file exists; only writes if changes are made; recursion-guarded
- Save game management: `saves.py` provides core logic (no CLI dependency) — `find_save_files()` recursively scans data dirs for `.dsg`/`.zds` files; `create_backup()`/`restore_backup()` zip/unzip entire data dirs; `SAVE_EXTENSIONS` and `ALL_SAVE_EXTENSIONS` in `sourceports.py` define known extensions; backups stored at `~/.local/share/caco/backups/` (`BACKUP_DIR` / `get_backup_dir()` in `config.py`)

**IWAD CLI commands:**
- `caco ls --iwad [-o plain]` — list registered IWADs (family, variant, title, path); preferred variant marked with `*`
- `caco import <path>` — auto-detects IWAD via MD5 and registers; copies to managed dir
- `caco trash --iwad FAMILY[/VARIANT]` — without variant removes all variants (with warning); with variant removes one; also deletes managed file if inside iwad_dir

**id24 CLI commands:**
- `caco ls --id24 [-o plain|-o json]` — list registered id24 WADs (name, version, title, path)
- `caco import <path>` — auto-detects id24 via MD5 (or filename fallback) and registers; copies to managed dir
- `caco trash --id24 NAME` — remove id24 WAD from DB and managed storage
- id24 registry: `id24_wads` table with name/version/title/path/md5; `KNOWN_ID24_WADS` (MD5→(name, version, title)), `KNOWN_ID24_FILENAMES` (filename fallback); `identify_id24()` for detection; managed storage at `~/.local/share/caco/id24/{name}.wad`

**Companion command group:**
- `caco companion add <query> <file>` — register companion file (MD5 dedup, copy to managed dir) and link to WAD
- `caco companion rm <query> <filename> [-y]` — unlink companion from WAD; orphan policy per config
- `caco companion enable <query> <filename>` — enable a companion file for a WAD
- `caco companion disable <query> <filename>` — disable without removing
- `caco companion ls [query] [--plain]` — list companions per-WAD or all; shows enabled/disabled status
- Also available: `caco modify --add-file PATH` / `--remove-file FILENAME` for quick add/remove

**GC (garbage collection) command:**
- `caco gc` — scan and clean finished/abandoned WAD data with prompts
- `caco gc --dry-run` — preview what would be cleaned without deleting
- `caco gc --keep-saves` — clean but preserve save files in data dirs
- `caco gc --keep-demos` — clean but preserve demo files in data dirs
- `caco gc --keep-data` — skip data dir cleanup (only clean cache files)
- `caco gc --keep-cache` — skip cache file cleanup (only clean data dirs)
- `caco gc --orphans-only` — only clean orphaned data dirs and backups
- `caco gc --ignore <query>` — permanently mark WAD(s) as GC-ignored
- `caco gc --unignore <query>` — remove GC-ignore from WAD(s)
- `caco gc -y` — skip confirmation prompts
- Finished/abandoned WADs from idgames auto-clean (re-downloadable), non-idgames prompt individually with y/n/i (ignore)
- Orphaned data dirs (WAD purged but dir remains) and orphaned backups are cleaned
- `gc_ignore INTEGER DEFAULT 0` column on wads table; `gc_ignore:` not a query field (use `--ignore`/`--unignore`)

**Status shortcuts (complete list):**
| Shortcut | Status |
|----------|--------|
| `t`, `tp`, `toplay` | to-play |
| `b`, `back` | backlog |
| `p`, `play` | playing |
| `f`, `fin`, `done` | finished |
| `a`, `drop`, `dropped` | abandoned |
| `w`, `au`, `await`, `waiting`, `wip` | awaiting-update |

**Enrich command:**
- `caco enrich [query]` — re-run enrichment for existing WADs (file-based detection + Doom Wiki lookup)
- `caco enrich --complevel` — only target WADs with NULL complevel
- `caco enrich --dry-run` — preview what would change without applying
- `caco enrich status:playing` — enrich only matching WADs

**Sessions command:**
- `caco sessions <query> [--plain] [--yes]` — show play session history with per-session map tracking; displays date, time, duration, sourceport, and maps played per session; sessions without stats data show `--` in maps column

**Completion tracking (via modify):**
- `caco modify <query> beaten+N` — add N completions (implicit 1 if bare `beaten+`)
- `caco modify <query> beaten+1 --notes "text" --date ISO --stats-file FILE` — add with metadata
- `caco modify <query> beaten-N` — remove N most recent completions
- `caco modify <query> beaten-TIMESTAMP` — remove completion by exact timestamp
- `caco modify <query> beaten=N` — set exact completion count
- `caco modify <query> -s FILE` — attach stats to most recent completion (standalone)
- `caco modify <query> -s FILE -b TIMESTAMP` — attach stats to specific completion
- Beaten actions coexist with field=value actions (e.g., `beaten+1 status=finished`)
- Auto-completion on `status=finished` is suppressed when beaten actions are present (`record_completion=False`)

**Per-map stats (via info):**
- `caco info <query> --levelstats` — show all stats entries (live + completions)
- `caco info <query> --levelstats --live` — show only live stats
- `caco info <query> --levelstats -b TIMESTAMP` — show specific completion's stats
- `caco info <query> --levelstats --plain` — TSV output

**Profile command group:**
- `caco profile ls [-p SOURCEPORT]` — list config profiles (all or for a specific port)
- `caco profile create <name> [-p SOURCEPORT]` — create a new profile (empty .cfg file)
- `caco profile edit <name> [-p SOURCEPORT]` — open profile in `$EDITOR`
- `caco profile cp <source> <dest> [-p SOURCEPORT]` — copy a profile
- `caco profile rm <name> [-p SOURCEPORT]` — remove a profile (warns if WADs reference it)
- `caco profile path <name> [-p SOURCEPORT]` — print absolute path to profile file

**Play `--config`/`-C` option:**
- `caco play --config controller id:1` — use "controller" config profile for this session
- Resolution order: CLI `--config` > WAD's `custom_config` > `"default"`
- Only injects `-config` flag for dsda-family sourceports
- Uses `wad_completions` table (auto-created via migration); stats stored as JSON in `stats_snapshot` column
- Supports nyan-doom/dsda-doom `stats.txt` format (persistent per-map tracking) and `levelstat.txt` format (human-readable `-levelstat` output)

**Saves command group:**
- `caco saves list <query> [--plain]` — list save files (.dsg, .zds) for a WAD's data directory
- `caco saves backup <query>` — zip the WAD's entire data directory to `~/.local/share/caco/backups/`
- `caco saves restore <query> [BACKUP]` — restore from zip (latest if omitted); prompts before overwriting
- `caco saves clean <query> [--dry-run]` — delete only save files, keeping stats and configs
- `caco saves backups [query] [--plain]` — list existing backup zips; without query lists all backups
- Save discovery scans recursively for `.dsg` (dsda/chocolate/woof/eternity) and `.zds` (zdoom) extensions
- Backup format: `{wad_id}_{sanitized_title}_{timestamp}.zip` containing entire data dir contents

**Demos command group:**
- `caco demos list <query> [--plain]` — list `.lmp` demo files for a WAD's demos directory
- `caco demos play <query> [DEMO] [-p PORT]` — play back a demo; most recent if DEMO omitted
- `caco demos clean <query> [--dry-run]` — delete demo files
- `caco play --record` / `caco play -r` — record a demo (auto-name); `caco play --record NAME` for custom name
- Demo files stored in `{data_dir}/demos/` within per-WAD data directories
- `demo_file` column on sessions table links recorded demos to play sessions
- `demos.py` module: `find_demo_files()`, `clean_demo_files()`, `generate_demo_name()`, `get_demos_dir()`

**Output formats:**
- `-o plain` on `ls`, `info`, `trash --list`, `cache list`, `stats` — TSV/key=value for scripting
- `-o json` on `ls`, `info` — JSON output with computed stats
- `--info` on `random` — print ID, title, author (TSV)

**Stats command options:**
- `--period month|year` — group activity by month (default) or year
- `--limit N` — number of periods to show (default: 12)
- `--plain` — key=value output for scripting

**Import command LLM options (Doomworld `--smart`):**
- `--llm-backend` — LLM backend: `claude-code`, `openrouter`, `anthropic`, `openai`
- `--llm-model` — model override for API backends

**Cache config options:**
- `cache_max_size_gb` — max cache size in GB (0 = unlimited)
- `cache_max_age_days` — remove files not played in N days (0 = never)
- `cache_auto_clean` — auto-cleanup on play (true/false)
- `auto_stats` — auto-track per-map stats after play sessions (default: true, requires `manage_data_dirs`)
- `auto_detect_iwad` — auto-detect required IWAD from WAD file contents on first play (default: true)
- `auto_detect_complevel` — auto-detect complevel from WAD lumps on first play (default: true)
- `auto_doomwiki_enrich` — auto-enrich non-Doomwiki imports with Doom Wiki metadata (default: true)

**TUI config (`[tui]` section):**
- `default_tab` — starting tab (all, playing, to-play, finished, backlog, other)
- `default_sort` — default sort field (id, title, author, playtime, last_played, year, rating)
- `default_sort_desc` — default sort direction (boolean)

**GUI config (`[gui]` section):**
- `default_tab` — starting tab (all, playing, to-play, finished, backlog, other)
- `default_sort` — default sort field (id, title, author, playtime, last_played, year, rating)
- `default_sort_desc` — default sort direction (boolean)
- `default_view` — "list" or "grid"
- `window_width` / `window_height` — initial window dimensions (overridden by saved geometry)
- `detail_panel_width` — initial detail panel width
- `show_detail_panel` — show detail panel on startup
- `thumbnail_size` — thumbnail pixel size

**GUI key patterns:**
- GUI uses `QAbstractTableModel` wrapping `db.search_wads()` with batch stats (same pattern as TUI `WadTable`)
- Single model, two views: both `QTableView` (list) and `QListView` (grid) share the same `WadTableModel`
- `QStyledItemDelegate` paints custom WAD cards in grid view (thumbnail + title + author + status badge)
- `QThreadPool` + `QRunnable` for search/import/thumbnail workers; `QThread` for sourceport launch
- `ThumbnailLoader` uses two-tier caching: in-memory dict in delegate + filesystem at `~/.cache/caco/thumbnails/`
- Thumbnail extraction: custom Doom WAD parser + patch format decoder (no external tools needed)
- Window geometry persisted via `QSettings` ("caco", "caco-gui") — auto-restores on next launch
- Signal relay: view → tab → MainWindow for action handling

## Dependencies

- `click` - CLI framework
- `rich` - Terminal output formatting
- `httpx` - HTTP client for idgames and Doomwiki APIs
- `pydantic` - Data validation for API responses
- `textual` - TUI framework (for `caco --tui`)
- `PySide6` - Qt6 GUI framework (optional, `[gui]` extra)
- `Pillow` - Image processing for thumbnail extraction (optional, `[gui]` extra)
- `pytest` / `pytest-cov` / `mypy` - Test framework and type checking (optional, `[test]` extra)

## Completions
- Always ensure that completions and `--help` flags are synced with any and all changes to functionality
- Hand-crafted completion scripts for fish, bash, and zsh — all use `caco _complete` for dynamic data
- Scripts embedded in `src/caco/cli/_completion_scripts.py` (works from installed packages); convenience copies at `completions/caco.fish`, `completions/caco.bash`, `completions/_caco`
- `caco completions [shell]` outputs our hand-crafted scripts (not Click's generic completions)
- Dynamic completion data via hidden `caco _complete <context>` command (contexts: `wads`, `tags`, `iwads`, `statuses`, `sort-fields`, `sourceports`, `modify-fields`, `query-fields`)
- Shell helpers (`__caco_wads`, `__caco_tags`, `__caco_iwads`, `__caco_sourceports`) call `caco _complete` for fast, purpose-built data

## Git Instructions
- Commit working changes to git
- Update the README.md, CLAUDE.md, TODO.md to document changes, features, and track progress
