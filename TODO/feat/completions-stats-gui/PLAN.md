# Completions & Stats Management — Implementation Plan

**Chosen layout:** A · Two-pane (see `a-two-pane.html`)

## Context

Today the user can view per-WAD stats and historical completions in both GUI and CLI,
but management is split and partial:

- **CLI** can add/remove/set beaten count (`modify beaten±N`, `beaten=N`) and attach a
  stats file to the *latest* completion (`modify --stats-file`). Cannot target a specific
  completion for notes, date, or stats edits. Cannot list completions with IDs.
- **GUI** (`dialogs/wad_stats.rs`) only views. Import overwrites the latest completion.
  No affordance for add/delete/edit-notes/edit-date/clear/targeted-import/targeted-export.

Goal: close the gap on both surfaces so a user can fully manage completion records
(CRUD) and attached stats (import/export/clear) from both GUI and CLI.

---

## GUI (layout A — two-pane)

**File:** `crates/caco-gui/src/dialogs/wad_stats.rs`

### New state

```rust
pub struct WadStatsDialogState {
    wad_id: i64,
    wad_title: String,
    entries: Vec<Entry>,        // live + completions
    selected_index: usize,
    edit: Option<EditBuffer>,   // inline edit form state
    pending_import: Option<(EntryRef, FileDialogReceiver)>,
    pending_export: Option<(StatsData, FileDialogReceiver)>,
    confirm_delete: Option<i64>, // completion_id awaiting confirmation
    error: Option<String>,
}

enum EntryKind { Live, Completion(i64) }
struct Entry { kind: EntryKind, label: String, date: String, notes: String, stats: Option<StatsData> }
struct EditBuffer { completion_id: Option<i64>, date: String, notes: String }
```

### Interactions
- **Select** a row → updates `selected_index`, right pane re-renders.
- **+ Add beaten** → calls `add_wad_completion`, selects the new row, opens edit buffer.
- **Edit** → prefill `EditBuffer` from selected completion; replaces the row with a
  small form (date + notes + Save/Cancel). Live row isn't editable.
- **Delete** → two-step: first click flips the button into a red "Confirm" state;
  second click calls `delete_wad_completion`.
- **Import…** (per-entry) → picks file, parses via `wad_stats::parse_stats_file`,
  calls `update_wad_completion(id, Some(json), None)` or updates `wad.stats_snapshot`
  if the live row is selected.
- **Export…** (per-entry) → writes selected entry's snapshot to disk.
- **Clear** → nulls `stats_snapshot` on the selected completion (needs tiny DB helper).

### Result variants
Keep `WadStatsResult::{Open, Closed, Modified}`. Emit `Modified` whenever a DB write
succeeds so `CacoApp` reloads.

---

## CLI

### `caco info --completions`
**File:** `crates/caco-cli/src/commands/info.rs`

- New `--completions` flag. Prints a table: `ID | Date | Has stats | Notes`.
- Supports `-o plain|json` via existing `OutputFormat`.
- Reuses `db::sessions::get_wad_completions`.

### `caco modify` — target a specific completion
**File:** `crates/caco-cli/src/commands/modify.rs`, `crates/caco-cli/src/parsing.rs`

Add three new `ModifyAction` variants:
- `completion.<id>.notes=<value>` → update notes
- `completion.<id>.date=<value>`  → update completed_at
- `completion.<id>.stats=<path>`  → attach stats file (path parsed at apply time)
- `completion.<id>.stats=`        → clear stats

The parser currently accepts `field=value`. Extend `parse_modify_actions` to recognise
a `completion.<id>.<subfield>` prefix and route to a new `CompletionEdit` action.

New DB helpers in `db/sessions.rs`:
- Extend `update_wad_completion` to also accept `Option<&str>` for `completed_at`
  (or add `update_wad_completion_date`).
- Existing `update_wad_completion(id, stats, notes)` already covers those two fields.

### `caco modify --stats-file --completion <id>`
Add `--completion <id>` flag to target a specific completion (instead of always the
latest). Keep today's behaviour when the flag is absent (latest completion).

---

## DB layer

**File:** `crates/caco-core/src/db/sessions.rs`

- Extend `update_wad_completion` signature:
  ```rust
  pub fn update_wad_completion(
      conn: &Connection,
      id: i64,
      stats_snapshot: Option<Option<&str>>, // None = untouched, Some(None) = clear
      notes: Option<Option<&str>>,
      completed_at: Option<&str>,
  ) -> Result<bool>
  ```
  Double-`Option` lets callers distinguish "don't touch" from "set to NULL", which is
  needed for stats clear. Update the two existing call sites in `dialogs/wad_stats.rs`
  and `commands/modify.rs`.

No migration needed — `wad_completions` already has the columns we need.

---

## Touched files

- `crates/caco-core/src/db/sessions.rs` — extend `update_wad_completion`
- `crates/caco-gui/src/dialogs/wad_stats.rs` — rewrite as two-pane + full CRUD
- `crates/caco-cli/src/commands/info.rs` — add `--completions`
- `crates/caco-cli/src/commands/modify.rs` — add `--completion <id>`, handle new actions
- `crates/caco-cli/src/parsing.rs` — parse `completion.<id>.<field>=<value>`
- `crates/caco-cli/src/output.rs` — render completions table (new helper)
- `completions/` — add new subcommand/flag entries to shell completions
- `README.md`, `CLAUDE.md` — update CLI reference

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# CLI smoke tests
cargo run -p caco-cli -- info 1 --completions
cargo run -p caco-cli -- modify id:1 beaten+ --notes "plan-test"
cargo run -p caco-cli -- info 1 --completions -o json
cargo run -p caco-cli -- modify id:1 completion.<ID>.notes="edited"
cargo run -p caco-cli -- modify id:1 completion.<ID>.date="2026-01-01T12:00:00"
cargo run -p caco-cli -- modify id:1 --stats-file stats.txt --completion <ID>
cargo run -p caco-cli -- modify id:1 completion.<ID>.stats=

# GUI smoke test
cargo run -p caco-gui
# → open WAD → Map Stats → add/edit/delete/import/export/clear each row
```
