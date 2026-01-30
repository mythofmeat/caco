# Quality-of-life improvements

- [ ] there should be a way to manually adjust the amount of times completed
  - id:49 and id:63 should only have `1` completion count

- [ ] change default sorting on list to id ascending

- [x] tag searches should support globs
  - e.g., `caco list --tag cacowards_2025*` should return items that contain `cacowards_2025_winner` and `cacowards_2025_runnerup`
- [x] tags should be shown when listing WADs
  - Configurable via `[list]` section in config.toml
- [ ] tags should have completions

- [x] ALL search and list fields should be supported in ALL commands, always
  - Universal query support now works across all commands with `resolve_wad_query()`

- [x] when giving an error that multiple WADs match the search, default behavior should be to do an interactive picker to select which one the user wanted
  - Interactive picker uses fzf if available, falls back to numbered selection

- [ ] the map progress feature should only apply to WADs that are currently playing. when a WAD is marked as finished, its map progress should be archived and reset to 0

- [ ] there should be a way to view completion details and history

## I don't like the syntax for a lot of the commands
- [x] config
  - Config command simplified: `caco config` shows config, `caco config --edit` opens in $EDITOR
  - All configuration should be done through the config file

- [x] delete
  - Delete commands now show preview with stats before prompting
  - Supports `--dry-run` to see what would be deleted
  - Soft delete by default: WADs go to trash, use `caco restore` to recover
  - Use `--purge` for permanent deletion

- [x] import
  - Added `caco import auto` (aliased as `caco add`) for auto-detection:
    - Integer → idgames ID lookup
    - URL → URL import
    - Existing file path → local import
    - Text → idgames search

- [x] list
  - Configurable via `[list]` section in config.toml
  - Supports custom columns, colors, default sort
  - Added `--deleted` flag to view trash

- [x] play
  - Now uses universal query support with interactive picker
- [x] update
  - Now supports `--dry-run` to preview changes

## Configuration file
- [x] there should be a way to use the configuration file to specify the default formatting when listing WADs
  - Implemented as `[list]` section with `format`, `sort`, `colors` options

# New Features

## Command Aliases
- [x] Added Unix-like aliases:
  - `caco add` → `caco import auto`
  - `caco rm` → `caco delete`
  - `caco ls` → `caco list`
  - `caco i` → `caco info`

## Status Shortcuts
- [x] Single-letter shortcuts for statuses:
  - `t` / `tp` → to-play
  - `b` / `back` → backlog
  - `p` / `play` → playing
  - `f` / `fin` / `done` → finished
  - `a` / `drop` → abandoned

## Batch Import
- [x] `caco import local` now accepts multiple files:
  - `caco import local *.wad --tag batch`
  - Titles inferred from filenames

## Soft Delete / Trash
- [x] WADs are soft-deleted by default (recoverable)
- [x] `caco list --deleted` shows trash
- [x] `caco restore <query>` recovers deleted WADs
- [x] `caco delete --purge` for permanent deletion
- [x] `caco delete --purge-all` empties trash

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
