# Counter-Review: Response to `REVIEW.md`

**Date:** 2026-04-16
**Scope:** Verification of claims in `docs/REVIEW.md` against current code, plus a consolidated refactor plan.
**Method:** Spot-checked all 12 CRITICAL and a cross-section of HIGH/MEDIUM findings against the source. File:line citations below refer to the code as it exists on `main` at review time.

---

## TL;DR

The original review is largely accurate on correctness, data-integrity, and architecture issues but is uneven on security. At least two security-labelled items are wrong (C2, H3), one is mislabelled (H10), and one critical (C1) omits the mitigation already present in the code. Severity is inflated in a few places — most notably C12 (stale docs labelled CRITICAL) and C11 (CI exists, just has no test gates).

The P0 list below strips those out and consolidates the real issues into coherent refactors rather than a long flat bug list.

---

## 1. Errata in the Original Review

These findings should be removed, downgraded, or retitled before anyone acts on the document.

### Remove

| Original | Why it's wrong |
|---|---|
| **C2 — "Command argument injection via DB fields"** | Misunderstands the `Command` API. `Command::args()` passes each element as a separate argv entry directly to the OS — there is no shell, so there is nothing to inject. A malicious DB can set *sourceport flags* (which is the feature's entire purpose), not execute arbitrary shell commands. |
| **H3 — "`sanitize_name` panic on multi-byte UTF-8"** | The slice at `utils.rs:134` runs *after* regex normalisation at lines 130–133 that reduces the string to `[a-z0-9-]` only. By the time the slice happens the string is pure ASCII. The existing `test_sanitize_name_custom_length` test already exercises this. |

### Downgrade / Rescope

| Original | Correction |
|---|---|
| **C1 — ZIP path traversal** | `saves.rs:183` uses `zip::read::ZipFile::enclosed_name()`, which returns `None` for entries that would escape the extraction root; the `continue` above drops them. The mitigation is real, just implicit. Downgrade to **MEDIUM** and frame as "add defence-in-depth: canonicalise + `starts_with(data_dir)` check, don't rely solely on the `zip` crate." |
| **C11 — "No CI/CD pipeline"** | `.gitea/workflows/package.yml` exists but only runs on version tags and only produces Arch packages. Rescope as **HIGH**: "CI runs release packaging only — no test, lint, or fmt gates on push/PR." |
| **C12 — "Python implementation does not exist"** | The *code claim* is correct: no `src/caco/` directory exists. The severity isn't: stale docs are not a CRITICAL defect. Merge with H22 as a single HIGH doc-hygiene item. |
| **H10 — "SSRF risk"** | The substring check is crude and bypassable, but this is not SSRF (the client only fetches URLs the user supplied). Relabel as "weak URL host validation" — parse `Url::parse` and check `url.host_str() == Some("www.doomworld.com")`. Still worth fixing, just for a different reason. |
| **H16 — "`play` silently consumes trailing sourceport args"** | Only applies in `--iwad` mode, where `trailing_var_arg` is deliberate pass-through. WAD mode routes through `resolve_one_wad` which ignores trailing tokens. Narrow the finding to "document IWAD-mode pass-through behaviour." |
| **H24 — "`run_sql` multi-statement check bypassable"** | The bypass is real on paper but `rusqlite::readonly()` blocks any mutation the bypass could cause. The review itself acknowledges this. Downgrade to MEDIUM. |

### Keep As-Is

Everything else verified holds up. The genuinely solid CRITICAL/HIGH items below are ready to act on.

---

## 2. Confirmed Findings to Action

Organised into coherent refactor units rather than a flat list. All items below were verified against current code.

### Refactor 1: Correctness bugs (days, not weeks)

One-touch fixes with high impact-per-line.

| Ref | File:line | Action |
|---|---|---|
| **C4** | `caco-sources/src/http.rs:74-81` | Change `\|\|` → `&&` in `is_aws_waf_challenged`. Compare to `is_cloudflare_challenged` at 62–69 which is already correct. |
| **H11** | `caco-cli/src/commands/gc.rs:790-796` | Replace local `truncate` with `output::truncate_str`, which already handles UTF-8 correctly. Also delete the duplicate. |
| **C7** | `caco-tui/src/app.rs:257-262`, `screens/tabbed_library.rs:76-78` | Populate the empty `SearchComplete` arm and wire up `set_bg_sender` — currently `ImportPaneState` is initialised with a `(tx, _rx)` channel whose receiver is dropped immediately. |
| **H12** | `caco-cli/src/output.rs:314-366` | `render_wad_info_plain` and `render_wad_info_json` drop `completions` and `companions` arguments entirely. Render them in both formats — JSON consumers are silently losing data. |
| **M49** | `caco-core/src/db/schema.rs:416-427` | Migration 22 merged `custom_complevel` into `complevel` but never dropped the source column. Add a migration that runs `ALTER TABLE wads DROP COLUMN custom_complevel`. |

### Refactor 2: Transactions around multi-step DB writes

The review is correct that the codebase has *zero* explicit transactions across multi-write flows. Consolidated fix:

Introduce a `with_transaction` helper on `Connection` (or use `rusqlite::Transaction` directly) and wrap these specific flows:

| Ref | Flow | Files |
|---|---|---|
| **C3** | `play()` — session start/end + playthrough + WAD updates + completion | `caco-core/src/player.rs:93-428` |
| **C3** | `update_wad()` multi-field paths | `caco-core/src/db/wads.rs:241-333` |
| **C6** | All `ImportService::import_*` methods | `caco-sources/src/import_service.rs:98-318` |
| **H21** | Each individual migration body + its `schema_migrations` insert | `caco-core/src/db/schema.rs:173-183` |

Migrations in particular should be `BEGIN; <migration_sql>; INSERT INTO schema_migrations; COMMIT;` so a failure during `migrate_merge_custom_complevel` (for example) doesn't leave the UPDATE half-applied with no recorded version.

### Refactor 3: Silent error suppression (`let _ =`)

**H5 confirmed at 13 sites** in `caco-core/src/player.rs`, not 9. In a playtime tracker these matter:

- `player.rs:375, 400, 416, 628, 676, 698` — DB writes for sessions, stats, demos, playthrough completion
- `player.rs:232, 235, 245, 257, 279` — filesystem ops for data dir / save dir / companion injection

At minimum, replace `let _ = foo();` with `if let Err(e) = foo() { tracing::warn!(...); }` so these surface in logs instead of disappearing. For DB writes on the session-tracking path, consider promoting to hard errors — losing a session-end update is exactly the kind of silent data corruption the review flags as "data integrity risk" elsewhere.

### Refactor 4: Resource management in IO-heavy paths

| Ref | File:line | Action |
|---|---|---|
| **C5** | `caco-sources/src/idgames/client.rs:238-240` | Replace `response.bytes()?` with a chunked stream loop (`response.copy_to(&mut file)` or manual `bytes_stream`). Keeps memory flat on 100+ MB downloads. |
| **C9** | `caco-gui/src/thumbnails.rs:79` | Replace unbounded `std::thread::spawn` with a bounded worker pool — either a fixed-size `rayon` pool or an mpsc-backed thread pool with a semaphore. Current code spawns one OS thread per visible grid cell. |
| **C8** | `caco-gui/src/dialogs/edit.rs:767`, `import/form_panel.rs:39`, plus 4 other sites | Move `rfd::FileDialog::pick_file()` off the egui update loop. Either use `rfd::AsyncFileDialog` with the egui frame's async runtime, or spawn a worker thread and post the result back via the existing message channel. |

### Refactor 5: CI + docs hygiene

**C11 (rescoped)** — add a `.gitea/workflows/ci.yml` (or equivalent) that runs on push/PR:

```
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets -- -D warnings
- cargo test --workspace
```

The existing `package.yml` stays; it just isn't a quality gate.

**C12 / H22 / H23** — three doc-hygiene items in one pass:
- Strip the "Dual Implementation", "Commands (Python)", "Python Architecture", and "Feature Parity Status" sections from `CLAUDE.md` — none of it reflects reality after the Python tree was removed.
- Update `config.example.toml` to include `auto_detect_complevel`, `auto_doomwiki_enrich`, `companion_orphan_cleanup`, `zdoom_sourceport`, `link_mode`, and the `[llm]` section.

### Refactor 6: `ImportService` statefulness

**H7 + H8** are the same bug wearing two hats. `pub struct ImportService;` is a unit struct that rebuilds a `DoomwikiClient` (and its connection pool) plus re-reads `config.toml` from disk on every single import. For a 50-WAD batch that's 50 destroyed connection pools + 50 config file reads.

Promote it to a real struct:

```rust
pub struct ImportService {
    doomwiki: DoomwikiClient,
    doomworld: DoomworldClient,
    idgames: IdgamesClient,
    config: Config,
}
```

Construct once at the CLI/TUI/GUI entry point and pass it through. Clients already share `http::build_client` under the hood, so this is the natural resting shape.

### Refactor 7: Config reloadability

**H4** — `OnceLock<Config>` at `caco-core/src/config.rs:261-292` means the TUI/GUI can never pick up config changes without a restart. For the CLI this is fine; for long-running processes it's user-visible surprise.

Replace with `Arc<RwLock<Config>>` or `ArcSwap<Config>` and add a `reload_config()` function. CLI keeps current behaviour by never calling reload; TUI/GUI can wire it to a file-watcher or a menu action.

### Refactor 8: GUI architecture cleanup

The review's `app.rs` observations are accurate:

- **H19** — `caco-gui/src/app.rs` is 1427 lines. Split the topbar, sidebar, hero/progress, status bar, and dialog hosts into their own modules under `caco-gui/src/app/`.
- **H20** — `actions.into_iter().next()` at `app.rs:676-678` drops every queued action past the first each frame. Change to `for action in actions { self.dispatch_action(action); }`. The current behaviour means rapid clicks silently evaporate.
- **H18** — `wad_grid.rs:86-363` renders every WAD row every frame. egui's `ScrollArea::show_rows` gives virtual scrolling for free; drop it in and stop painting 1000 gradient-shaded cards per frame.

### Refactor 9: Type-level hygiene

Lower-urgency but pays compounding interest:

- **H1** — `WadRecord.status`, `availability`, `source_type` are `String` despite having proper enums. Convert to `Status`, `Availability`, `SourceType`. Removes every `.status_enum()` call site and moves string-typo bugs to compile errors.
- **M1** — `WadUpdate` builder methods consume `self` and return `Result<Self>`. On error in the middle of a chain the partially-built state is lost. Either switch to `&mut self` returning `Result<&mut Self>`, or accumulate errors in an internal `Vec<ValidationError>` and surface them at `apply()` time.
- **M43** — `refresh_status_counts` in `caco-gui/src/state.rs:207-216` fetches all WADs to count statuses. A `SELECT status, COUNT(*) FROM wads WHERE deleted_at IS NULL GROUP BY status` version already exists at `caco-core/src/db/sessions.rs:671` — just call that.

### Refactor 10: TUI/GUI test coverage

**C10** is accurate: zero `#[test]` in either UI crate. That's a lot of ungated surface. Pragmatic starting set (pure logic, no rendering required):

- `caco-gui/src/thumbnails.rs` — cache key derivation, state machine transitions
- `caco-gui/src/panels/filter_bar.rs` — debounce + query parsing
- `caco-gui/src/import/state.rs` — state transitions
- `caco-tui/src/widgets/library_pane.rs` — pagination, selection, tag delta computation
- `caco-tui/src/widgets/filter_input.rs` — input handling, cursor logic

Rendering tests (egui/ratatui snapshots) are lower ROI; skip them until the logic tests exist.

---

## 3. Observations the Original Review Missed or Undersold

- **The `let _ =` pattern + no transactions is a single coherent risk.** The review splits them across C3, C6, H5, H21. In practice they're the same class of bug: caco writes to SQLite without atomicity *and* without surfacing failures. Fixing transactions without also fixing silent-error-swallowing is half a fix — a failed transaction needs to produce a logged, user-visible error, not a `let _ =`.

- **`ImportService` being a unit struct is the root cause, not a symptom.** H7, H8, and H9 all fall out of `pub struct ImportService;`. Fix the type and all three findings collapse.

- **Migration rollback strategy is undiscussed.** H21 calls out the lack of transactions but not the related gap: there is no tested recovery path when a migration fails on a user's existing DB. Wrapping in transactions fixes atomicity; a documented "restore from the pre-migration backup at `~/.local/share/caco/backups/pre-migration-N.db`" story is also worth having.

- **The `zip` crate's `enclosed_name` is load-bearing.** C1 relies on the `zip` crate's interpretation of safe paths. This is fine today, but a pinned version + a test that asserts `../` entries are dropped would keep it from silently regressing on a future bump. Worth adding alongside the defence-in-depth check.

- **`Command::args()` is safe from shell injection, but the `custom_args` field is still a UX footgun.** Pasting malformed JSON, or a flag the user's sourceport doesn't understand, produces confusing failures with no validation feedback. Not a security issue; worth a one-pass validator that at least checks the JSON parses and warns on obviously-wrong flag shapes.

---

## 4. Consolidated Priority Order

Ordered by impact-per-effort, not severity rating.

### P0 — Ship this week

1. Fix C4 (`||` → `&&`) — one-line correctness bug that breaks idgames error handling
2. Fix C7 (TUI import wiring) — a visible feature is currently dead
3. Fix H11 (`gc::truncate` panic) — call the existing safe helper
4. Fix H12 (`info` plain/JSON drops completions/companions) — silent data loss for scripted users
5. Strip Python docs from `CLAUDE.md` — unblocks every other doc update (C12/H22)

### P1 — Next

6. Wrap migrations in transactions (H21) — highest consequence if it bites
7. Wrap `player::play()` and imports in transactions (C3, C6)
8. Replace `let _ =` with logged `Err` in `player.rs` (H5)
9. Stream downloads (C5)
10. Bound thumbnail thread pool (C9)
11. Add CI with test/clippy/fmt gates (C11 rescoped)
12. Add defence-in-depth path check to `saves::restore_backup` (C1 rescoped)

### P2 — Architectural

13. Promote `ImportService` to stateful struct (H7, H8, H9)
14. Async file dialogs (C8)
15. Decompose `app.rs` and fix action-dispatch drop (H19, H20)
16. Virtual scrolling in grid view (H18)
17. Config reloadability (H4)
18. Drop `custom_complevel` column (M49)
19. Convert `WadRecord` string fields to enums (H1)
20. Standardise `-o/--output` across CLI (H14)
21. Starter test set for TUI/GUI (C10)
22. Sync `config.example.toml` (H23)

### Explicitly not doing

- C2 (command injection): not a real issue
- H3 (`sanitize_name` panic): not a real issue
- H10 (SSRF): reframe as URL host validation, don't fix as "SSRF mitigation"

---

*This counter-review is not exhaustive — the MEDIUM and LOW sections in the original are mostly plausible and worth triaging individually. The purpose here is to correct the mislabelled items, consolidate related issues into coherent refactors, and give a real priority order rather than a flat severity count.*
