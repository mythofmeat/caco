# TODO

## Current

- [x] if a WAD has been marked *finished* then create a record of the date that it was finished, and put its entire `stats.txt` file into the database, allowing for multiple entries if a map has been replayed multiple times
  - this is so that if a user wants to replay a wad, they can have a record of how they did previously
- [x] track how many times a wad has been completed (this can be manual tracking by the user)
- [x] rename the *wishlist* status to *to-play*, keeping the current data
- [x] remove the `caco wl` `caco pl` 'caco bl` shortcuts (these should just be abbreviations/aliases that the user sets in their shell, no need to have them hard-coded i think)
- [x] add a "maps completed" column to `caco list` commands
- [x] add a "times beaten" column to `caco list` commands


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
