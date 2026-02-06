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
```

No test suite exists yet.

## Architecture

```
src/caco/
├── cli/            # Click-based CLI (split into submodules)
│   ├── __init__.py     # cli group, shared helpers, command aliases
│   ├── library.py      # list, info, update, delete, restore, link, random
│   ├── import_cmds.py  # import group + auto/idgames/doomwiki/doomworld/url/local
│   ├── tags.py         # tag add/remove/list
│   ├── play_cmd.py     # play command
│   ├── cache.py        # cache list/clear/clean
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
│   ├── styles.tcss # Textual CSS styles
│   ├── screens/    # Screen classes
│   │   ├── tabbed_library.py  # Main tabbed interface (entry point)
│   │   ├── wad_detail.py  # WAD detail view
│   │   ├── wad_edit.py    # WAD metadata edit form
│   │   └── sessions.py    # Session history
│   └── widgets/    # Widget classes
│       ├── base_search_pane.py # Abstract base for search panes
│       ├── wad_table.py   # DataTable for WAD list (with vim bindings)
│       ├── wad_info.py    # Info panel widget
│       ├── filter_input.py # Search/filter input
│       ├── sort_select.py  # Sort dropdown widget
│       ├── library_pane.py # Reusable library view (table + panel + filter)
│       ├── import_pane.py  # Import container with source selector
│       ├── idgames_pane.py # idgames search (extends BaseSearchPane)
│       ├── doomwiki_pane.py # Doom Wiki search (extends BaseSearchPane)
│       ├── doomworld_pane.py # Doomworld forum URL import
│       ├── url_pane.py     # Manual URL import form
│       └── local_pane.py   # Local file import form
└── sources/
    ├── idgames.py  # idgames archive adapter
    ├── doomwiki.py # Doom Wiki adapter
    └── doomworld.py # Doomworld forum adapter
```

**Data locations:**
- Database: `~/.local/share/caco/library.db`
- Config: `~/.config/caco/config.toml`
- WAD cache: `~/.cache/caco/wads/`

**Key patterns:**
- `db.py` uses raw sqlite3 with `sqlite3.Row` for dict-like access; tag helpers (`_fetch_tags`, `_attach_tags`, `_fetch_tags_batch`) and batch query functions (`get_total_playtime_batch`, `get_last_played_batch`) reduce N+1 queries
- Source adapters are context managers; clients inherit `BaseHttpClient` from `utils.py`; errors inherit `CacoSourceError`
- CLI uses Click's decorator registration pattern: each `cli/*.py` submodule imports `cli` from `caco.cli` and registers commands; `__init__.py` imports all submodules at bottom to trigger registration
- `player.py` wraps sourceport execution to track session start/end times
- Status enum: `to-play`, `backlog`, `playing`, `finished`, `abandoned`, `awaiting-update`
- Query syntax (beets-style):
  - Fields: `id:`, `title:`, `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`
  - OR queries: `"status:playing , status:to-play"` (comma with spaces)
  - Negation: `^status:finished` (use `^` prefix, `-` also works but may conflict with CLI)
  - Status shortcuts: `status:p` (playing), `status:f` (finished), etc.
  - Glob patterns: `tag:caco*` (matches cacoward, etc.)
- Per-WAD config: `custom_iwad`, `custom_sourceport`, `custom_args` columns in wads table
- Cross-source downloading: `idgames_id` column allows any WAD to download via idgames API (set with `caco update --idgames-id`)

## Dependencies

- `click` - CLI framework
- `rich` - Terminal output formatting
- `httpx` - HTTP client for idgames and Doomwiki APIs
- `pydantic` - Data validation for API responses
- `textual` - TUI framework (for `caco --tui`)

## Completions
- Always ensure that completions and `--help` flags are synced with any and all changes to functionality

## Git Instructions
- Commit working changes to git
- Update the README.md, CLAUDE.md, TODO.md to document changes, features, and track progress

