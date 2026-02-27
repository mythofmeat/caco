# caco

A personal Doom WAD library manager taking inspiration from beets. Track what you've played, what you want to play, and download WADs on-demand.

## Features

- **Import WADs from multiple sources**
  - idgames archive
  - Doom Wiki (doomwiki.org)
  - Doomworld forums (with optional LLM-powered metadata extraction)
  - URLs / local files

- **Lazy downloads**
  - WADs from idgames are downloaded and cached when you play

- **Library tracking**
  - Status (to-play, backlog, playing, finished, abandoned, awaiting-update)
  - Version tracking for WIP/beta releases
  - Ratings
  - Custom Tags
  - Arbitrary Notes

- **Completion tracking**
  - Track how many times you've beaten each WAD
  - Auto-track per-map stats from sourceport output

- **Playtime tracking**
  - Automatically tracks how long you play each WAD

- **IWAD management**
  - Register IWADs with family/variant model (e.g., doom2/v1.9, doom2/bfg)
  - Multiple variants per family with configurable priority resolution
  - Import IWADs from files or directories with MD5-based identification
  - Auto-detect required IWAD from WAD file contents (TNT, Plutonia, Doom 1 vs 2)
  - Freedoom fallback when primary IWAD is unavailable
  - Auto-link to registered IWADs on Doom Wiki import

## Installation

Requires Python 3.10+.

```bash
python -m venv .venv
pip install -e .
```

## Quick Start

```bash
# Set your sourceport (opens config in your editor)
caco config --edit

# Import a WAD (auto-detects source type)
caco import "scythe 2"                              # Search idgames
caco import 19509                                   # idgames ID
caco import https://doomwiki.org/wiki/Eviternity   # Doomwiki URL
caco import https://www.doomworld.com/forum/topic/134292-myhousewad/  # Doomworld forum
caco import ~/Downloads/map.wad                     # Local file

# List your library
caco ls

# Play a WAD (interactive picker if multiple match)
caco play scythe

# Update status after playing
caco modify scythe status=finished rating=5
caco modify scythe status=f rating=5   # Short form (f=finished)
```

## GUI (Graphical User Interface)

Launch a desktop GUI with a dark Doom-inspired theme, grid/list views, and WAD thumbnails:

```bash
# Install with GUI dependencies
pip install -e '.[gui]'

# Launch
caco --gui
```

### Features

- **Dark Doom-inspired theme** with reds, greens, and browns
- **Hybrid views**: Toggle between list (table) and grid (card thumbnails) views
- **Tab-based filtering**: All, Playing, To-Play, Finished, Backlog, Other, Import
- **Detail panel**: Thumbnail, metadata, stats, tags, and action buttons
- **WAD thumbnails**: Extracted from TITLEPIC in WAD files, scraped from Doom Wiki, or generated as colored placeholders
- **Import from all sources**: idgames, Doom Wiki, Doomworld, URLs, local files
- **Per-map stats import/export**: Import stats.txt or levelstat.txt files and attach to completions, export stats back to text — accessible from detail panel "Map Stats" button or right-click context menu
- **WAD unavailable dialog**: When a WAD has no cached file, offers to open the source page, link a local file, or cancel
- **Window state persistence**: Window position, size, and splitter state saved between sessions

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+F` | Focus filter input |
| `F5` | Refresh library |
| `Ctrl+S` | Library statistics |
| `Ctrl+K` | Cache management |
| `Alt+1-9` | Switch tabs |
| `Escape` | Clear filter |

### Configuration

Add a `[gui]` section to `~/.config/caco/config.toml`:

```toml
[gui]
default_tab = "all"
default_sort = "id"
default_sort_desc = false
default_view = "list"      # "list" or "grid"
window_width = 1200
window_height = 800
detail_panel_width = 350
show_detail_panel = true
thumbnail_size = 128
```

## TUI (Terminal User Interface)

Launch an interactive terminal interface with vim-style navigation:

```bash
caco --tui
```

### Tabbed Interface

The TUI features a tabbed interface for quick filtering:

| Tab | Description |
|-----|-------------|
| **All** | Complete library view |
| **Playing** | WADs with status "playing" |
| **To-Play** | WADs with status "to-play" |
| **Finished** | WADs with status "finished" |
| **Backlog** | WADs with status "backlog" |
| **Other** | Abandoned + awaiting-update WADs |
| **Import** | Import from multiple sources |

Use `Tab` key to switch between tabs.

### Key Bindings

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Switch between tabs |
| `j/k` | Navigate up/down in list |
| `gg/G` | Jump to top/bottom |
| `Ctrl+d/u` | Page down/up |
| `Enter` | Play selected WAD / Import (in Search tab) |
| `/` or `f` | Focus filter input (live filtering as you type) |
| `Escape` | Clear filter and return to list |
| `i` | View WAD details |
| `e` | Edit WAD metadata |
| `h` | View session history |
| `d` | Delete WAD (moves to trash with confirmation) |
| `s` | Enter status mode |
| `o` | Open sort dropdown |
| `O` | Toggle sort direction (↑/↓) |
| `r` | Cycle rating (0→1→2→3→4→5→0) |
| `R` | Clear rating |
| `+` / `-` | Increment/decrement beaten count |
| `M` | View per-map stats (if stats attached) |
| `T` | Toggle trash view (All tab only) |
| `u` | Restore WAD from trash (in trash view) |
| `S` | Open library stats screen |
| `C` | Open cache management screen |
| `P` | Toggle info panel visibility |
| `q` | Quit/go back |

### Sort Dropdown

Each library tab includes a sort dropdown in the header. Press `o` to focus it, or click to select a sort field. Use `O` to toggle ascending/descending order.

Sort fields: ID, Title, Author, Playtime, Last Played, Year, Rating.

### Edit Screen

Press `e` on any WAD to open the edit form with fields for:
- **Basic Info**: Title, Author, Year, Status, Rating, Tags
- **Text Fields**: Notes, Description
- **Launch Config**: Custom IWAD, Sourceport, Extra Args

Save with `Ctrl+S`, cancel with `Escape`.

### Import Tab

The Import tab provides access to all import sources in one place. Use the source selector at the top or press `1-5` to switch between sources:

| Source | Description |
|--------|-------------|
| **idgames** | Search the idgames archive |
| **Doomwiki** | Search Doom Wiki for WAD pages |
| **Doomworld** | Import from Doomworld forum threads (paste URL) |
| **URL** | Manual entry with title and download URL |
| **Local** | Import local WAD files |

**Search-based sources (idgames, Doomwiki):**
1. Enter a search query and press Enter or click Search
2. Browse results in the table, preview details on the right
3. Press `Enter` to import the selected WAD

**URL-based source (Doomworld):**
1. Paste a forum thread URL and click Fetch
2. Edit pre-filled metadata if needed
3. Press `Ctrl+Enter` or click Import

**Form-based sources (URL, Local):**
1. Fill in the form fields (title is required)
2. Press `Ctrl+Enter` or click Import

Library tabs automatically refresh with new imports.

### Status Mode

Press `s` then one of:
- `p` - playing
- `f` - finished
- `t` - to-play
- `b` - backlog
- `a` - abandoned
- `w` - awaiting-update

### Filter Bar

The search filter supports live filtering as you type (debounced). The status column is color-coded by status type.

The filter supports the same beets-style query syntax as `caco ls`.

## Usage

### Importing WADs

```bash
# Auto-detect source type (default behavior)
caco import "sunlust"                              # Search idgames
caco import 19509                                  # idgames ID
caco import https://doomwiki.org/wiki/Eviternity  # Doomwiki URL
caco import ~/Downloads/mymap.wad                 # Local file (title inferred)
caco import https://example.com/wad.zip -t "My WAD"  # URL

# Force a specific source with flags
caco import "sunlust" --idgames             # Force idgames search
caco import "Scythe" --doomwiki             # Force Doom Wiki search
caco import https://www.doomworld.com/forum/topic/134292-myhousewad/ --doomworld
caco import URL --doomworld --smart         # Use LLM for metadata extraction
caco import URL --doomworld --smart --llm-backend openrouter  # Specific LLM backend
caco import URL --doomworld --smart --llm-model gpt-4o        # Override LLM model
caco import --url https://example.com/wad.zip -t "My WAD" -a "Author"
caco import *.wad --local --tag new         # Batch local import

# Duplicate detection - caco warns if WAD already exists
caco import 19509                   # "Already in library" warning
caco import 19509 --force           # Import anyway

# Interactive selection with fzf (if installed)
caco import scythe                  # Opens fzf fuzzy picker
caco import "doom 2" --multi        # Multi-select mode

# Link a downloaded file to a metadata-only entry (e.g., Doomwiki import)
caco modify id:73 --link ~/Downloads/heartland.wad
```

### Managing Library

```bash
# List all WADs (sorted by ID ascending by default)
caco ls

# Inline sort (append + for ascending, - for descending)
caco ls playtime-                       # Most played first
caco ls rating-                         # Highest rated first
caco ls title+                          # Alphabetical (A-Z, ascending)
caco ls title-                          # Reverse alphabetical (Z-A)
caco ls last_played-                    # Recently played first

# Available sort fields: id, playtime, rating, created, title, author, last_played, year
# Suffix + for ascending, - for descending

# Search with beets-style queries
caco ls scythe                          # Free text (title/author/description)
caco ls id:1                            # By database ID
caco ls title:"scythe"                  # By title (or name:)
caco ls author:"erik alm"               # By author
caco ls year:2020                       # By year
caco ls filename:scythe2                # By filename
caco ls tag:megawad                     # By tag (supports globs: tag:caco*)
caco ls status:playing                  # By status (shortcuts: status:p)
caco ls source:idgames                  # By source type (idgames, doomwiki, url, local)
caco ls iwad:doom2                      # By IWAD (matches custom_iwad)
caco ls author:alm title:scythe         # Combine filters (AND logic)

# Combine query + sort
caco ls status:playing playtime-        # Playing WADs, most played first

# OR queries (comma with spaces)
caco ls "status:playing , status:to-play"   # Match either status
caco ls "tag:megawad , tag:cacoward"        # Match either tag

# Negation (use ^ prefix to exclude)
caco ls ^status:finished                # Exclude finished WADs
caco ls status:playing ^tag:slaughter   # Playing but not slaughter-tagged

# List tags with counts
caco ls --tags                          # Rich table of all tags with counts
caco ls --tags -o plain                 # TSV output for scripting

# List registered IWADs
caco ls --iwad                          # IWAD registry table

# View details (by ID or query)
caco info id:1
caco info filename:tnto
caco info "TNT: Overcharged"
caco info tag:megawad                   # Multiple matches shown in sequence

# Modify metadata (beets-style field=value syntax)
caco modify id:1 status=playing
caco modify tag:megawad rating=5 --yes         # Query with confirmation skip
caco modify id:1 rating=4 notes="Great level design"

# Edit core metadata
caco modify id:1 title="My Custom Title"
caco modify id:1 author="John Romero" year=1994
caco modify id:1 description="A classic megawad"
caco modify id:1 version="v1.0"                # Track version for non-idgames releases

# Clear fields (! prefix)
caco modify id:1 !author !year                 # Clear optional fields
caco modify id:1 !description                  # Clear description

# Tag management via modify
caco modify id:1 tag=megawad tag=slaughter      # Add tags
caco modify id:1 !tag                           # Remove all tags
caco modify id:1 !tag:slaughter                 # Remove specific tag
caco modify id:1 "!tag:caco*"                   # Remove tags matching glob

# Link a local file to a metadata-only entry
caco modify id:1 --link ~/Downloads/heartland.wad

# Trash (soft delete - can be restored)
caco trash id:1                                 # Move to trash
caco trash status:abandoned --yes               # Trash with confirmation skip
caco trash id:1 --dry-run                       # Preview what would be trashed

# View and restore from trash
caco trash --list                               # Show trashed WADs
caco trash --restore id:1                       # Restore from trash
caco trash --purge --yes                        # Empty trash (permanently delete all)
caco trash --purge id:1 --yes                   # Permanently delete specific WAD

# Remove IWADs via trash
caco trash --iwad doom2/bfg                     # Remove BFG variant
caco trash --iwad doom2                         # Remove all doom2 variants
```

### Playing

```bash
# Play a WAD by ID
caco play 1

# Play by query (auto-select first match with --first/-1)
caco play filename:tnto
caco play "TNT: Overcharged"
caco play --first scythe               # Auto-select first match (scripting)

# Use a specific sourceport
caco play 1 --sourceport /usr/bin/dsda-doom

# Pass extra args to sourceport
caco play 1 -- -warp 15 -skill 4

# Play an IWAD directly (no PWAD needed)
caco play --iwad doom2
caco play --iwad doom2 -- -warp 1
caco play --iwad doom2 -p gzdoom
caco play --iwad doom2/v1.9            # Exact variant
```

### Per-WAD Custom Config

Set WAD-specific IWAD, sourceport, or extra arguments:

```bash
# Set custom IWAD for a WAD
caco modify id:1 iwad=tnt

# Set custom sourceport
caco modify id:1 sourceport=dsda-doom

# Set custom arguments
caco modify id:1 args="-complevel 2 -warp 1"

# Clear custom settings
caco modify id:1 !iwad !sourceport !args
```

Priority: CLI arguments > Per-WAD config > Global config

### Cross-Source Downloading

WADs imported from non-idgames sources (Doomwiki, Doomworld, etc.) can be linked to an idgames file ID for auto-downloading:

```bash
# Set idgames file ID for a WAD imported from another source
caco modify Eviternity idgames-id=19509

# Now `caco play` will auto-download from idgames
caco play "Eviternity"

# Clear the idgames ID
caco modify Eviternity !idgames-id
```

### Completion Tracking

Track how many times you've beaten each WAD, with optional notes, stats, and backdating:

```bash
# Add completions (via modify with beaten+ syntax)
caco modify id:1 beaten+1                              # Mark as beaten once
caco modify id:1 beaten+3                              # Add 3 completions at once
caco modify id:1 beaten+1 --notes "UV max"             # With notes
caco modify id:1 beaten+1 --date 2024-06-15            # Backdate
caco modify id:1 beaten+1 -s stats.txt                 # With stats file attached
caco modify id:1 beaten+1 status=finished              # Combine with status change

# Remove completions
caco modify id:1 beaten-1                              # Remove most recent
caco modify id:1 beaten-2024-06-15T18:30:00            # Remove by timestamp

# Set exact count
caco modify id:1 beaten=5                              # Set to 5 completions
caco modify id:1 beaten=0                              # Reset to 0

# View completion history (shown in info output)
caco info id:1                                         # Shows completions section
caco info id:1 -o json                                 # Includes completions array
```

### Per-Map Statistics

Import and view per-map statistics from sourceport stats files:

```bash
# Attach stats when adding a completion
caco modify id:1 beaten+1 -s ~/path/to/stats.txt

# Attach stats to an existing completion (standalone --stats-file)
caco modify id:1 -s stats.txt                          # Attach to most recent
caco modify id:1 -s stats.txt -b 2024-06-15            # Attach to specific completion

# View per-map statistics (via info --levelstats)
caco info id:1 --levelstats                            # All entries (live + completions)
caco info id:1 --levelstats --live                     # Live stats only
caco info id:1 --levelstats -b 2024-06-15              # Specific completion
caco info id:1 --levelstats --plain                    # TSV output for scripting
```

**Supported formats:**
- **nyan-doom/dsda-doom `stats.txt`**: Persistent per-map tracking with kills, items, secrets, time, skill level, exit count, and best-of stats
- **dsda-doom `levelstat.txt`**: Human-readable output from the `-levelstat` flag with per-map time, kills, items, and secrets

Format is auto-detected. Exported files are lossless round-trips of the original.

### Automatic Stats Tracking

When per-WAD data directories are enabled (default), caco automatically reads stats after each play session:

1. After the sourceport exits, caco searches the WAD's data directory for `stats.txt` or `levelstat.txt`
2. The stats are parsed and stored as a live snapshot on the WAD record
3. When you add a completion (`caco modify id:1 beaten+1` or `caco modify id:1 status=finished`), the snapshot is automatically archived to the completion record

This means you don't need to manually run `--stats-file` — just play and mark as beaten.

```bash
caco play 1                         # Stats auto-tracked after session
caco modify id:1 beaten+1           # Auto-attaches stored stats to completion
caco info 1 --levelstats            # View the per-map stats
```

**Opt-out:** Set `auto_stats = false` in config to disable auto-tracking.

### Library Statistics

```bash
caco stats                         # Overview: playtime, completion rate, status breakdown
caco stats --period year           # Group activity by year (default: month)
caco stats --limit 6               # Show last 6 periods (default: 12)
caco stats --plain                 # Key=value output for scripting
```

### Random WAD

Pick a random WAD for scripting:

```bash
caco random                        # Random WAD from entire library (prints ID)
caco random status:to-play         # Random to-play WAD
caco random --info                 # Print ID, title, and author (TSV)
caco play $(caco random)           # Play a random WAD
caco play $(caco random tag:megawad)  # Play a random megawad
```

### Cache Management

WADs from idgames are downloaded on-demand and cached locally. Manage your cache with these commands:

```bash
# View cache contents
caco cache list                      # List cached files with sizes
caco cache list --orphans            # Show orphaned files (not in database)

# Clear cache
caco cache clear --all               # Clear entire cache
caco cache clear 1,3,5               # Clear specific WADs
caco cache clear status:finished     # Clear finished WADs' cache
caco cache clear --all --dry-run     # Preview what would be deleted

# Prune orphaned files
caco cache prune                     # Remove files not tracked in database
caco cache prune --dry-run           # Preview orphan cleanup
```

**Auto-cleanup**: Configure automatic cache cleanup in config.toml:

```toml
# Remove cached files not played in 30 days
cache_max_age_days = 30

# Keep cache under 5 GB (removes least recently played)
cache_max_size_gb = 5

# Enable auto-cleanup before downloading new WADs
cache_auto_clean = true
```

**Note**: Only idgames sources are affected by cache commands. Local imports and URL imports are never deleted (they may not be re-downloadable).

## IWAD Management

IWADs are organized by **family** (doom, doom2, plutonia, tnt) with multiple **variants** per family (v1.9, bfg, enhanced, kex). Resolution uses a configurable priority list to pick the preferred variant. Managed IWADs are stored as `iwads/{variant}/{family}.wad` so sourceports see canonical filenames (e.g., `tnt.wad`).

```bash
# Import IWADs (auto-detects family + variant by MD5)
caco import ~/games/doom2.wad                   # Auto-detected as IWAD
caco import ~/iwads/                             # Scan directory for IWADs
caco import ~/iwads/ --yes                       # Import all without prompting

# List registered IWADs (* marks preferred variant)
caco ls --iwad
caco ls --iwad -o plain                          # TSV output for scripting

# Remove a specific variant or all variants of a family
caco trash --iwad doom2/bfg                      # Remove just BFG variant
caco trash --iwad doom2                          # Remove all doom2 variants (with warning)
```

Once registered, IWADs can be referenced by family name anywhere — the preferred variant is resolved automatically:

```bash
# Global config
iwad = "doom2"               # Resolves to preferred variant's path

# Per-WAD config
caco modify id:1 iwad=doom2  # Uses preferred variant's path
```

### Variant Priority

The default priority prefers original releases over newer ports:

| Family | Priority Order |
|--------|------|
| doom | v1.9ud, v1.9, bfg, enhanced, kex |
| doom2 | v1.9, bfg, enhanced, kex |
| plutonia | v1.9, v1.9alt, unity, kex |
| tnt | v1.9, v1.9alt, unity, kex |

Override priority per-family in config:

```toml
[iwad_priority]
doom2 = ["bfg", "v1.9", "enhanced", "kex"]
```

Freedoom is used as a cross-family fallback (freedoom2 for doom2/plutonia/tnt, freedoom1 for doom) when no variant of the requested family is registered.

Doom Wiki imports automatically set `custom_iwad` when the entry's IWAD field matches a registered IWAD.

### Auto-Detection

On first play, caco inspects the WAD file to auto-detect the required IWAD:

1. **PNAMES analysis** — checks for texture patches unique to TNT: Evilution or The Plutonia Experiment (strongest signal, unambiguous)
2. **Map name format** — `E1M1`-style lumps indicate Doom 1, `MAP01`-style indicates Doom 2

The detected IWAD is saved to `custom_iwad` so detection only runs once per WAD. Self-contained WADs that bundle their own patches are correctly handled (won't trigger false detection).

**Opt-out:** Set `auto_detect_iwad = false` in config to disable auto-detection.

## Configuration

Config file: `~/.config/caco/config.toml` (see `config.example.toml` for a template).
New config keys are automatically added with default values when you update caco.

### Available Options

| Option | Description |
|--------|-------------|
| `sourceport` | Default sourceport (name on PATH or full path) |
| `iwad` | Default IWAD (path or short name with `iwad_dirs`) |
| `iwad_dirs` | Directories to search for IWADs |
| `sourceport_args` | Default args passed to sourceport |
| `db_path` | Path to the library database file |
| `cache_dir` | WAD cache directory |
| `download_mirror` | Preferred idgames mirror (0-4) |
| `cache_max_size_gb` | Max cache size in GB (0 = unlimited) |
| `cache_max_age_days` | Remove files not played in N days (0 = never) |
| `cache_auto_clean` | Auto-cleanup cache on play (true/false) |
| `manage_data_dirs` | Manage per-WAD data directories (default: true) |
| `data_dir` | Base directory for per-WAD data (default: `~/.local/share/caco/data/`) |
| `auto_stats` | Auto-track per-map stats after play sessions (default: true) |
| `auto_detect_iwad` | Auto-detect required IWAD from WAD file contents (default: true) |
| `[list] format` | Columns to display (see config example) |
| `[list] sort` | Default sort order |
| `[list] default_status` | Default status filter (empty = all statuses) |

### Example Config

```toml
sourceport = "nyan-doom"
iwad = "doom2"
iwad_dirs = ["/usr/share/games/doom", "~/.steam/steam/steamapps/common/Doom 2/base"]
sourceport_args = ["-nomusic"]
db_path = "~/.local/share/caco/library.db"
cache_dir = "~/.local/share/caco/wads"
download_mirror = 0

# Customize list display
[list]
# Available columns: id, title, author, status, beaten, playtime,
#                    last_played, rating, year, tags, source, filename
format = ["id", "title", "author", "status", "playtime", "tags"]

# Sort options: id, title, author, status, playtime, rating, year,
#               last_played, created, beaten
# Append + for ascending (default), - for descending
sort = "id+"

# Default status filter (empty shows all statuses)
# default_status = ["to-play", "playing"]

[list.colors]
# Available colors: black, red, green, yellow, blue, magenta, cyan, white, dim
to-play = "blue"
backlog = "yellow"
playing = "green"
finished = "dim"
abandoned = "red"
awaiting-update = "magenta"

[tui]
# Default tab: all, playing, to-play, finished, backlog, other
default_tab = "all"
# Default sort: id, title, author, playtime, last_played, year, rating
default_sort = "id"
default_sort_desc = false
```

## Command Aliases

| Alias | Full Command |
|-------|--------------|
| `caco i` | `caco info` |

## Status Shortcuts

Use single letters or abbreviations for status values:

| Shortcut | Status |
|----------|--------|
| `t`, `tp`, `toplay` | to-play |
| `b`, `back` | backlog |
| `p`, `play` | playing |
| `f`, `fin`, `done` | finished |
| `a`, `drop`, `dropped` | abandoned |
| `w`, `au`, `await`, `waiting`, `wip` | awaiting-update |

```bash
caco modify id:1 status=p   # Set status to "playing"
caco ls status:f             # List finished WADs (query syntax)
```

### CLI Commands

```bash
# View config file contents (pipeable)
caco config

# Open config in $EDITOR
caco config --edit
caco config -e
```

## Shell Completions

Generate and install shell completions:

```bash
# Generate completions (auto-detects shell from $SHELL)
caco completions

# Generate for specific shell
caco completions fish
caco completions bash
caco completions zsh

# Install completions to shell config
caco completions --install
caco completions fish --install
```

Manual installation:

```bash
# Fish
cp completions/caco.fish ~/.config/fish/completions/

# Bash — add to ~/.bashrc
source ~/.local/share/bash-completion/completions/caco
# Or: cp completions/caco.bash ~/.local/share/bash-completion/completions/caco

# Zsh — add to ~/.zshrc: fpath=(~/.zfunc $fpath) && compinit
cp completions/_caco ~/.zfunc/_caco
```

## Scripting

Use `-o plain` for TSV output or `-o json` for structured data:

```bash
# List WADs as TSV (tab-separated)
caco ls -o plain
# Output: ID	Title	Author	Status	Beaten	Playtime	LastPlayed

# Get WAD info as key=value pairs
caco info 1 -o plain
# Output: id=1
#         title=Scythe 2
#         author=Erik Alm
#         ...

# JSON output (includes all fields, computed stats, tags)
caco ls -o json
caco ls status:playing -o json
caco info 1 -o json

# Stats output for scripting
caco stats --plain                  # Key=value output

# Tag list shows WAD counts
caco ls --tags                      # Rich table with Tag + Count columns
caco ls --tags -o plain             # TSV output for scripting

# Random WAD with metadata
caco random --info                  # Prints ID, title, author (TSV)
```

## Per-WAD Data Directories

When playing a WAD, caco automatically creates an isolated data directory for each WAD's saves, stats, and other sourceport output. This prevents data conflicts between WADs (e.g., stats.txt getting mixed, save files piling up in one folder).

**How it works:**
- On play, caco injects `-data`/`-save` flags (or `-savedir` for GZDoom-family ports) to redirect sourceport output
- Each WAD gets its own directory: `~/.local/share/caco/data/{id}_{title}/`
- For dsda-family ports, `-save` points to the nested stats directory (`{exe}_data/{iwad}/{wad_stem}/`) so saves live alongside per-map stats
- Sourceport family detection is automatic based on the executable name

**Supported sourceports:**

| Family | Executables | Flags |
|--------|------------|-------|
| dsda | dsda-doom, nyan-doom, nugget-doom, prboom+, glboom+ | `-data`, `-save` |
| zdoom | gzdoom, lzdoom, vkdoom, qzdoom, zdoom | `-savedir` |
| chocolate | chocolate-doom, crispy-doom | `-savedir` |
| woof | woof | `-data`, `-save` |
| eternity | eternity | `-savedir` |

Unknown sourceports play normally without any injection.

**Opt-out:** Set `manage_data_dirs = false` in config to disable data directory management.

## Data Storage

*Default locations:*

- **Database**: `~/.local/share/caco/library.db`
- **Managed IWADs**: `~/.local/share/caco/iwads/{variant}/{family}.wad`
- **Config**: `~/.config/caco/config.toml`
- **WAD cache**: `~/.local/share/caco/wads/`
- **WAD data**: `~/.local/share/caco/data/` (per-WAD saves, stats, configs)
- **Thumbnail cache**: `~/.cache/caco/thumbnails/`

## Development

```bash
# Install in development mode with test dependencies
pip install -e '.[test]'

# Run tests
pytest tests/ -v

# Run tests with coverage
pytest tests/ -v --cov=caco --cov-report=term-missing

# Type checking (mypy)
mypy src/caco
```

### CI

GitHub Actions runs on every push/PR:
- **Tests**: Python 3.10, 3.11, 3.12 matrix
- **Type checking**: mypy on Python 3.12 with `[test,gui]` extras

## License

MIT
