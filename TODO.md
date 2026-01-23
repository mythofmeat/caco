# TODO

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
- [ ] Advanced query syntax (e.g., `status:unplayed year:2020..`)
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
