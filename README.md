# caco

A personal Doom WAD library manager inspired by [beets](https://beets.io). Import WADs from multiple sources, track what you've played, and launch them with your preferred sourceport — all from the command line, a terminal UI, or a desktop GUI.

## Features

- **Import from anywhere** — idgames archive, Doom Wiki, Doomworld forums, URLs, or local files. Auto-enriches with Doom Wiki metadata.
- **Smart queries** — beets-style filters (`status:in-progress`, `tag:megawad`, `author:"erik alm"`) with OR, negation, and glob support.
- **Play tracking** — automatic playtime, session history, per-map stats (kills/items/secrets/time), and completion counts.
- **IWAD management** — register your IWADs once, and caco auto-detects which one each WAD needs.
- **Companion files** — manage DEH patches, music WADs, and other companion files with automatic deduplication.
- **Per-WAD isolation** — saves, stats, and configs are separated per WAD so nothing gets mixed up.
- **On-demand downloads** — idgames WADs are cached when you play, with configurable auto-cleanup.
- **Garbage collection** — reclaim disk space from completed / abandoned WADs with smart cleanup.
- **Three interfaces** — CLI, TUI (ratatui), and GUI (egui with thumbnails and grid/list views).
- **MCP server** — expose the library to Claude or other MCP clients via a sandboxed read/write interface (`caco-mcp`).
- **Smart collections** — save queries by name and re-run them (`caco collection add`).

## Installation

### From source (Rust)

```bash
git clone https://github.com/evansheen/caco && cd caco

# Build all binaries
cargo build --release

# Install CLI
cargo install --path crates/caco-cli

# Install GUI
cargo install --path crates/caco-gui
```

### From source (Python — legacy)

Requires Python 3.10+.

```bash
pip install -e .
pip install -e '.[gui]'  # Optional: Qt6 GUI
```


## Quick Start

```bash
# 1. Configure your sourceport
caco config --edit

# 2. Import some WADs
caco import "scythe 2"                 # Search idgames
caco import ~/Downloads/map.wad        # Local file
caco import https://doomwiki.org/wiki/Eviternity

# 3. Browse and play
caco ls
caco play scythe

# 4. Track progress
caco modify scythe status=completed rating=5
caco modify scythe beaten+1
```

## Interfaces

### CLI

The primary interface. Every command supports `--help` for detailed usage.

```bash
caco ls                            # List library
caco ls status:in-progress playtime-  # Filter + sort
caco info "Eviternity"             # WAD details
caco modify id:1 status=p tag=megawad rating=4
caco play 1 -- -warp 15 -skill 4   # Play with extra args
caco sessions "Eviternity"         # Session history
caco stats                         # Library statistics
caco enrich --cacowards --year 2023  # Scrape that year's Cacowards from Doom Wiki
caco stats --cacowards             # Year × category completion grid
caco stats --cacowards --year 2023 -o json  # Per-entry detail as JSON
caco ls cacoward:2023 cacoward:winner status:unplayed  # Browse unplayed 2023 winners
caco import --cacoward c.2023.winner.1   # Import the Nth winner of YEAR
```

### TUI

Terminal interface with vim-style navigation, tabbed filtering, and live search.

```bash
caco-tui
```

Key bindings:

- **Navigate:** `j`/`k`, `gg`/`G`, `Ctrl-d`/`Ctrl-u`
- **Filter / sort:** `/` or `f` filter, `o` cycle sort, `O` reverse
- **Actions:** `Enter` play, `i` info, `e` edit, `d` delete, `h` sessions, `M` map stats
- **Status:** `s`, then `u` / `p` / `c` / `a`
- **Rating / beaten:** `r` cycle rating, `R` clear, `+` / `-` adjust beaten count
- **Screens:** `S` stats, `C` cache, `W` resources, `A` Cacowards, `Tab`/`Shift-Tab` switch tabs
- **Trash view:** `T` toggle, `u` untrash
- **Help / quit:** `?` help, `q` quit

### GUI

Desktop application with a dark Doom-inspired theme. The left sidebar's
`Cacowards` entry opens a magazine-style year-by-year view that doubles as
a "what should I play / import next?" dashboard — entries you don't own
yet are flagged `absent` with an inline Import button.

```bash
caco-gui
```

- Grid and list views with sortable columns and a live filter bar
- WAD thumbnails scraped from the Doom Wiki (or extracted from TITLEPIC) with on-disk caching
- Right-click context menu (play, edit, delete, sessions, map stats, new playthrough)
- Right-hand detail sidebar with metadata, play stats, and quick actions
- Dialogs for editing WADs, confirming deletes, browsing sessions, viewing library stats, managing the cache, and registering IWADs / id24 WADs
- Keyboard shortcuts: `j/k`, `g`/`G`, `Home`/`End`, `Enter`, `E`, `D`, `S`, `P`, `Esc`

## Importing

`caco import` auto-detects the source type:

```bash
caco import "sunlust"              # idgames search (opens fzf picker)
caco import 19509                  # idgames file ID
caco import https://doomwiki.org/wiki/Eviternity
caco import https://www.doomworld.com/forum/topic/134292-myhousewad/
caco import ~/Downloads/mymap.wad  # Local file
caco import ~/iwads/doom2.wad      # Auto-detected as IWAD
caco import saved_search.json      # Offline JSON fallback
```

Non-Doomwiki imports are auto-enriched with Doom Wiki metadata (author, year, description, IWAD). Duplicate detection warns before re-importing.

## Queries

Caco uses beets-style query syntax across all commands (`ls`, `play`, `modify`, `trash`, etc.):

```bash
caco ls scythe                     # Free text search
caco ls title:scythe author:alm    # Field queries (AND)
caco ls "status:in-progress , status:unplayed"  # OR queries
caco ls ^status:completed          # Negation
caco ls tag:caco*                  # Glob patterns
caco ls status:p playtime-         # Query + sort (shortcut + sort)
```

**Fields:** `id`, `title`, `author`, `year`, `filename`, `tag`, `status`, `source`, `iwad`, `complevel`, `config`

**Status values:** `unplayed`, `in-progress`, `completed`, `abandoned`

**Status shortcuts:** `u` (unplayed); `p`, `ip` (in-progress); `c`, `f`, `done` (completed); `a`, `d` (abandoned)

## Managing Your Library

```bash
# Modify metadata
caco modify id:1 status=in-progress rating=4 tag=megawad
caco modify id:1 title="New Title" author="Author" year=2024
caco modify id:1 !rating              # Clear a field

# Completion tracking
caco modify id:1 beaten+1             # Mark beaten
caco modify id:1 beaten+1 --notes "UV max" --date 2024-06-15
caco info id:1 --completions          # List completion records with IDs
caco modify id:1 completion.42.notes="pacifist run"
caco modify id:1 completion.42.date=2026-04-15T15:42:00+00:00
caco modify id:1 completion.42.stats=/path/to/levelstat.txt
caco modify id:1 completion.42.stats= # Clear the attached stats

# Per-WAD launch config
caco modify id:1 iwad=tnt sourceport=dsda-doom complevel=boom
caco modify id:1 config=controller
caco modify id:1 args="-warp 1"

# Companion files (DEH patches, music WADs, etc.)
caco companion add id:1 /path/to/music.wad
caco companion ls id:1

# Smart collections (saved queries)
caco collection add megawads tag:megawad status:u --sort year
caco collection ls
caco collection run megawads
caco collection rm megawads

# Trash (soft delete with restore)
caco trash id:1
caco trash --restore id:1
```

## Playing

```bash
caco play scythe                   # Interactive picker
caco play 1 -p dsda-doom           # Specific sourceport
caco play 1 -c boom                # Override complevel
caco play 1 -C controller          # Config profile
caco play 1 --record               # Record a demo
caco play --iwad doom2             # Play IWAD directly
caco play 1 -- -warp 15 -skill 4   # Extra sourceport args
```

On first play, caco auto-detects the required IWAD and complevel from the WAD file. Per-map stats are auto-tracked after each session.

## IWADs

Register your IWADs once and reference them by family name everywhere:

```bash
caco import ~/iwads/doom2.wad      # Auto-detected by MD5
caco import ~/iwads/               # Scan a directory
caco ls --iwad                     # List registered IWADs
```

IWADs are organized by family (doom, doom2, tnt, plutonia) with variant support (v1.9, bfg, kex). The preferred variant is resolved automatically, with Freedoom as a cross-family fallback.

## Configuration

Config file: `~/.config/caco/config.toml`

```bash
caco config --edit                 # Open in $EDITOR
caco config                        # Print current config
```

### Essential Settings

```toml
sourceport = "dsda-doom"
iwad = "doom2"
iwad_dirs = ["/usr/share/games/doom"]
```

### Example Config

```toml
sourceport = "nyan-doom"
iwad = "doom2"
iwad_dirs = ["/usr/share/games/doom", "~/games/iwads"]
sourceport_args = ["-nomusic"]

auto_detect_iwad = true
auto_detect_complevel = true
auto_doomwiki_enrich = true
cache_max_size_gb = 20.0
cache_auto_clean = true

[list]
format = ["id", "title", "author", "status", "beaten", "playtime", "last_played"]
sort = "id+"

[gui]
default_view = "list"
thumbnail_size = 160

[tui]
default_tab = "all"
default_sort = "id"
```

See `config.example.toml` for all available options.

## Shell Completions

```bash
caco completions --install         # Auto-install for your shell
caco completions fish              # Generate for specific shell
```

Supports fish, bash, and zsh with dynamic completions for WAD names, tags, IWADs, and more.

## Scripting

```bash
caco ls -o plain                   # TSV output (default is `table`)
caco ls -o json                    # JSON output
caco info 1 -o json                # Structured WAD data
caco random status:unplayed        # Random WAD ID
caco play $(caco random)           # Play a random WAD
```

## Garbage Collection

```bash
caco gc                            # Clean completed/abandoned WAD data
caco gc --dry-run                  # Preview reclaimable space
caco gc --keep-saves               # Clean but keep save files
caco gc --orphans-only             # Only clean orphaned dirs/backups
caco gc --ignore id:5              # Permanently exclude from GC
```

idgames WADs are cleaned automatically (re-downloadable). Non-idgames WADs prompt individually with the option to permanently ignore.

## Data Storage

| Location | Contents |
|----------|----------|
| `~/.config/caco/config.toml` | Configuration |
| `~/.local/share/caco/library.db` | Library database |
| `~/.local/share/caco/wads/` | Cached WAD files |
| `~/.local/share/caco/data/` | Per-WAD saves, stats, configs |
| `~/.local/share/caco/iwads/` | Managed IWADs |
| `~/.local/share/caco/id24/` | Managed id24 WADs |
| `~/.local/share/caco/companions/` | Managed companion files |
| `~/.local/share/caco/sourceports/` | Per-sourceport config profiles |
| `~/.local/share/caco/backups/` | Save backups + pre-migration DB snapshots |
| `~/.cache/caco/thumbnails/` | GUI thumbnail cache |

### Recovering from a bad migration

Each time caco starts with pending schema migrations it first copies the live
library to `~/.local/share/caco/backups/pre-migration-<N>.db`, where `<N>` is
the schema version before the migration runs. Migrations themselves are
transactional, so a crash or SQL error rolls back cleanly; the file-level
snapshot is for the rarer case where a migration commits successfully but
leaves user data in a bad state. To recover:

```bash
# stop any running caco instances, then:
cp ~/.local/share/caco/library.db ~/.local/share/caco/library.db.broken
cp ~/.local/share/caco/backups/pre-migration-<N>.db ~/.local/share/caco/library.db
```

You will need a caco binary old enough to read schema version `N`. Please also
file a bug report with the broken DB attached.

## Supported Sourceports

Caco recognises six sourceport families. Family membership determines which per-WAD features caco can inject at launch time:

| Family | Members | Data dir | Save dir | Complevel |
|--------|---------|----------|----------|-----------|
| `dsda` | dsda-doom, nyan-doom, nugget-doom, prboom+, glboom+ | yes | yes | yes |
| `woof` | woof | yes | yes | yes |
| `zdoom` | uzdoom, gzdoom, lzdoom, vkdoom, qzdoom, zdoom | no | yes | — |
| `chocolate` | chocolate-doom, crispy-doom | no | yes | — |
| `eternity` | eternity | no | yes | — |
| `helion` | helion | no | yes | — |

Unknown sourceports still launch, they just skip isolation and auto-injection.

Per-map stat tracking (which feeds completion detection and progress bars) works with:

- **dsda / woof** — native `stats.txt` / `levelstat.txt` in the per-WAD data dir.
- **zdoom** — caco injects a small ZScript reporter PK3 plus `+logfile`, then converts the log into a managed `stats.txt` after each session.
- **helion** — caco passes `-levelstat` and consumes Helion's global `levelstat.txt` (from `~/.config/Helion`) into the WAD's managed `stats.txt` after each session.

## Development

```bash
# Rust (primary)
cargo test --workspace
cargo clippy --workspace -- -D warnings

# Python (legacy)
pip install -e '.[test]'
pytest tests/ -v
```

## License

MIT
