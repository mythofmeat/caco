# New Features *(ordered by priority)*

## Data Sources
- [x] Doomworld Forums (Phase 1 - MVP)
  - [x] Import from forum thread URL
  - [x] Parse JSON-LD structured data for title, author, date
  - [x] Extract first post content as description
  - [x] Auto-detect Doomworld URLs in `caco import auto`
  - [x] `caco import doomworld <URL>` command
  - [x] Duplicate detection by thread ID
- [x] Doomworld Forums (Phase 2 - Enhanced Parsing)
  - [x] Extract download links from post content (Dropbox, Google Drive, Mega, etc.)
  - [x] Parse complevel from post text ("complevel 9", "cl21", "boom compatible", etc.)
  - [x] Parse IWAD requirements ("requires Doom 2", "doom2.wad", "Plutonia", etc.)
  - [x] Parse port requirements ("GZDoom required", "DSDA-Doom", "Crispy Doom", etc.)
  - [x] Display extracted metadata in CLI output
- [x] Doomworld Forums (Phase 3 - LLM Integration)
  - [x] `--smart` flag for LLM-based metadata extraction
  - [x] Multiple LLM backends (claude-code, openrouter, anthropic, openai)
  - [x] Auto-detection of available backends
  - [x] Extracts: description, map count, difficulty, themes, version

## Version Tracking
- [ ] Track version info for non idgames releases (as idgames releases are final by design)
- [ ] Create a new category for WADs awaiting updates/a full release.
  - idk what to call this category, but i'm open to suggestions

## Statistics
- [ ] `caco stats` command
  - [ ] Total playtime across all WADs
  - [ ] WADs played per month/year
  - [ ] Completion rate

## TUI
- [ ] Textual-based TUI
- [ ] The TUI should be able to be called with `caco --tui`
- [ ] Browse library with vim keybindings
- [ ] Quick-play from list
- [ ] Session history view

## GUI
- [ ] gui for launching and managing WADs
- [ ] The GUI should be able to be called with `caco --gui`
- [ ] downloaded WADs should have a thumbnail which is extracted directly from the TITLEPIC in the WAD
  - there are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info
