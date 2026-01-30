# New Features *(ordered by priority)*

## Cache management
- [ ] `caco cache clear` - remove cached WADs
- [ ] `caco cache list` - show cached files and sizes
- [ ] Auto-cleanup old cached files
  - configurable in caco.conf

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
