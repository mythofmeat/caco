# MCP Server for caco — Design Spec

**Date:** 2026-04-14
**Status:** Approved (pending user review of this doc)
**Primary goal:** Give Claude the ability to end-to-end test changes to caco against a sandboxed copy of the user's library, without touching the user's real data.

## Background

Caco is a Doom WAD library manager with a Rust-first workspace (`crates/caco-core`, `caco-sources`, `caco-cli`, `caco-tui`, `caco-gui`). Environment-variable and flag-based isolation already exists (`CACO_HOME`, `CACO_DB_PATH`, `CACO_CONFIG`, `--home`, `--db`, etc.).

This spec defines an MCP server that exposes caco functionality to an MCP client (e.g. Claude Code). The primary consumer is Claude running against a sandboxed copy of the user's library, so that end-to-end tests of in-progress code changes are safe and realistic.

Secondary uses (library access for LLMs in conversation) are out of scope for this spec but the design should not preclude them.

## Architecture

A new workspace crate **`caco-mcp`** at `crates/caco-mcp/`, producing binary `caco-mcp-server`.

- **MCP SDK:** [`rmcp`](https://crates.io/crates/rmcp) (Anthropic's official Rust MCP SDK).
- **Transport:** stdio. This matches standard MCP client launch conventions (Claude Code, etc.).
- **Execution model: hybrid.**
  - **CLI tools** shell out to the caco CLI binary to exercise the full command surface end-to-end (clap parsing, exit codes, stderr, JSON output).
  - **Introspection tools** link `caco-core` directly and read the DB with `rusqlite`. These inspect raw state that the CLI does not expose.

### CLI binary resolution

CLI tools shell out to a resolved `caco` binary. The intent is to test the **dev build**, never the user's installed copy.

Resolution order (checked at runtime, not compile-time):

1. `--caco-bin <path>` flag, if given.
2. Sibling of the running MCP server binary: `current_exe().parent()/caco` (or `caco.exe` on Windows). Works because both binaries land in `target/{debug,release}/` next to each other.
3. Fallback: `cargo run -p caco-cli --` (slower, but self-compiling; useful when no prior build exists). Invoked with working directory set to the workspace root.

The installed `caco` on `$PATH` is **never** used. This is a hard rule — resolution explicitly skips `$PATH` lookup.

### Server CLI flags

```
caco-mcp-server
  [--sandbox-path <PATH>]    # default: ~/.local/share/caco-mcp-sandbox/
  [--source-home <PATH>]     # default: ~/.local/share/caco/
  [--caco-bin <PATH>]        # default: workspace target/debug/caco, else cargo run
```

Env: `CACO_MCP_LOG=debug|info|warn|error` controls server logging. Logs go to stderr; stdout is reserved for MCP JSON-RPC frames.

## Sandbox / isolation model

### Layout

The sandbox is a full mirror of the user's caco state:

```
<sandbox>/
├── library.db              # copied from <source-home>/library.db
├── wads/                   # full WAD cache
├── data/                   # per-WAD saves/stats/configs
├── iwads/, id24/           # registered IWADs
├── companions/             # MD5-deduped companion files
├── backups/, sourceports/  # config backups & profile configs
└── config/
    └── config.toml         # copied from ~/.config/caco/config.toml
```

### Environment applied to every CLI shell-out

- `CACO_HOME=<sandbox>`
- `CACO_CONFIG=<sandbox>/config/config.toml`
- Working directory: inherited from the MCP server's cwd. No override. Tools that accept file paths receive absolute paths from the MCP client; relative paths resolve against the server's cwd.

### Bootstrap / reset

Bootstrap is **explicit**, not automatic at server startup. Triggered by the `reset_sandbox` tool:

1. Wipe `<sandbox>/`.
2. Deep-copy `--source-home` into `<sandbox>/` (all directories above, including WADs).
3. Copy `~/.config/caco/config.toml` into `<sandbox>/config/config.toml`.
4. Skip thumbnails cache (`~/.cache/caco/thumbnails/`) — regenerable.

Optional `skip_wads: true` argument on `reset_sandbox` skips `wads/`, trading faithfulness for speed.

### Safety guard

The server **refuses to start** (and every sandbox-mutating tool refuses to run) if `--sandbox-path`, after canonicalization and symlink resolution, is equal to or a parent of:

- `~/.local/share/caco/`
- `$CACO_HOME` (if set)
- `$XDG_DATA_HOME/caco/` (if set)

No flag overrides this. Typos should never nuke the real library.

## Tool surface

### CLI tools (shell out)

One MCP tool per non-interactive caco subcommand:

| Tool name | Wraps | Notes |
|---|---|---|
| `caco_ls` | `caco ls` | `-o json` by default |
| `caco_info` | `caco info` | `-o json` by default |
| `caco_modify` | `caco modify` | |
| `caco_trash` | `caco trash` | |
| `caco_random` | `caco random` | |
| `caco_import` | `caco import` | all subsources |
| `caco_cache` | `caco cache` | list/clear/prune |
| `caco_stats` | `caco stats` | |
| `caco_sessions` | `caco sessions` | |
| `caco_saves` | `caco saves` | list/backup/restore/clean/backups |
| `caco_demos` | `caco demos` | list/play/clean |
| `caco_collection` | `caco collection` | |
| `caco_companion` | `caco companion` | add/rm/enable/disable/ls |
| `caco_profile` | `caco profile` | ls/create/edit/cp/rm/path |
| `caco_enrich` | `caco enrich` | |
| `caco_gc` | `caco gc` | non-interactive mode only (`-y` injected) |
| `caco_config` | `caco config` | read-only view; `--edit` not exposed |

Each tool's parameters map 1:1 to the subcommand's clap args. Each tool returns:

```json
{
  "stdout": "string",
  "stderr": "string",
  "exit_code": 0,
  "parsed_json": { ... } | null
}
```

`parsed_json` is populated when `-o json` was used and output parses cleanly. Non-zero exit codes are **not** MCP errors — test code needs to observe failures.

Skipped subcommands (may be added later if a good reason emerges):

- `caco play` — spawns sourceports interactively; blocks indefinitely.
- `caco completions` — generates shell scripts, no testing value.
- `caco config --edit` — opens `$EDITOR`.

### Introspection tools (direct DB)

Read raw sandbox state that the CLI does not expose:

| Tool | Purpose |
|---|---|
| `inspect_wad` | Raw DB row + tags + companions for a given id or query. Includes all columns (including `custom_iwad`, `custom_args`, `complevel`, `gc_ignore`, etc.). |
| `inspect_sessions` | Session log rows. Filterable by `wad_id`, date range. |
| `inspect_companions` | Companion registry rows + wad_companions junction. Filterable by wad. |
| `inspect_iwads` | Registered IWADs with family/variant and priority resolution. |
| `inspect_id24` | Registered id24 WADs with identified hashes. |
| `inspect_schema_version` | Current migration version + list of migrations applied. |
| `run_sql` | Arbitrary read-only SQL query against `<sandbox>/library.db`. |

### `run_sql` guards

- Connection opened with `SQLITE_OPEN_READ_ONLY`.
- Input statement parsed; rejects if not exactly one `SELECT` (or `WITH ... SELECT`) statement. No `PRAGMA`, no `ATTACH`, no transactions.
- Result capped at 10,000 rows. If exceeded: first 10k rows returned with `truncated: true`.
- Returns: `{ columns: [...], rows: [[...], ...], truncated: bool }`.

### Sandbox tools

| Tool | Purpose |
|---|---|
| `reset_sandbox` | Wipe and re-bootstrap from `--source-home`. Args: `skip_wads: bool = false`. |
| `sandbox_info` | Returns `{ sandbox_path, source_home, exists, db_size_bytes, last_reset_ts, db_schema_version }`. |

## Error / output model

- **CLI tools:** always succeed from MCP's perspective, even on non-zero exit. Caller observes `exit_code`/`stderr` and decides. Tool returns MCP error only if the caco binary can't be resolved or fails to spawn.
- **Introspection tools:** structured JSON on success. MCP errors on: DB unreachable, sandbox doesn't exist (hint: "run reset_sandbox"), query failures, schema mismatch.
- **`run_sql`:** MCP error on guard violation (non-SELECT, multiple statements, parse failure). Includes guard failure reason.
- **Sandbox safety:** canonicalized path matches real caco home → hard MCP error on server startup and on every sandbox-mutating tool call. Never silently proceeds.
- **Logging:** `tracing` crate to stderr. Level controlled by `CACO_MCP_LOG`.

## Testing

### Unit tests (`crates/caco-mcp/src/...`)

- Tool schema validation for every exposed tool.
- Arg-to-clap-flag mapping for each CLI tool.
- `run_sql` guard: non-SELECT rejection, multi-statement rejection, read-only enforcement, truncation.
- Sandbox safety guard: canonicalization against real caco home under symlink/relative/parent-dir variants.
- CLI binary resolution: flag > workspace target > `cargo run` fallback.

### Integration tests (`crates/caco-mcp/tests/`)

- Start an in-process server pointed at a committed fixture source-home at `crates/caco-mcp/tests/fixtures/caco-home/` (small seeded DB + minimal WAD placeholders).
- Drive via `rmcp` client.
- At least one test per CLI tool exercising happy path + one failure mode.
- At least one test per introspection tool.
- Test the sandbox safety guard end-to-end: attempt to point sandbox at a fake "real caco home" and verify the server refuses to start.

### Gates

- `cargo test --workspace` — includes new crate, all tests pass.
- `cargo clippy --workspace -- -D warnings` — clean.
- No new CI pipeline.

Target: **~30–40 new tests**, matching the density of existing workspace crates.

## Out of scope

- `caco play` MCP tool (interactive sourceport launch).
- Library/LLM-facing MCP features (conversational library queries). The design does not preclude adding these later — tools can be added incrementally.
- A shared "library server" mode serving multiple sandboxes or clients.
- MCP resources / prompts. Tools only for v1.

## File-level plan (rough)

```
crates/caco-mcp/
├── Cargo.toml              # depends on caco-core, rmcp, tokio, serde, tracing, rusqlite
├── src/
│   ├── main.rs             # bin entry: parse flags, spawn server, stdio loop
│   ├── server.rs           # rmcp server wiring: tool registry, handler dispatch
│   ├── sandbox.rs          # sandbox path, safety guard, reset_sandbox impl, sandbox_info impl
│   ├── bin_resolve.rs      # CLI binary resolution (flag > workspace target > cargo run)
│   ├── cli_tools/          # one module per caco_* tool (args type, handler, output shape)
│   │   ├── mod.rs
│   │   ├── ls.rs, info.rs, modify.rs, ...  # 17 modules
│   ├── introspect/         # one module per inspect_* tool
│   │   ├── mod.rs
│   │   ├── wad.rs, sessions.rs, companions.rs, iwads.rs, id24.rs,
│   │   ├── schema_version.rs, run_sql.rs
│   └── error.rs            # McpError variants, From impls
└── tests/
    ├── fixtures/
    │   └── caco-home/      # seeded fixture source-home for reset_sandbox tests
    ├── cli_tools.rs        # integration tests for caco_* tools
    ├── introspect.rs       # integration tests for inspect_* tools
    ├── sandbox.rs          # sandbox lifecycle + safety guard tests
    └── common/mod.rs       # shared test harness: spawn server, rmcp client, assertions
```

## Open items (to resolve during planning)

- Exact `rmcp` version and whether it needs a specific tokio runtime flavor.
- Whether `parsed_json` should be a typed shape per tool (e.g. `Vec<WadRow>` for `caco_ls`) or a generic `serde_json::Value`. Leaning generic for v1 — typed shapes would double the maintenance surface.
- How fixture source-home WAD placeholders are represented (empty files of the right name? Checksummed stubs?). Decide during test-writing.
