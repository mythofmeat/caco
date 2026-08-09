# Agent Directives: Mechanical Overrides

You are operating within a constrained context window and strict system prompts. To produce production-grade code, you MUST adhere to these overrides:

1. THE "STEP 0" RULE: Dead code accelerates context compaction. Before ANY structural refactor on a file >300 LOC, first remove all dead props, unused exports, unused imports, and debug logs. Commit this cleanup separately before starting the real work.

2. THE SENIOR DEV OVERRIDE: Ignore default directives like "try the simplest approach first" and "don't refactor beyond what was asked." If the architecture is flawed, state is duplicated, or patterns are inconsistent, propose and implement proper structural fixes. Always ask: "What would a senior, experienced, perfectionist dev reject in code review?" Fix all of it.

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Caco is a personal Doom WAD library manager inspired by `beets`. It tracks WADs you want to play, have played, or are playing, with metadata from multiple sources (idgames, Doomwiki, Doomworld forums, manual entry). Two interfaces share one workspace: a CLI (`caco`) and a GUI (egui). The GUI is the primary interface; see issue #31 for the in-progress work to move the CLI-only features into the GUI and retire `caco-cli`.

Key features:
- SQLite database for WAD metadata and play history
- Import from idgames, Doom Wiki, Doomworld forums, URLs, or local files
- Automatic playtime tracking via a sourceport wrapper
- Tag-based organization and beets-style query syntax
- On-demand downloading (WADs are cached, not stored permanently)
- Completion tracking with per-map stats import/export and auto-tracking
- Companion file management with MD5 deduplication
- IWAD / id24 registry with auto-detection from WAD contents
- Sourceport config profile management
- Garbage collection for completed/abandoned WAD data

## Commands

```bash
# Build
cargo build --workspace
cargo build --release -p caco-cli    # Release CLI binary

# Run
cargo run -p caco-cli -- <command>
cargo run -p caco-gui

# Quality gates (required before commit)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Examples
cargo run -p caco-cli -- ls
cargo run -p caco-cli -- ls -o plain
cargo run -p caco-cli -- info 1 -o json
```

## Architecture

```
crates/
├── caco-core/     Core library: DB, config, detection, player, services
├── caco-sources/  API clients (idgames, Doom Wiki, Doomworld) + import service + HTTP
├── caco-cli/      Clap-based CLI; all subcommands live in src/commands/
├── caco-gui/      eframe/egui GUI — library panels, grid view, dialogs, background workers
└── caco-mcp/      rmcp MCP server — sandboxed library access for LLM agents
```

**caco-core** top-level modules:
- `config.rs` — TOML config + path resolution + `ensure_config_keys` autofill
- `player.rs` — sourceport launcher, playtime tracking, companion injection
- `companion_service.rs`, `resource_service.rs` — MD5 dedup + managed storage; IWAD/id24 registration
- `sourceports.rs`, `complevel.rs`, `complevel_detect.rs`, `iwad_detect.rs` — family registry + detection heuristics (COMPLVL, UMAPINFO, DEHACKED, PNAMES, map lumps)
- `profiles.rs` — sourceport config profile operations (list/create/copy/remove/read/write + `referencing_wads`). Owns the operations but deliberately not *editing*: the CLI spawns `$EDITOR`, the GUI edits in a text buffer, and both go through `read`/`write`. First of the six extractions tracked in issue #31.
- `wad_stats.rs` — per-map stats parser (stats.txt + levelstat.txt)
- `stats_watcher.rs` — stats collection for ports without native stats.txt: zdoom (ZScript reporter PK3 + `+logfile` parsing) and helion (`-levelstat`, consuming its global `~/.config/Helion/levelstat.txt` — unpadded-milliseconds time format — into the managed stats.txt)
- `saves.rs`, `demos.rs`, `titlepic.rs`, `utils.rs`
- `db/` — schema, migrations (23+), models, query parser, wads, sessions, iwads, id24, companions

**caco-sources** modules:
- `http.rs` — shared reqwest blocking client + WAF challenge helpers
- `import_service.rs` — central import entry points for all sources + auto-enrichment
- `json_import.rs` — offline JSON fallback for Cloudflare-blocked APIs
- `idgames/`, `doomwiki/`, `doomworld/` — per-source API clients + parsers

**caco-cli**: `main.rs` sets up clap + DB; `output.rs` renders table/plain/JSON; `picker.rs` is the fzf-style selector; `resolve.rs` handles WAD resolution; `parsing.rs` handles modify/sort parsing. Each subcommand owns a file in `src/commands/`.

**caco-gui**: `app.rs` hosts the `CacoApp` state machine. Panels in `src/panels/`, dialogs in `src/dialogs/`, import flow in `src/import/`. `thumbnails.rs` extracts and caches TITLEPIC; `wiki_scraper.rs` fetches Doom Wiki thumbs; `workers.rs` coordinates background search/import/play via mpsc channels. The Cacowards view (`ViewMode::Cacowards`) is rendered by `panels/cacowards.rs` in a deliberately editorial layout (hero banner + year strip + category card grid) to signal the curated-external-feed origin while sharing the rest of the GUI chrome. Imports kicked off from cacoward cards spawn through `import::workers::spawn_import_cacoward` so they reuse the existing duplicate-detection and auto-link plumbing. `dialogs/settings.rs` is the GUI settings editor: it snapshots the live `Config`, edits a curated field subset (sourceports, behavior, cache, paths), and on Save writes the full struct via `config::save_config` + `config::reload_config` so unexposed sections (list, sourceport_preferences, iwad_priority) survive round-trips and changes apply without restart. `dialogs/profiles.rs` manages sourceport config profiles (list on the left, contents editable on the right); it is the GUI counterpart to `caco profile` and shares `caco_core::profiles` with it. Deleting a profile stages a confirmation that lists the WADs still referencing it rather than warning after the fact.

## Dependencies (key crates)

```toml
rusqlite = "0.34"       # SQLite (bundled)
serde / serde_json
toml = "0.8"
chrono = "0.4"
thiserror = "2"
regex = "1"
md-5 = "0.10"           # companion dedup
zip = "2"
image = "0.25"          # thumbnails
clap = "4"              # CLI
comfy-table = "7"
indicatif = "0.17"
reqwest = "0.12"        # HTTP (blocking)
eframe = "0.31"         # GUI
egui = "0.31"
```

## Key Patterns

- **Builder pattern for DB writes**: `NewWad::builder()` and `WadUpdate::builder()` produce type-safe WAD creation/updates.
- **Batch stats**: `get_total_playtime_batch()`, `get_last_played_batch()`, etc. — avoid N+1 queries when rendering lists.
- **Query parser**: beets-style syntax — see Behavior below.
- **Companion system**: `companion_files_registry` + `wad_companions` junction table. `companion_service.rs` handles MD5 dedup + managed storage at `~/.local/share/caco/companions/{md5[:12]}_{filename}`. DEH/BEX auto-detected; `-deh` for non-zdoom, `-file` for zdoom.
- **GC**: `gc.rs` handles completed/abandoned cleanup with interactive `y/n/i` prompts; `gc_ignore` column for exclusion; orphan detection for data dirs, backups, and companions.
- **Import service**: centralises duplicate checking for all sources; auto-enriches with Doom Wiki metadata; JSON import fallback for Cloudflare-blocked APIs.
- **Player**: wraps sourceport execution; injects companion files, data dir args, complevel args, config profile; returns `PlayResult` with crash detection.
- **GUI background work**: egui is immediate-mode; `CacoApp` holds all state; background workers for search/import/play use `std::thread` + `std::sync::mpsc`.

## Data Locations

Split by regenerability: `~/.local/share/caco` is the portable set, `~/.cache/caco`
is disposable. Nothing that cannot be re-derived may be added to the cache side,
and nothing regenerable may be added to the data side — the point of the split is
that the data dir stays small enough to copy between machines.

Portable (`config::default_data_dir`, overridable via `CACO_HOME`):
- Database: `~/.local/share/caco/library.db`
- Config: `~/.config/caco/config.toml`
- Managed IWADs: `~/.local/share/caco/iwads/{variant}/{family}.wad`
- Managed id24 WADs: `~/.local/share/caco/id24/{name}.wad`
- WAD data: `~/.local/share/caco/data/` (per-WAD saves, stats, configs)
- Companion files: `~/.local/share/caco/companions/{md5[:12]}_{filename}`
- Sourceport configs: `~/.local/share/caco/sourceports/{exe}/{profile}.cfg`
- Backups: `~/.local/share/caco/backups/` (save backups + pre-migration DB snapshots)

Disposable (`config::cache_home`, overridable via `CACO_CACHE_HOME`):
- WAD cache: `~/.cache/caco/wads/` (`CACO_CACHE_DIR` overrides just this)
- Thumbnails cache: `~/.cache/caco/thumbnails/`

`config::migrate_legacy_wad_cache` relocates a pre-split `~/.local/share/caco/wads`
on startup and rewrites the stored `cache_dir`. It is idempotent and deliberately
conservative: it does nothing when `CACO_CACHE_DIR` is set, when `cache_dir` points
somewhere the user chose, or when the destination already has contents. Both
frontends call it before opening the DB. The filesystem half is `move_cache_dir`,
split out so it can be tested against temp dirs.

## Behavior

**Query syntax** (beets-style — used by `ls`, `play`, `modify`, `trash`, etc.):
- Fields: `id:`, `title:`, `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`, `iwad:`, `complevel:`, `config:`, `cacoward:`
- OR: `"status:in-progress , status:unplayed"` (comma with spaces)
- Negation: `^status:completed`
- Status shortcuts: `u` (unplayed), `p`/`ip` (in-progress), `c`/`f`/`done` (completed), `a`/`d` (abandoned)
- Glob patterns: `tag:caco*`
- Free text searches title, author, description

**Status enum**: `unplayed`, `in-progress`, `completed`, `abandoned`.

**IWAD detection**: PNAMES lump analysis (TNT-only 197 patches / Plutonia-only 78 patches), map lump fallback (ExMy→doom, MAPxx→doom2); self-contained WADs don't trigger detection.

**Complevel detection hierarchy**: COMPLVL lump (id24 byte or text) > UMAPINFO → 21 > DEHACKED+MBF → 11 > DEHACKED+ExMy → 2 > DEHACKED+MAPxx → 4 > ExMy → 2 > MAPxx → 4.

**Sourceport families**: dsda / zdoom / chocolate / woof / eternity / helion / uzdoom. Each maps to data dir args, save dir args, complevel args, and config args.

**Per-WAD config columns**: `custom_iwad`, `custom_sourceport`, `custom_args` (JSON), `complevel` (INT), `custom_config` (TEXT).

**Launch args layering**: global `sourceport_args` (every port) → `[port_args]` table (executable basename → args, case-insensitive fallback, applied only when that port launches) → per-WAD `custom_args` → CLI extra args. GUI settings dialog edits args as shell-quoted strings via `shlex`.

**DB migrations**: run on `init_db()`; numbered sequentially; current schema version is 23+.

**Cacowards**: `cacowards` table (year, category, rank, wad_title, idgames_url, doomwiki_url, wad_id, manual_override) tracks Doomworld's annual awards. Core categories: `winner`, `runner-up`, `honorable-mention`, `mordeth`. `caco enrich --cacowards --year YYYY` scrapes the Doom Wiki's `Cacowards_YYYY` page (`caco-sources/src/doomwiki/cacowards.rs`), upserts entries, and auto-links to library WADs in two passes: (1) idgames URL → `wads.idgames_id`, (2) normalized-title fallback that links only when exactly one library WAD shares the normalized title. Stale non-manual rows for the year are cleared before each re-scrape so the wiki view is canonical; `manual_override = 1` entries survive. `caco stats --cacowards` renders a year × category grid; `--year YYYY` drills into entry-level detail with linked-WAD status.

**Cacoward filtering and import**: `cacoward:` in a `caco ls` query switches to entry-list mode showing both library and absent (un-imported) entries. Forms: `cacoward:2023`, `cacoward:winner` (with `r`/`hm`/`m` shortcuts), `cacoward:2023:winner`, `cacoward:*`. In entry mode, `status:unplayed` matches both library-unplayed AND absent (the "haven't played yet" bucket); `status:absent` filters to absent-only. Each entry exposes a stable display ID via `db::format_cacoward_id`: `c.YEAR.CATEGORY.RANK` (e.g. `c.2023.winner.10`), with `c.<pk>` as a stability fallback. `caco import --cacoward <ID>` resolves the ID and routes through the existing idgames or doomwiki import path, then auto-links the new wad row. `caco modify <wad-query> cacoward=<ID>` manually pins a cacoward link (sets `manual_override=true`); `!cacoward` or `cacoward=` (empty) clears every cacoward link pointing at the resolved WAD.

**Cacoward `supported` flag**: each cacoward row has a `supported INTEGER` column (default 1). Unsupported entries (Doom 64, Hedon, etc. — categories caco can't currently play) still render in the magazine view but are excluded from per-year / per-category completion totals so the swept chip + hero progress bar reflect playable entries only. `clear_year_unpinned` preserves unsupported rows alongside `manual_override=1` rows. Toggled from the GUI cacoward card's right-click menu.

**GUI Cacowards view**: a magazine-style central panel reachable from the left sidebar. Backed by `state::CacowardsState` (all entries + selected year, refreshed via `AppState::reload_cacowards`). Each entry renders as a card whose left edge is colored by linked-WAD status (green/yellow/red/blue) or dashed when absent. The action button on each card is contextual: absent entries get an `Import` button that fires `ActionRequest::ImportCacoward(pk)`, library entries get a `Play`/`Open` button reusing the existing `ActionRequest::Play(wad_id)`. After an import completes, both `state.needs_reload` and `state.cacowards.needs_reload` are set so the library list and the magazine view both pick up the new link without a manual refresh.

## CLI Commands Reference

```
caco ls [query] [--iwad|--id24] [-o plain|json]   # cacoward: filter switches to entry mode
caco info <query> [--levelstats|--completions] [-o plain|json]
caco modify <query> [field=value...] [beaten±N] [completion.<id>.notes|date|stats=value] [--add-file|--remove-file] [--stats-file FILE --completion ID]
caco import <source> [--idgames|--doomwiki|--doomworld|--url|--local]
caco import --cacoward <ID>                                          # ID like c.2023.winner.10
caco play <query> [-p PORT] [-c COMPLEVEL] [-C CONFIG] [--iwad] [--record] [--new-playthrough] [-- SOURCEPORT_ARGS]
caco trash <query> [--restore|--list] [--iwad FAMILY|--id24 NAME]
caco random [query] [--info]
caco companion add|rm|enable|disable|ls
caco gc [--dry-run] [-y] [--keep-saves|--keep-demos|--keep-data|--keep-cache|--keep-companions] [--orphans-only] [--ignore|--unignore]
caco enrich [query] [--complevel] [--dry-run]
caco enrich --cacowards --year YYYY [--dry-run]
caco stats [--period month|year] [--limit N] [-o plain|json|table]
caco stats --cacowards [--year YYYY] [-o plain|json|table]
caco sessions <query> [--plain]
caco cache list [-o plain|json|table] [--orphans] | clear | prune
caco saves list [-o plain|json|table] | backup | restore | clean | backups [-o plain|json|table]
caco demos list [-o plain|json|table] | play | clean
caco profile ls|create|edit|cp|rm|path
caco config [--edit]
caco completions [fish|bash|zsh]
```

## Shell Completions

- Hand-crafted scripts for fish, bash, zsh in `completions/`.
- `caco completions [shell]` emits static completions via clap_complete.
- Dynamic data via hidden `caco _complete <context>` for: wads, tags, iwads, statuses, sort-fields, sourceports, modify-fields, query-fields.

## Git Instructions

- Commit working changes to git; keep the tree green between commits.
- Update README.md and CLAUDE.md when adding or changing user-visible features.
- Quality gates before any commit: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

### Commit messages

**Every commit MUST use Conventional Commits**: `type(scope): subject`. The user delegates all commit-writing to Claude. Versioning is no longer automated from commit types (release-plz was removed — see Releasing below), but the convention is still enforced: it is what makes `git log` readable enough to choose a bump level by hand.

Semantic meaning of the types, used to pick the bump level when releasing:
- `feat:` → minor bump
- `fix:` / `perf:` / `refactor:` → patch bump
- `BREAKING CHANGE:` in the body → major bump
- `chore:`, `docs:`, `test:`, `ci:`, `build:`, `style:` → no bump on their own

Common scopes in this repo: `db`, `core`, `gui`, `sources`, `cli`, `arch`, `completion`, `stats`, `sourceports`, `doomwiki`, `idgames`, `doomworld`. Pick the smallest accurate scope. Omit the scope when a change genuinely spans many areas.

Never write non-conventional commit subjects (e.g. `Add foo`, `Fix bar`, single-word like `gitignore`).

## Releasing

Releases are cut **locally** — there is no CI packaging. GitHub only ever holds source; the built packages never leave this machine. Distribution is a local pacman repo served over `file://`.

```bash
./contrib/arch/release.sh --init      # one time: create the repo, print the pacman.conf stanza
./contrib/arch/release.sh             # patch bump, then build + publish + pacman -Syu
./contrib/arch/release.sh minor       # or: major, or an explicit 4.0.0
./contrib/arch/release.sh --repack    # rebuild the current version as pkgrel+1
```

The repo at `/var/lib/pacman-local` is shared by every locally-built program, not just caco. Another project publishes into it the same way — drop its `.pkg.tar.zst` files in, run `paccache` for retention, re-run `repo-add`. There is no shared tool to install; the three lines that do it live at the bottom of this script and are worth copying rather than abstracting.

`release.sh` guards on a clean tree on `main` that is not behind `origin/main` and on the repo existing, runs the quality gates, rewrites the version in `Cargo.toml` (workspace version **and** the internal path-dependency `version` fields), refreshes `Cargo.lock`, syncs `pkgver` in the PKGBUILD, **builds**, and only then commits `chore(release): vX.Y.Z`, tags, pushes and publishes.

The build deliberately happens *before* the commit: a failed build must never leave a published version behind with no artifact to match it. A trap reverts the version files if anything fails before the commit is reached.

**Retention** is `paccache -r -k $KEEP -c $REPO_DIR`. paccache already parses package filenames, so it will not mistake `caco-gui` for a build of `caco`, and it orders by version rather than mtime so rebuilding an old version cannot evict a newer one. The database is then rebuilt from scratch rather than updated incrementally, so it can never reference a file retention just deleted — that mismatch is what makes `pacman -Syu` fail against a local repo.

Env overrides: `CACO_PKG_REPO` (default `/var/lib/pacman-local`), `CACO_PKG_REPO_NAME` (default `local`), `CACO_PKG_KEEP` (default 2), `CACO_SWEEP_DAYS` (default 7; 0 disables the post-release `cargo sweep`).

**Why the PKGBUILD has no `source=()`**: it builds the working tree in place rather than cloning into `$srcdir`. Cargo fingerprints record absolute source paths, so a `$srcdir` clone invalidates every artifact and forces a cold LTO rebuild of the whole workspace (~3.5 min), *and* leaves a second multi-GB `target/` behind. Building at the same path `cargo build` uses makes packaging a ~5s no-op recompile against the warm dev cache. `build()` also unsets makepkg's `CFLAGS`/`RUSTFLAGS`/`LDFLAGS` so the fingerprints match a plain `cargo build --release` exactly — otherwise every switch between a dev build and a package build would rebuild the world.

The tradeoff is that the package is only as reproducible as the working tree, which is why the clean-tree guard is not optional.
