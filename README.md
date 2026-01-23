# caco

A personal Doom WAD library manager. Track what you've played, what you want to play, and download WADs on-demand.

## Features

- **Library tracking** - Status (wishlist, backlog, playing, finished), ratings, tags, notes
- **Multiple sources** - Import from idgames archive, URLs, or local files
- **Playtime tracking** - Automatically tracks how long you play each WAD
- **On-demand downloads** - WADs are cached when you play, not stored permanently

## Installation

Requires Python 3.10+ and the [idgames-api](../idgames-api) package.

```bash
pip install -e ../idgames-api
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
# List all WADs
caco list

# Search with beets-style queries
caco list scythe                    # Free text (title/author/description)
caco list id:1                      # By database ID
caco list title:tnt                 # By title (or name:tnt)
caco list author:romero             # By author
caco list year:2020                 # By year
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
# Play a WAD
caco play 1

# Use a specific sourceport
caco play 1 --sourceport /usr/bin/dsda-doom

# Pass extra args to sourceport
caco play 1 -- -warp 15 -skill 4
```

## Configuration

Config file: `~/.config/caco/config.toml`

```bash
# View config
caco config

# Set default sourceport
caco config sourceport /usr/bin/gzdoom

# Set cache directory
caco config cache_dir ~/.cache/caco/wads
```

## Data Storage

- **Database**: `~/.local/share/caco/library.db`
- **Config**: `~/.config/caco/config.toml`
- **WAD cache**: `~/.cache/caco/wads/`

## License

MIT
