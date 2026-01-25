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
# Set your sourceport
caco config sourceport /usr/bin/gzdoom

# Import a WAD from idgames
caco import idgames "scythe 2"

# List your library
caco list

# Play a WAD (downloads if needed, tracks playtime)
caco play title:"Scythe 2"

# Update status after playing
caco update title:"Scythe 2" --status finished --rating 5
```

## Usage

### Importing WADs

```bash
# From idgames (search or by ID)
caco import idgames "sunlust"
caco import idgames 19509

# From a URL (Doomworld forums, etc.)
caco import url "Eviternity" "https://www.doomworld.com/forum/topic/..." --author "Dragonfly"

# Local file
caco import local "MyWad" ~/wads/mywad.wad

# Duplicate detection - caco warns if WAD already exists
caco import idgames 19509           # "Already in library" warning
caco import idgames 19509 --force   # Import anyway

# Interactive selection with fzf (if installed)
caco import idgames scythe          # Opens fzf fuzzy picker
caco import idgames "doom 2" --multi # Multi-select mode
```

### Managing Library

```bash
# List all WADs (sorted by status: playing → backlog → to-play → abandoned → finished)
caco list

# Sort by different fields
caco list --sort playtime              # Most played first
caco list --sort rating                # Highest rated first
caco list --sort -title                # Reverse alphabetical (Z-A)
caco list --sort last_played           # Recently played first

# Available sort fields: playtime, rating, created, title, author, last_played, year
# Prefix with - to reverse (e.g., -title for Z-A)

# Filter by status
caco list --status playing             # List playing WADs
caco list --status to-play             # List to-play WADs
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

# Delete WADs (supports ID ranges and queries)
caco delete 1                                   # Single ID (prompts)
caco delete status:abandoned --yes              # Bulk delete

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
caco map list 1                     # Show all completed maps
caco map progress 1 --total 32      # Show progress (29/32, 90.6%)
caco info 1                         # Also shows completion summary
```

Default Stats file location: `~/.local/share/nyan-doom/nyan_doom_data/{iwad}/{wad}/stats.txt`

Configure custom stats directory:
```bash
caco config stats_dir /path/to/stats
```

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

### Example Config

```toml
sourceport = "/usr/bin/nyan-doom"
iwad = "/usr/share/games/doom/doom2.wad"
sourceport_args = ["-nomusic"]
cache_dir = "~/.cache/caco/wads"
stats_dir = "~/.local/share/nyan-doom/nyan_doom_data"
download_mirror = 0
```

### CLI Commands

```bash
# View config
caco config

# Set values
caco config sourceport /usr/bin/gzdoom
caco config iwad /usr/share/games/doom/doom2.wad
caco config cache_dir ~/.cache/caco/wads
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
