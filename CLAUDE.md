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
- Completion tracking (times beaten per WAD)
- Soft-delete with trash/restore lifecycle

## Commands

```bash
# Activate the virtual environment (REQUIRED before running caco)
source .venv/bin/activate

# Install in development mode (only needed once, or after adding dependencies)
pip install -e .

# Run CLI
caco <command>

# Example: list library
caco list

# Example: test with plain output
caco list --plain

# Example: JSON output (for scripting)
caco list --json
caco info 1 --json

# Run tests
pip install -e '.[test]'
pytest tests/ -v
```

## Architecture

```
src/caco/
├── cli/            # Click-based CLI (split into submodules)
│   ├── __init__.py     # cli group, shared helpers, command aliases, JSON/plain renderers
│   ├── library.py      # list, info, update, delete, restore, link, random
│   ├── import_cmds.py  # unified import command with source flags
│   ├── tags.py         # tag add/remove/list
│   ├── play_cmd.py     # play command
│   ├── cache.py        # cache list/clear/prune
│   ├── config_cmd.py   # config, completions commands
│   └── stats.py        # stats, beaten commands
├── utils.py        # Shared utilities (coerce_str, BaseHttpClient, CacoSourceError, extract_year)
├── db.py           # SQLite database (models, queries, STATUS_SHORTCUTS)
├── config.py       # TOML config in ~/.config/caco/
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
│   │   └── cache.py       # Cache management screen
│   └── widgets/    # Widget classes
│       ├── base_search_pane.py # Abstract base for search panes
│       ├── wad_table.py   # DataTable for WAD list (with vim bindings, batch stats)
│       ├── wad_info.py    # Info panel widget (accepts pre-fetched stats)
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
│   │   ├── sessions_dialog.py # Session history table
│   │   ├── stats_dialog.py   # Library statistics overview
│   │   └── cache_dialog.py   # Cache management
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
├── sources/
│   ├── idgames.py  # idgames archive adapter
│   ├── doomwiki.py # Doom Wiki adapter
│   └── doomworld.py # Doomworld forum adapter
└── tests/          # pytest test suite
    ├── conftest.py     # In-memory DB fixture
    └── unit/           # Unit tests (utils, query parser, db, models, player)
```

**Data locations:**
- Database: `~/.local/share/caco/library.db`
- Config: `~/.config/caco/config.toml`
- WAD cache: `~/.cache/caco/wads/`

**Key patterns:**
- `db.py` uses raw sqlite3 with `sqlite3.Row` for dict-like access; tag helpers (`_fetch_tags`, `_attach_tags`, `_fetch_tags_batch`) and batch query functions (`get_total_playtime_batch`, `get_last_played_batch`, `get_times_beaten_batch`, `get_session_count_batch`) reduce N+1 queries
- TUI widgets use batch stats: `WadTable.load_wads()` batch-fetches all stats, `get_wad_stats()` exposes them to `WadInfoPanel`; `update_row()` handles incremental cell updates
- Status colors/display centralized in `tui/theme.py` (`STATUS_CONFIG` dict with `get_status_display/color/css_class` helpers)
- Source adapters are context managers; clients inherit `BaseHttpClient` from `utils.py`; errors inherit `CacoSourceError`
- CLI uses Click's decorator registration pattern: each `cli/*.py` submodule imports `cli` from `caco.cli` and registers commands; `__init__.py` imports all submodules at bottom to trigger registration
- `player.py` wraps sourceport execution to track session start/end times
- Status enum: `to-play`, `backlog`, `playing`, `finished`, `abandoned`, `awaiting-update`
- Import command uses flag-based source selection: `caco import <source> [--idgames|--doomwiki|--doomworld|--local|--url URL]`
- Query syntax (beets-style):
  - Fields: `id:`, `title:`, `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`
  - OR queries: `"status:playing , status:to-play"` (comma with spaces — spaces required!)
  - Negation: `^status:finished` (prefer `^` prefix, `-` also works but may conflict with CLI flags)
  - Status shortcuts: `status:p` (playing), `status:f` (finished), etc.
  - Glob patterns: `tag:caco*` (matches cacoward, etc.)
  - Free text searches title, author, and description
  - Multiple terms are joined with implicit AND
- Per-WAD config: `custom_iwad`, `custom_sourceport`, `custom_args` (JSON array) columns in wads table
- Cross-source downloading: `idgames_id` column allows any WAD to download via idgames API (set with `caco update --idgames-id`)
- Soft-delete: `deleted_at` column; `caco delete` moves to trash, `caco restore` recovers, `caco list --deleted` shows trash
- `link` command: copies/moves a local file to cache and updates `cached_path`/`filename` for metadata-only entries (e.g., Doomwiki imports)
- `version` column tracks WAD version strings for non-idgames releases
- Database migrations run on `init_db()`: add columns, create tables, rename statuses

**Status shortcuts (complete list):**
| Shortcut | Status |
|----------|--------|
| `t`, `tp`, `toplay` | to-play |
| `b`, `back` | backlog |
| `p`, `play` | playing |
| `f`, `fin`, `done` | finished |
| `a`, `drop`, `dropped` | abandoned |
| `w`, `au`, `await`, `waiting`, `wip` | awaiting-update |

**Beaten command group:**
- `caco beaten list` — show all WADs with completion counts
- `caco beaten add <query>` — increment completion count
- `caco beaten remove <query>` — decrement completion count
- `caco beaten set <query> <count>` — set exact count
- Uses `wad_completions` table (auto-created via migration)

**Output formats:**
- `--plain` on `list`, `info`, `tag list`, `cache list` — TSV/key=value for scripting
- `--json` on `list`, `info` — JSON output with computed stats
- `--info` on `random` — print ID, title, author (TSV)

**Cache config options:**
- `cache_max_size_gb` — max cache size in GB (0 = unlimited)
- `cache_max_age_days` — remove files not played in N days (0 = never)
- `cache_auto_clean` — auto-cleanup on play (true/false)

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
- `pytest` / `pytest-cov` - Test framework (optional, `[test]` extra)

## Completions
- Always ensure that completions and `--help` flags are synced with any and all changes to functionality
- Fish completions are in `completions/caco.fish`

## Git Instructions
- Commit working changes to git
- Update the README.md, CLAUDE.md, TODO.md to document changes, features, and track progress
