# caco

A personal Doom WAD library manager inspired by [beets](https://beets.io). Import WADs from multiple sources, track what you've played, and launch them with your preferred sourceport — all from the command line, a terminal UI, or a desktop GUI.

## Features

- **Import from anywhere** — idgames archive, Doom Wiki, Doomworld forums, URLs, or local files. Auto-enriches with Doom Wiki metadata.
- **Smart queries** — beets-style filters (`status:playing`, `tag:megawad`, `author:"erik alm"`) with OR, negation, and glob support.
- **Play tracking** — automatic playtime, session history, per-map stats (kills/items/secrets/time), and completion counts.
- **IWAD management** — register your IWADs once, and caco auto-detects which one each WAD needs.
- **Per-WAD isolation** — saves, stats, and configs are separated per WAD so nothing gets mixed up.
- **On-demand downloads** — idgames WADs are cached when you play, with configurable auto-cleanup.
- **Three interfaces** — CLI, TUI (vim-style navigation), and GUI (Qt6 with thumbnails and grid/list views).

## Installation

Requires Python 3.10+.

```bash
pip install caco

# Or from source
git clone https://github.com/evansheen/caco && cd caco
pip install -e .

# Optional: GUI support (requires Qt6)
pip install -e '.[gui]'
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
caco modify scythe status=finished rating=5
caco modify scythe beaten+1
```

## Interfaces

### CLI

The primary interface. Every command supports `--help` for detailed usage.

```bash
caco ls                            # List library
caco ls status:playing playtime-   # Filter + sort
caco info "Eviternity"             # WAD details
caco modify id:1 status=p tag=megawad rating=4
caco play 1 -- -warp 15 -skill 4   # Play with extra args
caco sessions "Eviternity"         # Session history
caco stats                         # Library statistics
```

### TUI

Terminal interface with vim-style navigation, tabbed filtering, and live search.

```bash
caco --tui
```

Key bindings: `j/k` navigate, `Enter` plays, `/` filters, `e` edits, `s` sets status, `r` cycles rating, `+/-` adjusts beaten count. Press `?` or `q` to quit.

### GUI

Desktop application with a dark Doom-inspired theme, WAD thumbnails, and grid/list views.

```bash
pip install -e '.[gui]'
caco --gui
```

## Importing

`caco import` auto-detects the source type:

```bash
caco import "sunlust"              # idgames search (opens fzf picker)
caco import 19509                  # idgames file ID
caco import https://doomwiki.org/wiki/Eviternity
caco import https://www.doomworld.com/forum/topic/134292-myhousewad/
caco import ~/Downloads/mymap.wad  # Local file
caco import ~/iwads/doom2.wad      # Auto-detected as IWAD
```

Non-Doomwiki imports are auto-enriched with Doom Wiki metadata (author, year, description, IWAD). Duplicate detection warns before re-importing.

## Queries

Caco uses beets-style query syntax across all commands (`ls`, `play`, `modify`, `trash`, etc.):

```bash
caco ls scythe                     # Free text search
caco ls title:scythe author:alm    # Field queries (AND)
caco ls "status:playing , status:to-play"  # OR queries
caco ls ^status:finished           # Negation
caco ls tag:caco*                  # Glob patterns
caco ls status:p playtime-         # Query + sort
```

**Fields:** `id`, `title`, `author`, `year`, `filename`, `tag`, `status`, `source`, `iwad`, `complevel`, `config`

**Status shortcuts:** `t` (to-play), `b` (backlog), `p` (playing), `f` (finished), `a` (abandoned), `w` (awaiting-update)

## Managing Your Library

```bash
# Modify metadata
caco modify id:1 status=playing rating=4 tag=megawad
caco modify id:1 title="New Title" author="Author" year=2024
caco modify id:1 !rating              # Clear a field

# Completion tracking
caco modify id:1 beaten+1             # Mark beaten
caco modify id:1 beaten+1 --notes "UV max" --date 2024-06-15

# Per-WAD launch config
caco modify id:1 iwad=tnt sourceport=dsda-doom complevel=boom
caco modify id:1 args="-warp 1"

# Companion files (DEH patches, music WADs, etc.)
caco companion add id:1 /path/to/music.wad
caco companion ls id:1

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

[list]
format = ["id", "title", "author", "status", "playtime", "tags"]
sort = "id+"

[list.colors]
to-play = "blue"
playing = "green"
finished = "dim"

[tui]
default_tab = "all"
default_sort = "id"

[gui]
default_view = "list"
thumbnail_size = 128
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
caco ls -o plain                   # TSV output
caco ls -o json                    # JSON output
caco info 1 -o json                # Structured WAD data
caco random status:to-play         # Random WAD ID
caco play $(caco random)           # Play a random WAD
```

## Garbage Collection

```bash
caco gc                            # Clean finished/abandoned WAD data
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
| `~/.local/share/caco/backups/` | Save backups |

## Supported Sourceports

dsda-doom, nyan-doom, nugget-doom, prboom+, glboom+, gzdoom, lzdoom, vkdoom, chocolate-doom, crispy-doom, woof, eternity, Helion, uzdoom. Unknown sourceports work too — they just don't get data directory isolation or config injection.

## Development

```bash
pip install -e '.[test]'
pytest tests/ -v
```

## License

MIT
