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
- [x] Batch queries: replaced N+1 DB calls with batch-fetched stats (playtime, last_played, times_beaten, session_count)
- [x] Incremental table updates: `update_row()` method for in-place cell updates (rating, status) without full reload
- [x] Enter=Play binding now visible in footer
- [x] Fixed rating cycle bug: 0→1→2→3→4→5→0 (was skipping 0/unrated)
- [x] Delete WAD binding (d) with confirmation modal showing session stats
- [x] Beaten tracking keybindings: + to increment, - to decrement
- [x] Loading indicators in search panes (clears results + "Searching..." status)
- [x] Richer info panel: yellow stars, tag chips, description snippet, source type
- [x] "Other" tab for abandoned + awaiting-update WADs (list status filter)
- [x] Year and Rating sort options added to dropdown
- [x] Filter placeholder shows available query fields
- [x] Centralized status colors in tui/theme.py (replaces duplicated dicts)
- [x] Fixed silent exception swallowing: except Exception → except NoMatches
- [x] Removed deprecated handle_g_key() dead code
- [x] Stats screen (S) showing library overview, completion rate, monthly activity
- [x] Trash view (T) with restore (u) in All tab
- [x] Cache management screen (C) with clear selected/all
- [x] Responsive layout: auto-hide info panel < 100 cols, P to toggle manually
- [x] Cleaned up styles.tcss: removed duplicates already in widget DEFAULT_CSS

# GUI
- [ ] GUI for launching and managing WADs
- [ ] The GUI should be able to be called with `caco --gui`
- [ ] Downloaded WADs should have a thumbnail extracted from TITLEPIC in the WAD
  - There are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info
