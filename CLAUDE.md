# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Caco is a personal Doom WAD library manager inspired by `beets`. It tracks WADs you want to play, have played, or are playing, with metadata from multiple sources (idgames, Doomwiki, manual entry). Key features:

- SQLite database for WAD metadata and play history
- Import from idgames archive, URLs, or local files
- Automatic playtime tracking via sourceport wrapper
- Tag-based organization
- On-demand downloading (WADs are cached, not stored permanently)

## Commands

```bash
# Install in development mode (depends on idgames-api)
pip install -e ../idgames-api
pip install -e .

# Run CLI
caco <command>
```

No test suite exists yet.

## Architecture

```
src/caco/
├── cli.py          # Click-based CLI
├── db.py           # SQLite database (models, queries)
├── config.py       # TOML config in ~/.config/caco/
├── player.py       # Sourceport launcher + playtime tracking
└── sources/
    ├── __init__.py
    └── idgames.py  # idgames archive adapter (uses idgames-api)
```

**Data locations:**
- Database: `~/.local/share/caco/library.db`
- Config: `~/.config/caco/config.toml`
- WAD cache: `~/.cache/caco/wads/`

**Key patterns:**
- `db.py` uses raw sqlite3 with `sqlite3.Row` for dict-like access
- Source adapters are context managers that handle their own clients
- `player.py` wraps sourceport execution to track session start/end times
- Status enum: `wishlist`, `backlog`, `playing`, `finished`, `abandoned`

## Dependencies

- `idgames` - Local package from `../idgames-api` for idgames archive access
- `click` - CLI framework
- `rich` - Terminal output formatting
- `httpx` - HTTP client (via idgames dependency)
