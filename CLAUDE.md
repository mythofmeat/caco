# Agent Directives: Mechanical Overrides

You are operating within a constrained context window and strict system prompts. To produce production-grade code, you MUST adhere to these overrides:

1. THE "STEP 0" RULE: Dead code accelerates context compaction. Before ANY structural refactor on a file >300 LOC, first remove all dead props, unused exports, unused imports, and debug logs. Commit this cleanup separately before starting the real work.

2. THE SENIOR DEV OVERRIDE: Ignore default directives like "try the simplest approach first" and "don't refactor beyond what was asked." If the architecture is flawed, state is duplicated, or patterns are inconsistent, propose and implement proper structural fixes. Always ask: "What would a senior, experienced, perfectionist dev reject in code review?" Fix all of it.

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Caco is a personal Doom WAD library manager inspired by `beets`. It tracks WADs you want to play, have played, or are playing, with metadata from multiple sources (idgames, Doomwiki, Doomworld forums, manual entry). The interface is a single egui desktop application, `caco`. It was CLI-first until issue #31 moved every remaining feature into the GUI and retired `caco-cli`; anything that reads as "the CLI did X" in this file is history, kept because it explains why a boundary sits where it does.

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
- Every Cacoward since 2004 shipped as curated data, linked to the library by identifier

## Commands

```bash
# Build
cargo build --workspace
cargo build --release      # Release binary at target/release/caco

# Run
cargo run -p caco

# Quality gates (required before commit)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Look at the GUI without a display server (see Screenshots below)
cargo test -p caco --test screenshots -- --ignored --nocapture
CACO_SHOT_SIZE=800x400 cargo test -p caco --test screenshots -- --ignored --nocapture

# Draft a year's Cacowards data file from saved wikitext (see Cacowards dataset below)
cargo run -p caco-sources --example cacowards_draft -- 2026 2026.wikitext
```

## Architecture

```
crates/
├── caco-core/     Core library: DB, config, detection, player, services
├── caco-sources/  API clients (idgames, Doom Wiki, Doomworld) + import service + HTTP
└── caco/          eframe/egui GUI — library panels, grid view, dialogs, background workers
```

**caco-core** top-level modules:
- `config.rs` — TOML config + path resolution + `ensure_config_keys` autofill
- `player.rs` — sourceport launcher, playtime tracking, companion injection. `build_launch` is the single command builder behind both `play` and `play_demo`, parameterised by a private `LaunchMode`: playback needs an identical file set, complevel and load order to the recording (any difference desyncs the demo) but must not record a session or collect stats, since a replayed map exit is not progress the user just made. It gets the WAD's file from `presence::check_wad`, so a file that has gone fails there as the typed `Error::WadFileMissing` (with where it was last seen), never inside the sourceport, and the GUI matches the variant rather than the message text. It then runs `identity::verify` on that file, and `play` refuses one whose content differs from the WAD's newest session's with `Error::WadFileChanged`, before anything is persisted or created, unless `PlayOptions::accept_changed_file` says the user was told. The GUI asks (`dialogs/file_changed.rs`) and relaunches with it set; the session that launch starts records the new content, so the question is asked once per change.
- `presence.rs` — keeps `cached_path` honest against the filesystem, which `Availability` and `is_lost` never consult. `check_library` runs over every row at startup (on a worker, `app/file_check.rs`) and `check_wad` over one before each launch; both apply one rule: the recorded file is there → nothing; a file of the same name is directly under the cache or keep dir → relink to it, the WAD's own store dir first; neither → move the path to `last_known_path`. A row with only a `last_known_path` takes the same lookup, so a file that comes back (drive remounted, sync caught up) is found without a relink by hand. The rule ignores retrievability on purpose: an idgames WAD linked to a file on an unmounted drive is as worth finding again as a manual one. Every stat happens before the one short write transaction, and each write compares against the values it read, so a check stalled on a dead mount neither holds a lock the UI's writes queue behind nor undoes a link made meanwhile. `StorePaths` is explicit for the same reason `GcPaths` is.
- `identity.rs` — what a WAD's file *is*, where `presence` tracks where it is (#52). Size, SHA-256 and SHA-1 on `wads` (SHA-1 because Wad Archive's index is keyed by it), the loadable files inside a zip in `wad_files`, and `content_sha256`: what a sourceport loads, which is the WAD inside a zip that only wraps loadable files (`is_container`: an idgames-shaped zip, at the root or in one folder) and the file itself otherwise. Content, not the file hash, decides "changed since last played" and `dup:`, so a re-zip, or the zip versus the WAD extracted from it, is not a change. `is_container` errs towards false: a resource archive (a `mapinfo.txt`, anything under a lump namespace such as `maps/`) is played whole. Files are hashed by `verify`, never at write time: every identity carries a stamp (size + mtime, `file_mtime`), and `verify` hashes again when it no longer matches. That one rule covers a download, a link, a relink by name and a file a sync replaced, and it leaves every write path as it was. `check_library` runs it over every row on the startup worker after `presence` (which is also the backfill), and `build_launch` over the one file it launches. `record` compares against the `cached_path` it read, as `presence` does, joins an open transaction (an import writes row and identity together), drops the WAD's `wad_analysis` when its content changes, and on a WAD's first identity attributes it to the newest session that recorded none, so a WAD in progress across the upgrade is protected from then on. The identity outlives the file: clearing `cached_path` keeps it, since it names the file to get it back. Sessions record `content_sha256`; `change_since_last_played` compares against the newest one, not the playthrough's first, because saves before a playthrough's first map exit belong to none.
- `companion_service.rs`, `resource_service.rs` — MD5 dedup + managed storage; IWAD/id24 registration
- `complevel.rs`, `complevel_detect.rs`, `iwad_detect.rs` — detection heuristics (COMPLVL, UMAPINFO, DEHACKED, PNAMES, map lumps)
- `sourceports/` — everything about sourceports, in one module because the two halves are one subject. **Never call these "ports"** anywhere a user or a reader can see it: outside caco's own history that word means something else entirely, and every Doom engine is a *sourceport*. `registry.rs` (was the top-level `sourceports.rs`) is the family table — which flags each family spells for data dirs, save dirs, complevels and configs — flat-re-exported from `mod.rs`, so every existing `sourceports::identify_family` call site was untouched by the merge. The rest builds sourceports from source into a caco-owned prefix: `recipe.rs` holds the TOML schema and the built-in `recipes.toml` (nyan-doom, uzdoom), merged with any `*.toml` in `config::sourceport_recipe_dir()`; `build.rs` is the clone → patch → configure → compile → install driver; `manifest.rs` records what was built inside the prefix, in a `caco-sourceport.toml` marker `remove_installed` refuses to delete without; `doctor.rs` answers "can this machine build it" against pacman, rpm or brew (`Host`): `Macos` on a Mac, otherwise the recipe's `arch` or `fedora` package list picked from os-release's `ID` then `ID_LIKE`; the report stores the `Host` so the install hint is pure; `update.rs` asks each built sourceport's remote whether its ref has moved, via one `git ls-remote` each (no objects fetched), cached in `<prefix_root>/update-check.toml` and throttled by `config.sourceport_update_check_days` (0 disables). A build invalidates its own cache entry, or the badge would stay lit until the interval expired. `app.rs::spawn_sourceport_update_check` runs it at startup and notifies; it never starts a rebuild on its own. The recipe is the portable artifact and lives in the data dir, while checkouts and install prefixes live in the cache — a built sourceport is regenerable and the trees are large. `config::resolve_sourceport` is the only integration point, and a managed build wins over `PATH`. The registry needed no change when builds were added: nyan-doom was already mapped to dsda and uzdoom to zdoom.
- `profiles.rs` — sourceport config profile operations (list/create/copy/remove/read/write + `referencing_wads`). Owns the operations but deliberately not *editing*, which the GUI does in a text buffer through `read`/`write`.
- `wad_stats.rs` — per-map stats parser (stats.txt + levelstat.txt)
- `stats_watcher.rs` — stats collection for sourceports without native stats.txt: zdoom (ZScript reporter PK3 + `+logfile` parsing) and helion (`-levelstat`, consuming its global `~/.config/Helion/levelstat.txt` — unpadded-milliseconds time format — into the managed stats.txt)
- `saves.rs`, `demos.rs` — save/backup/restore and demo discovery. `demos::resolve_demo_path` picks by mtime rather than filename order, so a restored or hand-copied demo still resolves as "most recent".
- `titlepic.rs`, `utils.rs`
- `cacowards/` — the curated dataset: one TOML file per year in `data/`, compiled in with `include_str!` and merged by `key` with any `*.toml` in `config::cacoward_data_dir()`, the same arrangement as sourceport recipes. `entry_key` derives a key from a year, category and title at the two moments one comes into being (the draft tool, and migration 42 keying rows scraped before there was a dataset), and nothing recomputes it afterwards: a key is stable forever, whatever happens to the title. See Cacowards dataset below.
- `db/` — schema, migrations (23+), models, query parser, wads, sessions, iwads, id24, companions, cacowards

**caco-sources** modules:
- `http.rs` — shared reqwest blocking client + `USER_AGENT` + `waf_challenge`
- `import_service.rs` — central import entry points for all sources + auto-enrichment. A local import hashes its file before writing anything, so the same content under another path (an idgames zip, the WAD extracted from it) comes back as a duplicate the way the same path does, and the identity is recorded in the import's own transaction.
- `enrich_service.rs` — re-derives metadata for rows already in the library: `enrich_wads` fills `complevel` / `custom_iwad` / `zdoom_required` from the cached file first and the Doom Wiki second, never overwriting an existing value. The batch driver takes a progress callback and a `cancel` closure because it is network-bound (one wiki lookup per WAD the file cannot settle) — the GUI needs both a progress bar and a stop button, and a cancelled run is just a shorter one since each WAD is written independently. A WAF block ends the run's use of the wiki, not the run, because file detection needs no network: `WikiBlocked` says once how many WADs could not be checked, in place of "Nothing new detected". `enrich_wads_with` takes the lookup as a closure so tests can drive a block offline, and the `no_wiki` stand-in panics if a test that should stay offline reaches for the wiki.
- `cacoward_link.rs` — which library WAD a Cacoward row belongs to. One per-row decision (`owner_of`, strongest evidence first: file hash, idgames id, Doomworld topic, Doom Wiki page, unique normalized title) reached from two moments, because either side can arrive first: `link_rows_with_keys` after a sync and `link_imported_wad` after every import. Weaker evidence refuses a WAD whose stronger identifiers differ from the row's, so a title never outweighs two ids that disagree. Per row rather than per WAD so the answer cannot depend on iteration order: a WAD claiming rows as it is visited could take one by title that a later WAD owns by id. There is deliberately no whole-table sweep, because Unlink clears a row and a sweep would quietly undo that.
- `cacoward_sync.rs` — writes the dataset to the `cacowards` table, at every launch. An unchanged dataset reads and returns without taking the write lock, which is the ordinary case and what makes running it before the window opens cheap. A change updates rows in place by key, considers only new rows and rows with new evidence for linking, and removes rows the dataset dropped unless they hold a pin or the user's word on `supported`.
- `idgames/`, `doomwiki/`, `doomworld/` — per-source API clients + parsers
- `examples/cacowards_draft/` — the maintainer's tool for the year's data, and the only code that still parses a `Cacowards_YYYY` wiki page. An example rather than an `xtask`, because `cargo build --workspace --release` would compile a new crate into every package, while `cargo test` builds examples anyway and `test = true` on its `[[example]]` runs its parser's tests.

**caco**: `app.rs` hosts the `CacoApp` state machine. Panels in `src/panels/`, dialogs in `src/dialogs/`, import flow in `src/import/`. `app/file_check.rs` runs `presence::check_library` on a worker when the app is created — it used to run in `main.rs` before the window existed, where a stat on a dead mount held the window back — and turns its report into at most one status-bar line, a warning only for files caco cannot fetch again. It sends that answer before running `identity::check_library`, whose first run reads the whole library, and that pass is silent: a changed file is only worth a word when it is about to be played, so it just reloads the views (a changed file has lost its analysis). `thumbnails.rs` extracts and caches TITLEPIC; `wiki_scraper.rs` fetches Doom Wiki thumbs; `workers.rs` coordinates background search/import/play via mpsc channels. The Cacowards view (`ViewMode::Cacowards`) is rendered by `panels/cacowards.rs` in a deliberately editorial layout (hero banner + year strip + category card grid) to signal the curated-external-feed origin while sharing the rest of the GUI chrome. Imports kicked off from cacoward cards spawn through `import::workers::spawn_import_cacoward` so they reuse the existing duplicate-detection and auto-link plumbing, trying each source the entry names in turn (idgames, then the Doomworld thread, then the Doom Wiki), since which site is behind a challenge changes from day to day. `app.rs::sync_cacowards` writes the dataset to the table as the app is created, before the window opens, so the view can never open empty on a new library. `dialogs/settings.rs` is the GUI settings editor: it snapshots the live `Config`, edits a curated field subset (sourceports, behavior, cache, paths), and on Save writes the full struct via `config::save_config` + `config::reload_config` so unexposed sections (list, sourceport_preferences, iwad_priority) survive round-trips and changes apply without restart. `dialogs/profiles.rs` manages sourceport config profiles (list on the left, contents editable on the right); it is the GUI counterpart to `caco profile` and shares `caco_core::profiles` with it. Deleting a profile stages a confirmation that lists the WADs still referencing it rather than warning after the fact. `dialogs/gc.rs` is the cleanup panel (Storage's Clean tab): keep-toggles across the top re-measure the plan on change, every row is a checkbox (so one WAD's saves can be kept while another's go), and Clean stages a confirmation naming the item count and size. `dialogs/enrich.rs` drives `caco_sources::enrich_service::enrich_wads` from the GUI: it owns only the request, live progress and the report, while `app.rs::spawn_enrich` runs the work on a thread and streams `AppMessage::EnrichProgress` / `EnrichComplete` back. Cancellation is an `Arc<AtomicBool>` the dialog shares with the worker; a fresh one is minted per run so a previously cancelled flag can't abort the next one instantly. `dialogs/companions.rs` is the library-wide companion registry (Storage's Files tab, `caco companion ls` with no query): every managed file with its size, MD5 and the WADs still linking it, plus per-row and bulk orphan deletion. Per-WAD linking stays in the edit dialog's Companions tab, which stages the orphan question when `plan_unregister` returns `None`. `dialogs/wad_data.rs` is the per-WAD Saves / Backups / Demos dialog (`caco saves` + `caco demos` in one place, since both act on the same data dir); every destructive action stages a confirmation, and demo playback returns `WadDataResult::PlayDemo` so `app.rs` can run the blocking launch on a worker thread while the dialog stays open. `dialogs/file_changed.rs` is the question the Play worker's `AppMessage::PlayFileChanged` opens: Play Anyway dispatches `ActionRequest::PlayAnyway`, which goes through the same `app.rs::spawn_play` worker with `accept_changed_file` set. `dialogs/wad_file.rs` is the read-only File section at the foot of the edit dialog's Sources tab: where the file is (or was last seen), its size and hashes, the files inside it, and any other WAD with the same content. It is shown when the file is gone too, since then the hash is the one thing that can still name it.

## Dependencies (key crates)

```toml
rusqlite = "0.41"       # SQLite (bundled)
serde / serde_json
toml = "1.1"
chrono = "0.4"
thiserror = "2"
regex = "1"
md-5 = "0.11"           # companion dedup (hex::encode — digest 0.11 dropped LowerHex)
zip = "9"
image = "0.25"          # thumbnails
reqwest = "0.13"        # HTTP (blocking, rustls; `query` feature is opt-in)
eframe = "0.37"         # GUI (wgpu renderer by default since 0.36)
egui = "0.37"
```

## Key Patterns

- **Builder pattern for DB writes**: `NewWad::builder()` and `WadUpdate::builder()` produce type-safe WAD creation/updates. Both trim `source_url` through `wads::clean_source_url`, and `find_duplicate` compares through it too, or a URL with a pasted `\r` would miss the row it is a duplicate of. reqwest strips that `\r` while parsing, so the fetch succeeds and nothing else notices it was there. `WadUpdate` setting a non-empty `cached_path` also clears `last_known_path`, so the two are never both set and `presence` never restores a row from a location that a newer file superseded; `last_known_path` is deliberately not an allowed update field, because `presence` is its only writer.
- **Batch stats**: `get_wad_stats_batch()` (playtime, last played, session count and times beaten in two queries), `get_analyses_batch()`, `get_companions_batch()` — avoid N+1 queries when rendering lists.
- **Query parser**: beets-style syntax — see Behavior below.
- **Companion system**: `companion_files_registry` + `wad_companions` junction table. `companion_service.rs` handles MD5 dedup + managed storage at `<data>/companions/{md5[:12]}_{filename}`. DEH/BEX auto-detected; `-deh` for non-zdoom, `-file` for zdoom. Because files are deduplicated by MD5, one managed file can serve several WADs, so unlinking is not the same as deleting: `companion_service::plan_unregister` answers "what should happen to the file", returning `None` only when the file would be left with no owner *and* the configured policy is `ask` (the default). Both frontends must go through it — `unregister_companion` takes a resolved `OrphanPolicy`, so neither can swallow an `ask` and orphan a file unprompted. `delete_orphan` refuses while anything still links the file.
- **WAD file placement**: `config::wad_store_dir(retrievability)` is the only
  answer to "where does this file go" — cache dir for `Automatic`, `keep_dir()`
  (`<data>/wads`) for `Manual`. `wad_store_dir_in` takes both roots
  explicitly so tests cannot write into the real library, same reason as
  `GcPaths`. `dialogs/link.rs::link_picked_file` re-reads the WAD from the DB
  rather than trusting its caller, because retrievability is a property of the
  record. The play-time download path in `app.rs` writes to the cache dir
  without asking, because only an `Automatic` WAD reaches it: Play sends a
  manual one whose file is not here straight to the link dialog, rather than
  announce a download that cannot start.
- **Destructive flows respect retrievability**: deleting a re-fetchable file
  costs a download, deleting a manual one costs the WAD, so the two must not
  look alike. `dialogs/gc.rs::is_at_risk` requires *both* manual and an actual
  cache file queued for deletion — `redownloadable` alone describes the WAD, not
  what the plan does to it — and those rows render warning-coloured and start
  **unticked**. `dialogs/cache.rs` excludes them from bulk clear entirely
  (the button relabels to "Clear N Re-downloadable" and says why) and makes a
  single delete take two clicks. An explicit per-row tick or Select All is still
  the user's call; only the defaults are protective.
- **GC**: `caco_core::gc` splits cleanup in two — `plan` measures and touches nothing, `execute` deletes exactly the `GcSelection` handed back. The GUI renders the plan as a checkbox list and passes back the ticked subset, so nothing is deleted that was not shown first. `gc_ignore` column excludes a WAD; orphan detection covers data dirs, backups, and companions. `plan` takes an explicit `GcPaths { data_dir, backup_dir }` rather than reading config: every path in it is a path something gets deleted from, and planning against an empty DB makes every directory look like an orphan, so a plan built over the real data dir and executed wipes the library's saves. The required argument is what keeps that unreachable from a test — `GcPaths::from_config()` is for frontends, and `gc`'s tests build every path from a tempdir. `saves::list_backups_in` exists for the same reason.
- **Sourceport build driver**: every command is assembled by a pure function (`clone_args`, `configure_args`, ...) and only then handed to a process, so flags that are load-bearing and silently fatal if dropped — uzdoom's `-DINSTALL_PK3_PATH=bin`, whose absence produces a build that succeeds and aborts at launch — are testable without a network or a compiler. `SourceportPaths` is passed explicitly for the same reason `GcPaths` is: a test that could reach the real roots is one that can compile 74M of C++ into a user's cache, or delete out of it. The end-to-end build lives in `tests/sourceports_build.rs` behind `#[ignore]` — `cargo test --workspace` must never compile a sourceport. The install prefix is not touched until the compile succeeds, so a failed rebuild leaves a working sourceport in place, and `remove_installed` refuses any directory without caco's own manifest.
- **Import service**: centralises duplicate checking for all sources; auto-enriches with Doom Wiki metadata. Every route inserts through `insert_wad`, which links Cacoward rows last and in the same transaction, so a new route cannot forget to, and a Doom Wiki import's link sees the idgames id it only learns during enrichment. `wads.idgames_id` is the one spelling of "this WAD's idgames id": the idgames importer writes it alongside `source_id` (which stays the duplicate key), and every other source writes it when it learns the id.
- **WAF challenges**: `http::waf_challenge` is the only thing that knows what one looks like, and all three clients ask it before looking at the status, because which WAF a site sits behind is not theirs to assume: the Doom Wiki moved from AWS to Cloudflare, and a client that only checked for AWS reported the challenge as a bare HTTP 403. The headers are the whole signal. Cloudflare documents `cf-mitigated: challenge` with no status; AWS WAF answers a challenge with 202 and a CAPTCHA with 405. The helpers this replaced both required a 403, so the AWS one could never fire, and a 202 passes `error_for_status`, where the forum client would have imported the interstitial as a library row. `SourceError::WafBlocked { site, kind }` words its own message, so the WAF it names is the one that answered. Each client's handling of a response is a free function over `reqwest::blocking::Response` (`read_api_response`, `read_thread_page`), so tests hand it an `http::canned_response` with no network. That is what the `http` dev-dependency is for: reqwest does not re-export `http::Response`. The Import view's search tabs render the message where "No results" would be (`SearchState::set_error`), because an empty list means the source answered, and a blocked search never got that far. Auto-enrichment and thumbnails swallow failures on purpose, so a blocked wiki stays silent there.
- **Player**: wraps sourceport execution; injects companion files, data dir args, complevel args, config profile; returns `PlayResult` with crash detection. Anything in `player.rs` that reads on-disk stats has a `*_with` twin holding the filesystem-free logic — `reconcile_stats` / `reconcile_stats_with`, `read_session_stats_before` / `session_stats_before_with` — and the tests only ever call the twin. `read_stats_snapshot` resolves the data dir through global config, so a test that calls the outer function reads whatever WAD happens to share its id in the developer's own library: `read_session_stats_before` had exactly that bug and failed only when run against a populated library. Same reasoning as `GcPaths` — the difference is that the seam here is the resolved snapshot rather than the path.
- **GUI background work**: egui is immediate-mode; `CacoApp` holds all state; background workers for search/import/play use `std::thread` + `std::sync::mpsc`.
- **Transactions take the write lock up front**: `db::with_transaction` is `BEGIN IMMEDIATE`. Every caller writes, and several read first (`update_wad` reads the status, `identity::record` the previous identity). Under WAL a deferred transaction that reads and then writes cannot wait for the lock. If another connection commits in between, SQLite fails the write at once instead of calling the busy handler (rusqlite's 5 s default). The startup identity pass writes from a worker for as long as a library takes to hash, which made that race reachable. `test_with_transaction_takes_the_write_lock_before_it_reads` pins it.
- **Modal sizing**: `dialogs::modal_window` is the only place a dialog's size is
  decided, and `dialogs::modal_body` the only place its scrolling is. Every
  dialog used to spell out its own `default_size([W, H])` picked for a
  comfortable window and never checked against a small one, so at the 800x400
  minimum window four of them rendered past the app's edges with their button
  rows unreachable. Clamping alone does not fix that: egui resolves a window as
  `natural.at_most(max).at_least(min)`, so a body with a large *minimum* —
  a full-length list, an unwrappable absolute path — beats the cap. The body
  must be able to shrink, which is what `modal_body` (and `TableBuilder`'s
  `max_scroll_height`) provides, as does the shrinking `ScrollArea` in
  `dialogs/enrich.rs`, whose idle state has no use for a full-height window. Heights
  come from `modal_body_height`, which
  budgets off `ctx.content_rect()` rather than `ui.available_height()`: inside a
  window that is still settling the latter reports last frame's size, so a cap
  derived from it never converges. (`content_rect` was `screen_rect` before egui
  0.36 — same rect on desktop, minus any OS status bar or display notch.)
- **A body rendered inside another dialog is handed its height, never asks for
  it**: `modal_body` budgets against the screen, which is right for a body that
  *is* the window and wrong for a Storage tab — asking the screen ignores the
  tab strip and Close row above and below it, so every tab overflowed by the
  same amount. `dialogs::scroll_body(ui, height, ..)` takes the budget instead,
  and `storage.rs` derives it once from `CHROME`, passes it to the tab, *and*
  wraps the tab in it. Both, because a tab's own furniture (Cache's summary,
  at-risk toggle and button row) can exceed the whole budget at the 800x400
  minimum, and then no cap on its list would save it; the wrapper's scrollbar
  only ever appears in that case.
- **Inputs are named after their label**: `response.labelled_by(label.id)` ties
  a field to the label a sighted user reads (`dialogs/settings.rs::form_label`
  returns its `Response` for this), and `accessible_name` is for a control with
  nothing visible to point at. The screenshot harness fails on an input with no
  name at all, but a placeholder or a tooltip is enough to pass it, and the name
  that leaves is often wrong: "Max size (GB)" was announced as "0 = unlimited".
  With `labelled_by` the tooltip becomes the description, which is what it is.
- **Sidebar rows size to `ui.available_width()`**: `ui.horizontal` does not
  wrap, and a row wider than a `SidePanel` is not merely clipped — egui sizes a
  panel from the rect its contents actually occupied, so the overflow displaces
  every panel after it. Ten management buttons in one row cost five of them
  (Clean, Trash, IWADs, Sourceports, Settings were unreachable) *and* 180pt of the
  library grid. Stacked `theme::sidebar_tool_item` rows cannot do either.

## Screenshots

`crates/caco/tests/screenshots.rs` renders every GUI surface to
`target/screenshots/<WxH>/*.png` with no display server, through `egui_kittest` +
wgpu. It exists because whether a row fits inside its panel is decided at paint
time against a real font atlas and a real viewport — there is no unit test for
it, and reading the layout code is how the two bugs above survived.

- `#[ignore]`d, like `sourceports_build.rs`: `cargo test --workspace` must not need a
  GPU. Run it explicitly, or read CI's: `verify.yml`'s `screenshots` job runs it at
  1200x800 and 800x400 on Mesa's software Vulkan driver and uploads the PNGs as an
  artifact. Being in `verify` is what makes it gate `update-deps`' auto-merge: an
  egui bump once broke the harness and nothing noticed, because nothing ran it.
- It also *asserts*, not just captures: after opening each dialog it reads
  egui's own area rects and fails if any window escapes the viewport. Run it at
  `CACO_SHOT_SIZE=800x400` (the `min_inner_size` in `main.rs`) when touching
  dialog layout — that is the size things break at, and a PNG cannot tell a
  window clipped at the screen edge from one that merely ends there.
- kittest's own checks stay on, because each catches what a PNG hides. A
  character no installed font can draw panics instead of painting tofu, and an
  input widget with no accessible name fails the run (see *Inputs are named after
  their label*). The first is why the theme has to be on the context before the
  first pass, in the harness as much as in `main.rs`. eframe's system font fallback
  is not installed here, so a WAD title in a script the bundled fonts lack fails a
  run against a real library even where the app would find a system font for it.
- `build_eframe` drives the real `eframe::App`, down the same creation path as
  `main.rs`: there is no test-only entry point into the app. kittest's `build_ui*`
  kinds would need one, and also wrap the app in an 8pt margin, so what they
  shoot is not what a user sees. `dispatch_action`, `close_dialog`, `set_view` and
  `background_sender` are public for this — the harness opens dialogs the way a
  click does rather than building dialog state by hand.
- Frames go through `Shooter::settle`, not `Harness::run`. Workers wake the context
  being painted, as they do in the app, and `run` panics on a UI still being asked
  to repaint after `max_steps`, so a burst of worker messages failed it at random.
  `run_ok` stops at the budget instead, and because it then skips its accessibility
  check, `shot` makes that check itself. Raising `max_steps` instead would only
  make the flake rarer, and this job gates a merge nobody is watching.
- A control that only exists in some state is unchecked until a run reaches that
  state, which is why `scratch_home` puts an empty `nyan-doom` on `PATH` (Settings'
  detected controls; CI has no sourceport), why worker-only states are posted
  through `background_sender`, why the Cacowards view is shot on both its newest year
  and its oldest, reached by clicking the chip (the strip has to scroll that far, and
  an old year has different sections to lay out), why one shot adds a hashed WAD whose last session played
  other content and presses Play on it (the File Changed dialog, reached through
  `presence`, `identity::verify` and the refusal), and why the last shot adds a WAD whose file is gone
  and presses Play on it (the link dialog, with an absolute path long enough to
  wrap). Add a shot when you add such a state.
- The library is a copy of the real `library.db` into a temp `CACO_HOME`, so
  shots show real WADs and nothing can write to the real library. CI has no library,
  so it falls back to an empty database: every surface still renders, with less in it.
  (Cacowards always has the whole dataset, which is synced as the app is created.)

## Data Locations

Split by regenerability: the **data dir** is the portable set, the **cache dir**
is disposable. Nothing that cannot be re-derived may be added to the cache side,
and nothing regenerable may be added to the data side — the point of the split is
that the data dir stays small enough to copy between machines.

Both roots come from the `dirs` crate, so they are platform-native: XDG on Linux
(honouring `XDG_DATA_HOME` / `XDG_CACHE_HOME`), `~/Library/Application Support`
and `~/Library/Caches` on macOS. **Never spell either root literally** — call
`config::default_data_dir` / `config::cache_home`, or a path helper built on
them. `default_data_dir` used to hardcode `~/.local/share/caco` while
`cache_home` already went through `dirs`, which meant a machine with
`XDG_DATA_HOME` set split caco across two conventions; the paths below are the
Linux-default spelling, shown for illustration only.

Portable (`config::default_data_dir`, overridable via `CACO_HOME`):
- Database: `<data>/library.db`
- Config: `<data>/config.toml` (deliberately not `~/.config` — it is app-managed state written by the settings dialog and first-run detection far more often than by hand, and keeping it here makes the portable set one directory and `CACO_HOME` a complete isolation switch)
- Managed IWADs: `<data>/iwads/{variant}/{family}.wad`
- Managed id24 WADs: `<data>/id24/{name}.wad`
- WAD data: `<data>/data/` (per-WAD saves, stats, configs)
- Companion files: `<data>/companions/{md5[:12]}_{filename}`
- Sourceport config profiles: `<data>/profiles/{exe}/{profile}.cfg`
- Sourceport build recipes + patches: `<data>/sourceports/*.toml`
- Cacowards corrections and additions: `<data>/cacowards/*.toml`
- Backups: `<data>/backups/` (save backups + pre-migration DB snapshots)

Disposable (`config::cache_home`, overridable via `CACO_CACHE_HOME`):
- WAD cache: `<cache>/wads/` (`CACO_CACHE_DIR` overrides just this)
- Thumbnails cache: `<cache>/thumbnails/`
- Built sourceport prefixes: `<cache>/sourceports/{name}/{ref-slug}/` (+ `update-check.toml`)
- Sourceport checkouts + build trees: `<cache>/sourceports-src/`

Caco carries **no migrations between layouts**. It is pre-1.0 and single-user;
a layout change is applied by moving the files by hand, which is why nothing in
`config.rs` knows about a previous location. The startup file check (`presence`)
still relinks by file name, but for live reasons rather than historical ones:
`cache_dir` is editable in Settings, the data dir is somewhere different on every
platform, and every `wads.cached_path` records an absolute path into wherever
those used to be. It looks in both the cache and the keep dir, because the keep
dir is the half that travels between machines.

## Behavior

**Query syntax** (beets-style — used by `ls`, `play`, `modify`, `trash`, etc.):
- Fields: `id:`, `title:`, `author:`, `year:`, `filename:`, `tag:`, `status:`, `source:`, `iwad:`, `complevel:`, `config:`, `cacoward:`, `retrievable:`, `avail:`, `hash:`, `dup:`
- `cacoward:` finds WADs linked to an entry: `cacoward:*`, `cacoward:2023`, `cacoward:winner` (with `r`/`hm`/`m` shortcuts), `cacoward:doom-moma` (any other category, by name), `cacoward:2023:winner`.
- `hash:` matches the start of any recorded hash (file SHA-256/SHA-1, content, inner files' SHA-256/SHA-1); a value that is not hex is ignored. Its SQL repeats a bare `?` per comparison: a numbered `?1` counts from the start of the whole query, so after another term it would bind that term's value.
- `dup:yes` / `dup:no`: `identity::DUPLICATE_SQL`, another live WAD with the same content; `test_duplicate_sql_matches_rust` keeps it and `identity::same_content` in step.
- OR: `"status:in-progress , status:unplayed"` (comma with spaces)
- Negation: `^status:completed`
- Status shortcuts: `u` (unplayed), `p`/`ip` (in-progress), `c`/`f`/`done` (completed), `a`/`d` (abandoned)
- Retrievability shortcuts: `auto`/`a` (automatic), `m` (manual)
- Glob patterns: `tag:caco*`
- Free text searches title, author, description

**Status enum**: `unplayed`, `in-progress`, `completed`, `abandoned`.

**Lost WADs** (`WadRecord::is_lost`, `Retrievability::LOST_SQL`, query
`retrievable:lost`): `Manual` intersected with "no local copy" — the set caco
can neither play nor fetch. Deliberately not a third enum variant, since a WAD
enters and leaves it purely by its file coming and going. The library does not
mark these rows: the title marker and the filter-bar chip that once did were
dropped when the storage controls were simplified, so the query is how to list
them.

"No local copy" used to mean only "no `cached_path`", and a file that went away
without caco deleting it never cleared that: a data dir synced from another
machine listed six manual WADs as cached, My House among them, on a machine that
had none of their files (#51). `presence` now checks every recorded path at
startup and before each launch, and moves a vanished one into `last_known_path`
(migration 40), which the link dialog shows as where the file was last seen.
That column is history rather than a copy of derivable state, so it does not
bring back the drift migration 38 removed.

**Two derived axes on a WAD, neither stored.** Both live in `db/models.rs` as an
enum with a `derive` constructor, a SQL spelling of the same rule, a `WadRecord`
accessor, and a test in `query.rs` asserting the Rust and SQL spellings select
the same rows. Treat that quartet as the pattern for anything similar.

- **`Retrievability`** (`Automatic` / `Manual`): can caco fetch the file again
  unattended — `source_type = idgames` or a non-empty `idgames_id`. Never
  inferred from where the file currently sits: placement is a *consequence* of
  retrievability, so reading it back off the path would let the two drift the
  moment `cache_dir` or `CACO_HOME` moves. Query field `retrievable:`, SQL in
  `AUTOMATIC_SQL`. `gc.rs`'s `redownloadable` flag calls it rather than keeping
  its own inline copy of the rule. `config::wad_store_dir` is the single
  placement decision it drives — see Placement below.
- **`Availability`** (`Cached` / `Downloadable` / `Unavailable`): is the file on
  this machine, and if not is there a URL to try — from `cached_path` +
  `source_url`, treating `""` as absent. Query field `avail:`, SQL in
  `Availability::sql()`, one predicate per variant, and they partition the table.
  Deriving it never touches the disk; it is as true as `cached_path`, which
  `presence` keeps true.

The two are orthogonal and all combinations occur in a real library.
`Availability::Downloadable` deliberately overstates for manual WADs — it means
"has some `source_url`", including Doomworld threads and one-off hosts that rot,
which is exactly what `Retrievability` is for.

`Availability` **was** a stored column (dropped in migration 38). Nothing kept
the copy honest: `sessions::clear_cached_path` nulls `cached_path` with raw SQL
while the auto-maintenance lived inside `update_wad`, so every cache eviction —
the Cache dialog's remove and clear-all, and GC's execute step — left a row
still reading `cached`. 22 rows had drifted in the author's library before the
column was removed. Deriving on read makes that unrepresentable, which is why
migration 38 has nothing to backfill; `sessions.rs::test_eviction_downgrades_availability`
pins the behaviour.

**IWAD detection**: PNAMES lump analysis (TNT-only 197 patches / Plutonia-only 78 patches), map lump fallback (ExMy→doom, MAPxx→doom2); self-contained WADs don't trigger detection.

**Complevel detection hierarchy**: COMPLVL lump (id24 byte or text) > UMAPINFO → 21 > DEHACKED+MBF → 11 > DEHACKED+ExMy → 2 > DEHACKED+MAPxx → 4 > ExMy → 2 > MAPxx → 4.

**Sourceport families**: dsda / zdoom / chocolate / woof / eternity / helion / uzdoom. Each maps to data dir args, save dir args, complevel args, and config args.

**Per-WAD config columns**: `custom_iwad`, `custom_sourceport`, `custom_args` (JSON), `complevel` (INT), `custom_config` (TEXT).

**Launch args layering**: global `sourceport_args` (every sourceport) → `[executable_args]` table (executable basename → args, case-insensitive fallback, applied only when that executable launches) → per-WAD `custom_args`. GUI settings dialog edits args as shell-quoted strings via `shlex`.

**DB migrations**: run on `init_db()`; numbered sequentially; current schema version is 42. `init_db` snapshots the DB into the backup dir whenever migrations are pending.

**Cacowards**: `cacowards` table, one row per entry in the dataset, keyed on `key`. The facts (year, category, page_order, wad_title, wad_author, idgames_id, doomworld_id, doomwiki_page, sha256, supported_default) are rewritten from the dataset; what belongs to the user (wad_id, manual_override, supported_override) survives every update. Rows used to be `UNIQUE(year, category, wad_title)`, under which a corrected title turned a pinned row into a duplicate; migration 42 keys each one the way the dataset would have, which is how a row pinned before the migration meets its curated entry.

**Linking is by evidence, strongest first** (`cacoward_link.rs`): the entry's file hash, against anything `identity` recorded; its idgames id, against `wads.idgames_id`; its Doomworld topic, against the number in a `doomworld.com` `source_url` (`vb/thread/151858` and `forum/topic/152740-…` both occur); its Doom Wiki page, against a `doomwiki.org` `source_url`; and its normalized title, when exactly one WAD has it. A link is not revisited once made, so each step refuses a WAD whose stronger identifiers differ from the row's: the 2023 runner-up "Corruption" (20790) shares its name with two 2007 WADs, and a wrong link is the worse failure because a missed one stays visibly absent and takes one pin. The hash comes first because the same bytes are the same WAD. Neither moment touches a pinned row or one already linked to a live WAD.

**Categories**: `category` is free text. The core four (`winner`, `runner-up`, `honorable-mention`, `mordeth`, `db::CORE_CATEGORIES`) lead the view and are what a year's completion counts. Everything else (`multiplayer`, `gameplay-mod`, `mockaward`, `doom-moma`, …) is shown under them and counts for nothing, because a deathmatch map has no ending to reach. An entry is something the library can hold, so awards to people (Creator of the Year, Most Promising Newcomers, the Espi) and to tools (the Codeaward) are not in the dataset at all, and neither is a joke with nothing behind it ("1994 Wads"). Doomworld's write-ups aren't either: they are its own text, and the view links to the year's page.

**`supported`**: unsupported entries (Doom 64, Hedon: nothing caco launches can run them) still render in the view but are excluded from completion totals, so the swept chip and hero bar reflect playable entries only. The dataset's `supported = false` is the default and the card menu's toggle is a nullable override on top, so a data update can't undo the user's choice.

**Cacowards dataset**: `crates/caco-core/src/cacowards/data/YYYY.toml`, facts taken from the Doom Wiki's `Cacowards_YYYY` pages and checked against the idgames API, which found real errors: the 2010 page gives Base Ganymede's second episode the id of Cheogsh 2. It is embedded rather than fetched because it is small, append-only and changes once a year, and because fetching kept failing: the wiki sits behind a Cloudflare challenge.

```toml
year = 2025
page = "https://www.doomworld.com/cacowards/2025/"

[[entry]]
key       = "2025-winner-lonesome-road"   # stable forever; rows key on it
category  = "winner"
order     = 8                             # page order, not a ranking
title     = "Lonesome Road"
author    = "Thomas Seifert (tourniquet)"
doomworld = 151858                        # forum topic id; not on idgames
doomwiki  = "Lonesome_Road"
```

Also optional per entry: `idgames`, `sha256`, and `supported = false`. A `*.toml` of the same shape in `<data>/cacowards/` is merged over the built-ins by `key`, so a correction or a private addition doesn't need a release. A malformed one holds the update back and says so in the status bar, since applying the rest would remove its rows.

The annual update, usually in December:
1. Save `https://doomwiki.org/w/index.php?title=Cacowards_YYYY&action=raw` from a browser, which passes the challenge that a client does not. The Wayback Machine's captures of `action=edit` also have the wikitext, in their `<textarea>`, which is how these files were first drafted.
2. `cargo run -p caco-sources --example cacowards_draft -- YYYY saved.wikitext > crates/caco-core/src/cacowards/data/YYYY.toml`. It fills in the ids of entries the page links by path, checks the rest against what the archive holds at that id, and looks for forum-only entries that have since reached idgames.
3. Settle each `# CHECK:` comment, which a test refuses to ship: split joint entries, drop what the library couldn't hold, and set `supported = false` on what no sourceport caco launches can run. A suggested idgames id is only a shared title, so take it when the author and date agree too: "Crusader" is a 2004 WAD and a 2023 one.
4. Add the file to `BUILTIN` in `cacowards/mod.rs`, which a test also enforces. The release workflow ships it.

**GUI Cacowards view**: a magazine-style central panel reachable from the left sidebar. Backed by `state::CacowardsState` (all entries + selected year, refreshed via `AppState::reload_cacowards`). Each entry renders as a card tagged with its linked WAD's status. The action button on each card is contextual: absent entries get an `Import` button that fires `ActionRequest::ImportCacoward(pk)`, library entries get a `Play`/`Open` button reusing the existing `ActionRequest::Play(wad_id)`. After an import completes, both `state.needs_reload` and `state.cacowards.needs_reload` are set so the library list and the magazine view both pick up the new link without a manual refresh. The year strip scrolls, since no window is wide enough to be sure of twenty-odd chips, and the thumbnail loop only asks the wiki about the year on screen, or opening the view would ask about every entry there is.

## GUI Surfaces

The sidebar separates **views**, which change what the central panel shows,
from **tools**, which open a modal. They were one undifferentiated list; the
split is why `Stats` moved — nothing in it is an action, so blocking the app
behind a window to read four numbers was the wrong shape.

Views (`app/sidebar.rs` sets `ViewMode` directly):

| Entry | Panel | Backed by |
|-------|-------|-----------|
| Library | `panels/library.rs` + `wad_grid` / `wad_table` | `db::search_wads` |
| Import | `import/` | `caco_sources::import_service` |
| Cacowards | `panels/cacowards.rs` | `state::CacowardsState` |
| Stats | `panels/stats.rs` | `state::StatsState` ← `db::sessions::get_stats_snapshot` |

Each view owns a `needs_reload` flag its sidebar entry sets on entry, so a
figure that predates the session you just finished cannot be shown.

Tools (`app/sidebar.rs` → `ActionRequest` → `app.rs::dispatch_action`):

| Entry | Dialog | Backed by |
|-------|--------|-----------|
| Storage | `dialogs/storage.rs` | four tabs, below |
| Profiles | `dialogs/profiles.rs` | `caco_core::profiles` |
| IWADs | `dialogs/resources.rs` | `resource_service` |
| Sourceports | `dialogs/sourceports.rs` | `caco_core::sourceports` (worker thread) |
| Enrich | `dialogs/enrich.rs` | `caco_sources::enrich_service` (worker thread) |
| Settings | `dialogs/settings.rs` | `config::save_config` + `reload_config` |

**Storage** is one window over four tabs, because they are one question — what
is on disk and how do I get space back — and they overlap: Cache and Clean
delete the same `wads.cached_path` files, and Files and Clean both remove
orphaned companions. Four entry points to overlapping deletions is how Cache
and Clean came to disagree about irreplaceable files in the first place.

| Tab | Module | Backed by |
|-----|--------|-----------|
| Cache | `dialogs/cache.rs` | `db::sessions::get_cached_wads` |
| Clean | `dialogs/gc.rs` | `caco_core::gc` plan/execute |
| Trash | `dialogs/trash.rs` | `db::restore_wad` / `purge_all_deleted` |
| Files | `dialogs/companions.rs` | `companion_service` + `db::get_wads_for_companion` |

Each tab keeps its own module and its own state, and exposes three things to
`storage.rs`: `render_body(ui, conn, avail)`, `escape() -> bool` (false when it
consumed the key to back out of a staged confirmation, so dismissing "Empty
Trash?" cannot also close the window), and a `modified` flag. Tab state is
built on first visit — `GcDialogState::new` runs a full `gc::plan` over the
data dir, the backup dir and every cached file, which opening Storage to look
at Trash must not pay for — and `StorageDialogState::modified()` therefore
reports nothing for a tab that was never opened.

Per-WAD, from the context menu or a shortcut: Edit (`E`), Delete (`D`),
Sessions (`S`), Map Stats (`M`), Saves & Demos (`F`), Play (`Enter`/`P`).

Deleting a WAD soft-deletes it (`deleted_at`), so Storage's Trash tab is the
only route back — without it a soft delete would be indistinguishable from a
permanent one, and the play history it preserves would be unreachable.

## Git Instructions

- Commit working changes to git; keep the tree green between commits.
- Update README.md and CLAUDE.md when adding or changing user-visible features.
- Quality gates before any commit: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. `.github/workflows/verify.yml` runs the same three on every pull request and push to `main`; there is no pre-commit hook, so CI is what enforces them.

### Commit messages

**Every commit MUST use Conventional Commits**: `type(scope): subject`. The user delegates all commit-writing to Claude. The release workflow takes the bump level as an input rather than deriving it from commit types (see Versioning and packaging below), but the convention is still enforced: it is what makes `git log` readable enough to choose that level.

Semantic meaning of the types, used to pick the bump level when releasing:
- `feat:` → minor bump
- `fix:` / `perf:` / `refactor:` → patch bump
- `BREAKING CHANGE:` in the body → major bump
- `chore:`, `docs:`, `test:`, `ci:`, `build:`, `style:` → no bump on their own

Common scopes in this repo: `db`, `core`, `gui`, `sources`, `cli`, `arch`, `completion`, `stats`, `sourceports`, `doomwiki`, `idgames`, `doomworld`. Pick the smallest accurate scope. Omit the scope when a change genuinely spans many areas.

Never write non-conventional commit subjects (e.g. `Add foo`, `Fix bar`, single-word like `gitignore`).

## Versioning and packaging

Four workflows in `.github/workflows/`, mirroring the ones in shore so the two
projects release the same way:

- `verify.yml` — fmt, clippy (`-D warnings`, since the workspace has no
  `[lints]` table) and `cargo test --workspace` on every pull request and push
  to `main`, plus `brew style` and `brew audit --strict` on the Homebrew
  cask (on a macOS runner, since cask rules only apply there). The
  `homebrew` job runs `brew trust --cask` on the linked-in tap first: since
  Homebrew 7, audit refuses to load a cask from an untrusted tap, and that
  failure kept the 2026-11-01 weekly update from merging. A `screenshots` job
  renders every GUI surface on a software Vulkan driver (see Screenshots). The
  jobs are named `rust`, `screenshots` and `homebrew` so a ruleset can require
  them. The `rust` job exports `CFLAGS=-O2`, as makepkg and rpmbuild do: v4.1.6
  tagged and then failed its Arch build because `cc` 1.6 stopped `aws-lc-sys`
  from forcing `-O0` on jitterentropy, and a CFLAGS-free verify could not see
  it. `cc` is pinned `=1.5.1` in the workspace manifest until that is fixed
  upstream.
- `update-deps.yml` — every Sunday, `cargo upgrade --incompatible` plus
  `cargo update`, opened as a pull request on `deps/weekly`. It uses the
  `DEPS_PR_TOKEN` secret because a pull request opened with the default
  `GITHUB_TOKEN` would never start `verify`. When the run opened or updated
  the pull request, a second job waits for `verify` on that exact commit,
  squash-merges it with `--match-head-commit` once every check passes, and
  dispatches `release.yml` with `patch`. If any check fails the pull request
  stays open and nothing is released.
- `package.yml` — builds the Arch package for x86_64 in an
  `archlinux:base-devel` container and the Fedora package for aarch64 in a
  `fedora:43` container on GitHub's arm64 runner. The release calls it with
  the new tag; it also runs on any pull request that touches `contrib/`, the
  installed assets or itself, since nothing else would catch a broken recipe
  before a release.
- `release.yml` — started by hand from the Actions tab with `patch`, `minor`
  or `major`, or by `update-deps` with `patch`. It runs `verify`, then bumps every version file, commits
  `chore(release): vX.Y.Z`, tags it and pushes both atomically (refusing if
  started from anything but `main`, or if the version files disagree), runs
  `package` on the tag and builds `Caco.app` on an arm64 macOS runner in
  parallel, creates the GitHub release with all three and generated notes,
  and finally commits `chore(release): cask vX.Y.Z` to point the cask at the
  new zip. The release job downloads those three artifacts by name: `verify`
  runs inside the same workflow run, so its `screenshots` artifact would
  otherwise land in the directory whose contents get attached.

Never bump the version by hand; the release workflow is the only writer. The
version files it moves together are the root `Cargo.toml`, the three
`caco*` entries in `Cargo.lock`, the PKGBUILD's `pkgver` (with `pkgrel`
reset to 1) and the Fedora spec's `Version` (with `Release` reset to 1). A new
workspace crate needs adding to its `Cargo.lock` pattern, or the agreement
check fails the next release.

Nothing in this repository publishes the packages anywhere but the GitHub
release. A scheduled job on the author's Gitea polls the latest release and
mirrors its `.pkg.tar.zst` and `.rpm` into the Arch and RPM registries there.
It picks the files out by extension and reads each package's name and version
from the file name, so the release must attach the packages themselves under
the names makepkg and rpmbuild give them.

**The PKGBUILD clones the tag over HTTPS** (`#tag=v$pkgver`), which needs
the repository to be public. It was private until the cask, and the `arch` job
used to rewrite an SSH URL to a token-authenticated one through
`/etc/makepkg.d/gitconfig` (makepkg sets `GIT_CONFIG_GLOBAL=/dev/null`, so
nothing else is read). `makepkg` runs `check()`, so `cargo test --workspace`
gates every package build.

**The Fedora package is for the author's Asahi MacBook**, alongside the
macOS cask on the same hardware. `contrib/fedora/caco.spec` builds from a `git archive` of the
checkout, so unlike the PKGBUILD it packages the commit in hand. Cargo comes
from rustup rather than Fedora, since the weekly dependency updates can need a
newer Rust than Fedora ships. `%build` and `%check` unset Fedora's
`RUSTFLAGS`, which add full debug info and turn stripping off and so override
the workspace's release profile. `%check` runs `cargo test --workspace`, as
`check()` does, and is the only place the tests run on aarch64. The spec's
`Requires` name the libraries winit and wgpu `dlopen`, which rpm's dependency
scan cannot see: a minimal install without them died at startup under X11 on
a missing libXcursor. They come from `/proc/<pid>/maps` of a running caco
under Xvfb and headless weston, so re-derive them that way after a winit or
wgpu upgrade rather than guessing.

**The version the binary reports comes from `Cargo.toml`.** The root
`[workspace.package] version` is the single source; every crate inherits it via
`version.workspace = true`, and `caco::VERSION` (read by the sidebar footer and
the About dialog) is `env!("CARGO_PKG_VERSION")`. There is no build script and
nothing derives the version from git.

**This repository is its own Homebrew tap**: `Casks/caco.rb`, tapped as
`mythofmeat/caco`. It is a cask rather than a formula because a formula
compiled caco on the user's Mac — a Rust toolchain, `resvg` and a fat-LTO build
per upgrade — where a cask unpacks the `Caco.app` the release's `macos` job
already built with `contrib/macos/bundle.sh`. Its `binary` stanza links
`Contents/MacOS/caco-bin`, not the bundle's wrapper: the wrapper locates
`caco-bin` beside `$0`, which through a symlink is the Homebrew bin dir. The app
is signed ad hoc and not notarised, so a `postflight_steps` `run` strips the
quarantine flag or Gatekeeper would refuse it (brew 7 rejects a free-form
`postflight` block, and steps take `{{appdir}}` rather than Ruby interpolation).
arm64 only.

The cask's `version` and `sha256` are written by the release's last job, after
the zip exists, and are deliberately outside the version-agreement check: a
release that dies between tag and cask (as v4.1.6 did) would otherwise block the
next one. `bundle.sh` defaults its Info.plist `VERSION` to the Cargo version.

The internal path dependencies carry no `version` field and
`workspace.package` sets `publish = false`; nothing runs `cargo package`.
