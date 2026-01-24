# caco

A personal Doom WAD library manager. Track what you've played, what you want to play, and download WADs on-demand.

## Features

- **Library tracking** - Status (wishlist, backlog, playing, finished), ratings, tags, notes
- **Multiple sources** - Import from idgames archive, URLs, or local files
- **Playtime tracking** - Automatically tracks how long you play each WAD
- **On-demand downloads** - WADs are cached when you play, not stored permanently

## Installation

Requires Python 3.10+.

```bash
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
caco play 1

# Update status after playing
caco update 1 --status finished --rating 5
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
```

### Managing Library

```bash
# List all WADs (sorted by status: playing → backlog → wishlist → abandoned → finished)
caco list

# Quick status aliases
caco pl                             # List playing WADs
caco wl                             # List wishlist WADs
caco bl                             # List backlog WADs

# Search with beets-style queries
caco list scythe                    # Free text (title/author/description)
caco list id:1                      # By database ID
caco list title:tnt                 # By title (or name:tnt)
caco list author:romero             # By author
caco list year:2020                 # By year
caco list filename:tnto             # By filename
caco list tag:megawad               # By tag
caco list status:playing            # By status
caco list source:idgames            # By source type
caco list author:alm title:scythe   # Combine multiple filters

# Filter options (can combine with queries)
caco list --status backlog
caco list --tag megawad
caco list --source idgames

# View details
caco info 1

# Update metadata
caco update 1 --status playing
caco update 1 --rating 4 --notes "Great level design"

# Manage tags
caco tag add 1 megawad slaughter
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

## Configuration

Config file: `~/.config/caco/config.toml` (see `config.example.toml` for a template)

### Available Options

| Option | Description |
|--------|-------------|
| `sourceport` | Path to default sourceport |
| `iwad` | Path to default IWAD (doom2.wad, etc.) |
| `sourceport_args` | Default args passed to sourceport |
| `cache_dir` | WAD cache directory |
| `download_mirror` | Preferred idgames mirror (0-4) |

### Example Config

```toml
sourceport = "/usr/bin/gzdoom"
iwad = "/usr/share/games/doom/doom2.wad"
sourceport_args = ["-nomusic"]
cache_dir = "~/.cache/caco/wads"
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

## Data Storage

- **Database**: `~/.local/share/caco/library.db`
- **Config**: `~/.config/caco/config.toml`
- **WAD cache**: `~/.cache/caco/wads/`

## License

MIT
