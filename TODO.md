# Bugfixes

# Quality-of-life improvements

# New Features

## Cache management
- [ ] `caco cache clear` - remove cached WADs
- [ ] `caco cache list` - show cached files and sizes
- [ ] Auto-cleanup old cached files
  - configurable in caco.conf

## TUI
- [ ] Textual-based TUI
- [ ] Browse library with vim keybindings
- [ ] Quick-play from list
- [ ] Session history view

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

## Statistics
- [ ] Total playtime across all WADs
- [ ] WADs played per month/year
- [ ] Most played WADs
- [ ] Completion rate
