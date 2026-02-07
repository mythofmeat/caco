# General
- [x] The description from idgames should also include the `textfile` element from the idgames api (https://www.doomworld.com/idgames/api/)
- [x] i think we should just remove the `completed maps` feature entirely. not enough sourceports provide ways to access this data and it's not particularly useful.
- [x] there should be a `caco random` command to print the info of a random WAD for use in scripting.
  - [x] it should support filtering arguments, and a command like `caco play $(caco random status:to-play)` should work

# Refactoring (completed)
- [x] Consolidated STATUS_SHORTCUTS to single definition in db.py
- [x] Extracted _coerce_str to shared utils.py
- [x] Removed dead LibraryScreen code (397 lines) and legacy CSS
- [x] Added tag-fetching helpers (_fetch_tags, _attach_tags, _fetch_tags_batch) to eliminate N+1 queries
- [x] Collapsed find_duplicate strategies 1-3 into single block
- [x] Added batch query functions (get_total_playtime_batch, get_last_played_batch)
- [x] Extracted shared _check_and_import_entry helper in CLI
- [x] Created BaseHttpClient and CacoSourceError hierarchy in utils.py
- [x] Extracted extract_year helper to utils.py
- [x] Created BaseSearchPane for TUI search panes (idgames + doomwiki)
- [x] Split cli.py (2857 lines) into cli/ package with 8 submodules

# CLI Improvements (completed)
- [x] Fixed `-y` flag collision: removed `-y` short form from `--year` so `-y` exclusively means `--yes`
- [x] Renamed `cache clean` → `cache prune`
- [x] Enriched `tag list` with WAD counts (Rich table + `--plain` flag)
- [x] Added `--info` flag to `random` command (prints ID, title, author as TSV)
- [x] Added `--json` output to `list` and `info` commands
- [x] Flattened `caco import` from group with subcommands to single command with source flags (`--idgames`, `--doomwiki`, `--doomworld`, `--local`, `--url`)
- [x] Removed `caco add` alias (use `caco import` directly)
- [x] Improved query syntax help text in `list` command docstring
- [x] Updated fish shell completions

# Testing (completed)
- [x] Set up pytest with 94 unit tests covering models, db CRUD, query parser, player, and utils

# TUI Improvements
- [x] able to choose a default start page and sort via the config file

# GUI
- [ ] GUI for launching and managing WADs
- [ ] The GUI should be able to be called with `caco --gui`
- [ ] Downloaded WADs should have a thumbnail extracted from TITLEPIC in the WAD
  - There are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info
