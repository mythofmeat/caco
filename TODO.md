# TODO

## User requests
- [ ] add a `completions` command that will automatically update completions for fish/bash/etc.
- [ ] have per-game custom arguments
  - for example, if a specific wad requires tnt, i can set that as a wad-specific configuration, so i just have to do `caco play 1` and it launches with the `-iwad tnt` argument
- [ ] can we please change the way `caco list` sorts files?
  - what i want:
    - wads with status `playing` listed first and sorted by last played (newest first)
    - wads with status `backlog` listed second and sorted by time added (newest first)
    - wads with status `wishlist` listed third and sorted by time added (newest first)
    - wads with status `abandoned` listed fourth and sorted by last played (newest first)
    - wads with status `finished` listed last and sorted by last played (newest first)
- [ ] can we include shortened commands for `caco list [argument]`
  - `caco list -s playing` -> `caco pl`
  - `caco list -s wishlist` -> `caco wl`
- [ ] listing by `filename:` doesn't seem to work
- [ ] can we add completions for all of the fields we can use to list and sort wads?
- [ ] can we add the ability to launch a wad by identifying info other than its id?
  - e.g., `caco play filename:tnto` and `caco play "TNT: Overcharged" both work` but `caco play "Doom 2 in"` does not work because it's ambiguous (list  the results within the error message)

## Core Features

### Import Sources
- [x] idgames archive - search and import with metadata
- [x] Manual URL entry - for Doomworld forums, etc.
- [x] Local files - track existing WADs
- [ ] Doomwiki scraper - parse infoboxes for metadata

### Library Management
- [x] Add/remove/update WADs
- [x] Tag system
- [x] Status tracking (wishlist, backlog, playing, finished, abandoned)
- [x] Star ratings (1-5)
- [x] Notes field
- [ ] Bulk import (multiple WADs at once)
- [ ] Duplicate detection

### Play
- [x] Launch WAD with sourceport
- [x] Automatic playtime tracking
- [ ] IWAD selection (doom.wad, doom2.wad, etc.)
- [ ] Save sourceport preferences per-WAD
- [ ] Track which maps/levels completed

### Querying
- [x] List with filters (status, tag, source)
- [x] Text search (title, author, description)
- [x] Advanced query syntax (e.g., `id:1 author:romero year:2020`)
- [ ] Sort options (playtime, rating, date added, etc.)

## UI

### CLI Enhancements
- [ ] Interactive import picker (fzf-style)
- [ ] Progress bars for downloads
- [ ] Shell completions

### TUI
- [ ] Textual-based TUI (like idgames-tui)
- [ ] Browse library with vim keybindings
- [ ] Quick-play from list
- [ ] Session history view

## Data Sources

### Doomwiki Integration
- [ ] Fetch metadata from wiki pages
- [ ] Parse infobox (author, year, IWAD, etc.)
- [ ] Link to wiki page in WAD info

### Doomworld Forums
- [ ] Store thread URL for reference
- [ ] Consider: parse thread title for basic metadata?

## Quality of Life

### Cache Management
- [ ] `caco cache clear` - remove cached WADs
- [ ] `caco cache list` - show cached files and sizes
- [ ] Auto-cleanup old cached files

### Backup/Export
- [ ] Export library to JSON
- [ ] Import library from JSON
- [ ] Sync between machines?

### Statistics
- [ ] Total playtime across all WADs
- [ ] WADs played per month/year
- [ ] Most played WADs
- [ ] Completion rate
