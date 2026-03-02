# Changelog

All notable changes to Caco are documented in this file.

Development log for Caco.

---

## 2026-03-02

### Added

- **Companion files redesign**: Moved from a JSON array column to a proper
  relational model with managed storage, MD5-based deduplication, and per-WAD
  enable/disable toggles
  - New `companion_files_registry` table (id, md5, filename, path, size) for
    deduplicated managed files
  - New `wad_companions` junction table (wad_id, companion_id, enabled,
    load_order) for per-WAD linking
  - Managed storage at `~/.local/share/caco/companions/` with filename format
    `{md5[:12]}_{original_filename}`
  - `companion_orphan_cleanup` config option: `"delete"`, `"keep"`, or `"ask"`
    (default: `"ask"`) controls behavior when a companion is unlinked from its
    last WAD
- **`caco companion` command group**: Dedicated CLI for managing companion files
  - `caco companion add <query> <file>` — register and link
  - `caco companion rm <query> <filename> [-y]` — unlink with orphan policy
  - `caco companion enable/disable <query> <filename>` — toggle without removing
  - `caco companion ls [query] [--plain]` — list per-WAD or all companions
- **Migration #23**: Automatically migrates existing `companion_files` JSON data
  to the new relational tables, copying files to managed storage
- **Shell completions**: Fish, bash, and zsh completions for all companion
  subcommands

### Changed

- **Player companion loading**: Uses `db.get_wad_companions()` instead of JSON
  parsing; respects enabled/disabled state and load order
- **GUI edit dialog**: Companion files shown as checkable list with Add File
  picker and Remove button (deferred registration — only persists on Save)
- **TUI edit screen**: Companion files shown with `[enabled]`/`[disabled]`
  status in TextArea
- **TUI detail screen**: Companion files loaded from junction table instead of
  stale JSON column; shows `(off)` for disabled companions
- **`modify --add-file/--remove-file`**: Delegates to companion service instead
  of direct JSON manipulation; warns when `--remove-file` doesn't match
- **Info display**: CLI, GUI, and TUI all read from `get_wad_companions()`

### Fixed

- **N+1 query in `companion ls`**: Global listing uses single query with
  `COUNT` join instead of per-companion `get_companion_wads()` calls
- **Fragile orphan check**: `companion rm` uses read-only `would_be_orphan()`
  instead of destructive unlink-check-relink pattern
- **GUI cancel discards changes**: Add File in edit dialog no longer immediately
  registers companions — deferred to Save, so Cancel is side-effect-free
- **Redundant imports**: Removed unnecessary `from caco import db as _db` in
  TUI files that already had `db` imported at module level
- **Stale data in TUI detail view**: `wad_detail.py` was reading the old
  `companion_files` JSON column (no longer maintained); now uses junction table

---

## 2026-02-28 (3)

### Added

- **`caco enrich` command**: Re-run enrichment for existing WADs — detects
  complevel from cached WAD files (COMPLVL lump, UMAPINFO, DEHACKED, map names)
  and Doom Wiki port field lookup. Supports `--complevel` (only WADs missing
  complevel), `--dry-run`, and query filtering
- **COMPLVL lump consolidated into `detect_complevel()`**: The function is now
  self-contained — callers don't need to separately check for the COMPLVL lump

### Fixed

- **Text-based COMPLVL lump parsing**: Some WADs use text strings in the COMPLVL
  lump (e.g., `"mbf21"`, `"vanilla"`) instead of the id24 single-byte format.
  Previously the first byte was misread as a numeric complevel (e.g., ASCII `'m'`
  = 109). Now correctly distinguishes 1-byte id24 format from text strings and
  parses both
- **id24 resource loading**: `detect_complvl()` now only matches 1-byte COMPLVL
  lumps, preventing text-based lumps from falsely triggering id24 resource loading

### Changed

- **Simplified player.py auto-detect**: Replaced two-step COMPLVL-then-heuristic
  logic with a single `detect_complevel()` call
- **Removed duplicate `_read_wad_data()`**: `complevel_detect.py` now uses the
  shared `_load_wad_data()` from `iwad_detect.py`

---

## 2026-02-28 (2)

### Fixed

- **Complevel auto-detection on import**: `_auto_enrich_doomwiki()` now sets
  complevel from the Doom Wiki `port` field (e.g., "Boom-compatible" → 9) for
  idgames, Doomworld, URL, and local imports — previously only direct Doomwiki
  imports got complevel auto-linked

### Changed

- **Switched to uv**: Project uses `uv sync` / `uv run` instead of manual venv
  activation and pip install

---

## 2026-02-28

IWAD/id24 resource management screens for TUI and GUI, edit form upgrades, and
GUI menu bar.

### Added

- **TUI Resources Screen** (`W` key): Combined tabbed screen for managing IWAD
  and id24 WAD registries — list, import from file path, remove
- **GUI Resources Dialog** (`Ctrl+R`): Combined tabbed dialog for IWAD/id24
  management with browse/import and remove
- **GUI menu bar**: Tools menu with Resources (`Ctrl+R`), Library Statistics
  (`Ctrl+S`), and Cache Management (`Ctrl+K`)
- **TUI IWAD dropdown**: Edit form IWAD field upgraded from text input to Select
  dropdown populated with registered families, with "Other..." fallback for
  custom values
- **TUI companion files**: TextArea added to edit form for managing companion
  files (one path per line)
- **GUI IWAD combo**: Edit dialog IWAD field upgraded from QLineEdit to editable
  QComboBox populated with registered families
- **IWAD display**: `custom_iwad` now shown in both TUI info panel and GUI
  detail panel
- **Shared resource service**: `register_iwad()` / `register_id24()` extracted
  into `services/resource_service.py` — used by CLI, TUI, and GUI

### Changed

- **CLI import refactor**: `_register_iwad()` and `_register_id24()` in
  `import_cmds.py` now delegate to shared `resource_service` helpers
- **GUI shortcuts**: `Ctrl+S` and `Ctrl+K` moved from `_setup_shortcuts()` to
  menu bar actions (avoids double-trigger)

---

## 2026-02-28

Investigated binary compilation/distribution options.

### Research

- **Nuitka --onefile**: 245MB binary, ~50% slower startup than venv (235ms vs
  162ms); extracts to temp dir each launch — same overhead as PyInstaller
- **Nuitka --standalone**: directory mode avoids extraction but requires
  distributing a folder
- **PyInstaller**: similar tradeoffs to Nuitka for Python CLI tools
- **Conclusion**: Python binary packaging isn't worth it for a CLI tool called in
  quick succession; native language (Rust) would be the real solution
- Added speculative TODO for Rust port (clap, ratatui, rusqlite, reqwest)

---

## 2026-02-28

Strip version numbers and clean up git branches — project is pre-release alpha,
not following semver.

### Changed

- **Changelog**: Stripped version numbers from all section headers (dates only);
  replaced semver preamble with plain description
- **Package version**: Set to `0.0.1-dev` in both `pyproject.toml` and
  `src/caco/__init__.py`

### Removed

- **Git branches**: Deleted 18 stale remote branches (versioned releases,
  feature branches, worktrees) and 9 local branches; only `main` and `dev`
  remain

---

## 2026-02-28

Consolidate complevel to single `complevel INTEGER` column and remove redundant `beaten` command group.

### Changed

- **Complevel column consolidation**: Merged `custom_complevel TEXT` into
  `complevel INTEGER`; migration 22 copies non-null values; single column
  used for both COMPLVL lump detection and heuristic detection
- **id24 resource loading**: `_get_id24_resource_args()` now checks for COMPLVL
  lump directly via `detect_complvl()` instead of relying on a DB column
- **Complevel query**: `complevel:` field now queries single `complevel` column
  (was OR-ing two columns)

### Removed

- **`beaten` command group**: Removed `caco beaten list/add/attach/remove/set`;
  use `caco modify beaten+/beaten-/beaten=` instead
- **`COMPLEVEL_SHORTCUTS`** in `doomworld/parser.py`: Redundant with
  `caco.complevel.COMPLEVEL_ALIASES`
- **`custom_complevel`** from `WadRecord` TypedDict and `ALLOWED_UPDATE_FIELDS`

---

## 2026-02-28

id24 play integration: COMPLVL lump detection, auto-load id24 resource WADs during play, and complevel flags for compatible sourceports.

### Added

- **COMPLVL lump detection** (`iwad_detect.py`): `detect_complvl()` inspects WAD
  files for the COMPLVL lump (id24 signal) and returns the complevel byte value;
  auto-detected on first play and persisted to `complevel` column
- **Auto-load id24 resource WADs** (`player.py`): When a WAD has a COMPLVL lump,
  `id24res.wad` is automatically injected into the `-file` list; when playing
  `id1.wad` (Legacy of Rust), also loads `id1-res`, `id1-tex`, `id1-weap`,
  `id1-mus` from the id24 registry
- **Complevel flags** (`sourceports.py`): `get_complevel_args()` passes
  `-complevel N` to sourceports that support it (dsda and woof families);
  automatically injected during play when `complevel` is set
- **`complevel` modify field**: `caco modify id:1 complevel=boom` — accepts
  integer values or named shortcuts (vanilla=2, boom=9, mbf=11, mbf21=21);
  `!complevel` to clear
- **`complevel:` query field**: `caco ls complevel:boom` — filter WADs by
  complevel value (accepts same shortcuts)
- **Complevel in `info` output**: Shows "Complevel: 9 (Boom)" in custom play
  config section with human-readable name from `COMPLEVEL_NAMES`
- **`auto_detect_complevel` config option** (default: true): Controls whether
  COMPLVL lump detection runs on first play
- **Database migration 18**: Adds `custom_complevel TEXT` column to wads table
  (superseded by migration 22 which consolidates to `complevel INTEGER`)
- **Fish completions**: `complevel:` for ls queries, `complevel=` for modify,
  `!complevel` for clearing
- **Tests**: COMPLVL detection (with/without lump, ZIP handling, edge cases),
  `get_complevel_args()` for each sourceport family, query and modify validation,
  player integration (auto-detect, skip-when-set)

### Changed

- **`_load_wad_data()` extracted** from `detect_iwad()`: Shared helper for
  loading WAD bytes (handles ZIP-wrapped WADs), used by both `detect_iwad()`
  and `detect_complvl()`

---

## 2026-02-27

id24 WAD management: register, list, and remove id24 content files (Legacy of Rust, id24res, modder packs) from the 2024 Doom re-release.

### Added

- **id24 WAD registry** (`db/_id24.py`): Known MD5 hashes for all id24 WADs
  (id1, id24res, id1-res, id1-tex, id1-weap, id1-mus, iddm1) with version
  detection (initial vs update2); filename fallback for unrecognized MD5s
- **Auto-detect on import**: `caco import` now auto-detects id24 files (and
  IWADs) from local paths — copies to managed directory and registers in DB
- **`caco ls --id24`**: List registered id24 WADs (name, version, title, path);
  supports `-o plain` (TSV) and `-o json` output
- **`caco trash --id24 NAME`**: Remove a registered id24 WAD from DB and
  managed storage
- **Managed id24 storage**: `~/.local/share/caco/id24/{name}.wad` — flat layout
  (one copy of each file)
- **`ID24_DIR`** and **`get_id24_dir()`** in `config.py`: Directory constant
  and accessor for managed id24 WADs
- **Database migration 17**: Creates `id24_wads` table (name, version, title,
  path, md5)
- **Fish completions**: `--id24` flag for `ls` and `trash` commands

---

## 2026-02-27

Demo recording and playback: record, list, play back, and clean demo files per-WAD.

### Added

- **`--record`/`-r` flag on `caco play`**: Record a demo during play; auto-generates
  a timestamped name or accepts a custom name (`caco play 1 --record`,
  `caco play 1 --record mydemo`)
- **`caco demos` command group**: Full demo file lifecycle management
  - **`demos list <query>`**: List `.lmp` demo files with size and modification time;
    `--plain` for TSV output
  - **`demos play <query> [DEMO]`**: Play back a recorded demo; plays most recent if
    DEMO omitted; supports `--sourceport`/`-p` override
  - **`demos clean <query>`**: Delete demo files; `--dry-run` for preview
- **`demos.py` module**: Core demo management logic decoupled from CLI —
  `find_demo_files()`, `clean_demo_files()`, `generate_demo_name()`, `get_demos_dir()`
- **`demo_file` column on sessions table** (migration 16): Links recorded demos to
  play sessions; `update_session_demo()` DB function
- **Fish completions**: `demos` group with all subcommands and flags; `--record` on play
- **Tests**: 15 new tests covering demo file discovery, cleanup, name generation

---

## 2026-02-27

Save game management: list, backup, restore, and clean save files per-WAD.

### Added

- **`caco saves` command group**: Full save game lifecycle management
  - **`saves list <query>`**: List save files for a WAD with size and
    modification time; `--plain` for TSV output
  - **`saves backup <query>`**: Zip the WAD's entire data directory (saves,
    stats, configs) to `~/.local/share/caco/backups/`
  - **`saves restore <query> [BACKUP]`**: Restore from a backup zip; uses
    latest backup if omitted; prompts before overwriting existing files
  - **`saves clean <query>`**: Delete only save files (.dsg, .zds) while
    keeping stats and configs; `--dry-run` for preview
  - **`saves backups [query]`**: List existing backup files; without query
    lists all backups across all WADs; `--plain` for TSV output
- **`saves.py` module**: Core save management logic decoupled from CLI —
  `find_save_files()`, `create_backup()`, `restore_backup()`,
  `list_backups()`, `list_all_backups()`, `clean_save_files()`,
  `resolve_backup_path()`
- **Save extension constants** in `sourceports.py`: `SAVE_EXTENSIONS` dict
  mapping sourceport families to save file extensions, `ALL_SAVE_EXTENSIONS`
  frozenset for unified matching
- **`BACKUP_DIR`** and **`get_backup_dir()`** in `config.py`: Backup
  directory constant and accessor (`~/.local/share/caco/backups/`)
- **Fish completions**: `saves` group with all subcommands and flags
- **Tests**: 24 new tests covering save file discovery, backup/restore,
  backup listing, save cleanup, and backup path resolution

---

## 2026-02-27

Per-session map tracking: see which maps were completed in each play session.

### Added

- **Per-session map tracking**: Each play session now captures stats snapshots
  before and after launching the sourceport, enabling per-session map delta
  tracking
- **`caco sessions` command**: Show play session history with per-session map
  info — displays date, start time, duration, sourceport, and maps completed;
  `--plain` for TSV output, `--yes`/`-y` for auto-select
- **`compute_stats_delta()`** in `wad_stats.py`: Diffs before/after
  `WadStats` snapshots to determine which maps were played in a session;
  handles both stats.txt (persistent/cumulative — detects exit count changes)
  and levelstat.txt (all maps are this session's) semantics
- **`_read_stats_snapshot()`** in `player.py`: Extracted from
  `_auto_track_stats()` — reads stats file to JSON string for reuse
- **`update_session_stats()`** in `db/_sessions.py`: Attaches before/after
  stats snapshots to session records
- **Migration #15**: `stats_before` and `stats_after` TEXT columns on
  `sessions` table
- **Fish completions**: `sessions` command with `--plain` and `--yes`
- **Tests**: 21 new tests covering delta computation, session stats CRUD,
  migration, `_read_stats_snapshot`, and CLI sessions command

### Changed

- **`_auto_track_stats()`**: Now returns the JSON string (or None) in
  addition to storing it, enabling capture of the "after" snapshot

### Removed

- **`TODO/wad-features/multi-file-wads.todo.md`**: Deleted — companion files
  feature fully implemented

---

## 2026-02-27

Final QA pass on CLI rework.

### Fixed

- **`player.py` stale error messages**: `caco link {id}` → `caco modify id:{id} --link`,
  `caco iwad import` → `caco import`
- **Config auto-update**: `manage_data_dirs`, `auto_stats`, `auto_detect_iwad`,
  `auto_detect_complevel`, `cache_max_size_gb`, `cache_max_age_days`, and
  `cache_auto_clean` are now included in `DEFAULT_CONFIG` so they appear in
  the config file via `ensure_config_keys()` — previously they were only
  defined as `.get()` defaults and not discoverable in the config file

### Removed

- **`TODO/cli-rework/`**: All items completed; removed folder

---

## 2026-02-27

Multi-file WAD support, beaten command merge into modify/info, sourceport config profile management, crash detection, and complevel auto-detection.

### Fixed

- **Fish stats completions**: Removed stale `beaten` subcommand guard
  (`and not __fish_seen_subcommand_from list add remove set export`)
- **`ls --iwad` help text**: Fixed stale `caco iwad import` reference → `caco import`
- **`random` docstring**: Fixed stale `caco list` reference → `caco ls`
- **Fish modify completions**: Added missing `description=` and `args=` field suggestions

### Added

- **Companion files**: WADs can now have associated companion files (DEH
  patches, music WADs, extra PWADs) that are automatically loaded on play
- **`--add-file` / `--remove-file` on `modify`**: Add or remove companion
  files by path; `--add-file` resolves to absolute path with dedup;
  `--remove-file` matches by basename or full path
- **DEH/BEX auto-detection**: `.deh` and `.bex` files automatically use
  `-deh` flag for dsda/chocolate/woof/eternity families, `-file` for
  zdoom family
- **`uses_deh_flag()` in `sourceports.py`**: Helper to determine correct
  DEH loading flag per sourceport family
- **Display in all UIs**: Companion files shown in `caco info` (rich,
  plain, JSON), TUI detail/info panels, GUI detail panel, and GUI edit
  dialog
- **GUI edit dialog**: New "Companion Files" field (one path per line)
  in the Launch Config section
- **Migration #14**: `companion_files` TEXT column (JSON array) on `wads`
  table
- **Beaten syntax in `modify`**: `beaten+N` adds N completions, `beaten-N`
  removes N most recent, `beaten=N` sets exact count, `beaten-TIMESTAMP`
  removes by date — all coexist with field=value actions in a single command
- **`modify --notes`**: Annotate completions when adding (`beaten+1 --notes "UV max"`)
- **`modify --stats-file`/`-s`**: Attach stats file to completion; standalone
  use (without beaten action) attaches to most recent or `-b`-targeted completion
- **`modify --date`**: Backdate completions with ISO timestamp
- **`info --levelstats`**: Per-map statistics display (reuses stats helpers);
  shows all entries (live + completions) by default
- **`info --live`**: Show only live stats snapshot
- **`info --plain`**: TSV output for levelstats
- **`info -b TIMESTAMP`**: Target specific completion by timestamp prefix match
- **Completions section in `info`**: Replaces simple "Times beaten: N" with
  date/notes/stats listing; included in JSON and plain output formats
- **DB functions**: `delete_wad_completion_by_timestamp()`,
  `find_completion_by_timestamp()`, `completed_at` parameter on
  `add_wad_completion()`
- **`update_wad()` `record_completion` parameter**: Suppresses auto-completion
  when beaten actions already handle it
- **Crash detection**: `exit_code` INTEGER column on sessions table;
  `PlayResult` dataclass in `player.py` with `crashed` property (non-zero
  exit code); CLI/TUI/GUI warn on crash; session history views show
  "Crash (N)" indicator
- **Complevel auto-detection**: `complevel_detect.py` auto-detects from WAD
  lumps (UMAPINFO→21, DEHACKED with MBF codepointers→11, ExMy-only→2);
  `complevel.py` has shared names/aliases/parser; auto-detected on first play
  and persisted; `-complevel N` auto-injected for dsda-family ports
- **`play --complevel`/`-c`**: Override complevel for a session
- **Config profile management**: Managed sourceport config files stored at
  `~/.local/share/caco/sourceports/{exe}/{profile}.cfg` — caco owns the config
  lifecycle so settings persist cleanly per-sourceport
- **`caco profile` command group**: `ls`, `create`, `edit`, `cp`, `rm`, `path`
  subcommands for managing config profiles
- **`play --config`/`-C` option**: Override config profile for a play session
  (e.g., `caco play --config controller id:1`)
- **Per-WAD `custom_config`**: Store a profile override per WAD via
  `caco modify id:1 config=controller` — automatically used on play
- **`config:` query field**: Search WADs by config profile name
  (e.g., `caco ls config:controller`)
- **Auto-created default profile**: On first play with a dsda-family port, an
  empty `default.cfg` is created and passed via `-config` — the sourceport
  populates it with defaults on first launch
- **Profile injection**: For dsda-family ports, `-config <path>` is always
  injected; resolution order: CLI `--config` > WAD's `custom_config` > `"default"`
- **Config helpers**: `get_sourceport_dir()`, `get_profile_path()`,
  `list_profiles()` in `config.py`; `get_config_args()` in `sourceports.py`
- **TUI/GUI support**: Config profile shown in info panel and editable in
  WAD edit forms
- **Fish completions**: `--add-file` and `--remove-file` for modify command;
  `profile` subcommands, `play --config`, `modify config=`, `ls config:`
  completions
- **Migration #16**: `custom_config TEXT` column on wads table
- **Tests**: 32 new tests covering DEH flag detection, DB migration,
  CLI add/remove/dedup/info output, and player command building with
  companion files

### Removed

- **`beaten` command group**: All 7 subcommands (`list`, `add`, `attach`,
  `remove`, `set`, `stats`, `export`) removed — functionality merged into
  `modify` and `info`

### Changed

- **Shell completions**: Removed beaten subcommand completions; added new
  modify flags (`--notes`, `-s`, `--date`, `-b`, `beaten+`/`beaten-`/`beaten=`)
  and info flags (`--levelstats`, `--live`, `--plain`, `-b`)
- **Stats entry labels**: Use timestamp format ("Completion (2024-06-15 18:30)")
  instead of ID-based ("Completion #42")

---

## 2026-02-27

### Added

- **Hand-crafted bash completions**: Full `_caco()` completion function with
  subcommand detection, dynamic WAD/tag/IWAD/sourceport lookups via
  `caco _complete`, nested `beaten`/`cache` subcommand handling, and file
  fallback for `--link`/`--stats-file` paths
- **Hand-crafted zsh completions**: `_arguments`-based structured completion
  with `_describe` for ID:Title WAD pairs, `_files` for path options, nested
  dispatch for `beaten`/`cache` groups, and combined completion actions
- **Embedded completion scripts**: `src/caco/cli/_completion_scripts.py` module
  stores all three shell scripts as string constants — works from installed
  packages, not just editable installs
- **Convenience copies**: `completions/caco.bash` and `completions/_caco` (zsh)
  alongside the existing `completions/caco.fish`
- **`caco _complete` hidden command**: Purpose-built completion data for shell
  scripts — replaces slow `caco ls -o plain | awk` with direct SQL/registry
  lookups; supports 8 contexts: `wads`, `tags`, `iwads`, `statuses`,
  `sort-fields`, `sourceports`, `modify-fields`, `query-fields`
- **Dynamic fish completions**: `--iwad` on play/trash completes from registered
  IWADs, `--sourceport` on play completes from known sourceport executables,
  `--tag` on import completes from existing tags
- **Fish completion helpers**: `__caco_iwads` and `__caco_sourceports` functions

### Changed

- **Fish completions**: `__caco_wads` and `__caco_tags` now call
  `caco _complete` instead of parsing `caco ls -o plain` output through `awk`

- **`caco completions` command**: Now outputs hand-crafted scripts instead of
  Click's generic completion mechanism; uses `click.echo()` to avoid Rich
  mangling shell `[` brackets in output

---

## 2026-02-27

Auto-enrich imports with Doom Wiki metadata.

### Added

- **Auto Doom Wiki enrichment**: After importing from idgames, Doomworld, URL,
  or local sources, caco automatically searches Doom Wiki for a matching title
  and backfills missing author/year, appends wiki description (with separator),
  and auto-links IWAD — never overwrites existing fields
- **`auto_doomwiki_enrich` config option**: Controls auto-enrichment (default:
  `true`); failures are silently logged and never affect the import result
- **Fuzzy title matching**: `_normalize_title()` / `_titles_match()` helpers
  strip accents, punctuation, and whitespace for reliable title comparison

---

## 2026-02-27

**Breaking**: CLI rework to follow beets conventions more closely.

### Added

- **`modify` command**: Replaces `update` with beets-style `field=value` syntax
  (e.g., `caco modify id:1 status=playing rating=5`); supports `!field` to clear
  fields, `tag=value` to add tags, `!tag` to remove all tags, `!tag:pattern` to
  remove matching tags; `--link PATH` absorbs the old `link` command; `--dry-run`
  for previewing changes
- **`trash` command**: Unified trash management replacing `delete`/`restore` —
  `caco trash <query>` soft-deletes, `--list` shows trash, `--restore` recovers,
  `--purge` permanently deletes, `--iwad FAMILY[/VARIANT]` removes IWADs
- **Inline sort syntax**: `caco ls status:playing playtime-` instead of
  `--sort playtime-`; sort terms extracted from query args by `+`/`-` suffix on
  known fields
- **`ls --tags` flag**: Lists all tags with counts, replacing `tag list`
- **`ls --iwad` flag**: Lists registered IWADs, replacing `iwad list`
- **`iwad:` query field**: `caco ls iwad:doom2` filters by custom_iwad column
- **`play --iwad` option**: `caco play --iwad doom2` replaces `caco play iwad:doom2`
  prefix syntax; supports `FAMILY/VARIANT` format
- **`play --first`/`-1`**: Replaces `--yes`/`-y` for auto-selecting first match
- **`parsing.py` module**: New `cli/parsing.py` with `extract_sort_from_args()`,
  `parse_modify_args()`, `ModifyAction` dataclass, and field validation
- **`link_mode` config**: Controls whether `modify --link` copies or moves files
  (default: "move")
- **DB tag functions**: `remove_all_tags()` and `remove_tags_by_pattern()` in
  `db/_wads.py`

### Changed

- **`list` → `ls`**: `ls` is now the primary command name (not an alias)
- **`--json`/`--plain` → `--output`/`-o`**: Unified output format flag on `ls`,
  `info`, and `trash --list` (`-o json`, `-o plain`)
- **`info` multiple matches**: Now displays all results in sequence instead of
  interactive picker; `--yes` removed
- **`config` default**: Prints raw config text to stdout (pipeable); `--path`
  removed

### Removed

- **`update` command**: Replaced by `modify`
- **`delete` command**: Replaced by `trash`
- **`restore` command**: Replaced by `trash --restore`
- **`link` command**: Replaced by `modify --link`
- **`tag` command group**: Tag management folded into `modify` and `ls --tags`
- **`iwad` command group**: IWAD management folded into `ls --iwad`, `trash --iwad`
- **`rm` alias**: Removed (use `trash`)
- **`--sort`/`-S` flag on `ls`**: Use inline sort syntax instead
- **`config --path`**: Use `caco config` (prints to stdout) instead

---

## 2026-02-27

Fix dsda-family sourceport save directory placement.

### Fixed

- **dsda-family save directory**: For dsda-doom, nyan-doom, nugget-doom, and
  prboom+, saves now go to the nested stats directory
  (`{data_dir}/{exe}_data/{iwad}/{wad_stem}/`) instead of the data dir root —
  keeps saves alongside per-map stats where they belong
- **`get_dsda_save_dir()`**: New function in `sourceports.py` computes the
  nested save path and creates the directory
- **`get_data_dir_args()`**: Now accepts optional `iwad` and `wad_path` keyword
  args; dsda family uses the nested path for `-save` when both are provided,
  falls back to previous behavior otherwise
- **Tests**: 11 new tests for `get_dsda_save_dir()` and nested save dir behavior

---

## 2026-02-27

Playability improvements: direct IWAD play, sourceport detection, config auto-update.

### Added

- **Direct IWAD play**: `caco play iwad:doom2` launches an IWAD directly
  without needing a PWAD in the library — supports `-p`/`--sourceport` and
  extra args (e.g., `-- -warp 1`); no session tracking or WAD record created
- **Sourceport auto-detection**: `detect_sourceports()` in `sourceports.py`
  scans `SOURCEPORT_FAMILIES` executables via `shutil.which()`; play command
  now lists detected sourceports when no sourceport is configured (e.g.,
  "Found on PATH: gzdoom, dsda-doom")
- **Config auto-update**: `ensure_config_keys()` runs on every `load_config()`
  — compares existing config file against `DEFAULT_CONFIG` and section defaults
  (`[tui]`, `[gui]`, `[list]`); adds missing keys with default values; only
  writes if changes are made; recursion-guarded
- **`play_iwad()` function** in `player.py`: standalone IWAD launcher with
  sourceport resolution, config args, and wall-clock duration tracking
- **Example config updated**: added `iwad_dir`, `data_dir`, `manage_data_dirs`,
  `auto_stats`, `auto_detect_iwad`, `[tui]` section, `[gui]` section; fixed
  `cache_dir` path (was `~/.cache/caco/wads`, now `~/.local/share/caco/wads`)
- **Fish completions**: `iwad:` added to play query completions
- **Tests**: 12 new tests for `play_iwad()`, `detect_sourceports()`, and
  `ensure_config_keys()`

---

## 2026-02-27

Reworked `beaten stats` to show all stats entries and added `--live` flag.

### Changed

- **`caco beaten stats`**: Without a COMPLETION_ID, now shows all stats
  entries — live stats first (from `wads.stats_snapshot`), then each
  completion with stats — with section headers and summary lines
- **`caco beaten export`**: Falls back to live stats when no completion
  has stats attached (via `allow_live` in `_find_completion_with_stats()`)

### Added

- **`--live` flag on `beaten stats`**: Show only the current live stats
  snapshot, skipping completion records
- **`--live` flag on `beaten export`**: Export the WAD's live stats
  snapshot instead of a completion's
- **`_build_stats_entries()` helper**: Builds ordered list of stats
  entries (live + completions), matching the GUI/TUI pattern
- **CLI stats tests**: 14 new tests in `tests/unit/test_cli_stats.py`
  covering all beaten stats and export scenarios
- **Fish completions**: `--live` flag for `beaten stats` and `beaten export`

---

## 2026-02-27

Managed IWAD directory restructure and live stats in GUI/TUI.

### Changed

- **IWAD managed path format**: Changed from `{family}_{variant}.wad` to
  `{variant}/{family}.wad` — gives sourceports canonical filenames (e.g.,
  `doom2.wad`, `tnt.wad`) while keeping variants in subdirectories
- **Migration #13**: Automatically moves existing managed IWAD files to
  new directory structure and updates DB paths
- **GUI WadStatsDialog**: Now shows live stats as "Current (live)" entry
  prepended before completion stats in the selector dropdown
- **TUI WadStatsScreen**: Now shows live stats as "Current (live)" entry
  navigable with n/p keys alongside completion stats
- **IWAD remove cleanup**: Now cleans up empty variant subdirectories
  after deleting managed IWAD files; uses `is_relative_to()` for safer
  managed directory detection

---

## 2026-02-26

Automatic per-map stats tracking after play sessions.

### Added

- **Auto stats tracking**: After each play session, caco automatically reads
  `stats.txt` or `levelstat.txt` from the WAD's data directory and stores a
  running stats snapshot on the WAD record (`wads.stats_snapshot` column)
- **Auto-attach stats on completion**: When marking a WAD as beaten via
  `caco beaten add` (without `--stats-file`) or `caco update --status finished`,
  the WAD's live stats snapshot is automatically archived to the completion record
- **Recursive stats file discovery**: Handles nyan-doom's nested directory
  layout (`{iwad}/{wad}/stats.txt`) via recursive search
- **`auto_stats` config option**: Controls auto-tracking (default: `true`);
  requires `manage_data_dirs = true`
- **Migration #11**: `stats_snapshot` TEXT column on `wads` table

---

## 2026-02-26

Per-WAD data directories and sourceport family registry.

### Added

- **Per-WAD data directories**: Each WAD gets an isolated data directory at
  `~/.local/share/caco/data/{id}_{title}/` for saves, stats, and other
  sourceport output — eliminates cross-WAD data conflicts
- **Sourceport family registry** (`sourceports.py`): Hardcoded mapping of
  sourceport executables to CLI flags for data/save directory redirection
  - **dsda family**: dsda-doom, nyan-doom, nugget-doom, prboom+, glboom+ (`-data`, `-save`)
  - **zdoom family**: gzdoom, lzdoom, vkdoom, qzdoom, zdoom (`-savedir`)
  - **chocolate family**: chocolate-doom, crispy-doom (`-savedir`)
  - **woof family**: woof (`-data`, `-save`)
  - **eternity family**: eternity (`-savedir`)
- **Automatic data dir injection**: When playing a WAD with a recognized
  sourceport, caco injects `-data`/`-save` (or `-savedir`) flags to redirect
  output to the WAD's data directory
- **`manage_data_dirs` config option**: Controls per-WAD data directory
  management (default: `true`); set to `false` to use sourceport defaults
- **`data_dir` config option**: Custom base directory for WAD data
  (default: `~/.local/share/caco/data/`)
- **`find_wad_data_dir()`**: Finds existing data directories by ID prefix,
  handling title renames gracefully

---

## 2026-02-26

Managed IWAD storage and WAD cache relocation.

### Added

- **Managed IWAD directory**: IWADs are now copied to `~/.local/share/caco/iwads/`
  on import, giving caco full ownership of IWAD files
- **`caco iwad import`**: unified command replaces `iwad add` + `iwad scan` — handles
  both single files and directory scanning with auto-detection
- **`managed_iwad_filename()`**: canonical naming for managed IWADs (`{family}_{variant}.wad`)
- **`remove_iwad_with_paths()`**: atomic remove + path return to avoid TOCTOU races
- **`get_iwad_dir()`**: configurable managed IWAD directory (`iwad_dir` config key)
- **Migration #10**: automatically relocates WAD cache from `~/.cache/caco/wads/`
  to `~/.local/share/caco/wads/` with file migration and DB path updates

### Changed

- **WAD cache relocated**: default cache directory moved from `~/.cache/caco/wads/`
  to `~/.local/share/caco/wads/` — downloaded WADs are managed library data, not
  ephemeral cache (thumbnail cache stays at `~/.cache/caco/thumbnails/`)
- **`caco iwad remove`**: now also deletes the managed IWAD file (only if inside
  the managed IWAD directory — safe for pre-migration external paths)

### Removed

- **`caco iwad add`**: replaced by `caco iwad import`
- **`caco iwad scan`**: replaced by `caco iwad import <directory>`

---

## 2026-02-26

IWAD management — first-class IWAD registry with family/variant model and
configurable priority resolution.

### Added

- **IWAD registry**: `caco iwad` command group for managing IWADs as first-class
  entities in the database (`iwads` table with family, variant, title, path, MD5)
- **Family/variant model**: IWADs organized by family (doom, doom2, plutonia, tnt)
  with multiple variants per family (v1.9, bfg, enhanced, kex, unity)
- **Priority resolution**: `resolve_iwad("doom2")` returns the preferred variant
  based on a configurable priority list; `[iwad_priority]` config section for
  per-family overrides
- **Cross-family fallbacks**: Freedoom used as last-resort fallback (freedoom2 for
  doom2/plutonia/tnt, freedoom1 for doom)
- **`caco iwad scan`**: Auto-discover known IWADs in `iwad_dirs` by MD5 checksum,
  detecting both family and variant; filename fallback for modded/newer releases
- **`caco iwad add`**: Register an IWAD file with auto-detection (MD5 → family +
  variant), or `--family`/`--variant` overrides for custom IWADs
- **`caco iwad list`**: Display registered IWADs with Family, Variant, Title, Path
  columns; preferred variant marked with `*`; `--plain` TSV output
- **`caco iwad remove`**: Unregister by family + variant, or remove all variants
  of a family (with confirmation prompt)
- **Expanded MD5 database**: ~22 known MD5s across 4 primary families covering
  v1.9, BFG, Enhanced, Unity, and KEX variants
- **IWAD resolution from registry**: `resolve_iwad()` checks the IWAD registry
  with priority resolution before falling back to `iwad_dirs` filesystem search
- **Auto-link on Doom Wiki import**: When a Doom Wiki entry has an IWAD field
  (e.g., "Doom II"), automatically sets `custom_iwad` if that IWAD is registered
- **IWAD alias mapping**: Normalizes free-text IWAD names from wikis/forums to
  family names (e.g., "Doom II: Hell on Earth" → "doom2")
- **New DB functions**: `get_iwad_variant()`, `get_family_iwads()`,
  `get_iwad_priority()`
- **Fish completions**: `iwad` subcommand completions with `--family`/`--variant`
  flags and dynamic family completion for `iwad remove`
- **Migration #8**: `iwads` table creation
- **Migration #9**: Restructure `iwads` table from `name UNIQUE` to
  `(family, variant) UNIQUE` with data migration

---

## 2026-02-26

GUI stats import/export and context menu access.

### Added

- **GUI stats import/export**: WadStatsDialog now has "Import Stats..." and
  "Export Stats..." buttons for loading stats.txt/levelstat.txt files and
  saving stats back to text files directly from the GUI
- **GUI "Map Stats..." context menu**: Right-click any WAD in list or grid
  view to open the per-map stats dialog
- **Always-visible "Map Stats" button**: Detail panel shows the button for
  all WADs (not just those with existing stats), enabling stats import on
  any WAD

### Changed

- **Detail panel stats button**: Renamed from "Stats" to "Map Stats" for
  clarity; now always visible when a WAD is selected
- **WadStatsDialog**: Refactored to support import/export lifecycle — tracks
  changes via `changed` property so callers refresh after import

---

## 2026-02-25

Per-map statistics import/export and session dialog cleanup.

### Added

- **Stats.txt import/export**: Import per-map completion statistics from
  sourceport stats files and attach them to completion records
  - Supports nyan-doom/dsda-doom `stats.txt` format (persistent per-map tracking
    with kills, items, secrets, time, skill, exits, and best-of stats)
  - Supports dsda-doom `levelstat.txt` format (human-readable `-levelstat` output)
  - Auto-detects format; lossless round-trip (parse → store → export matches original)
- **`caco beaten add --stats-file`**: Attach stats when adding a completion
- **`caco beaten attach`**: Attach stats to an existing completion record
- **`caco beaten stats`**: View full per-map statistics table for a completion
- **`caco beaten export`**: Export stats back to original text format
- **`beaten list` Stats column**: Shows indicator when a completion has stats attached
- **GUI WadStatsDialog**: Per-map stats table with completion selector, accessible
  via "Stats" button in detail panel
- **TUI WadStatsScreen**: Per-map stats screen with n/p keys to switch completions,
  accessible via `M` keybinding in library pane
- **`wad_stats.py` module**: Parser, formatter, and JSON serialization for sourceport
  per-map statistics (MapStats/WadStats dataclasses)
- **`db.update_wad_completion()`**: Update stats_snapshot and/or notes on existing
  completion records
- **Fish completions**: Added missing `beaten`, `stats`, `restore`, `link`, `cache`
  command completions, plus new `beaten stats`, `beaten attach`, `beaten export`

### Removed

- **Session Notes column**: Removed unused "Notes" column from GUI and TUI session
  history dialogs (DB schema unchanged for forward-compatibility)

---

## 2026-02-20

Documentation accuracy overhaul and version alignment.

### Fixed

- **`beaten list` docs**: corrected signature from non-existent `--min` flag to
  actual `<query>` argument showing per-WAD completion history
- **`beaten add` docs**: documented `--notes` flag for annotating completions
- **`beaten remove` docs**: documented optional `COMPLETION_ID` argument for
  removing specific records
- **Missing `stats` options**: documented `--period`, `--limit`, and `--plain` flags
- **Missing `update` flags**: documented `--clear-description` and `--clear-version`
- **Missing `import` flags**: documented `--llm-backend` and `--llm-model` options
- **Missing list columns**: added `source`, `filename`, `rating` to available columns
  list in config example
- **Missing `[list] default_status`** config option documented
- **Status shortcuts table**: now shows all shortcuts matching source code
  (`toplay`, `dropped`, `await`, `waiting`)
- **GUI config**: added `detail_panel_width`, `show_detail_panel`, `thumbnail_size`
  to README
- **CLAUDE.md architecture**: added `services/` module and `link_dialog.py`

### Added

- **CI section** in README Development: documents GitHub Actions test matrix and
  mypy type checking
- **Library Statistics section** in README with full `caco stats` usage examples
- **WAD unavailable dialog** documented in GUI features
- `stats --plain` added to Scripting section

---

## 2026-02-20

Major internal quality overhaul: database refactoring, security hardening,
performance improvements, comprehensive test suite, and code modernization
across all layers.

### Added

- **Test suite**: 127 new tests covering DB sessions, batch stats,
  completions, duplicate detection, migration versioning, wikitext/Doomworld
  parsers, CLI integration, config round-trips, and sort parsing
- **CI**: GitHub Actions workflow with Python 3.10/3.11/3.12 matrix
- **Source adapter tests**: 17 mock tests using `respx` for IdgamesSource,
  DoomwikiSource, and DoomworldSource
- **`WadRecord` TypedDict** for WAD dict return types throughout the codebase
- **`ProgressCallback` type alias** in `player.py`
- **`StatsSnapshot` dataclass**: bundles library stats, completion rate, and
  play-period data into a single `get_stats_snapshot()` call
- **`ImportService`**: centralized duplicate-check-and-import for all 5 source
  types, replacing ~15 duplicate blocks across CLI/TUI/GUI
- **`BaseSource` mixin** in `sources/base.py` for shared context-manager
  lifecycle across source adapters
- **Batch wiki fetch**: MediaWiki pipe-separated titles API reduces N+1 HTTP
  requests to 2 (search + batch content) in Doom Wiki search
- **`search_wads()` limit parameter** for efficient random selection
- **Schema migration versioning**: `schema_migrations` table tracks applied
  versions; `init_db()` skips already-applied migrations
- **Ruff and mypy configuration** in `pyproject.toml`
- **`set_query()` and `get_selected_wad_id()`** public API on GUI `LibraryTab`
- 9 public wrapper methods on `LibraryTab`; `MainWindow` no longer accesses
  private attributes

### Changed

- **Split `db.py`** (1593 lines) into `db/` package with 6 submodules
  (`_models`, `_connection`, `_schema`, `_query`, `_wads`, `_sessions`) and
  `__init__.py` re-exporting all symbols for backward compatibility
- **Unified batch stats**: `get_wad_stats_batch()` replaces 4 separate batch
  calls with 2 queries on 1 connection
- **Query chunking**: `_batch_query()` chunks queries to stay under SQLite's
  variable limit (`SQLITE_MAX_VARS=900`)
- **Unified `STATUS_METADATA`** in `db.py`: single source of truth for display
  names, hex colors, Rich colors, CSS classes; TUI/GUI themes derive from it
- **Batch cache cleanup** in `player.py`: uses `get_last_played_batch()` instead
  of N+1 individual calls in `auto_clean_cache()`
- **Decoupled `player.py` from Rich**: removed `Console` parameter; CLI creates
  Rich progress callback in `play_cmd.py` instead
- **Thumbnail extraction** now uses `mmap` for direct WAD file reads, avoiding
  loading entire WADs into memory
- **Random command** uses `ORDER BY RANDOM() LIMIT 1` instead of fetching all rows
- **Download chunk size** increased from 8KB to 256KB
- Moved function-body imports to top-level per PEP 8
- Narrow `except Exception` to specific exception types in doomworld
  adapter/scraper
- `.format()` calls converted to f-strings in CLI interactive picker
- `normalize_status()` made public; CLI delegates to it

### Fixed

- **SQL injection guard**: `ALLOWED_UPDATE_FIELDS` whitelist on `update_wad()`
- **Atomic completions**: completion recording moved inside `update_wad()`
  transaction
- **Sourceport validation**: verify binary existence before subprocess launch
- **Config save bug**: `save_config()` was dropping nested `[tui]`/`[gui]`/`[list]`
  sections
- **Pydantic mutable default**: `download_links` uses `Field(default_factory=list)`
- **Tag query escaping**: `ESCAPE` clause added to non-glob tag `LIKE` queries
- **Grayscale palette**: fixed RGB ordering in thumbnail extractor fallback
- **Thread safety**: `_pending` set in `ThumbnailLoader` protected with
  `threading.Lock`
- **Static SQL**: `get_wads_played_by_period` no longer uses f-string SQL
- **ZIP bomb protection**: 256 MB entry size limit in thumbnail extraction
- Proper `mmap` lifecycle management with try/finally cleanup
- `logger.debug()` added to 5 silent except blocks in thumbnail extractor/loader
- **Shell completions**: added "awaiting-update" to `QUERY_STATUS_VALUES`
- Shared `httpx.Client` across thumbnail scraper workers
- O(1) `wad_id` to row index mapping in GUI `WadTableModel`

### Performance

- SQLite WAL mode, `synchronous=NORMAL`, `cache_size`, `temp_store` PRAGMAs
- Database indexes on `wads(deleted_at)`, `wads(cached_path)`,
  `sessions(wad_id, started_at)`
- `executemany` for bulk completion inserts
- Cached `load_config()` with invalidation on `save_config()`
- Detail panels skip redundant `db.get_wad()` when caller provides WAD data

---

## [1.2] - 2026-02-20

### Added

- **Force-stop dialog** in GUI when a sourceport is already running
- **Wayland support**: set desktop filename and window icon for proper
  Wayland window identification

### Changed

- Sourceport launch uses `Popen` instead of `subprocess.run` so launch
  failures (missing binary, permission denied) are caught before creating
  a session record
- Process handle exposed via `process_ref` for external cancellation

### Fixed

- Coverage artifacts added to `.gitignore`

---

## [1.1] - 2026-02-18

The first major feature release, expanding Caco from a basic CLI tool into a
full-featured WAD library manager with TUI, GUI, multiple import sources,
advanced query syntax, and comprehensive library management.

### Added

#### Import Sources
- **Doom Wiki import** (`caco import --doomwiki`): search and import WADs from
  Doom Wiki with wikitext infobox parsing
- **Doomworld forum import** (`caco import --doomworld`): import WADs from
  Doomworld forum threads with HTML/JSON-LD parsing and optional LLM-powered
  metadata extraction
- **URL import** (`caco import --url`): import WADs from arbitrary URLs
- **Local file import** (`caco import --local`): import from local filesystem
  with batch support (`caco import local *.wad`)
- **Import auto-detection**: `caco add <source>` detects URLs, files, and
  idgames IDs automatically
- **`caco link` command**: attach local files to metadata-only entries (e.g.,
  Doom Wiki imports that lack download URLs)

#### Query System
- **Beets-style query syntax**: `field:value` queries (`id:`, `title:`,
  `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`)
- **OR queries**: comma-separated values (`"status:playing , status:to-play"`)
- **Negation**: `^status:finished` or `-status:finished`
- **Glob patterns**: `tag:caco*` matches cacoward, cacowards, etc.
- **Free text search**: searches title, author, and description
- **Universal query support**: all commands accept queries (info, play,
  update, delete, etc.)
- **Sort suffix notation**: `+` ascending, `-` descending (e.g., `--sort title+`)
- **Interactive picker**: uses `fzf` if available, falls back to numbered selection

#### Library Management
- **Soft delete with trash/restore**: `caco delete` moves to trash,
  `caco restore` recovers, `caco list --deleted` shows trash
- **Version tracking**: `version` column for non-idgames releases
- **`awaiting-update` status**: for WADs waiting on new versions
- **Completion tracking**: `wad_completions` table, `caco beaten` command group
  (list, add, remove, set)
- **Batch operations**: range syntax `3-6,9,11` for update, delete, tag commands
- **Per-WAD config**: `custom_iwad`, `custom_sourceport`, `custom_args` columns
- **Cross-source downloading**: `idgames_id` column allows any WAD to download
  via idgames API
- **`--plain` output**: TSV/key=value format for scripting on list, info, tag
  list, cache list
- **`--json` output**: JSON format for list and info commands
- **`--dry-run` flag**: preview changes on delete, update, tag operations
- **Delete preview**: shows stats before deletion

#### Cache Management
- **`caco cache list`**: show cached files with sizes
- **`caco cache list --orphans`**: show files not tracked in database
- **`caco cache clear`**: remove cached files (specific WADs or `--all`)
- **`caco cache clean`**: remove orphaned files
- **Auto-cleanup**: `cache_max_size_gb`, `cache_max_age_days`, `cache_auto_clean`
  config options
- Cache cleanup only affects idgames sources (local files are never deleted)

#### Statistics
- **`caco stats`**: library statistics overview
- **`caco beaten list`**: view completion counts
- **Playthrough cycles**: when status changes to `finished`, increments cycle;
  map completions tracked per cycle

#### CLI Polish
- **Status shortcuts**: `p`=playing, `f`=finished, `t`=to-play, `b`=backlog,
  `a`=abandoned, `w`=awaiting-update
- **Command aliases**: `add`, `rm`, `ls`, `i`
- **Tag globs**: `--tag cacowards*` supports wildcards
- **Configurable columns** via `[list]` config section
- **`caco config --edit`**: opens config in `$EDITOR`
- **`caco completions`**: install shell completions
- **Fish shell completions**: comprehensive completions for all commands,
  options, and query fields
- **Download progress bar**: filename, progress bar, size, and transfer speed
- **Play by query**: `caco play filename:tnto`
- **Play most recent**: `caco play` with no arguments plays most recently
  played WAD
- **Update metadata**: `--title`, `--author`, `--year`, `--description` flags
- **Source URL display**: shows source URL and link instructions when WAD file
  is missing

#### TUI (`caco --tui`)
- **Textual-based TUI** with tabbed interface (All, Playing, To-Play,
  Finished, Backlog, Other)
- **Vim keybindings** in WAD table
- **Sort dropdown**: ID, Title, Author, Playtime, Last Played, Year, Rating
- **Edit screen**: inline WAD metadata editing
- **idgames search pane**: search and import from TUI
- **Doom Wiki search pane**: search and import from TUI
- **Doomworld import pane**: URL-based import from TUI
- **URL and local import panes**: additional import sources in TUI
- **WAD info panel**: detailed view with batch-fetched stats
- **Filter input**: real-time search/filter
- **Status-colored display**: centralized theme in `tui/theme.py`

#### GUI (`caco --gui`)
- **PySide6-based GUI** with dark Doom-inspired theme
- **Dual view modes**: list view (`QTableView`) and grid view (`QListView`
  with custom card delegate)
- **Thumbnail support**: TITLEPIC extraction from WAD files + Doom patch
  decoder + Doom Wiki image scraping
- **Two-tier thumbnail caching**: in-memory dict + filesystem at
  `~/.cache/caco/thumbnails/`
- **Detail panel**: right sidebar with thumbnail, metadata, stats, action buttons
- **5-source import tab**: idgames, Doom Wiki, Doomworld, URL, local
- **Edit dialog**: WAD metadata editing form
- **Delete dialog**: confirmation with WAD stats
- **Sessions dialog**: session history table
- **Stats dialog**: library statistics overview
- **Cache dialog**: cache management
- **Saved searches on tab bar**: random sort + persisted filter state
- **Proportional column sizing**
- **Context menu and vim keys** in list view
- **Window geometry persistence** via `QSettings`

#### Configuration
- **TOML config file** at `~/.config/caco/config.toml`
- **Configurable `db_path`**: custom database location with tilde expansion
- **`iwad_dirs`**: IWAD directory search paths (use short names like `doom2`
  instead of full paths)
- **Sourceport PATH resolution**: resolve sourceport names via `$PATH` lookup
- **`[tui]` section**: `default_tab`, `default_sort`, `default_sort_desc`
- **`[gui]` section**: `default_tab`, `default_sort`, `default_sort_desc`,
  `default_view`, `window_width`, `window_height`, `detail_panel_width`,
  `show_detail_panel`, `thumbnail_size`
- **`[list]` section**: configurable columns, colors, sort
- **Cache config**: `cache_max_size_gb`, `cache_max_age_days`, `cache_auto_clean`

#### Other
- **Desktop launcher** with icon (`.desktop` file)
- Vendored idgames API client (no external dependency)
- **Renamed "wishlist" status to "to-play"** with data migration
- Last played tracking in list and info output

### Changed

- Full-scope codebase refactoring for maintainability
- CLI split into submodules (`library.py`, `import_cmds.py`, `tags.py`,
  `play_cmd.py`, `cache.py`, `config_cmd.py`, `stats.py`)
- Default list sorting changed to ID ascending (was status priority)
- Import command uses flag-based source selection

### Fixed

- `filename:` query filter in `search_wads()`
- `cached_path` parameter missing from `add_wad()`
- Old vBulletin URL format for Doomworld import
- Wiki thumbnail scraping: User-Agent header added to API requests
- Thumbnail thread safety
- Sort behavior for nullable fields
- Code review findings: hardening, safety, and deduplication

---

## [1.0] - 2026-01-24

Initial release of Caco.

### Added

- **SQLite database** for WAD metadata and play session tracking
- **Import from idgames archive**: search and import WADs with full metadata
- **Playtime tracking**: automatic session recording via sourceport wrapper
- **Tag-based organization**: add/remove/list tags on WADs
- **Status tracking**: wishlist, backlog, playing, finished, abandoned
- **CLI commands**: list, info, import, play, update, delete, tag
- **Rich terminal output**: colored tables, formatted info display
- Click-based CLI framework
- httpx-based HTTP client for idgames API
- Pydantic models for API response validation
