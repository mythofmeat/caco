# New Features *(ordered by priority)*

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

## Version Tracking
- [x] Track version info for non idgames releases (as idgames releases are final by design)
  - Added `version` column to database
  - `caco update <id> --version "v1.0"` to set version
  - `caco info <id>` displays version
  - LLM extraction (`--smart`) auto-extracts version from Doomworld forum posts
- [x] Create a new category for WADs awaiting updates/a full release.
  - Added `awaiting-update` status
  - Shortcuts: `w`, `wip`, `au`, `await`, `waiting`
  - Displayed in magenta in list output
