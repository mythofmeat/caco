# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Caco is a personal Doom WAD library manager inspired by `beets`. It tracks WADs you want to play, have played, or are playing, with metadata from multiple sources (idgames, Doomwiki, Doomworld forums, manual entry). Three interfaces share one workspace: a CLI (`caco`), a TUI (ratatui), and a GUI (egui). An MCP server (`caco-mcp`) exposes a sandboxed view of the library to LLMs.

Key features:
- SQLite database for WAD metadata and play history
- Import from idgames, Doom Wiki, Doomworld forums, URLs, or local files
- Automatic playtime tracking via a sourceport wrapper
- Tag-based organization and beets-style query syntax
- On-demand downloading (WADs are cached, not stored permanently)
- Optional LLM-powered metadata extraction (used by Doomworld imports)
- Completion tracking with per-map stats import/export and auto-tracking
- Companion file management with MD5 deduplication
- IWAD / id24 registry with auto-detection from WAD contents
- Sourceport config profile management
- Garbage collection for completed/abandoned WAD data

## Commands

```bash
# Build
cargo build --workspace
cargo build --release -p caco-cli    # Release CLI binary

# Run
cargo run -p caco-cli -- <command>
cargo run -p caco-tui
cargo run -p caco-gui

# Quality gates (required before commit)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Examples
cargo run -p caco-cli -- ls
cargo run -p caco-cli -- ls -o plain
cargo run -p caco-cli -- info 1 -o json
```

## Architecture

```
crates/
├── caco-core/     Core library: DB, config, detection, player, services
├── caco-sources/  API clients (idgames, Doom Wiki, Doomworld) + import service + HTTP
├── caco-cli/      Clap-based CLI; all subcommands live in src/commands/
├── caco-tui/      ratatui + crossterm TUI — tabbed library, import pane, detail/edit screens
├── caco-gui/      eframe/egui GUI — library panels, grid view, dialogs, background workers
└── caco-mcp/      rmcp MCP server — sandboxed library access for LLM agents
```

**caco-core** top-level modules:
- `config.rs` — TOML config + path resolution + `ensure_config_keys` autofill
- `player.rs` — sourceport launcher, playtime tracking, companion injection
- `companion_service.rs`, `resource_service.rs` — MD5 dedup + managed storage; IWAD/id24 registration
- `sourceports.rs`, `complevel.rs`, `complevel_detect.rs`, `iwad_detect.rs` — family registry + detection heuristics (COMPLVL, UMAPINFO, DEHACKED, PNAMES, map lumps)
- `wad_stats.rs` — per-map stats parser (stats.txt + levelstat.txt)
- `saves.rs`, `demos.rs`, `titlepic.rs`, `utils.rs`
- `db/` — schema, migrations (23+), models, query parser, wads, sessions, iwads, id24, companions

**caco-sources** modules:
- `http.rs` — shared reqwest blocking client + WAF challenge helpers
- `import_service.rs` — central import entry points for all sources + auto-enrichment
- `json_import.rs` — offline JSON fallback for Cloudflare-blocked APIs
- `idgames/`, `doomwiki/`, `doomworld/` — per-source API clients + parsers

**caco-cli**: `main.rs` sets up clap + DB; `output.rs` renders table/plain/JSON; `picker.rs` is the fzf-style selector; `resolve.rs` handles WAD resolution; `parsing.rs` handles modify/sort parsing. Each subcommand owns a file in `src/commands/`.

**caco-tui**: `app.rs` drives the event loop and screen stack. Screens live in `src/screens/`; shared widgets (wad_table, wad_info, filter_input, library_pane, import_pane, …) in `src/widgets/`. Background work flows through an mpsc channel drained each tick.

**caco-gui**: `app.rs` hosts the `CacoApp` state machine. Panels in `src/panels/`, dialogs in `src/dialogs/`, import flow in `src/import/`. `thumbnails.rs` extracts and caches TITLEPIC; `wiki_scraper.rs` fetches Doom Wiki thumbs; `workers.rs` coordinates background search/import/play via mpsc channels.

**caco-mcp**: `server.rs` hosts the rmcp `ServerHandler`. `cli_tools.rs` exposes 17 `caco_*` tools that shell out to a sandboxed `caco` binary; `introspect.rs` adds 7 `inspect_*` read-only tools plus `run_sql`. `sandbox.rs` enforces that writes only land in the sandbox copy.

## Dependencies (key crates)

```toml
rusqlite = "0.34"       # SQLite (bundled)
serde / serde_json
toml = "0.8"
chrono = "0.4"
thiserror = "2"
regex = "1"
md-5 = "0.10"           # companion dedup
zip = "2"
image = "0.25"          # thumbnails
clap = "4"              # CLI
comfy-table = "7"
indicatif = "0.17"
reqwest = "0.12"        # HTTP (blocking)
ratatui = "0.29"        # TUI
crossterm = "0.28"
eframe = "0.31"         # GUI
egui = "0.31"
```

## Key Patterns

- **Builder pattern for DB writes**: `NewWad::builder()` and `WadUpdate::builder()` produce type-safe WAD creation/updates.
- **Batch stats**: `get_total_playtime_batch()`, `get_last_played_batch()`, etc. — avoid N+1 queries when rendering lists.
- **Query parser**: beets-style syntax — see Behavior below.
- **Companion system**: `companion_files_registry` + `wad_companions` junction table. `companion_service.rs` handles MD5 dedup + managed storage at `~/.local/share/caco/companions/{md5[:12]}_{filename}`. DEH/BEX auto-detected; `-deh` for non-zdoom, `-file` for zdoom.
- **GC**: `gc.rs` handles completed/abandoned cleanup with interactive `y/n/i` prompts; `gc_ignore` column for exclusion; orphan detection for data dirs, backups, and companions.
- **Import service**: centralises duplicate checking for all sources; auto-enriches with Doom Wiki metadata; JSON import fallback for Cloudflare-blocked APIs.
- **Player**: wraps sourceport execution; injects companion files, data dir args, complevel args, config profile; returns `PlayResult` with crash detection.
- **GUI background work**: egui is immediate-mode; `CacoApp` holds all state; background workers for search/import/play use `std::thread` + `std::sync::mpsc`.

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

## Behavior

**Query syntax** (beets-style — used by `ls`, `play`, `modify`, `trash`, etc.):
- Fields: `id:`, `title:`, `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`, `iwad:`, `complevel:`, `config:`
- OR: `"status:in-progress , status:unplayed"` (comma with spaces)
- Negation: `^status:completed`
- Status shortcuts: `u` (unplayed), `p`/`ip` (in-progress), `c`/`f`/`done` (completed), `a`/`d` (abandoned)
- Glob patterns: `tag:caco*`
- Free text searches title, author, description

**Status enum**: `unplayed`, `in-progress`, `completed`, `abandoned`.

**IWAD detection**: PNAMES lump analysis (TNT-only 197 patches / Plutonia-only 78 patches), map lump fallback (ExMy→doom, MAPxx→doom2); self-contained WADs don't trigger detection.

**Complevel detection hierarchy**: COMPLVL lump (id24 byte or text) > UMAPINFO → 21 > DEHACKED+MBF → 11 > DEHACKED+ExMy → 2 > DEHACKED+MAPxx → 4 > ExMy → 2 > MAPxx → 4.

**Sourceport families**: dsda / zdoom / chocolate / woof / eternity / helion / uzdoom. Each maps to data dir args, save dir args, complevel args, and config args.

**Per-WAD config columns**: `custom_iwad`, `custom_sourceport`, `custom_args` (JSON), `complevel` (INT), `custom_config` (TEXT).

**DB migrations**: run on `init_db()`; numbered sequentially; current schema version is 23+.

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

- Hand-crafted scripts for fish, bash, zsh in `completions/`.
- `caco completions [shell]` emits static completions via clap_complete.
- Dynamic data via hidden `caco _complete <context>` for: wads, tags, iwads, statuses, sort-fields, sourceports, modify-fields, query-fields.

## Git Instructions

- Commit working changes to git; keep the tree green between commits.
- Update README.md and CLAUDE.md when adding or changing user-visible features.
- Quality gates before any commit: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
