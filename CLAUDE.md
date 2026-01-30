# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Caco is a personal Doom WAD library manager inspired by `beets`. It tracks WADs you want to play, have played, or are playing, with metadata from multiple sources (idgames, Doomwiki, manual entry). Key features:

- SQLite database for WAD metadata and play history
- Import from idgames archive, Doom Wiki, URLs, or local files
- Automatic playtime tracking via sourceport wrapper
- Tag-based organization
- On-demand downloading (WADs are cached, not stored permanently)

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
├── cli.py          # Click-based CLI
├── db.py           # SQLite database (models, queries)
├── config.py       # TOML config in ~/.config/caco/
├── player.py       # Sourceport launcher + playtime tracking
├── idgames/        # idgames API client
│   ├── client.py   # HTTP client for doomworld.com/idgames/api
│   └── models.py   # Pydantic models (FileEntry, etc.)
├── doomwiki/       # Doom Wiki API client
│   ├── client.py   # HTTP client for doomwiki.org MediaWiki API
│   ├── models.py   # Pydantic models (WikiEntry, SearchResult)
│   └── parser.py   # Wikitext parser for {{Wad}} infobox template
└── sources/
    ├── idgames.py  # idgames archive adapter
    └── doomwiki.py # Doom Wiki adapter
```

**Data locations:**
- Database: `~/.local/share/caco/library.db`
- Config: `~/.config/caco/config.toml`
- WAD cache: `~/.cache/caco/wads/`

**Key patterns:**
- `db.py` uses raw sqlite3 with `sqlite3.Row` for dict-like access
- Source adapters are context managers that handle their own clients
- `player.py` wraps sourceport execution to track session start/end times
- Status enum: `to-play`, `backlog`, `playing`, `finished`, `abandoned`
- Query syntax: `id:`, `title:`, `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`
- Per-WAD config: `custom_iwad`, `custom_sourceport`, `custom_args` columns in wads table

## Dependencies

- `click` - CLI framework
- `rich` - Terminal output formatting
- `httpx` - HTTP client for idgames and Doomwiki APIs
- `pydantic` - Data validation for API responses

## Completions
- Always ensure that completions and `--help` flags are synced with any and all changes to functionality

## Git Instructions
- Commit working changes to git
- Update the README.md, CLAUDE.md, TODO.md to document changes, features, and track progress

