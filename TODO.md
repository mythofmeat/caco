# New Features *(ordered by priority)*

## Cache management
- [x] `caco cache clear` - remove cached WADs
- [x] `caco cache list` - show cached files and sizes
- [x] `caco cache clean` - remove orphaned files
- [x] Auto-cleanup old cached files
  - configurable via `cache_max_size_gb`, `cache_max_age_days`, `cache_auto_clean`

## Data Sources
- [ ] Doomwiki
  - [ ] Doomwiki scraper - parse infoboxes for metadata
  - [ ] Fetch metadata from wiki pages
  - [ ] Parse infobox (author, year, IWAD, etc.)
  - [ ] Link to wiki page in WAD info

- [ ] Doomworld Forums
  - [ ] Store thread URL for reference
    - Consider: parse thread title for basic metadata?
    - Could even implement a call to an AI LLM to scrape the first post of the thread and fill in the relevant info?
      - what *is* the relevant info? what data are we actually trying to scrape?
        - title
        - author
        - date
        - description
        - other info...
          - complevel?

## Statistics
- [ ] Total playtime across all WADs
- [ ] WADs played per month/year
- [ ] Most played WADs
- [ ] Completion rate

## TUI
- [ ] Textual-based TUI
- [ ] Browse library with vim keybindings
- [ ] Quick-play from list
- [ ] Session history view

## GUI
- [ ] gui for launching and managing WADs
- [ ] downloaded WADs should have a thumbnail which is extracted directly from the TITLEPIC in the WAD
  - there are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info
