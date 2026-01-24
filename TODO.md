# TODO

## High Priority

- [ ] Track which maps/levels completed
- [ ] Sort options (playtime, rating, date added, etc.)
- [ ] Interactive import picker (fzf-style)
- [ ] Duplicate detection

## Later

- [ ] Cache management
  - [ ] `caco cache clear` - remove cached WADs
  - [ ] `caco cache list` - show cached files and sizes
  - [ ] Auto-cleanup old cached files

- [ ] Import/Export
  - [ ] Export library to JSON
  - [ ] Import library from JSON
  - [ ] Sync between machines?

- [ ] TUI
  - [ ] Textual-based TUI (like idgames-tui)
  - [ ] Browse library with vim keybindings
  - [ ] Quick-play from list
  - [ ] Session history view

- [ ] Data Source
  - [ ] Doomwiki
    - [ ] Doomwiki scraper - parse infoboxes for metadata
    - [ ] Fetch metadata from wiki pages
    - [ ] Parse infobox (author, year, IWAD, etc.)
    - [ ] Link to wiki page in WAD info
  - [ ] Doomworld Forums
    - [ ] Store thread URL for reference
    - [ ] Consider: parse thread title for basic metadata?

- [ ] Statistics
  - [ ] Total playtime across all WADs
  - [ ] WADs played per month/year
  - [ ] Most played WADs
  - [ ] Completion rate
