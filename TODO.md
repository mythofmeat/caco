# New Features *(ordered by priority)*

## TUI Improvements
- [ ] Should be able to launch a WAD by pressing enter without going to the info submenu
- [ ] Can we make the search/filter update instantly?
- [ ] There should be a way to sort and filter in a style similar to the CLI
- [ ] There should be a way to edit and update all the info
- [ ] Basically I want all the CLI features to be usable from the TUI, including adding WADs

## GUI
- [ ] gui for launching and managing WADs
- [ ] The GUI should be able to be called with `caco --gui`
- [ ] downloaded WADs should have a thumbnail which is extracted directly from the TITLEPIC in the WAD
  - there are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info

# Implemented Features

## TUI (Terminal User Interface)
- [x] Textual-based TUI accessible via `caco --tui`
- [x] Browse library with vim keybindings (j/k, gg/G, Ctrl+d/u)
- [x] Filter/search with beets-style queries
- [x] WAD info panel showing details of selected WAD
- [x] Quick-play from list (Enter to launch)
- [x] WAD detail screen (i key)
- [x] Session history view (h key)
- [x] Quick status change (s + p/f/t/b/a/w)
- [x] Fixed: MountError when viewing WAD details (used single Static instead of Horizontal container)
- [x] Fixed: Filter input text now visible (added explicit color styling)

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

## Statistics
- [x] `caco stats` command
  - [x] Total playtime across all WADs
  - [x] WADs played per month/year
  - [x] Completion rate
