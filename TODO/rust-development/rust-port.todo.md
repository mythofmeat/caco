# Rust Port

Port caco from Python to Rust. Both versions coexist in the same repo, sharing the same database (`~/.local/share/caco/library.db`) and config (`~/.config/caco/config.toml`) so they can be used interchangeably during development.

## Crate Mapping

| Python | Rust |
|---|---|
| `click` | `clap` (derive) |
| `httpx` | `reqwest` |
| `pydantic` | `serde` |
| `sqlite3` | `rusqlite` (bundled) |
| `tomllib` | `toml` |
| `rich` | `comfy-table` + `indicatif` |
| `struct` / binary | `byteorder` |
| `Pillow` | `image` |
| `subprocess` | `std::process::Command` |
| `pathlib` | `std::path::PathBuf` |
| `zipfile` | `zip` |
| `re` | `regex` |
| `textual` | `ratatui` + `crossterm` |
| `PySide6` | `egui` + `eframe` |
| BeautifulSoup/HTML | `scraper` |

## Workspace Layout

```
caco-dev/
├── Cargo.toml              # workspace root
├── crates/
│   ├── caco-core/          # DB, config, player, parsers, models
│   ├── caco-sources/       # API clients, import service
│   ├── caco-cli/           # clap-based CLI binary
│   ├── caco-tui/           # ratatui TUI
│   └── caco-gui/           # egui GUI
├── src/caco/               # existing Python (kept during transition)
└── pyproject.toml          # existing Python build
```

## Phase 0: Repo Setup

- [ ] Create `Cargo.toml` workspace at repo root
- [ ] Scaffold `crates/` directory with all 5 crate stubs
- [ ] Set up shared `caco-core` dependencies
- [ ] Verify `cargo build` and `cargo test` work alongside Python

## Phase 1: Core Library (`caco-core`)

Port the foundational logic everything depends on.

### Config
- [ ] TOML config loading (`config.py` → ~500 lines)
- [ ] XDG paths via `dirs` crate
- [ ] Config auto-update (add missing keys on load)
- [ ] IWAD resolution chain (DB registry → iwad_dirs scan)
- [ ] Data directory helpers (`get_wad_data_dir`, `find_wad_data_dir`, `_sanitize_dirname`)
- [ ] Profile path helpers (`get_sourceport_dir`, `get_profile_path`, `list_profiles`)

### Database
- [ ] Schema SQL + `init_db()` (`db/_schema.py`)
- [ ] All 22 migrations (including filesystem migrations 10/12/13)
- [ ] `Status` enum, `WadRecord` struct, constants (`db/_models.py`)
- [ ] Connection helpers, tag helpers, batch query chunking (`db/_connection.py`)
- [ ] WAD CRUD — add/get/update/delete, tag add/remove (`db/_wads.py`)
- [ ] Sessions, completions, batch stats, cache helpers (`db/_sessions.py`)
- [ ] Beets-style query parser — OR groups, AND terms, negation, globs, 11 fields (`db/_query.py`)
- [ ] IWAD registry — family/variant model, known hashes, priority resolution (`db/_iwads.py`)
- [ ] id24 registry — known hashes, identification, CRUD (`db/_id24.py`)

### WAD Parsing
- [ ] `parse_wad_directory()` — header + directory reading
- [ ] IWAD detection — PNAMES analysis, map lump names, ZIP handling (`iwad_detect.py`)
- [ ] Complevel detection — UMAPINFO, DEHACKED, COMPLVL lump (`complevel_detect.py`)
- [ ] Complevel names, aliases, parser (`complevel.py`)

### Player & Sourceports
- [ ] Sourceport family registry, CLI flag generation (`sourceports.py`)
- [ ] Player — subprocess launch, playtime tracking, crash detection (`player.py`)
- [ ] Data dir injection, stats snapshot capture
- [ ] Demo recording, id24 resource auto-loading
- [ ] Config profile resolution (CLI > WAD > default)

### Stats & Data Management
- [ ] WAD stats parser — `stats.txt` + `levelstat.txt` formats (`wad_stats.py`)
- [ ] `compute_stats_delta()` for per-session map tracking
- [ ] Save game management — find, backup, restore, clean (`saves.py`)
- [ ] Demo file management — find, clean, generate names (`demos.py`)

### Utilities
- [ ] `BaseHttpClient` equivalent (shared reqwest client config)
- [ ] `CacoSourceError` hierarchy (`thiserror`)
- [ ] `coerce_str`, `extract_year`, shared helpers (`utils.py`)

## Phase 2: API Clients (`caco-sources`)

### idgames
- [ ] HTTP client — search, get, download (streaming) (`idgames/client.py`)
- [ ] Response models via serde (`idgames/models.py`)
- [ ] Configurable download mirrors

### Doom Wiki
- [ ] MediaWiki API client — search, fetch wikitext (`doomwiki/client.py`)
- [ ] Wikitext `{{Wad}}` infobox parser (`doomwiki/parser.py`)
- [ ] Response models (`doomwiki/models.py`)
- [ ] Batch page fetching (pipe-separated titles)

### Doomworld
- [ ] HTML scraper — thread parsing, JSON-LD extraction (`doomworld/parser.py`)
- [ ] HTTP client (`doomworld/client.py`)
- [ ] Response models (`doomworld/models.py`)
- [ ] LLM backends — claude-code, openrouter, anthropic, openai (`doomworld/llm.py`)

### Services
- [ ] Import service — duplicate checking, auto-enrichment (`services/import_service.py`)
- [ ] Auto Doomwiki enrichment (`_auto_enrich_doomwiki`)
- [ ] Auto IWAD linking
- [ ] Resource service — IWAD/id24 registration (`services/resource_service.py`)

### Source Adapters
- [ ] `BaseSource` trait (shared context-manager lifecycle)
- [ ] idgames, doomwiki, doomworld adapters (`sources/*.py`)

## Phase 3: CLI (`caco-cli`)

### Command Structure
- [ ] `clap` derive setup — main group, subcommands
- [ ] Output format system — rich tables, plain TSV, JSON
- [ ] Progress bar wrapper for downloads

### Commands
- [ ] `ls` — library listing with query, sort, status tabs, IWAD/id24 listing
- [ ] `info` — WAD detail view, `--levelstats` with live/completion/timestamp modes
- [ ] `import` — unified command with source flags (idgames/doomwiki/doomworld/url/local)
- [ ] `play` — sourceport launch, `--iwad`, `--first`, `--record`, `--config`/`-C`, `--complevel`/`-c`
- [ ] `modify` — field=value, beaten+/-/=, tag add/remove, `--link`, `--add-file`/`--remove-file`
- [ ] `trash` — soft delete, `--restore`, `--list`, `--iwad`, `--id24`
- [ ] `random` — random WAD picker, `--info`
- [ ] `cache` — list/clear/prune
- [ ] `stats` — library statistics, `--period`, `--limit`
- [ ] `sessions` — play session history with per-session maps
- [ ] `saves` — list/backup/restore/clean/backups
- [ ] `demos` — list/play/clean
- [ ] `profile` — ls/create/edit/cp/rm/path
- [ ] `config` — show/edit config
- [ ] `completions` — shell completion script output
- [ ] `_complete` — hidden dynamic completion data command

### Shell Completions
- [ ] `clap_complete` for base completions
- [ ] Custom dynamic completions (wads, tags, iwads, sourceports, etc.)
- [ ] Fish, bash, zsh scripts

## Phase 4: TUI (`caco-tui`)

Built with `ratatui` + `crossterm`. Different paradigm from Textual — no reactive model or CSS, but the functionality is the same.

### Core
- [ ] App struct, event loop, screen management
- [ ] Theme/status colors (port `tui/theme.py`)
- [ ] Keybindings (vim-style navigation)

### Screens
- [ ] Tabbed library view — status tabs, WAD table, info panel
- [ ] WAD detail view
- [ ] WAD edit form
- [ ] Session history
- [ ] Delete confirmation modal
- [ ] Library statistics
- [ ] Per-map stats
- [ ] Cache management
- [ ] IWAD/id24 resource management (tabbed)

### Widgets
- [ ] WAD table with batch stats, vim bindings
- [ ] Info panel (pre-fetched stats)
- [ ] Filter input
- [ ] Sort selector
- [ ] Library pane (composite: table + panel + filter)
- [ ] Import panes — idgames, doomwiki, doomworld, URL, local

## Phase 5: GUI (`caco-gui`)

Built with `egui` + `eframe`. Immediate-mode GUI — different from Qt's signal/slot model but clean and cross-platform.

### Core
- [ ] App struct, dark theme, styling
- [ ] Main window — tab bar, toolbar, status bar
- [ ] Window geometry save/restore

### Views
- [ ] List view (table) with context menu, vim keys
- [ ] Grid view with WAD cards (thumbnail + title + author + status)
- [ ] Detail panel — thumbnail, metadata, stats, action buttons
- [ ] Filter bar with debounce
- [ ] Sort controls

### Tabs
- [ ] Library tab — filter + sort + list/grid + detail panel
- [ ] Import tab — 5 source panes

### Dialogs
- [ ] WAD edit form
- [ ] Delete confirmation
- [ ] WAD unavailable (open source page / link local file)
- [ ] Session history
- [ ] Library statistics
- [ ] Per-map stats with import/export
- [ ] Cache management
- [ ] IWAD/id24 resource management

### Workers
- [ ] Async sourceport launch
- [ ] Search workers (idgames, doomwiki)
- [ ] Import workers
- [ ] Thumbnail loading (async, two-tier cache)

### Thumbnails
- [ ] TITLEPIC extraction from WAD files + Doom patch decoder
- [ ] Doom Wiki image scraping via MediaWiki API
- [ ] Filesystem thumbnail cache
- [ ] Async thumbnail loader

## Tests

Port tests alongside each phase. Rust test conventions:
- Unit tests inline (`#[cfg(test)] mod tests`)
- Integration tests in `crates/*/tests/`
- Shared test fixtures (in-memory SQLite, factory functions)

- [ ] Core: DB CRUD, query parser, migrations, config, WAD parsing, IWAD detection, complevel
- [ ] Sources: API client mocking, import service, parsers
- [ ] CLI: command integration tests (assert on stdout)
- [ ] TUI: widget unit tests where possible
- [ ] GUI: smoke tests
