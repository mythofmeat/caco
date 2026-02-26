# Changelog

All notable changes to Caco are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

---

## [1.4.0] - 2026-02-26

IWAD management — first-class IWAD registry with family/variant model and
configurable priority resolution.

### Added

- **IWAD registry**: `caco iwad` command group for managing IWADs as first-class
  entities in the database (`iwads` table with family, variant, title, path, MD5)
- **Family/variant model**: IWADs organized by family (doom, doom2, plutonia, tnt)
  with multiple variants per family (v1.9, bfg, enhanced, kex, unity)
- **Priority resolution**: `resolve_iwad("doom2")` returns the preferred variant
  based on a configurable priority list; `[iwad_priority]` config section for
  per-family overrides
- **Cross-family fallbacks**: Freedoom used as last-resort fallback (freedoom2 for
  doom2/plutonia/tnt, freedoom1 for doom)
- **`caco iwad scan`**: Auto-discover known IWADs in `iwad_dirs` by MD5 checksum,
  detecting both family and variant; filename fallback for modded/newer releases
- **`caco iwad add`**: Register an IWAD file with auto-detection (MD5 → family +
  variant), or `--family`/`--variant` overrides for custom IWADs
- **`caco iwad list`**: Display registered IWADs with Family, Variant, Title, Path
  columns; preferred variant marked with `*`; `--plain` TSV output
- **`caco iwad remove`**: Unregister by family + variant, or remove all variants
  of a family (with confirmation prompt)
- **Expanded MD5 database**: ~22 known MD5s across 4 primary families covering
  v1.9, BFG, Enhanced, Unity, and KEX variants
- **IWAD resolution from registry**: `resolve_iwad()` checks the IWAD registry
  with priority resolution before falling back to `iwad_dirs` filesystem search
- **Auto-link on Doom Wiki import**: When a Doom Wiki entry has an IWAD field
  (e.g., "Doom II"), automatically sets `custom_iwad` if that IWAD is registered
- **IWAD alias mapping**: Normalizes free-text IWAD names from wikis/forums to
  family names (e.g., "Doom II: Hell on Earth" → "doom2")
- **New DB functions**: `get_iwad_variant()`, `get_family_iwads()`,
  `get_iwad_priority()`
- **Fish completions**: `iwad` subcommand completions with `--family`/`--variant`
  flags and dynamic family completion for `iwad remove`
- **Migration #8**: `iwads` table creation
- **Migration #9**: Restructure `iwads` table from `name UNIQUE` to
  `(family, variant) UNIQUE` with data migration

---

## [1.3.1] - 2026-02-26

GUI stats import/export and context menu access.

### Added

- **GUI stats import/export**: WadStatsDialog now has "Import Stats..." and
  "Export Stats..." buttons for loading stats.txt/levelstat.txt files and
  saving stats back to text files directly from the GUI
- **GUI "Map Stats..." context menu**: Right-click any WAD in list or grid
  view to open the per-map stats dialog
- **Always-visible "Map Stats" button**: Detail panel shows the button for
  all WADs (not just those with existing stats), enabling stats import on
  any WAD

### Changed

- **Detail panel stats button**: Renamed from "Stats" to "Map Stats" for
  clarity; now always visible when a WAD is selected
- **WadStatsDialog**: Refactored to support import/export lifecycle — tracks
  changes via `changed` property so callers refresh after import

---

## [1.3.0] - 2026-02-25

Per-map statistics import/export and session dialog cleanup.

### Added

- **Stats.txt import/export**: Import per-map completion statistics from
  sourceport stats files and attach them to completion records
  - Supports nyan-doom/dsda-doom `stats.txt` format (persistent per-map tracking
    with kills, items, secrets, time, skill, exits, and best-of stats)
  - Supports dsda-doom `levelstat.txt` format (human-readable `-levelstat` output)
  - Auto-detects format; lossless round-trip (parse → store → export matches original)
- **`caco beaten add --stats-file`**: Attach stats when adding a completion
- **`caco beaten attach`**: Attach stats to an existing completion record
- **`caco beaten stats`**: View full per-map statistics table for a completion
- **`caco beaten export`**: Export stats back to original text format
- **`beaten list` Stats column**: Shows indicator when a completion has stats attached
- **GUI WadStatsDialog**: Per-map stats table with completion selector, accessible
  via "Stats" button in detail panel
- **TUI WadStatsScreen**: Per-map stats screen with n/p keys to switch completions,
  accessible via `M` keybinding in library pane
- **`wad_stats.py` module**: Parser, formatter, and JSON serialization for sourceport
  per-map statistics (MapStats/WadStats dataclasses)
- **`db.update_wad_completion()`**: Update stats_snapshot and/or notes on existing
  completion records
- **Fish completions**: Added missing `beaten`, `stats`, `restore`, `link`, `cache`
  command completions, plus new `beaten stats`, `beaten attach`, `beaten export`

### Removed

- **Session Notes column**: Removed unused "Notes" column from GUI and TUI session
  history dialogs (DB schema unchanged for forward-compatibility)

---

## [1.2.2] - 2026-02-20

Documentation accuracy overhaul and version alignment.

### Fixed

- **`beaten list` docs**: corrected signature from non-existent `--min` flag to
  actual `<query>` argument showing per-WAD completion history
- **`beaten add` docs**: documented `--notes` flag for annotating completions
- **`beaten remove` docs**: documented optional `COMPLETION_ID` argument for
  removing specific records
- **Missing `stats` options**: documented `--period`, `--limit`, and `--plain` flags
- **Missing `update` flags**: documented `--clear-description` and `--clear-version`
- **Missing `import` flags**: documented `--llm-backend` and `--llm-model` options
- **Missing list columns**: added `source`, `filename`, `rating` to available columns
  list in config example
- **Missing `[list] default_status`** config option documented
- **Status shortcuts table**: now shows all shortcuts matching source code
  (`toplay`, `dropped`, `await`, `waiting`)
- **GUI config**: added `detail_panel_width`, `show_detail_panel`, `thumbnail_size`
  to README
- **CLAUDE.md architecture**: added `services/` module and `link_dialog.py`

### Added

- **CI section** in README Development: documents GitHub Actions test matrix and
  mypy type checking
- **Library Statistics section** in README with full `caco stats` usage examples
- **WAD unavailable dialog** documented in GUI features
- `stats --plain` added to Scripting section

---

## [1.2.1] - 2026-02-20

Major internal quality overhaul: database refactoring, security hardening,
performance improvements, comprehensive test suite, and code modernization
across all layers.

### Added

- **Test suite**: 127 new tests covering DB sessions, batch stats,
  completions, duplicate detection, migration versioning, wikitext/Doomworld
  parsers, CLI integration, config round-trips, and sort parsing
- **CI**: GitHub Actions workflow with Python 3.10/3.11/3.12 matrix
- **Source adapter tests**: 17 mock tests using `respx` for IdgamesSource,
  DoomwikiSource, and DoomworldSource
- **`WadRecord` TypedDict** for WAD dict return types throughout the codebase
- **`ProgressCallback` type alias** in `player.py`
- **`StatsSnapshot` dataclass**: bundles library stats, completion rate, and
  play-period data into a single `get_stats_snapshot()` call
- **`ImportService`**: centralized duplicate-check-and-import for all 5 source
  types, replacing ~15 duplicate blocks across CLI/TUI/GUI
- **`BaseSource` mixin** in `sources/base.py` for shared context-manager
  lifecycle across source adapters
- **Batch wiki fetch**: MediaWiki pipe-separated titles API reduces N+1 HTTP
  requests to 2 (search + batch content) in Doom Wiki search
- **`search_wads()` limit parameter** for efficient random selection
- **Schema migration versioning**: `schema_migrations` table tracks applied
  versions; `init_db()` skips already-applied migrations
- **Ruff and mypy configuration** in `pyproject.toml`
- **`set_query()` and `get_selected_wad_id()`** public API on GUI `LibraryTab`
- 9 public wrapper methods on `LibraryTab`; `MainWindow` no longer accesses
  private attributes

### Changed

- **Split `db.py`** (1593 lines) into `db/` package with 6 submodules
  (`_models`, `_connection`, `_schema`, `_query`, `_wads`, `_sessions`) and
  `__init__.py` re-exporting all symbols for backward compatibility
- **Unified batch stats**: `get_wad_stats_batch()` replaces 4 separate batch
  calls with 2 queries on 1 connection
- **Query chunking**: `_batch_query()` chunks queries to stay under SQLite's
  variable limit (`SQLITE_MAX_VARS=900`)
- **Unified `STATUS_METADATA`** in `db.py`: single source of truth for display
  names, hex colors, Rich colors, CSS classes; TUI/GUI themes derive from it
- **Batch cache cleanup** in `player.py`: uses `get_last_played_batch()` instead
  of N+1 individual calls in `auto_clean_cache()`
- **Decoupled `player.py` from Rich**: removed `Console` parameter; CLI creates
  Rich progress callback in `play_cmd.py` instead
- **Thumbnail extraction** now uses `mmap` for direct WAD file reads, avoiding
  loading entire WADs into memory
- **Random command** uses `ORDER BY RANDOM() LIMIT 1` instead of fetching all rows
- **Download chunk size** increased from 8KB to 256KB
- Moved function-body imports to top-level per PEP 8
- Narrow `except Exception` to specific exception types in doomworld
  adapter/scraper
- `.format()` calls converted to f-strings in CLI interactive picker
- `normalize_status()` made public; CLI delegates to it

### Fixed

- **SQL injection guard**: `ALLOWED_UPDATE_FIELDS` whitelist on `update_wad()`
- **Atomic completions**: completion recording moved inside `update_wad()`
  transaction
- **Sourceport validation**: verify binary existence before subprocess launch
- **Config save bug**: `save_config()` was dropping nested `[tui]`/`[gui]`/`[list]`
  sections
- **Pydantic mutable default**: `download_links` uses `Field(default_factory=list)`
- **Tag query escaping**: `ESCAPE` clause added to non-glob tag `LIKE` queries
- **Grayscale palette**: fixed RGB ordering in thumbnail extractor fallback
- **Thread safety**: `_pending` set in `ThumbnailLoader` protected with
  `threading.Lock`
- **Static SQL**: `get_wads_played_by_period` no longer uses f-string SQL
- **ZIP bomb protection**: 256 MB entry size limit in thumbnail extraction
- Proper `mmap` lifecycle management with try/finally cleanup
- `logger.debug()` added to 5 silent except blocks in thumbnail extractor/loader
- **Shell completions**: added "awaiting-update" to `QUERY_STATUS_VALUES`
- Shared `httpx.Client` across thumbnail scraper workers
- O(1) `wad_id` to row index mapping in GUI `WadTableModel`

### Performance

- SQLite WAL mode, `synchronous=NORMAL`, `cache_size`, `temp_store` PRAGMAs
- Database indexes on `wads(deleted_at)`, `wads(cached_path)`,
  `sessions(wad_id, started_at)`
- `executemany` for bulk completion inserts
- Cached `load_config()` with invalidation on `save_config()`
- Detail panels skip redundant `db.get_wad()` when caller provides WAD data

---

## [1.2] - 2026-02-20

### Added

- **Force-stop dialog** in GUI when a sourceport is already running
- **Wayland support**: set desktop filename and window icon for proper
  Wayland window identification

### Changed

- Sourceport launch uses `Popen` instead of `subprocess.run` so launch
  failures (missing binary, permission denied) are caught before creating
  a session record
- Process handle exposed via `process_ref` for external cancellation

### Fixed

- Coverage artifacts added to `.gitignore`

---

## [1.1] - 2026-02-18

The first major feature release, expanding Caco from a basic CLI tool into a
full-featured WAD library manager with TUI, GUI, multiple import sources,
advanced query syntax, and comprehensive library management.

### Added

#### Import Sources
- **Doom Wiki import** (`caco import --doomwiki`): search and import WADs from
  Doom Wiki with wikitext infobox parsing
- **Doomworld forum import** (`caco import --doomworld`): import WADs from
  Doomworld forum threads with HTML/JSON-LD parsing and optional LLM-powered
  metadata extraction
- **URL import** (`caco import --url`): import WADs from arbitrary URLs
- **Local file import** (`caco import --local`): import from local filesystem
  with batch support (`caco import local *.wad`)
- **Import auto-detection**: `caco add <source>` detects URLs, files, and
  idgames IDs automatically
- **`caco link` command**: attach local files to metadata-only entries (e.g.,
  Doom Wiki imports that lack download URLs)

#### Query System
- **Beets-style query syntax**: `field:value` queries (`id:`, `title:`,
  `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`)
- **OR queries**: comma-separated values (`"status:playing , status:to-play"`)
- **Negation**: `^status:finished` or `-status:finished`
- **Glob patterns**: `tag:caco*` matches cacoward, cacowards, etc.
- **Free text search**: searches title, author, and description
- **Universal query support**: all commands accept queries (info, play,
  update, delete, etc.)
- **Sort suffix notation**: `+` ascending, `-` descending (e.g., `--sort title+`)
- **Interactive picker**: uses `fzf` if available, falls back to numbered selection

#### Library Management
- **Soft delete with trash/restore**: `caco delete` moves to trash,
  `caco restore` recovers, `caco list --deleted` shows trash
- **Version tracking**: `version` column for non-idgames releases
- **`awaiting-update` status**: for WADs waiting on new versions
- **Completion tracking**: `wad_completions` table, `caco beaten` command group
  (list, add, remove, set)
- **Batch operations**: range syntax `3-6,9,11` for update, delete, tag commands
- **Per-WAD config**: `custom_iwad`, `custom_sourceport`, `custom_args` columns
- **Cross-source downloading**: `idgames_id` column allows any WAD to download
  via idgames API
- **`--plain` output**: TSV/key=value format for scripting on list, info, tag
  list, cache list
- **`--json` output**: JSON format for list and info commands
- **`--dry-run` flag**: preview changes on delete, update, tag operations
- **Delete preview**: shows stats before deletion

#### Cache Management
- **`caco cache list`**: show cached files with sizes
- **`caco cache list --orphans`**: show files not tracked in database
- **`caco cache clear`**: remove cached files (specific WADs or `--all`)
- **`caco cache clean`**: remove orphaned files
- **Auto-cleanup**: `cache_max_size_gb`, `cache_max_age_days`, `cache_auto_clean`
  config options
- Cache cleanup only affects idgames sources (local files are never deleted)

#### Statistics
- **`caco stats`**: library statistics overview
- **`caco beaten list`**: view completion counts
- **Playthrough cycles**: when status changes to `finished`, increments cycle;
  map completions tracked per cycle

#### CLI Polish
- **Status shortcuts**: `p`=playing, `f`=finished, `t`=to-play, `b`=backlog,
  `a`=abandoned, `w`=awaiting-update
- **Command aliases**: `add`, `rm`, `ls`, `i`
- **Tag globs**: `--tag cacowards*` supports wildcards
- **Configurable columns** via `[list]` config section
- **`caco config --edit`**: opens config in `$EDITOR`
- **`caco completions`**: install shell completions
- **Fish shell completions**: comprehensive completions for all commands,
  options, and query fields
- **Download progress bar**: filename, progress bar, size, and transfer speed
- **Play by query**: `caco play filename:tnto`
- **Play most recent**: `caco play` with no arguments plays most recently
  played WAD
- **Update metadata**: `--title`, `--author`, `--year`, `--description` flags
- **Source URL display**: shows source URL and link instructions when WAD file
  is missing

#### TUI (`caco --tui`)
- **Textual-based TUI** with tabbed interface (All, Playing, To-Play,
  Finished, Backlog, Other)
- **Vim keybindings** in WAD table
- **Sort dropdown**: ID, Title, Author, Playtime, Last Played, Year, Rating
- **Edit screen**: inline WAD metadata editing
- **idgames search pane**: search and import from TUI
- **Doom Wiki search pane**: search and import from TUI
- **Doomworld import pane**: URL-based import from TUI
- **URL and local import panes**: additional import sources in TUI
- **WAD info panel**: detailed view with batch-fetched stats
- **Filter input**: real-time search/filter
- **Status-colored display**: centralized theme in `tui/theme.py`

#### GUI (`caco --gui`)
- **PySide6-based GUI** with dark Doom-inspired theme
- **Dual view modes**: list view (`QTableView`) and grid view (`QListView`
  with custom card delegate)
- **Thumbnail support**: TITLEPIC extraction from WAD files + Doom patch
  decoder + Doom Wiki image scraping
- **Two-tier thumbnail caching**: in-memory dict + filesystem at
  `~/.cache/caco/thumbnails/`
- **Detail panel**: right sidebar with thumbnail, metadata, stats, action buttons
- **5-source import tab**: idgames, Doom Wiki, Doomworld, URL, local
- **Edit dialog**: WAD metadata editing form
- **Delete dialog**: confirmation with WAD stats
- **Sessions dialog**: session history table
- **Stats dialog**: library statistics overview
- **Cache dialog**: cache management
- **Saved searches on tab bar**: random sort + persisted filter state
- **Proportional column sizing**
- **Context menu and vim keys** in list view
- **Window geometry persistence** via `QSettings`

#### Configuration
- **TOML config file** at `~/.config/caco/config.toml`
- **Configurable `db_path`**: custom database location with tilde expansion
- **`iwad_dirs`**: IWAD directory search paths (use short names like `doom2`
  instead of full paths)
- **Sourceport PATH resolution**: resolve sourceport names via `$PATH` lookup
- **`[tui]` section**: `default_tab`, `default_sort`, `default_sort_desc`
- **`[gui]` section**: `default_tab`, `default_sort`, `default_sort_desc`,
  `default_view`, `window_width`, `window_height`, `detail_panel_width`,
  `show_detail_panel`, `thumbnail_size`
- **`[list]` section**: configurable columns, colors, sort
- **Cache config**: `cache_max_size_gb`, `cache_max_age_days`, `cache_auto_clean`

#### Other
- **Desktop launcher** with icon (`.desktop` file)
- Vendored idgames API client (no external dependency)
- **Renamed "wishlist" status to "to-play"** with data migration
- Last played tracking in list and info output

### Changed

- Full-scope codebase refactoring for maintainability
- CLI split into submodules (`library.py`, `import_cmds.py`, `tags.py`,
  `play_cmd.py`, `cache.py`, `config_cmd.py`, `stats.py`)
- Default list sorting changed to ID ascending (was status priority)
- Import command uses flag-based source selection

### Fixed

- `filename:` query filter in `search_wads()`
- `cached_path` parameter missing from `add_wad()`
- Old vBulletin URL format for Doomworld import
- Wiki thumbnail scraping: User-Agent header added to API requests
- Thumbnail thread safety
- Sort behavior for nullable fields
- Code review findings: hardening, safety, and deduplication

---

## [1.0] - 2026-01-24

Initial release of Caco.

### Added

- **SQLite database** for WAD metadata and play session tracking
- **Import from idgames archive**: search and import WADs with full metadata
- **Playtime tracking**: automatic session recording via sourceport wrapper
- **Tag-based organization**: add/remove/list tags on WADs
- **Status tracking**: wishlist, backlog, playing, finished, abandoned
- **CLI commands**: list, info, import, play, update, delete, tag
- **Rich terminal output**: colored tables, formatted info display
- Click-based CLI framework
- httpx-based HTTP client for idgames API
- Pydantic models for API response validation
