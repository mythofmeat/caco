# caco

A personal Doom WAD library manager taking inspiration from beets. Track what you've played, what you want to play, and download WADs on-demand.

## Features

- **Import WADs from multiple sources**
  - idgames
  - Doomwiki *(planned)*
  - Doomworld forums *(planned)*

- **Lazy downloads**
  - WADs from idgames are downloaded and cached when you play

- **Library tracking**
  - Status (to-play, backlog, playing, finished)
  - Ratings
  - Custom Tags
  - Arbitrary Notes

- **Completion tracking**
  - Automatically track map and WAD completions (if using a sourceport that provides `stats.txt` files such as *dsda-doom* and its forks)

- **Playtime tracking** 
  - Automatically tracks how long you play each WAD

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
caco add "scythe 2"           # Search idgames
caco add 19509                # idgames ID
caco add ~/Downloads/map.wad  # Local file

# List your library
caco ls                       # Alias for 'list'

# Play a WAD (interactive picker if multiple match)
caco play scythe

# Update status after playing
caco update scythe --status finished --rating 5
caco update scythe -s f -r 5  # Short form (f=finished)
```

## Usage

### Importing WADs

```bash
# Smart import (auto-detects source type)
caco add "sunlust"                  # Search idgames
caco add 19509                      # idgames ID
caco add ~/Downloads/mymap.wad     # Local file (title inferred)
caco add https://example.com/wad.zip -t "My WAD"  # URL

# Batch local import
caco import local *.wad --tag new   # Import all WADs, add tag

# Explicit subcommands (if you prefer)
caco import idgames "sunlust"
caco import url "Title" "https://..." --author "Author"
caco import local "Title" ~/path/to/wad.wad

# Duplicate detection - caco warns if WAD already exists
caco add 19509                      # "Already in library" warning
caco add 19509 --force              # Import anyway

# Interactive selection with fzf (if installed)
caco add scythe                     # Opens fzf fuzzy picker
caco add "doom 2" --multi           # Multi-select mode
```

### Managing Library

```bash
# List all WADs (sorted by ID ascending by default)
caco list

# Sort by different fields
caco list --sort playtime              # Most played first
caco list --sort rating                # Highest rated first
caco list --sort -title                # Reverse alphabetical (Z-A)
caco list --sort last_played           # Recently played first

# Available sort fields: playtime, rating, created, title, author, last_played, year
# Prefix with - to reverse (e.g., -title for Z-A)

# Filter by status (accepts full names or shortcuts)
caco list --status playing             # List playing WADs
caco list --status to-play             # List to-play WADs (or use -s t)
caco list --status backlog             # List backlog WADs

# Search with beets-style queries
caco list scythe                    # Free text (title/author/description)
caco list id:1                      # By database ID
caco list title:"scythe"            # By title (or name:)
caco list author:"erik alm"         # By author
caco list year:2020                 # By year
caco list filename:scythe2          # By filename
caco list tag:megawad               # By tag
caco list status:playing            # By status
caco list source:idgames            # By source type
caco list author:alm title:scythe   # Combine multiple filters

# Filter options (can combine with queries)
caco list --status backlog
caco list --tag megawad
caco list --source idgames

# View details (by ID or query)
caco info id:1
caco info filename:tnto
caco info "TNT: Overcharged"

# Update metadata (supports ID ranges and queries)
caco update 1 --status playing
caco update 1-5 --rating 4                      # ID range
caco update tag:megawad --rating 5 --yes        # Query with confirmation skip
caco update 1 --rating 4 --notes "Great level design"

# Delete WADs (soft delete - can be restored)
caco rm 1                                       # Move to trash (alias for delete)
caco delete status:abandoned                    # Shows preview, prompts
caco delete 1 --dry-run                         # Preview what would be deleted
caco delete 1 --purge                           # Permanent deletion

# View and restore from trash
caco list --deleted                             # Show deleted WADs
caco restore 1                                  # Restore from trash
caco delete --purge-all                         # Empty trash

# Manage tags (supports ID ranges and queries)
caco tag add 1 megawad slaughter
caco tag add author:romero classic --yes        # Tag all WADs by author
caco tag remove 1 slaughter
```

### Playing

```bash
# Play a WAD by ID
caco play 1

# Play by query (must match exactly one WAD)
caco play filename:tnto
caco play "TNT: Overcharged"

# Use a specific sourceport
caco play 1 --sourceport /usr/bin/dsda-doom

# Pass extra args to sourceport
caco play 1 -- -warp 15 -skill 4
```

### Per-WAD Custom Config

Set WAD-specific IWAD, sourceport, or extra arguments:

```bash
# Set custom IWAD for a WAD
caco update 1 --iwad /path/to/tnt.wad

# Set custom sourceport
caco update 1 --sourceport /usr/bin/dsda-doom

# Set custom arguments
caco update 1 --args "-complevel 2 -warp 1"

# Clear custom settings
caco update 1 --clear-iwad --clear-sourceport --clear-args
```

Priority: CLI arguments > Per-WAD config > Global config

### Map Completion Tracking

Track which maps you've completed in each WAD. Automatically syncs with nyan-doom/dsda-doom stats files.

```bash
# Auto-sync happens after every play session (if stats file exists)
caco play 1                         # Syncs completions on exit

# Manual sync from stats.txt files
caco map sync 1                     # Sync specific WAD
caco map sync --all                 # Sync all WADs

# Manual completion tracking (for other sourceports)
caco map complete 1 MAP01 MAP02 MAP03
caco map complete 1 MAP01-MAP05     # Range support
caco map complete 1 MAP01 --skill 4 # Record UV completion
caco map uncomplete 1 MAP30         # Remove completion

# View completions
caco map list 1                     # Show completed maps (current playthrough)
caco map list 1 --all-cycles        # Show completions from all playthroughs
caco map progress 1 --total 32      # Show progress (29/32, 90.6%)
caco info 1                         # Also shows completion summary
```

#### Playthrough Cycles

When you mark a WAD as "finished", caco archives the current map progress and starts a new playthrough cycle. This lets you track completions across multiple playthroughs:

```bash
caco update 1 --status finished     # Archives current map progress, increments cycle
caco update 1 --status playing      # Start fresh playthrough (cycle 2)
caco map list 1                     # Shows only current cycle completions
caco map list 1 --all-cycles        # Shows all completions with cycle numbers
```

### Completion Count Tracking

Track how many times you've beaten each WAD:

```bash
# View completion counts
caco beaten list                    # Show all WADs with completion counts
caco beaten list --min 2            # WADs beaten 2+ times

# Manual adjustment
caco beaten add 1                   # Increment completion count
caco beaten remove 1                # Decrement completion count
caco beaten set 1 3                 # Set exact count
```

Default Stats file location: `~/.local/share/nyan-doom/nyan_doom_data/{iwad}/{wad}/stats.txt`

Configure custom stats directory:
```bash
caco config stats_dir /path/to/stats
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

# Clean orphaned files
caco cache clean                     # Remove files not tracked in database
caco cache clean --dry-run           # Preview orphan cleanup
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

**Note**: Local imports (files from your filesystem) are never deleted by cache commands.

## Configuration

Config file: `~/.config/caco/config.toml` (see `config.example.toml` for a template)

### Available Options

| Option | Description |
|--------|-------------|
| `sourceport` | Path to default sourceport |
| `iwad` | Path to default IWAD (doom2.wad, etc.) |
| `sourceport_args` | Default args passed to sourceport |
| `cache_dir` | WAD cache directory |
| `stats_dir` | Stats file directory for map completion tracking |
| `download_mirror` | Preferred idgames mirror (0-4) |
| `cache_max_size_gb` | Max cache size in GB (0 = unlimited) |
| `cache_max_age_days` | Remove files not played in N days (0 = never) |
| `cache_auto_clean` | Auto-cleanup cache on play (true/false) |

### Example Config

```toml
sourceport = "/usr/bin/nyan-doom"
iwad = "/usr/share/games/doom/doom2.wad"
sourceport_args = ["-nomusic"]
cache_dir = "~/.cache/caco/wads"
stats_dir = "~/.local/share/nyan-doom/nyan_doom_data"
download_mirror = 0

# Customize list display
[list]
# Available columns: id, title, author, status, maps, beaten, playtime,
#                    last_played, rating, year, tags
format = ["id", "title", "author", "status", "playtime", "tags"]

# Sort options: id, title, author, status, playtime, rating, year,
#               last_played, created, maps, beaten
# Append + for ascending (default), - for descending
sort = "id+"

[list.colors]
# Available colors: black, red, green, yellow, blue, magenta, cyan, white, dim
to-play = "blue"
backlog = "yellow"
playing = "green"
finished = "dim"
abandoned = "red"
```

## Command Aliases

Unix-like shortcuts for common commands:

| Alias | Full Command |
|-------|--------------|
| `caco add` | `caco import auto` |
| `caco rm` | `caco delete` |
| `caco ls` | `caco list` |
| `caco i` | `caco info` |

## Status Shortcuts

Use single letters or abbreviations for status values:

| Shortcut | Status |
|----------|--------|
| `t`, `tp` | to-play |
| `b`, `back` | backlog |
| `p`, `play` | playing |
| `f`, `fin`, `done` | finished |
| `a`, `drop` | abandoned |

```bash
caco update 1 -s p     # Set status to "playing"
caco list -s f         # List finished WADs
```

### CLI Commands

```bash
# View config file contents
caco config

# Open config in $EDITOR
caco config --edit

# Get config file path (for scripting)
caco config --path
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

Manual installation (fish):

```bash
cp completions/caco.fish ~/.config/fish/completions/
```

## Scripting

Use `--plain` to output machine-readable text suitable for scripting:

```bash
# List WADs as TSV (tab-separated)
caco list --plain
# Output: ID	Title	Author	Status	Maps	Beaten	Playtime	LastPlayed

# Get WAD info as key=value pairs
caco info 1 --plain
# Output: id=1
#         title=Scythe 2
#         author=Erik Alm
#         ...
```

## Data Storage

*Default locations:*

- **Database**: `~/.local/share/caco/library.db`
- **Config**: `~/.config/caco/config.toml`
- **WAD cache**: `~/.cache/caco/wads/`

## License

MIT
