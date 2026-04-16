# Counter-Review: Response to `REVIEW.md`

**Date:** 2026-04-16
**Scope:** Verification of claims in `docs/REVIEW.md` against current code, plus a consolidated refactor plan.
**Method:** Spot-checked all 12 CRITICAL and a cross-section of HIGH/MEDIUM findings against the source. File:line citations below refer to the code as it existed on `main` at review time.
**Status:** All P0 and P1 items below have been executed on `refactor/code-review`. This file now tracks only the analytical notes and the remaining P2 work.

---

## TL;DR

The original review is largely accurate on correctness, data-integrity, and architecture issues but is uneven on security. At least two security-labelled items are wrong (C2, H3), one is mislabelled (H10), and one critical (C1) omits the mitigation already present in the code. Severity is inflated in a few places — most notably C12 (stale docs labelled CRITICAL) and C11 (CI exists, just has no test gates).

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

---

## 2. Remaining Work

All P0 and P1 items from the original consolidation have been committed on `refactor/code-review`. The sections below are what's left to ship.

### Refactor: Async file dialogs

**C8** — `rfd::FileDialog::pick_file()` / `save_file()` at 7 call sites in the GUI blocks the egui update loop. Either switch to `rfd::AsyncFileDialog` polled via the existing `workers::BackgroundChannel`, or spawn a worker thread that posts the result back. Touches:

- `caco-gui/src/dialogs/edit.rs`
- `caco-gui/src/dialogs/link.rs`
- `caco-gui/src/dialogs/resources.rs`
- `caco-gui/src/dialogs/wad_stats.rs`
- `caco-gui/src/import/form_panel.rs`
- `caco-gui/src/workers.rs` (add `FileDialogResult` variant)
- `caco-gui/src/message.rs` (new message)

### Refactor: Config reloadability

**H4** — `OnceLock<Config>` at `caco-core/src/config.rs:261-292` means the TUI/GUI can never pick up config changes without a restart. Replace with `Arc<RwLock<Config>>` or `ArcSwap<Config>` and add a `reload_config()` function. CLI keeps current behaviour by never calling reload; TUI/GUI can wire it to a file-watcher or a menu action. Fallout touches ~30 `load_config()` call sites.

### Refactor: GUI architecture cleanup

- **H19** — `caco-gui/src/app.rs` is ~1400 lines. Split the topbar, sidebar, hero/progress, status bar, and dialog hosts into their own modules under `caco-gui/src/app/`.

### Refactor: Type-level hygiene

- **H1** — `WadRecord.status`, `availability`, `source_type` are `String` despite having proper enums. Convert to `Status`, `Availability`, `SourceType`. Removes every `.status_enum()` call site and moves string-typo bugs to compile errors. Expected fallout: 20+ files across CLI/TUI/GUI rendering code.
- **M1** — `WadUpdate` builder methods consume `self` and return `Result<Self>`. On error in the middle of a chain the partially-built state is lost. Either switch to `&mut self` returning `Result<&mut Self>`, or accumulate errors in an internal `Vec<ValidationError>` and surface them at `apply()` time.

### Refactor: CLI output standardisation

- **H14** — Unify `-o/--output` across all subcommands that produce output. Some currently take `-o plain|json|table`, others take `--plain`/`--json` flags. Unify on `-o`. Touches ~3–5 command files in `caco-cli/src/commands/` plus shell completions and README examples. Breaking UX change — roll with a version bump.

### Refactor: Test coverage remainder

- **C10** — Starter pure-logic tests for `caco-tui/src/widgets/filter_input.rs`, `library_pane.rs`, and `caco-gui/src/{thumbnails.rs, import/state.rs}` landed in Phase 9. Still open: a testable debounce/query surface for `caco-gui/src/panels/filter_bar.rs` (currently pure UI glue) and rendering snapshot tests for egui/ratatui (lower ROI — defer until the logic surface grows).

---

## 3. Observations the Original Review Missed or Undersold

- **The `let _ =` pattern + no transactions is a single coherent risk.** The review splits them across C3, C6, H5, H21. In practice they're the same class of bug: caco writes to SQLite without atomicity *and* without surfacing failures. Fixing transactions without also fixing silent-error-swallowing is half a fix — a failed transaction needs to produce a logged, user-visible error, not a `let _ =`.

- **`ImportService` being a unit struct is the root cause, not a symptom.** H7, H8, and H9 all fall out of `pub struct ImportService;`. Fix the type and all three findings collapse.

- **Migration rollback strategy is undiscussed.** H21 calls out the lack of transactions but not the related gap: there is no tested recovery path when a migration fails on a user's existing DB. Wrapping in transactions fixes atomicity; a documented "restore from the pre-migration backup at `~/.local/share/caco/backups/pre-migration-N.db`" story is also worth having.

- **The `zip` crate's `enclosed_name` is load-bearing.** C1 relies on the `zip` crate's interpretation of safe paths. This is fine today, but a pinned version + a test that asserts `../` entries are dropped would keep it from silently regressing on a future bump. Worth adding alongside the defence-in-depth check.

- **`Command::args()` is safe from shell injection, but the `custom_args` field is still a UX footgun.** Pasting malformed JSON, or a flag the user's sourceport doesn't understand, produces confusing failures with no validation feedback. Not a security issue; worth a one-pass validator that at least checks the JSON parses and warns on obviously-wrong flag shapes.

---

## 4. Explicitly Not Doing

- **C2 (command injection)**: not a real issue
- **H3 (`sanitize_name` panic)**: not a real issue
- **H10 (SSRF)**: reframe as URL host validation, don't fix as "SSRF mitigation"

---

*This counter-review is not exhaustive — the MEDIUM and LOW sections in the original are mostly plausible and worth triaging individually. The purpose here is to correct the mislabelled items, consolidate related issues into coherent refactors, and give a real priority order rather than a flat severity count.*
