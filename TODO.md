# Bugfixes

# Quality-of-life improvements

- [ ] there should be a way to manually adjust the amount of times completed
  - id:49 and id:63 should only have `1` completion count

- [ ] change default sorting on list to id ascending

- [ ] tag searches should support globs
  - e.g., `caco list --tag cacowards_2025*` should return items that contain `cacowards_2025_winner` and `cacowards_2025_runnerup`
- [ ] tags should be shown when listing WADs
- [ ] tags should have completions

- [ ] ALL search and list fields should be supported in ALL commands, always
  - there are some commands that do not accept anything other than `id` arguments

- [ ] when giving an error that multiple WADs match the search, default behavior should be to do an interactive picker to select which one the user wanted

- [ ] the map progress feature should only apply to WADs that are currently playing. when a WAD is marked as finished, its map progress should be archived and reset to 0

- [ ] there should be a way to view completion details and history

## I don't like the syntax for a lot of the commands
- [ ] config
  - i don't actually like setting any configuration options this way. *all* config options should be set exclusively through a config file that can be overridden by command line flags, but that should be it

- [ ] delete
  - delete commands *really* need to prompt yes/no along with a list of what will actually be deleted. it's way too easy to accidentally delete the wrong thing

- [ ] import
  - the import command currently works like `caco import [idgames|local|url] $arg` and that seems redundant
  - there should be a way to automatically detect what is being imported
    - i.e., a string or integer should automatically search idgames
    - a file path should automatically be a local import
    - a supported URL should automatically be detected
      - unsupported URLs should error

- [ ] list
  - 
- [ ] map
- [ ] play
- [ ] tag
- [ ] update

## Configuration file
- [ ] there should be a way to use the configuration file to specify the default formatting when listing WADs
  - something like: 
```
list_format: [ "id", "title", "author", "last_played" ]
list_sort:   "id-"
```
  - with `id+` meaning id ascending and `id-` meaning id descending
  - should also

# New Features

## Explicit Sourceport Support
- [ ] Helion
  - [ ] Map progress detection VIA save file

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
