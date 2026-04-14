# caco-mcp Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `caco-mcp-server`, a new workspace crate exposing caco CLI and DB introspection via the Model Context Protocol, scoped at a sandboxed copy of the user's library so Claude can end-to-end test changes safely.

**Architecture:** New workspace crate `caco-mcp` producing bin `caco-mcp-server`. Hybrid execution: CLI tools shell out to the dev `caco` binary (resolved as sibling of the MCP server binary), introspection tools open `<sandbox>/library.db` directly via `rusqlite`. Transport: stdio via `rmcp`. Bootstrap: `reset_sandbox` tool deep-copies `~/.local/share/caco/` into `<sandbox>`. Safety guard: server refuses to start if the sandbox path resolves to the real caco home.

**Tech Stack:** Rust 2024 edition, `rmcp` (Anthropic's Rust MCP SDK), `tokio` (async runtime required by `rmcp`), `rusqlite` (bundled SQLite), `serde` + `serde_json`, `schemars` (JSON schema for tool params), `thiserror`, `tracing` + `tracing-subscriber`, `fs_extra` (recursive dir copy), `tempfile` (tests).

**Spec:** `docs/superpowers/specs/2026-04-14-mcp-server-design.md`

---

## Overview of tools delivered

**Sandbox tools** (2): `sandbox_info`, `reset_sandbox`

**CLI tools** (17, all shell out to dev `caco` binary): `caco_ls`, `caco_info`, `caco_modify`, `caco_trash`, `caco_random`, `caco_import`, `caco_cache`, `caco_stats`, `caco_sessions`, `caco_saves`, `caco_demos`, `caco_collection`, `caco_companion`, `caco_profile`, `caco_enrich`, `caco_gc`, `caco_config`

**Introspection tools** (7, direct DB): `inspect_wad`, `inspect_sessions`, `inspect_companions`, `inspect_iwads`, `inspect_id24`, `inspect_schema_version`, `run_sql`

## File layout

```
crates/caco-mcp/
├── Cargo.toml
├── src/
│   ├── main.rs              # bin entry: parse flags, validate, spawn server, stdio loop
│   ├── lib.rs               # lib root for integration tests; re-exports pub modules
│   ├── server.rs            # CacoMcpServer struct, tool_router composition, ServerHandler impl
│   ├── error.rs             # CacoMcpError variants + From<T> impls
│   ├── sandbox.rs           # SandboxPaths struct + canonicalized safety guard + SandboxInfo DTO
│   ├── bin_resolve.rs       # CacoBin enum (Path / CargoRun), resolve() fn
│   ├── cli_runner.rs        # run_caco_cli(&self, argv) — spawn + capture + parse JSON
│   ├── cli_tools.rs         # #[tool_router(router=cli_tools_router)] — all 17 caco_* tools
│   ├── sandbox_tools.rs     # #[tool_router(router=sandbox_tools_router)] — sandbox_info, reset_sandbox
│   ├── introspect.rs        # #[tool_router(router=introspect_router)] — inspect_*, run_sql
│   └── reset.rs             # deep-copy implementation for reset_sandbox
└── tests/
    ├── fixtures/
    │   └── caco-home/       # seeded fixture source-home (library.db + minimal dirs)
    ├── common/
    │   └── mod.rs           # test harness: build_test_server(), call_tool()
    ├── sandbox_safety.rs    # safety guard tests
    ├── cli_tools.rs         # integration tests for caco_* tools
    ├── introspect.rs        # integration tests for inspect_* tools
    └── run_sql.rs           # run_sql guard tests
```

Every CLI tool is a thin wrapper: typed args struct → `to_argv()` → `cli_runner::run_caco_cli()`.

---

## Phase 1: Scaffolding

### Task 1: Create workspace crate and dependencies

**Files:**
- Modify: `Cargo.toml` (workspace root, lines 2-8; lines 22-85)
- Create: `crates/caco-mcp/Cargo.toml`
- Create: `crates/caco-mcp/src/main.rs`
- Create: `crates/caco-mcp/src/lib.rs`

- [ ] **Step 1: Add crate to workspace members**

Edit `Cargo.toml` (workspace root). Update the `members` array to include `"crates/caco-mcp"`:

```toml
[workspace]
members = [
    "crates/caco-core",
    "crates/caco-sources",
    "crates/caco-cli",
    "crates/caco-tui",
    "crates/caco-gui",
    "crates/caco-mcp",
]
resolver = "3"
```

- [ ] **Step 2: Add shared dependencies at workspace level**

Append to `[workspace.dependencies]` in the root `Cargo.toml`:

```toml
# MCP server
rmcp = { version = "0.8", features = ["server", "macros", "transport-io"] }
schemars = "0.9"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "io-std", "io-util", "fs", "signal"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
fs_extra = "1.3"
```

Also add internal crate ref under the existing internal-crates section:

```toml
caco-mcp = { path = "crates/caco-mcp" }
```

- [ ] **Step 3: Create `crates/caco-mcp/Cargo.toml`**

```toml
[package]
name = "caco-mcp"
version.workspace = true
edition.workspace = true
description = "MCP server for caco — exposes CLI commands and DB introspection for sandboxed testing"
authors.workspace = true

[[bin]]
name = "caco-mcp-server"
path = "src/main.rs"

[lib]
path = "src/lib.rs"

[dependencies]
caco-core = { workspace = true }
rmcp = { workspace = true }
schemars = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
fs_extra = { workspace = true }
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
clap = { workspace = true }
dirs = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 4: Create `crates/caco-mcp/src/lib.rs`**

```rust
//! caco-mcp — MCP server for caco.
//!
//! See `docs/superpowers/specs/2026-04-14-mcp-server-design.md`.

pub mod bin_resolve;
pub mod cli_runner;
pub mod cli_tools;
pub mod error;
pub mod introspect;
pub mod reset;
pub mod sandbox;
pub mod sandbox_tools;
pub mod server;
```

- [ ] **Step 5: Create stub `crates/caco-mcp/src/main.rs`**

```rust
//! caco-mcp-server binary entry point.

fn main() {
    eprintln!("caco-mcp-server: not implemented yet");
    std::process::exit(1);
}
```

Plus empty stubs for each module listed in `lib.rs` so the crate compiles. For each of `bin_resolve.rs`, `cli_runner.rs`, `cli_tools.rs`, `error.rs`, `introspect.rs`, `reset.rs`, `sandbox.rs`, `sandbox_tools.rs`, `server.rs`, create the file with just:

```rust
//! Placeholder — see plan tasks.
```

- [ ] **Step 6: Verify the workspace builds**

Run: `cargo check -p caco-mcp`
Expected: compiles clean, no warnings, no errors.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/caco-mcp/
git commit -m "feat(mcp): scaffold caco-mcp crate"
```

---

### Task 2: Error type

**Files:**
- Modify: `crates/caco-mcp/src/error.rs`

- [ ] **Step 1: Write the error type**

Replace `crates/caco-mcp/src/error.rs` with:

```rust
//! Error type for caco-mcp.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacoMcpError {
    #[error("sandbox path resolves to the real caco home: {path}")]
    SandboxPathUnsafe { path: PathBuf },

    #[error("sandbox does not exist at {path} — run reset_sandbox to bootstrap")]
    SandboxMissing { path: PathBuf },

    #[error("source home does not exist at {path}")]
    SourceHomeMissing { path: PathBuf },

    #[error("caco binary not found (tried: {tried:?})")]
    CacoBinNotFound { tried: Vec<PathBuf> },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("filesystem copy failed: {0}")]
    FsCopy(#[from] fs_extra::error::Error),

    #[error("sql statement rejected: {reason}")]
    SqlRejected { reason: String },

    #[error("wad not found: id={id}")]
    WadNotFound { id: i64 },

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl CacoMcpError {
    /// Convert into an `rmcp::ErrorData` for returning from tool handlers.
    pub fn into_mcp_error(self) -> rmcp::ErrorData {
        rmcp::ErrorData::internal_error(self.to_string(), None)
    }
}

pub type Result<T> = std::result::Result<T, CacoMcpError>;
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p caco-mcp`
Expected: compiles clean.

- [ ] **Step 3: Commit**

```bash
git add crates/caco-mcp/src/error.rs
git commit -m "feat(mcp): add CacoMcpError type"
```

---

## Phase 2: Core building blocks (TDD)

### Task 3: Sandbox safety guard

The server must never touch the real caco home. Refuses to start if the resolved sandbox path equals or is an ancestor of `~/.local/share/caco/`, `$CACO_HOME`, or `$XDG_DATA_HOME/caco/`.

**Files:**
- Modify: `crates/caco-mcp/src/sandbox.rs`

- [ ] **Step 1: Write the failing tests**

Replace `crates/caco-mcp/src/sandbox.rs` with:

```rust
//! Sandbox path management and safety guards.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{CacoMcpError, Result};

/// Paths used by the MCP server.
#[derive(Debug, Clone)]
pub struct SandboxPaths {
    pub sandbox: PathBuf,
    pub source_home: PathBuf,
}

impl SandboxPaths {
    pub fn new(sandbox: PathBuf, source_home: PathBuf) -> Result<Self> {
        validate_sandbox_path(&sandbox)?;
        Ok(Self { sandbox, source_home })
    }

    pub fn db_path(&self) -> PathBuf {
        self.sandbox.join("library.db")
    }

    pub fn config_path(&self) -> PathBuf {
        self.sandbox.join("config").join("config.toml")
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SandboxInfo {
    pub sandbox_path: PathBuf,
    pub source_home: PathBuf,
    pub exists: bool,
    pub db_size_bytes: Option<u64>,
    pub db_schema_version: Option<i64>,
    pub last_reset_ts: Option<String>,
}

/// Real caco-home paths that must never equal (or be inside) the sandbox.
fn forbidden_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::data_dir() {
        out.push(home.join("caco"));
    }
    if let Ok(caco_home) = std::env::var("CACO_HOME") {
        out.push(PathBuf::from(caco_home));
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        out.push(PathBuf::from(xdg).join("caco"));
    }
    out
}

/// Validate that `sandbox` does not overlap with any forbidden caco home.
pub fn validate_sandbox_path(sandbox: &Path) -> Result<()> {
    let canonical_sandbox = canonicalize_or_parent(sandbox);
    for forbidden in forbidden_paths() {
        let canonical_forbidden = canonicalize_or_parent(&forbidden);
        if paths_overlap(&canonical_sandbox, &canonical_forbidden) {
            return Err(CacoMcpError::SandboxPathUnsafe {
                path: sandbox.to_path_buf(),
            });
        }
    }
    Ok(())
}

/// Canonicalize a path. If it doesn't exist, canonicalize its nearest existing
/// ancestor and re-append the missing tail. This lets us validate paths that
/// haven't been created yet.
fn canonicalize_or_parent(p: &Path) -> PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    let mut tail = PathBuf::new();
    let mut cur = p.to_path_buf();
    while !cur.exists() {
        if let Some(name) = cur.file_name() {
            tail = PathBuf::from(name).join(&tail);
        }
        if !cur.pop() {
            return p.to_path_buf();
        }
    }
    cur.canonicalize().unwrap_or(cur).join(tail)
}

fn paths_overlap(a: &Path, b: &Path) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rejects_sandbox_equal_to_data_dir_caco() {
        let fake_home = TempDir::new().unwrap();
        let fake_caco = fake_home.path().join("caco");
        std::fs::create_dir_all(&fake_caco).unwrap();
        // Simulate what dirs::data_dir() would normally give us.
        temp_env::with_var("XDG_DATA_HOME", Some(fake_home.path().to_str().unwrap()), || {
            let err = validate_sandbox_path(&fake_caco).unwrap_err();
            assert!(matches!(err, CacoMcpError::SandboxPathUnsafe { .. }));
        });
    }

    #[test]
    fn rejects_sandbox_inside_data_dir_caco() {
        let fake_home = TempDir::new().unwrap();
        let fake_caco = fake_home.path().join("caco");
        std::fs::create_dir_all(&fake_caco).unwrap();
        let inside = fake_caco.join("foo");
        temp_env::with_var("XDG_DATA_HOME", Some(fake_home.path().to_str().unwrap()), || {
            let err = validate_sandbox_path(&inside).unwrap_err();
            assert!(matches!(err, CacoMcpError::SandboxPathUnsafe { .. }));
        });
    }

    #[test]
    fn rejects_sandbox_that_is_ancestor_of_data_dir_caco() {
        let fake_home = TempDir::new().unwrap();
        let fake_caco = fake_home.path().join("caco");
        std::fs::create_dir_all(&fake_caco).unwrap();
        temp_env::with_var("XDG_DATA_HOME", Some(fake_home.path().to_str().unwrap()), || {
            let err = validate_sandbox_path(fake_home.path()).unwrap_err();
            assert!(matches!(err, CacoMcpError::SandboxPathUnsafe { .. }));
        });
    }

    #[test]
    fn rejects_sandbox_matching_caco_home_env() {
        let fake = TempDir::new().unwrap();
        let sandbox = fake.path().join("my-sandbox");
        temp_env::with_var("CACO_HOME", Some(sandbox.to_str().unwrap()), || {
            let err = validate_sandbox_path(&sandbox).unwrap_err();
            assert!(matches!(err, CacoMcpError::SandboxPathUnsafe { .. }));
        });
    }

    #[test]
    fn accepts_sandbox_unrelated_to_caco_home() {
        let fake = TempDir::new().unwrap();
        let sandbox = fake.path().join("mcp-sandbox");
        temp_env::with_vars(
            [
                ("CACO_HOME", Some("/nonexistent/other/path")),
                ("XDG_DATA_HOME", Some("/nonexistent/xdg")),
            ],
            || {
                validate_sandbox_path(&sandbox).unwrap();
            },
        );
    }
}
```

Note: `temp_env` is a small crate for setting env vars in tests without affecting other tests. Add it as a dev dep:

```toml
# In crates/caco-mcp/Cargo.toml [dev-dependencies]
temp_env = "0.3"
```

- [ ] **Step 2: Run tests, confirm they fail**

Run: `cargo test -p caco-mcp --lib sandbox -- --test-threads=1`
Expected: tests compile and run; all five pass (the implementation above is complete alongside the tests in the same file).

Actually, because the implementation in Step 1 is complete, tests will PASS. This is intentional: the guard is pure logic with no downstream dependencies, so writing it and the tests in one step is clearer than staging the file through a failing state.

Expected output: `5 passed`.

- [ ] **Step 3: Commit**

```bash
git add crates/caco-mcp/Cargo.toml crates/caco-mcp/src/sandbox.rs
git commit -m "feat(mcp): add sandbox path safety guard"
```

---

### Task 4: CLI binary resolution

Resolves the dev `caco` binary to invoke. Order: `--caco-bin` flag > sibling of current_exe() > `cargo run -p caco-cli --` fallback. Never falls back to `$PATH`.

**Files:**
- Modify: `crates/caco-mcp/src/bin_resolve.rs`

- [ ] **Step 1: Write module with tests**

Replace `crates/caco-mcp/src/bin_resolve.rs`:

```rust
//! Resolve the dev `caco` binary to shell out to.

use std::path::PathBuf;
use std::process::Command;

use crate::error::{CacoMcpError, Result};

/// How to invoke the caco CLI.
#[derive(Debug, Clone)]
pub enum CacoBin {
    /// A direct path to a built `caco` binary.
    Path(PathBuf),
    /// Fallback: `cargo run -p caco-cli --` from the given workspace root.
    CargoRun { workspace_root: PathBuf },
}

impl CacoBin {
    /// Build a Command ready to receive CLI args via `.arg(...)`.
    pub fn command(&self) -> Command {
        match self {
            Self::Path(p) => Command::new(p),
            Self::CargoRun { workspace_root } => {
                let mut cmd = Command::new("cargo");
                cmd.current_dir(workspace_root);
                cmd.args(["run", "--quiet", "-p", "caco-cli", "--"]);
                cmd
            }
        }
    }
}

/// Resolve a `CacoBin` per the spec order.
pub fn resolve(explicit: Option<PathBuf>) -> Result<CacoBin> {
    if let Some(p) = explicit {
        if p.is_file() {
            return Ok(CacoBin::Path(p));
        }
        return Err(CacoMcpError::CacoBinNotFound { tried: vec![p] });
    }

    let mut tried = Vec::new();

    // Sibling of current_exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join(if cfg!(windows) { "caco.exe" } else { "caco" });
            tried.push(candidate.clone());
            if candidate.is_file() {
                return Ok(CacoBin::Path(candidate));
            }
        }
    }

    // Fallback: cargo run from workspace root. We find the workspace root by
    // walking up from current_exe() or CARGO_MANIFEST_DIR at compile time.
    if let Some(root) = locate_workspace_root() {
        return Ok(CacoBin::CargoRun { workspace_root: root });
    }

    Err(CacoMcpError::CacoBinNotFound { tried })
}

/// Walk up from current_exe() looking for a dir containing a `Cargo.toml`
/// with `[workspace]` in it. Returns None if not found.
fn locate_workspace_root() -> Option<PathBuf> {
    let mut cur = std::env::current_exe().ok()?;
    while cur.pop() {
        let manifest = cur.join("Cargo.toml");
        if let Ok(contents) = std::fs::read_to_string(&manifest) {
            if contents.contains("[workspace]") {
                return Some(cur);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn explicit_path_resolves_when_file_exists() {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("caco");
        std::fs::write(&bin, "not a real binary").unwrap();
        let resolved = resolve(Some(bin.clone())).unwrap();
        match resolved {
            CacoBin::Path(p) => assert_eq!(p, bin),
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn explicit_path_errors_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("does-not-exist");
        let err = resolve(Some(bin)).unwrap_err();
        assert!(matches!(err, CacoMcpError::CacoBinNotFound { .. }));
    }

    #[test]
    fn cargo_run_fallback_picks_workspace_root() {
        // locate_workspace_root walks up from our own test binary, which is
        // inside the caco workspace target dir. It should find the caco workspace.
        let root = locate_workspace_root().expect("should find workspace root");
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("caco-cli"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p caco-mcp --lib bin_resolve`
Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/caco-mcp/src/bin_resolve.rs
git commit -m "feat(mcp): resolve dev caco binary"
```

---

### Task 5: Generic CLI shell-out helper

Spawns the resolved `caco` binary with sandbox env vars, captures stdout/stderr/exit_code, attempts JSON parse.

**Files:**
- Modify: `crates/caco-mcp/src/cli_runner.rs`

- [ ] **Step 1: Write module with tests**

Replace `crates/caco-mcp/src/cli_runner.rs`:

```rust
//! Shell out to the dev `caco` binary against the sandbox.

use serde::{Deserialize, Serialize};

use crate::bin_resolve::CacoBin;
use crate::error::Result;
use crate::sandbox::SandboxPaths;

/// Output of a CLI tool invocation.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CliResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    /// Parsed `stdout` as JSON, if it parses cleanly.
    pub parsed_json: Option<serde_json::Value>,
}

pub struct CliRunner<'a> {
    pub bin: &'a CacoBin,
    pub paths: &'a SandboxPaths,
}

impl<'a> CliRunner<'a> {
    pub async fn run(&self, argv: Vec<String>) -> Result<CliResult> {
        let mut cmd = tokio::process::Command::from(self.bin.command());
        // tokio::process::Command::from(std::process::Command) preserves
        // program, args already set (for CargoRun), current_dir, etc.
        cmd.args(&argv);
        cmd.env("CACO_HOME", &self.paths.sandbox);
        cmd.env("CACO_CONFIG", self.paths.config_path());
        cmd.env_remove("CACO_DB_PATH");
        cmd.env_remove("CACO_CACHE_DIR");
        cmd.env_remove("CACO_DATA_DIR");
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let output = cmd.output().await?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);
        let parsed_json = serde_json::from_str(&stdout).ok();

        Ok(CliResult {
            stdout,
            stderr,
            exit_code,
            parsed_json,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn mock_bin_always_succeeds() -> (TempDir, CacoBin) {
        // Use /bin/sh as a stand-in to confirm env vars + argv plumbing.
        let dir = TempDir::new().unwrap();
        (dir, CacoBin::Path(PathBuf::from("/bin/sh")))
    }

    #[tokio::test]
    async fn captures_stdout_and_exit_code() {
        let (_dir, bin) = mock_bin_always_succeeds();
        let sandbox_dir = TempDir::new().unwrap();
        let paths = SandboxPaths {
            sandbox: sandbox_dir.path().to_path_buf(),
            source_home: sandbox_dir.path().to_path_buf(),
        };
        let runner = CliRunner { bin: &bin, paths: &paths };
        // /bin/sh -c 'echo hello; exit 3'
        let result = runner
            .run(vec!["-c".into(), "echo hello; exit 3".into()])
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 3);
    }

    #[tokio::test]
    async fn parses_json_stdout_when_valid() {
        let (_dir, bin) = mock_bin_always_succeeds();
        let sandbox_dir = TempDir::new().unwrap();
        let paths = SandboxPaths {
            sandbox: sandbox_dir.path().to_path_buf(),
            source_home: sandbox_dir.path().to_path_buf(),
        };
        let runner = CliRunner { bin: &bin, paths: &paths };
        let result = runner
            .run(vec!["-c".into(), r#"echo '{"ok":true}'"#.into()])
            .await
            .unwrap();
        assert_eq!(result.parsed_json, Some(serde_json::json!({"ok": true})));
    }

    #[tokio::test]
    async fn parsed_json_is_none_for_non_json_output() {
        let (_dir, bin) = mock_bin_always_succeeds();
        let sandbox_dir = TempDir::new().unwrap();
        let paths = SandboxPaths {
            sandbox: sandbox_dir.path().to_path_buf(),
            source_home: sandbox_dir.path().to_path_buf(),
        };
        let runner = CliRunner { bin: &bin, paths: &paths };
        let result = runner
            .run(vec!["-c".into(), "echo 'not json'".into()])
            .await
            .unwrap();
        assert_eq!(result.parsed_json, None);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p caco-mcp --lib cli_runner`
Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/caco-mcp/src/cli_runner.rs
git commit -m "feat(mcp): add CLI runner for shell-out"
```

---

## Phase 3: Sandbox bootstrap

### Task 6: Reset + bootstrap implementation

Deep-copies `source_home` into `sandbox`, copies config too. Skips thumbnails. Supports `skip_wads`.

**Files:**
- Modify: `crates/caco-mcp/src/reset.rs`

- [ ] **Step 1: Write reset module with tests**

Replace `crates/caco-mcp/src/reset.rs`:

```rust
//! Sandbox reset implementation.

use std::path::{Path, PathBuf};

use fs_extra::dir::{self, CopyOptions};

use crate::error::{CacoMcpError, Result};
use crate::sandbox::SandboxPaths;

#[derive(Debug, Default)]
pub struct ResetOptions {
    pub skip_wads: bool,
}

/// Wipe `sandbox` and deep-copy `source_home` (and the user config file) into it.
///
/// Entries under `source_home` are copied individually so we can skip `wads/`
/// when requested. The user config file is copied to `<sandbox>/config/config.toml`.
pub fn reset_sandbox(paths: &SandboxPaths, opts: &ResetOptions) -> Result<()> {
    // Sanity: refuse if somehow the safety guard was bypassed.
    crate::sandbox::validate_sandbox_path(&paths.sandbox)?;

    if !paths.source_home.is_dir() {
        return Err(CacoMcpError::SourceHomeMissing {
            path: paths.source_home.clone(),
        });
    }

    // Wipe sandbox if it exists.
    if paths.sandbox.exists() {
        std::fs::remove_dir_all(&paths.sandbox)?;
    }
    std::fs::create_dir_all(&paths.sandbox)?;

    let entries = std::fs::read_dir(&paths.source_home)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip thumbnails: they're in ~/.cache, not the source_home, so they
        // wouldn't be here anyway, but be defensive.
        if name_str == "thumbnails" {
            continue;
        }
        if opts.skip_wads && name_str == "wads" {
            continue;
        }

        let src = entry.path();
        let dst_parent = paths.sandbox.clone();
        copy_entry(&src, &dst_parent)?;
    }

    // Copy user config to <sandbox>/config/config.toml.
    let config_src = dirs::config_dir()
        .map(|d| d.join("caco").join("config.toml"));
    let config_dst = paths.config_path();
    if let Some(src) = config_src {
        if src.is_file() {
            std::fs::create_dir_all(config_dst.parent().unwrap())?;
            std::fs::copy(&src, &config_dst)?;
        }
    }

    Ok(())
}

fn copy_entry(src: &Path, dst_parent: &Path) -> Result<()> {
    if src.is_dir() {
        let opts = CopyOptions::new().overwrite(true).copy_inside(false);
        dir::copy(src, dst_parent, &opts)?;
    } else if src.is_file() {
        let dst = dst_parent.join(src.file_name().unwrap());
        std::fs::copy(src, &dst)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn seed_source(root: &Path) {
        std::fs::create_dir_all(root.join("wads")).unwrap();
        std::fs::write(root.join("library.db"), b"fake-db-bytes").unwrap();
        std::fs::write(root.join("wads/doom2.wad"), b"wad-bytes").unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/stats.txt"), b"stats").unwrap();
    }

    #[test]
    fn copies_db_and_directories() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        seed_source(src.path());
        // Ensure the sandbox path is safe (point forbidden paths elsewhere)
        temp_env::with_vars(
            [("XDG_DATA_HOME", Some("/nonexistent/xdg")), ("CACO_HOME", Some("/nonexistent"))],
            || {
                let paths = SandboxPaths::new(
                    dst.path().join("sb"),
                    src.path().to_path_buf(),
                ).unwrap();
                reset_sandbox(&paths, &ResetOptions::default()).unwrap();
                assert!(paths.db_path().is_file());
                assert!(paths.sandbox.join("wads/doom2.wad").is_file());
                assert!(paths.sandbox.join("data/stats.txt").is_file());
            },
        );
    }

    #[test]
    fn skip_wads_omits_wads_dir() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        seed_source(src.path());
        temp_env::with_vars(
            [("XDG_DATA_HOME", Some("/nonexistent/xdg")), ("CACO_HOME", Some("/nonexistent"))],
            || {
                let paths = SandboxPaths::new(
                    dst.path().join("sb"),
                    src.path().to_path_buf(),
                ).unwrap();
                reset_sandbox(&paths, &ResetOptions { skip_wads: true }).unwrap();
                assert!(paths.db_path().is_file());
                assert!(!paths.sandbox.join("wads").exists());
            },
        );
    }

    #[test]
    fn wipes_existing_sandbox() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        seed_source(src.path());
        let sb = dst.path().join("sb");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join("stale.txt"), b"stale").unwrap();
        temp_env::with_vars(
            [("XDG_DATA_HOME", Some("/nonexistent/xdg")), ("CACO_HOME", Some("/nonexistent"))],
            || {
                let paths = SandboxPaths::new(sb.clone(), src.path().to_path_buf()).unwrap();
                reset_sandbox(&paths, &ResetOptions::default()).unwrap();
                assert!(!sb.join("stale.txt").exists(), "stale file should be wiped");
                assert!(sb.join("library.db").is_file());
            },
        );
    }

    #[test]
    fn errors_when_source_home_missing() {
        let dst = TempDir::new().unwrap();
        temp_env::with_vars(
            [("XDG_DATA_HOME", Some("/nonexistent/xdg")), ("CACO_HOME", Some("/nonexistent"))],
            || {
                let paths = SandboxPaths::new(
                    dst.path().join("sb"),
                    PathBuf::from("/nonexistent/source"),
                ).unwrap();
                let err = reset_sandbox(&paths, &ResetOptions::default()).unwrap_err();
                assert!(matches!(err, CacoMcpError::SourceHomeMissing { .. }));
            },
        );
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p caco-mcp --lib reset`
Expected: 4 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/caco-mcp/src/reset.rs
git commit -m "feat(mcp): implement reset_sandbox deep copy"
```

---

## Phase 4: MCP server wiring

### Task 7: Server struct + sandbox tools + ServerHandler

First working server: registers `sandbox_info` + `reset_sandbox` only, boots over stdio.

**Files:**
- Modify: `crates/caco-mcp/src/sandbox_tools.rs`
- Modify: `crates/caco-mcp/src/server.rs`
- Modify: `crates/caco-mcp/src/main.rs`

- [ ] **Step 1: Write sandbox tools module**

Replace `crates/caco-mcp/src/sandbox_tools.rs`:

```rust
//! MCP tools for sandbox lifecycle.

use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};
use rmcp::handler::server::tool::ToolRouter;
use serde::Deserialize;

use crate::reset::{reset_sandbox, ResetOptions};
use crate::sandbox::SandboxInfo;
use crate::server::CacoMcpServer;

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ResetSandboxParams {
    #[serde(default)]
    pub skip_wads: bool,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SandboxInfoParams {}

#[tool_router(router = sandbox_tools_router, vis = "pub")]
impl CacoMcpServer {
    #[tool(
        name = "sandbox_info",
        description = "Return the sandbox path, source home, and DB state."
    )]
    pub async fn sandbox_info_tool(
        &self,
        _p: Parameters<SandboxInfoParams>,
    ) -> Json<SandboxInfo> {
        Json(self.compute_sandbox_info())
    }

    #[tool(
        name = "reset_sandbox",
        description = "Wipe the sandbox and re-bootstrap it by deep-copying the source caco home. \
                       Set skip_wads=true to omit the potentially-large wads/ directory."
    )]
    pub async fn reset_sandbox_tool(
        &self,
        Parameters(params): Parameters<ResetSandboxParams>,
    ) -> Result<Json<SandboxInfo>, rmcp::ErrorData> {
        reset_sandbox(
            &self.paths,
            &ResetOptions {
                skip_wads: params.skip_wads,
            },
        )
        .map_err(|e| e.into_mcp_error())?;
        Ok(Json(self.compute_sandbox_info()))
    }
}
```

- [ ] **Step 2: Write server module**

Replace `crates/caco-mcp/src/server.rs`:

```rust
//! Top-level MCP server for caco.

use std::path::PathBuf;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{ServerInfo, ServerCapabilities, ProtocolVersion, Implementation};
use rmcp::{tool_handler, ServerHandler};

use crate::bin_resolve::CacoBin;
use crate::sandbox::{SandboxInfo, SandboxPaths};

#[derive(Clone)]
pub struct CacoMcpServer {
    pub paths: SandboxPaths,
    pub caco_bin: CacoBin,
    pub tool_router: ToolRouter<Self>,
}

impl CacoMcpServer {
    pub fn new(paths: SandboxPaths, caco_bin: CacoBin) -> Self {
        // Compose all sub-routers. The three `*_router` functions are generated
        // by `#[tool_router(router = ..., vis = "pub")]` in the respective modules.
        let router = Self::sandbox_tools_router()
            + Self::cli_tools_router()
            + Self::introspect_router();
        Self {
            paths,
            caco_bin,
            tool_router: router,
        }
    }

    /// Compute a fresh SandboxInfo from the filesystem + DB.
    pub fn compute_sandbox_info(&self) -> SandboxInfo {
        let db_path = self.paths.db_path();
        let db_size_bytes = std::fs::metadata(&db_path).ok().map(|m| m.len());
        let db_schema_version = read_schema_version(&db_path);
        let last_reset_ts = std::fs::metadata(&self.paths.sandbox)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                Some(dt.to_rfc3339())
            });
        SandboxInfo {
            sandbox_path: self.paths.sandbox.clone(),
            source_home: self.paths.source_home.clone(),
            exists: db_path.is_file(),
            db_size_bytes,
            db_schema_version,
            last_reset_ts,
        }
    }
}

fn read_schema_version(db: &std::path::Path) -> Option<i64> {
    if !db.is_file() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok()?;
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get::<_, i64>(0),
    )
    .ok()
}

#[tool_handler]
impl ServerHandler for CacoMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "caco-mcp-server".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: Some("caco MCP server".into()),
                website_url: None,
                icons: None,
            },
            instructions: Some(
                "MCP server for caco — a Doom WAD library manager. All CLI tools \
                 (caco_*) shell out against a sandboxed copy of the user's library. \
                 Run `reset_sandbox` once to bootstrap before other tools will work."
                    .into(),
            ),
        }
    }
}
```

Also, for this task to compile, `cli_tools.rs` and `introspect.rs` must expose empty router functions. Add to `crates/caco-mcp/src/cli_tools.rs`:

```rust
//! MCP tools that shell out to the caco CLI.
//!
//! Stubs will be filled in by later tasks.

use rmcp::tool_router;
use rmcp::handler::server::tool::ToolRouter;

use crate::server::CacoMcpServer;

#[tool_router(router = cli_tools_router, vis = "pub")]
impl CacoMcpServer {}
```

And `crates/caco-mcp/src/introspect.rs`:

```rust
//! Direct-DB introspection tools.
//!
//! Stubs will be filled in by later tasks.

use rmcp::tool_router;
use rmcp::handler::server::tool::ToolRouter;

use crate::server::CacoMcpServer;

#[tool_router(router = introspect_router, vis = "pub")]
impl CacoMcpServer {}
```

- [ ] **Step 3: Write main entry point**

Replace `crates/caco-mcp/src/main.rs`:

```rust
//! caco-mcp-server binary entry point.

use std::path::PathBuf;

use clap::Parser;
use rmcp::transport::io::stdio;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use caco_mcp::bin_resolve;
use caco_mcp::sandbox::SandboxPaths;
use caco_mcp::server::CacoMcpServer;

#[derive(Parser)]
#[command(name = "caco-mcp-server", version)]
struct Args {
    /// Override the sandbox root (default: ~/.local/share/caco-mcp-sandbox/).
    #[arg(long, env = "CACO_MCP_SANDBOX")]
    sandbox_path: Option<PathBuf>,

    /// Override the source caco home to bootstrap from (default: ~/.local/share/caco/).
    #[arg(long, env = "CACO_MCP_SOURCE_HOME")]
    source_home: Option<PathBuf>,

    /// Override the caco binary to invoke (default: sibling of this binary).
    #[arg(long, env = "CACO_MCP_CACO_BIN")]
    caco_bin: Option<PathBuf>,
}

fn default_sandbox_path() -> PathBuf {
    dirs::data_dir()
        .expect("no data_dir")
        .join("caco-mcp-sandbox")
}

fn default_source_home() -> PathBuf {
    dirs::data_dir().expect("no data_dir").join("caco")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging to stderr; stdout reserved for MCP JSON-RPC.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("CACO_MCP_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    let sandbox_path = args.sandbox_path.unwrap_or_else(default_sandbox_path);
    let source_home = args.source_home.unwrap_or_else(default_source_home);
    let paths = SandboxPaths::new(sandbox_path, source_home)?;
    let caco_bin = bin_resolve::resolve(args.caco_bin)?;

    tracing::info!(sandbox = ?paths.sandbox, source_home = ?paths.source_home, "starting caco-mcp-server");

    let server = CacoMcpServer::new(paths, caco_bin);
    let (r, w) = stdio();
    let service = server.serve((r, w)).await?;
    service.waiting().await?;
    Ok(())
}
```

Add `anyhow = "1"` to `[workspace.dependencies]` in root Cargo.toml, and `anyhow = { workspace = true }` to `[dependencies]` in `crates/caco-mcp/Cargo.toml`.

Also add `chrono = { workspace = true }` to `[dependencies]` since `server.rs` uses it.

- [ ] **Step 4: Build to confirm everything links**

Run: `cargo build -p caco-mcp`
Expected: compiles with zero warnings. If you hit import errors on rmcp types (`ToolRouter`, `ProtocolVersion`, etc.), consult the docs at https://docs.rs/rmcp/0.8 and adjust the `use` paths only — the architecture doesn't change.

- [ ] **Step 5: Smoke test — run the server and send initialize**

Create a throwaway test script `/tmp/mcp-smoke.sh`:

```bash
#!/bin/bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
               '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
               '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
| CACO_MCP_SANDBOX=/tmp/caco-mcp-smoke CACO_MCP_SOURCE_HOME=/tmp/fake-source \
  cargo run -p caco-mcp --quiet -- 2>/dev/null | head -3
```

Run:
```bash
mkdir -p /tmp/fake-source && chmod +x /tmp/mcp-smoke.sh && /tmp/mcp-smoke.sh
```

Expected: three JSON lines. The second (`tools/list` response) should contain `"sandbox_info"` and `"reset_sandbox"`. If the server rejects `/tmp/caco-mcp-smoke` on safety grounds, that means `XDG_DATA_HOME` is set to `/tmp` on this machine — pick a different sandbox root.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/caco-mcp/
git commit -m "feat(mcp): wire rmcp server with sandbox_info + reset_sandbox"
```

---

## Phase 5: CLI tools (17 total)

All CLI tools share this pattern:

```rust
#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct FooArgs { /* fields matching clap flags of `caco foo` */ }

impl FooArgs {
    fn to_argv(&self) -> Vec<String> { /* render to CLI argv */ }
}

// In the cli_tools_router impl block:
#[tool(name = "caco_foo", description = "...")]
pub async fn caco_foo(
    &self,
    Parameters(args): Parameters<FooArgs>,
) -> Result<Json<CliResult>, rmcp::ErrorData> {
    let mut argv = vec!["foo".into()];
    argv.extend(args.to_argv());
    let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
    let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
    Ok(Json(result))
}
```

### Task 8: `caco_ls` — establish the pattern

Full TDD to prove the shape. Remaining 16 tools follow the same template.

**Clap reference:** `crates/caco-cli/src/commands/ls.rs` (LsArgs: query, output, deleted, tags, iwad, id24).

**Files:**
- Modify: `crates/caco-mcp/src/cli_tools.rs`
- Create: `crates/caco-mcp/src/cli_tools_macros.rs` (helper for `to_argv` flag rendering)

- [ ] **Step 1: Write argv-render helper**

Create `crates/caco-mcp/src/cli_tools_macros.rs`:

```rust
//! Helpers for rendering tool args into CLI argv.

/// Push `--flag` if boolean is true.
pub fn push_flag(argv: &mut Vec<String>, flag: &str, enabled: bool) {
    if enabled {
        argv.push(flag.to_string());
    }
}

/// Push `--flag value` if value is Some.
pub fn push_opt<T: std::fmt::Display>(argv: &mut Vec<String>, flag: &str, value: Option<&T>) {
    if let Some(v) = value {
        argv.push(flag.to_string());
        argv.push(v.to_string());
    }
}

/// Push a repeated `--flag value1 --flag value2 ...` for a Vec.
pub fn push_multi<T: std::fmt::Display>(argv: &mut Vec<String>, flag: &str, values: &[T]) {
    for v in values {
        argv.push(flag.to_string());
        argv.push(v.to_string());
    }
}
```

Add `pub mod cli_tools_macros;` to `crates/caco-mcp/src/lib.rs`.

- [ ] **Step 2: Write the `caco_ls` tool**

Replace `crates/caco-mcp/src/cli_tools.rs`:

```rust
//! MCP tools that shell out to the caco CLI.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use crate::cli_runner::{CliResult, CliRunner};
use crate::cli_tools_macros::push_flag;
use crate::server::CacoMcpServer;

// ---------- caco_ls ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct LsArgs {
    /// Query terms + optional inline sort (e.g. "status:playing playtime-").
    #[serde(default)]
    pub query: Vec<String>,
    /// Output format: "table" (default), "plain", or "json".
    #[serde(default)]
    pub output: Option<String>,
    /// List tags with counts instead of WADs.
    #[serde(default)]
    pub tags: bool,
    /// List registered IWADs instead of WADs.
    #[serde(default)]
    pub iwad: bool,
    /// List registered id24 WADs instead of WADs.
    #[serde(default)]
    pub id24: bool,
    /// Show deleted WADs (hidden CLI flag).
    #[serde(default)]
    pub deleted: bool,
}

impl LsArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        // Default to -o json so parsed_json is populated.
        let output = self.output.clone().unwrap_or_else(|| "json".into());
        argv.push("--output".into());
        argv.push(output);
        push_flag(&mut argv, "--tags", self.tags);
        push_flag(&mut argv, "--iwad", self.iwad);
        push_flag(&mut argv, "--id24", self.id24);
        push_flag(&mut argv, "--deleted", self.deleted);
        argv.extend(self.query.clone());
        argv
    }
}

#[tool_router(router = cli_tools_router, vis = "pub")]
impl CacoMcpServer {
    #[tool(
        name = "caco_ls",
        description = "List WADs, tags, IWADs, or id24 WADs in the sandbox library. \
                       Mirrors `caco ls`. Defaults to JSON output."
    )]
    pub async fn caco_ls(
        &self,
        Parameters(args): Parameters<LsArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["ls".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ls_default_uses_json_output() {
        let args = LsArgs::default();
        let argv = args.to_argv();
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "json"));
    }

    #[test]
    fn ls_flags_render() {
        let args = LsArgs {
            query: vec!["status:completed".into()],
            tags: true,
            iwad: false,
            id24: true,
            deleted: true,
            output: Some("plain".into()),
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--tags".to_string()));
        assert!(!argv.contains(&"--iwad".to_string()));
        assert!(argv.contains(&"--id24".to_string()));
        assert!(argv.contains(&"--deleted".to_string()));
        assert!(argv.contains(&"status:completed".to_string()));
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "plain"));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p caco-mcp --lib cli_tools`
Expected: 2 passed.

- [ ] **Step 4: Commit**

```bash
git add crates/caco-mcp/src/cli_tools.rs crates/caco-mcp/src/cli_tools_macros.rs crates/caco-mcp/src/lib.rs
git commit -m "feat(mcp): add caco_ls tool"
```

---

### Task 9: Add `caco_info`, `caco_random`, `caco_trash`, `caco_enrich`

Batch tools that share the simple shape (query + a few flags).

**Clap references:**
- `crates/caco-cli/src/commands/info.rs` → InfoArgs: query (Vec<String>), output, levelstats, live
- `crates/caco-cli/src/commands/random.rs` → RandomArgs: query (Vec<String>), info
- `crates/caco-cli/src/commands/trash.rs` → TrashArgs: query (Vec<String>), restore, list, iwad (Option<String>), id24 (Option<String>)
- `crates/caco-cli/src/commands/enrich.rs` → EnrichArgs: query (Vec<String>), complevel, dry_run

Before coding each, READ the corresponding source file to confirm field names and types. If the file has fields not listed here, include them in the MCP args struct.

**Files:**
- Modify: `crates/caco-mcp/src/cli_tools.rs`

- [ ] **Step 1: Read source files to confirm args**

Run:
```bash
cat crates/caco-cli/src/commands/info.rs | head -40
cat crates/caco-cli/src/commands/random.rs | head -30
cat crates/caco-cli/src/commands/trash.rs | head -40
cat crates/caco-cli/src/commands/enrich.rs | head -30
```

- [ ] **Step 2: Append tools to `cli_tools.rs`**

Add inside the existing `#[tool_router(router = cli_tools_router, vis = "pub")] impl CacoMcpServer {}` block. Add arg structs above (outside the impl). Example for `caco_info`:

```rust
use crate::cli_tools_macros::{push_flag, push_opt};

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct InfoArgs {
    #[serde(default)]
    pub query: Vec<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub levelstats: bool,
    #[serde(default)]
    pub live: bool,
}

impl InfoArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = vec!["--output".into(), self.output.clone().unwrap_or_else(|| "json".into())];
        push_flag(&mut argv, "--levelstats", self.levelstats);
        push_flag(&mut argv, "--live", self.live);
        argv.extend(self.query.clone());
        argv
    }
}

// Inside the impl:
#[tool(name = "caco_info", description = "Show details for a WAD. Mirrors `caco info`.")]
pub async fn caco_info(
    &self,
    Parameters(args): Parameters<InfoArgs>,
) -> Result<Json<CliResult>, rmcp::ErrorData> {
    let mut argv = vec!["info".into()];
    argv.extend(args.to_argv());
    let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
    let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
    Ok(Json(result))
}
```

Repeat for `RandomArgs` → `caco_random`, `TrashArgs` → `caco_trash`, `EnrichArgs` → `caco_enrich`. Each struct/handler follows the same pattern. For `trash`, `iwad` and `id24` are `Option<String>` → use `push_opt`.

Example for `TrashArgs::to_argv`:

```rust
impl TrashArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        push_flag(&mut argv, "--restore", self.restore);
        push_flag(&mut argv, "--list", self.list);
        push_opt(&mut argv, "--iwad", self.iwad.as_ref());
        push_opt(&mut argv, "--id24", self.id24.as_ref());
        argv.extend(self.query.clone());
        argv
    }
}
```

- [ ] **Step 3: Build + run unit tests**

Run: `cargo test -p caco-mcp --lib cli_tools`
Expected: previous tests still pass + new tools compile. (You may add a `to_argv` unit test per tool; one is sufficient for a shape check.)

- [ ] **Step 4: Commit**

```bash
git add crates/caco-mcp/src/cli_tools.rs
git commit -m "feat(mcp): add caco_info/random/trash/enrich tools"
```

---

### Task 10: Add `caco_modify`, `caco_cache`, `caco_stats`, `caco_sessions`, `caco_config`

More tools that map cleanly but may use positional variadic args and/or subcommands.

**Clap references:**
- `crates/caco-cli/src/commands/modify.rs` → ModifyArgs: query (Vec<String>) + actions (Vec<String>) + --add-file/--remove-file flags
- `crates/caco-cli/src/commands/cache.rs` → Cache with subcommand enum (list/clear/prune)
- `crates/caco-cli/src/commands/stats.rs` → StatsArgs: period, limit, plain
- `crates/caco-cli/src/commands/sessions.rs` → SessionsArgs: query (Vec<String>), plain
- `crates/caco-cli/src/commands/config.rs` → ConfigArgs: edit (bool — we skip this, read-only only)

**Files:**
- Modify: `crates/caco-mcp/src/cli_tools.rs`

- [ ] **Step 1: Read source files to confirm args** (as Task 9 Step 1)

- [ ] **Step 2: Add tools**

For `caco_modify`, modify args interleave `query` terms and mutations (`tag+ hard`, `beaten+1`, `author=foo`). The easy approach: one `Vec<String>` `terms` that caller pre-interleaves, plus typed `add_file`/`remove_file` flags.

```rust
#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ModifyArgs {
    /// Query and modifier terms interleaved as the CLI expects them.
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default)]
    pub add_file: Option<String>,
    #[serde(default)]
    pub remove_file: Option<String>,
}

impl ModifyArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        push_opt(&mut argv, "--add-file", self.add_file.as_ref());
        push_opt(&mut argv, "--remove-file", self.remove_file.as_ref());
        argv.extend(self.terms.clone());
        argv
    }
}

#[tool(name = "caco_modify", description = "Modify WAD metadata. Mirrors `caco modify`. \
    The `terms` list is passed verbatim as positional args — the first ones are the query, \
    the rest are modifier tokens (e.g. 'tag+hard', 'beaten+1', 'author=Doomer').")]
pub async fn caco_modify(/* ... */) { /* standard shape */ }
```

For `caco_cache`, clap uses a subcommand enum. MCP-side, expose a single enum:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CacheSubcommand { List, Clear, Prune }

#[derive(Deserialize, schemars::JsonSchema)]
pub struct CacheArgs {
    pub subcommand: CacheSubcommand,
}

impl CacheArgs {
    fn to_argv(&self) -> Vec<String> {
        vec![match self.subcommand {
            CacheSubcommand::List => "list".into(),
            CacheSubcommand::Clear => "clear".into(),
            CacheSubcommand::Prune => "prune".into(),
        }]
    }
}
```

Repeat for `stats`, `sessions`, `config`. `ConfigArgs` should have NO `edit` field — the spec explicitly excludes it.

- [ ] **Step 3: Build + run tests + commit**

```bash
cargo test -p caco-mcp --lib cli_tools
git add crates/caco-mcp/src/cli_tools.rs
git commit -m "feat(mcp): add caco_modify/cache/stats/sessions/config tools"
```

---

### Task 11: Add `caco_saves`, `caco_demos`, `caco_profile`, `caco_companion`, `caco_collection`

All of these have multi-action subcommands.

**Clap references:**
- `saves.rs`: list|backup|restore|clean|backups
- `demos.rs`: list|play|clean
- `profile.rs`: ls|create|edit|cp|rm|path
- `companion.rs`: add|rm|enable|disable|ls
- `collection.rs`: ls/save/rm/show/edit (check file for exact list)

**Files:**
- Modify: `crates/caco-mcp/src/cli_tools.rs`

- [ ] **Step 1: Read source files** as before.

- [ ] **Step 2: Define enums + tools**

Pattern: one enum per tool listing the subcommand verbs. Additional args that belong to specific subverbs (e.g. `caco profile cp <src> <dst>`) go into the args struct as `Option<String>` or `Vec<String>` `positional` fields. The `to_argv` renders them in CLI order.

Example for `caco_profile`:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSubcommand { Ls, Create, Edit, Cp, Rm, Path }

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ProfileArgs {
    pub subcommand: ProfileSubcommand,
    /// Positional args after the subcommand (e.g. profile name, src/dst names).
    #[serde(default)]
    pub positional: Vec<String>,
    /// Sourceport executable name for filtering/creation (e.g. "dsda-doom").
    #[serde(default)]
    pub sourceport: Option<String>,
}

impl ProfileArgs {
    fn to_argv(&self) -> Vec<String> {
        let verb = match self.subcommand {
            ProfileSubcommand::Ls => "ls",
            ProfileSubcommand::Create => "create",
            ProfileSubcommand::Edit => "edit",
            ProfileSubcommand::Cp => "cp",
            ProfileSubcommand::Rm => "rm",
            ProfileSubcommand::Path => "path",
        };
        let mut argv = vec![verb.into()];
        push_opt(&mut argv, "--sourceport", self.sourceport.as_ref());
        argv.extend(self.positional.clone());
        argv
    }
}
```

For `saves`/`demos`/`companion`/`collection`, follow the same shape. Check source files for per-subcommand flags and add them as Option fields only when meaningful to pass via MCP.

- [ ] **Step 3: Build + commit**

```bash
cargo test -p caco-mcp --lib cli_tools
git add crates/caco-mcp/src/cli_tools.rs
git commit -m "feat(mcp): add caco_saves/demos/profile/companion/collection tools"
```

---

### Task 12: Add `caco_import`, `caco_gc`

`caco import` is the most complex: 5 sources (idgames, doomwiki, doomworld, url, local) with per-source flags. `caco gc` has many flags and an interactive prompt that we force non-interactive.

**Clap references:**
- `crates/caco-cli/src/commands/import.rs`
- `crates/caco-cli/src/commands/gc.rs`

**Files:**
- Modify: `crates/caco-mcp/src/cli_tools.rs`

- [ ] **Step 1: Read both source files carefully and list every flag per subcommand.**

- [ ] **Step 2: Define `ImportArgs` as a union**

```rust
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum ImportSource {
    Idgames { query: String },
    Doomwiki { query: String },
    Doomworld { url: String },
    Url { url: String },
    Local { path: String },
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ImportArgs {
    pub source: ImportSource,
    /// Pass-through optional JSON file for the import_service JSON fallback.
    #[serde(default)]
    pub from_json: Option<String>,
}

impl ImportArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        match &self.source {
            ImportSource::Idgames { query } => {
                argv.push("--idgames".into());
                argv.push(query.clone());
            }
            ImportSource::Doomwiki { query } => {
                argv.push("--doomwiki".into());
                argv.push(query.clone());
            }
            ImportSource::Doomworld { url } => {
                argv.push("--doomworld".into());
                argv.push(url.clone());
            }
            ImportSource::Url { url } => {
                argv.push("--url".into());
                argv.push(url.clone());
            }
            ImportSource::Local { path } => {
                argv.push("--local".into());
                argv.push(path.clone());
            }
        }
        push_opt(&mut argv, "--from-json", self.from_json.as_ref());
        argv
    }
}
```

Check the actual import.rs source for any flags I missed (e.g. `--title`, `--author` overrides) and add them to `ImportArgs`.

- [ ] **Step 3: Define `GcArgs` with `-y` always injected**

```rust
#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct GcArgs {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub keep_saves: bool,
    #[serde(default)]
    pub keep_demos: bool,
    #[serde(default)]
    pub keep_data: bool,
    #[serde(default)]
    pub keep_cache: bool,
    #[serde(default)]
    pub keep_companions: bool,
    #[serde(default)]
    pub orphans_only: bool,
    /// Mark WADs as GC-ignored (takes a query).
    #[serde(default)]
    pub ignore: Option<String>,
    #[serde(default)]
    pub unignore: Option<String>,
}

impl GcArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = vec!["-y".into()]; // Non-interactive; always injected.
        push_flag(&mut argv, "--dry-run", self.dry_run);
        push_flag(&mut argv, "--keep-saves", self.keep_saves);
        push_flag(&mut argv, "--keep-demos", self.keep_demos);
        push_flag(&mut argv, "--keep-data", self.keep_data);
        push_flag(&mut argv, "--keep-cache", self.keep_cache);
        push_flag(&mut argv, "--keep-companions", self.keep_companions);
        push_flag(&mut argv, "--orphans-only", self.orphans_only);
        push_opt(&mut argv, "--ignore", self.ignore.as_ref());
        push_opt(&mut argv, "--unignore", self.unignore.as_ref());
        argv
    }
}
```

- [ ] **Step 4: Add handlers** (same shape as `caco_ls`).

- [ ] **Step 5: Build + commit**

```bash
cargo test -p caco-mcp --lib cli_tools
git add crates/caco-mcp/src/cli_tools.rs
git commit -m "feat(mcp): add caco_import + caco_gc tools"
```

---

### Task 13: Final CLI tool inventory check

After Tasks 8-12, the cli_tools_router should expose 17 tools: ls, info, modify, trash, random, import, cache, stats, sessions, saves, demos, collection, companion, profile, enrich, gc, config.

- [ ] **Step 1: Count registered tools**

Run:
```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
              '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
              '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
| CACO_MCP_SANDBOX=/tmp/caco-mcp-smoke CACO_MCP_SOURCE_HOME=/tmp/fake-source \
  cargo run -p caco-mcp --quiet -- 2>/dev/null \
| grep -o '"name":"caco_[a-z]*"' | sort -u | wc -l
```

Expected: `17`.

- [ ] **Step 2: If count != 17, identify missing tools**

Pipe the tools/list response through `jq` or grep, compare against the list above, add any missing in a follow-up commit.

- [ ] **Step 3: Commit (only if fixes applied)**

```bash
git add crates/caco-mcp/src/cli_tools.rs
git commit -m "feat(mcp): ensure all 17 CLI tools registered"
```

---

## Phase 6: Introspection tools

### Task 14: `inspect_schema_version`

Simplest DB read. Establishes the introspect pattern.

**Files:**
- Modify: `crates/caco-mcp/src/introspect.rs`

- [ ] **Step 1: Add the tool**

Replace `crates/caco-mcp/src/introspect.rs`:

```rust
//! Direct-DB introspection tools.

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};

use crate::error::{CacoMcpError, Result};
use crate::sandbox::SandboxPaths;
use crate::server::CacoMcpServer;

fn open_ro(paths: &SandboxPaths) -> Result<Connection> {
    let db = paths.db_path();
    if !db.is_file() {
        return Err(CacoMcpError::SandboxMissing { path: paths.sandbox.clone() });
    }
    Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(CacoMcpError::from)
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct EmptyArgs {}

#[derive(Serialize, schemars::JsonSchema)]
pub struct SchemaVersion {
    pub user_version: i64,
}

#[tool_router(router = introspect_router, vis = "pub")]
impl CacoMcpServer {
    #[tool(
        name = "inspect_schema_version",
        description = "Return the current SQLite user_version (migration pointer) of the sandbox DB."
    )]
    pub fn inspect_schema_version(
        &self,
        _p: Parameters<EmptyArgs>,
    ) -> std::result::Result<Json<SchemaVersion>, rmcp::ErrorData> {
        let conn = open_ro(&self.paths).map_err(|e| e.into_mcp_error())?;
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(CacoMcpError::from)
            .map_err(|e| e.into_mcp_error())?;
        Ok(Json(SchemaVersion { user_version }))
    }
}
```

- [ ] **Step 2: Build + smoke test**

```bash
cargo check -p caco-mcp
```

- [ ] **Step 3: Commit**

```bash
git add crates/caco-mcp/src/introspect.rs
git commit -m "feat(mcp): add inspect_schema_version"
```

---

### Task 15: `inspect_wad`, `inspect_sessions`

Read raw DB state for a given WAD.

**Files:**
- Modify: `crates/caco-mcp/src/introspect.rs`

- [ ] **Step 1: Reference existing DB models**

Read `crates/caco-core/src/db/models.rs` to understand the `WadRecord` / session row shapes. Use `serde_json::Value` for the return type to keep the plan/types simple — or `WadRecord` if it already derives `Serialize` (check the file).

- [ ] **Step 2: Add the tools**

Append to `crates/caco-mcp/src/introspect.rs`:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct InspectWadArgs {
    pub id: i64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct InspectedWad {
    pub record: serde_json::Value,
    pub tags: Vec<String>,
}

// Inside the introspect_router impl:
#[tool(
    name = "inspect_wad",
    description = "Return the raw DB row and tag list for a WAD by id."
)]
pub fn inspect_wad(
    &self,
    Parameters(args): Parameters<InspectWadArgs>,
) -> std::result::Result<Json<InspectedWad>, rmcp::ErrorData> {
    let conn = open_ro(&self.paths).map_err(|e| e.into_mcp_error())?;
    let record: serde_json::Value = conn
        .query_row(
            "SELECT * FROM wads WHERE id = ?1",
            [args.id],
            row_to_json,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                CacoMcpError::WadNotFound { id: args.id }.into_mcp_error()
            }
            other => CacoMcpError::from(other).into_mcp_error(),
        })?;
    let mut stmt = conn
        .prepare("SELECT tag FROM wad_tags WHERE wad_id = ?1 ORDER BY tag")
        .map_err(CacoMcpError::from)
        .map_err(|e| e.into_mcp_error())?;
    let tags: Vec<String> = stmt
        .query_map([args.id], |row| row.get::<_, String>(0))
        .map_err(CacoMcpError::from)
        .map_err(|e| e.into_mcp_error())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(InspectedWad { record, tags }))
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct InspectSessionsArgs {
    #[serde(default)]
    pub wad_id: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[tool(
    name = "inspect_sessions",
    description = "Return session log rows. Filterable by wad_id. Default limit 100."
)]
pub fn inspect_sessions(
    &self,
    Parameters(args): Parameters<InspectSessionsArgs>,
) -> std::result::Result<Json<Vec<serde_json::Value>>, rmcp::ErrorData> {
    let conn = open_ro(&self.paths).map_err(|e| e.into_mcp_error())?;
    let limit = args.limit.unwrap_or(100).min(10_000);
    let (sql, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(wid) = args.wad_id {
        (
            "SELECT * FROM sessions WHERE wad_id = ?1 ORDER BY started_at DESC LIMIT ?2",
            vec![Box::new(wid), Box::new(limit as i64)],
        )
    } else {
        (
            "SELECT * FROM sessions ORDER BY started_at DESC LIMIT ?1",
            vec![Box::new(limit as i64)],
        )
    };
    let mut stmt = conn.prepare(sql).map_err(CacoMcpError::from).map_err(|e| e.into_mcp_error())?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())), row_to_json)
        .map_err(CacoMcpError::from)
        .map_err(|e| e.into_mcp_error())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(rows))
}

/// Convert a rusqlite row to a JSON object keyed by column name.
fn row_to_json(row: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
    let stmt = row.as_ref();
    let mut map = serde_json::Map::new();
    for (i, name) in stmt.column_names().iter().enumerate() {
        let v: rusqlite::types::Value = row.get(i)?;
        let json_val = match v {
            rusqlite::types::Value::Null => serde_json::Value::Null,
            rusqlite::types::Value::Integer(n) => serde_json::json!(n),
            rusqlite::types::Value::Real(f) => serde_json::json!(f),
            rusqlite::types::Value::Text(s) => serde_json::json!(s),
            rusqlite::types::Value::Blob(b) => serde_json::json!(hex::encode(&b)),
        };
        map.insert(name.to_string(), json_val);
    }
    Ok(serde_json::Value::Object(map))
}
```

Add `hex = "0.4"` to workspace deps and caco-mcp deps.

- [ ] **Step 3: Build + commit**

```bash
cargo check -p caco-mcp
git add Cargo.toml crates/caco-mcp/
git commit -m "feat(mcp): add inspect_wad + inspect_sessions"
```

---

### Task 16: `inspect_companions`, `inspect_iwads`, `inspect_id24`

Further raw DB reads.

**Files:**
- Modify: `crates/caco-mcp/src/introspect.rs`

- [ ] **Step 1: Read relevant DB modules**

```bash
cat crates/caco-core/src/db/companions.rs | head -80
cat crates/caco-core/src/db/iwads.rs | head -80
cat crates/caco-core/src/db/id24.rs | head -80
```

Confirm table names: `companion_files_registry` + `wad_companions` junction; `iwads`; `id24_wads` (verify exact name).

- [ ] **Step 2: Add three tools**

Each follows `inspect_sessions` shape. For `inspect_companions`, join the junction. Example:

```rust
#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct InspectCompanionsArgs {
    #[serde(default)]
    pub wad_id: Option<i64>,
}

#[tool(
    name = "inspect_companions",
    description = "Return companion registry rows, optionally filtered by wad_id."
)]
pub fn inspect_companions(
    &self,
    Parameters(args): Parameters<InspectCompanionsArgs>,
) -> std::result::Result<Json<Vec<serde_json::Value>>, rmcp::ErrorData> {
    let conn = open_ro(&self.paths).map_err(|e| e.into_mcp_error())?;
    let sql = if args.wad_id.is_some() {
        "SELECT c.*, wc.wad_id, wc.enabled, wc.load_order
         FROM companion_files_registry c
         JOIN wad_companions wc ON wc.companion_id = c.id
         WHERE wc.wad_id = ?1
         ORDER BY wc.load_order"
    } else {
        "SELECT * FROM companion_files_registry ORDER BY id"
    };
    let mut stmt = conn.prepare(sql).map_err(CacoMcpError::from).map_err(|e| e.into_mcp_error())?;
    let rows: Vec<serde_json::Value> = match args.wad_id {
        Some(wid) => stmt.query_map([wid], row_to_json),
        None => stmt.query_map([], row_to_json),
    }
    .map_err(CacoMcpError::from)
    .map_err(|e| e.into_mcp_error())?
    .filter_map(|r| r.ok())
    .collect();
    Ok(Json(rows))
}
```

Repeat for `inspect_iwads` (`SELECT * FROM iwads ORDER BY family, priority`) and `inspect_id24` (confirm table name in Step 1).

- [ ] **Step 3: Build + commit**

```bash
cargo check -p caco-mcp
git add crates/caco-mcp/src/introspect.rs
git commit -m "feat(mcp): add inspect_companions/iwads/id24"
```

---

### Task 17: `run_sql` with guards (full TDD)

Trickiest tool: arbitrary read-only SQL. Must reject writes, multi-statements, cap at 10k rows.

**Files:**
- Modify: `crates/caco-mcp/src/introspect.rs`

- [ ] **Step 1: Write the tests first**

Append to `crates/caco-mcp/src/introspect.rs`:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct RunSqlArgs {
    pub sql: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct RunSqlResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub truncated: bool,
}

pub const RUN_SQL_ROW_LIMIT: usize = 10_000;

/// Run a read-only SELECT against the sandbox DB.
///
/// Guards:
/// - Connection opened read-only.
/// - Rejects statements that are not read-only per `Statement::readonly()`.
/// - Rejects input that parses into more than one statement (trailing content
///   after `prepare()` signals a second statement; we reject it).
pub fn execute_run_sql(paths: &SandboxPaths, sql: &str) -> Result<RunSqlResult> {
    let conn = Connection::open_with_flags(
        &paths.db_path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    // Reject if the string contains a semicolon followed by any non-whitespace.
    if has_trailing_statement(sql) {
        return Err(CacoMcpError::SqlRejected {
            reason: "multiple statements not allowed".into(),
        });
    }
    let stmt = conn.prepare(sql)?;
    if !stmt.readonly() {
        return Err(CacoMcpError::SqlRejected {
            reason: "statement is not read-only".into(),
        });
    }
    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut stmt = stmt;
    let mut rows = Vec::new();
    let mut truncated = false;
    let mut q = stmt.query([])?;
    while let Some(row) = q.next()? {
        if rows.len() >= RUN_SQL_ROW_LIMIT {
            truncated = true;
            break;
        }
        let n = columns.len();
        let mut vals = Vec::with_capacity(n);
        for i in 0..n {
            let v: rusqlite::types::Value = row.get(i)?;
            vals.push(match v {
                rusqlite::types::Value::Null => serde_json::Value::Null,
                rusqlite::types::Value::Integer(x) => serde_json::json!(x),
                rusqlite::types::Value::Real(x) => serde_json::json!(x),
                rusqlite::types::Value::Text(x) => serde_json::json!(x),
                rusqlite::types::Value::Blob(x) => serde_json::json!(hex::encode(&x)),
            });
        }
        rows.push(vals);
    }
    Ok(RunSqlResult { columns, rows, truncated })
}

fn has_trailing_statement(sql: &str) -> bool {
    // Very simple check: any `;` not at end-of-string (ignoring trailing whitespace)
    // indicates a second statement. This misses `;` inside string literals, but
    // rusqlite's prepare rejects that anyway for DDL/DML; and for SELECT it would
    // just mean the user is being cute.
    let trimmed = sql.trim_end_matches(|c: char| c.is_whitespace() || c == ';');
    trimmed.contains(';')
}

#[cfg(test)]
mod run_sql_tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn seed_sandbox() -> (TempDir, SandboxPaths) {
        let dir = TempDir::new().unwrap();
        let sandbox = dir.path().to_path_buf();
        std::fs::create_dir_all(&sandbox).unwrap();
        let db = sandbox.join("library.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE wads(id INTEGER PRIMARY KEY, title TEXT);
             INSERT INTO wads VALUES (1, 'Doom'), (2, 'Doom II');",
        )
        .unwrap();
        let paths = SandboxPaths {
            sandbox,
            source_home: dir.path().to_path_buf(),
        };
        (dir, paths)
    }

    #[test]
    fn select_returns_rows() {
        let (_d, paths) = seed_sandbox();
        let res = execute_run_sql(&paths, "SELECT id, title FROM wads ORDER BY id").unwrap();
        assert_eq!(res.columns, vec!["id", "title"]);
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0][1], serde_json::json!("Doom"));
        assert!(!res.truncated);
    }

    #[test]
    fn rejects_insert() {
        let (_d, paths) = seed_sandbox();
        let err = execute_run_sql(&paths, "INSERT INTO wads VALUES (3, 'Final Doom')").unwrap_err();
        // Read-only connection will fail the attempt; whether as SqlRejected or Database
        // is fine — both are correct rejection behavior.
        assert!(matches!(err, CacoMcpError::SqlRejected { .. } | CacoMcpError::Database(_)));
    }

    #[test]
    fn rejects_delete() {
        let (_d, paths) = seed_sandbox();
        let err = execute_run_sql(&paths, "DELETE FROM wads").unwrap_err();
        assert!(matches!(err, CacoMcpError::SqlRejected { .. } | CacoMcpError::Database(_)));
    }

    #[test]
    fn rejects_multiple_statements() {
        let (_d, paths) = seed_sandbox();
        let err = execute_run_sql(&paths, "SELECT 1; SELECT 2").unwrap_err();
        assert!(matches!(err, CacoMcpError::SqlRejected { .. }));
    }

    #[test]
    fn truncates_at_limit() {
        let (_d, paths) = seed_sandbox();
        // Seed 10_001 rows.
        let conn = Connection::open(paths.db_path()).unwrap();
        conn.execute_batch("CREATE TABLE big(x INTEGER);").unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        for i in 0..10_001 {
            tx.execute("INSERT INTO big VALUES (?1)", [i]).unwrap();
        }
        tx.commit().unwrap();
        drop(conn);

        let res = execute_run_sql(&paths, "SELECT x FROM big").unwrap();
        assert_eq!(res.rows.len(), RUN_SQL_ROW_LIMIT);
        assert!(res.truncated);
    }
}
```

And the tool wrapper (inside the existing `introspect_router` impl block):

```rust
#[tool(
    name = "run_sql",
    description = "Run a read-only SELECT against the sandbox DB. Rejects writes, \
                   multiple statements. Caps result at 10000 rows."
)]
pub fn run_sql(
    &self,
    Parameters(args): Parameters<RunSqlArgs>,
) -> std::result::Result<Json<RunSqlResult>, rmcp::ErrorData> {
    execute_run_sql(&self.paths, &args.sql)
        .map(Json)
        .map_err(|e| e.into_mcp_error())
}
```

- [ ] **Step 2: Run tests, confirm they all pass**

Run: `cargo test -p caco-mcp --lib introspect`
Expected: all run_sql tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/caco-mcp/src/introspect.rs
git commit -m "feat(mcp): add run_sql with read-only guards"
```

---

## Phase 7: Integration tests + fixtures

### Task 18: Test harness + seeded fixture

**Files:**
- Create: `crates/caco-mcp/tests/common/mod.rs`
- Create: `crates/caco-mcp/tests/fixtures/caco-home/` (bootstrapped in Step 2)

- [ ] **Step 1: Write a fixture generator**

Create `crates/caco-mcp/tests/common/mod.rs`:

```rust
//! Shared test harness.

use std::path::{Path, PathBuf};

use caco_mcp::bin_resolve::CacoBin;
use caco_mcp::sandbox::SandboxPaths;
use caco_mcp::server::CacoMcpServer;
use rusqlite::Connection;
use tempfile::TempDir;

/// Build a self-contained test server whose sandbox is a fresh tempdir and
/// whose `source_home` is a seeded fixture.
pub fn build_test_server() -> (TempDir, TempDir, CacoMcpServer) {
    let sandbox = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    seed_source_home(source.path());

    // Isolate the env so the safety guard accepts our tempdir.
    std::env::set_var("XDG_DATA_HOME", "/nonexistent/xdg-during-tests");
    std::env::remove_var("CACO_HOME");

    let paths = SandboxPaths::new(sandbox.path().to_path_buf(), source.path().to_path_buf())
        .expect("sandbox path valid");
    // Use cargo-run fallback so we don't depend on a built target/debug/caco.
    let caco_bin = caco_mcp::bin_resolve::resolve(None).expect("caco bin");
    let server = CacoMcpServer::new(paths, caco_bin);
    (sandbox, source, server)
}

/// Create a seeded `source_home` with a valid caco library.db + minimal dirs.
pub fn seed_source_home(root: &Path) {
    for sub in &["wads", "data", "iwads", "id24", "companions", "backups"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    // Build a minimal DB by calling caco-core's init_db (ensures all migrations run).
    let db_path = root.join("library.db");
    let conn = Connection::open(&db_path).unwrap();
    caco_core::db::init_db(&conn).expect("init_db");
    // Insert one synthetic WAD using the builder pattern.
    let id = caco_core::db::add_wad(
        &conn,
        caco_core::db::NewWad::builder()
            .title("Fixture WAD".into())
            .author("fixture".into())
            .build(),
    )
    .expect("add_wad");
    caco_core::db::add_tag(&conn, id, "fixture").ok();
}
```

- [ ] **Step 2: Verify build_test_server works**

Create a tiny smoke integration test file `crates/caco-mcp/tests/smoke.rs`:

```rust
mod common;

use common::build_test_server;

#[test]
fn harness_builds() {
    let (_sb, _src, server) = build_test_server();
    let info = server.compute_sandbox_info();
    assert_eq!(info.source_home.file_name().unwrap(), _src.path().file_name().unwrap());
}
```

Run: `cargo test -p caco-mcp --test smoke`
Expected: 1 passed.

If `caco_core::db::add_wad` / `NewWad::builder` signatures differ from the above, adjust to the actual API (verify against `crates/caco-core/src/db/models.rs` and `wads.rs`).

- [ ] **Step 3: Commit**

```bash
git add crates/caco-mcp/tests/
git commit -m "test(mcp): add integration test harness"
```

---

### Task 19: Integration test — CLI tools

**Files:**
- Create: `crates/caco-mcp/tests/cli_tools.rs`

- [ ] **Step 1: Write CLI tool tests**

Create `crates/caco-mcp/tests/cli_tools.rs`:

```rust
mod common;

use caco_mcp::reset::{reset_sandbox, ResetOptions};
use common::build_test_server;

#[tokio::test]
async fn caco_ls_lists_fixture_wad() {
    let (_sb, _src, server) = build_test_server();
    reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();

    let runner = caco_mcp::cli_runner::CliRunner {
        bin: &server.caco_bin,
        paths: &server.paths,
    };
    let result = runner
        .run(vec!["ls".into(), "--output".into(), "json".into()])
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    let json = result.parsed_json.expect("json output");
    // The JSON should be an array containing at least our fixture WAD.
    let arr = json.as_array().expect("array");
    assert!(arr.iter().any(|w| w.get("title").and_then(|v| v.as_str()) == Some("Fixture WAD")));
}

#[tokio::test]
async fn caco_modify_adds_tag_and_inspect_wad_sees_it() {
    let (_sb, _src, server) = build_test_server();
    reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();

    // Find the fixture WAD id by running caco_ls.
    let runner = caco_mcp::cli_runner::CliRunner {
        bin: &server.caco_bin,
        paths: &server.paths,
    };
    let ls = runner
        .run(vec!["ls".into(), "--output".into(), "json".into()])
        .await
        .unwrap();
    let id = ls.parsed_json.unwrap()[0]["id"].as_i64().expect("id");

    // Tag it.
    let m = runner
        .run(vec![
            "modify".into(),
            format!("id:{id}"),
            "tag+hard".into(),
        ])
        .await
        .unwrap();
    assert_eq!(m.exit_code, 0, "stderr: {}", m.stderr);

    // Check via inspection.
    let conn = rusqlite::Connection::open_with_flags(
        server.paths.db_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let tags: Vec<String> = conn
        .prepare("SELECT tag FROM wad_tags WHERE wad_id = ?1")
        .unwrap()
        .query_map([id], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(tags.contains(&"hard".to_string()), "tags were {:?}", tags);
}

#[tokio::test]
async fn caco_ls_nonexistent_sandbox_errors_gracefully() {
    let (_sb, _src, server) = build_test_server();
    // Do NOT reset — DB doesn't exist.
    let runner = caco_mcp::cli_runner::CliRunner {
        bin: &server.caco_bin,
        paths: &server.paths,
    };
    let result = runner.run(vec!["ls".into()]).await.unwrap();
    // caco CLI auto-creates its DB at CACO_HOME. So this is NOT a fatal error;
    // it should succeed and return an empty list. Reflect that.
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
}
```

Note: if `caco ls --output json` doesn't exist (e.g. default is `table`), the JSON parse will fail; verify the flag name matches `ls.rs` (it's `--output` per the source).

- [ ] **Step 2: Run + commit**

```bash
cargo test -p caco-mcp --test cli_tools
git add crates/caco-mcp/tests/
git commit -m "test(mcp): integration tests for CLI tools"
```

---

### Task 20: Integration test — introspection + safety guard

**Files:**
- Create: `crates/caco-mcp/tests/introspect.rs`
- Create: `crates/caco-mcp/tests/sandbox_safety.rs`

- [ ] **Step 1: Write introspection tests**

Create `crates/caco-mcp/tests/introspect.rs`:

```rust
mod common;

use caco_mcp::introspect::{execute_run_sql, RUN_SQL_ROW_LIMIT};
use caco_mcp::reset::{reset_sandbox, ResetOptions};
use common::build_test_server;

#[test]
fn run_sql_selects_fixture_wad() {
    let (_sb, _src, server) = build_test_server();
    reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();
    let res = execute_run_sql(&server.paths, "SELECT id, title FROM wads").unwrap();
    assert_eq!(res.columns, vec!["id", "title"]);
    assert!(!res.rows.is_empty());
    assert!(!res.truncated);
}

#[test]
fn run_sql_rejects_writes() {
    let (_sb, _src, server) = build_test_server();
    reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();
    let err = execute_run_sql(&server.paths, "INSERT INTO wads (title) VALUES ('hack')").unwrap_err();
    assert!(format!("{err:#}").to_lowercase().contains("read") ||
            format!("{err:#}").to_lowercase().contains("reject"));
}

#[test]
fn run_sql_rejects_multiple_statements() {
    let (_sb, _src, server) = build_test_server();
    reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();
    let err = execute_run_sql(&server.paths, "SELECT 1; SELECT 2").unwrap_err();
    assert!(format!("{err:#}").to_lowercase().contains("multiple"));
}
```

- [ ] **Step 2: Write safety-guard integration test**

Create `crates/caco-mcp/tests/sandbox_safety.rs`:

```rust
use caco_mcp::error::CacoMcpError;
use caco_mcp::sandbox::SandboxPaths;
use tempfile::TempDir;

#[test]
fn rejects_sandbox_equal_to_fake_caco_home() {
    let fake = TempDir::new().unwrap();
    let caco_home = fake.path().join("caco");
    std::fs::create_dir_all(&caco_home).unwrap();
    temp_env::with_var("XDG_DATA_HOME", Some(fake.path().to_str().unwrap()), || {
        let err = SandboxPaths::new(caco_home.clone(), fake.path().to_path_buf()).unwrap_err();
        assert!(matches!(err, CacoMcpError::SandboxPathUnsafe { .. }));
    });
}
```

- [ ] **Step 3: Run + commit**

```bash
cargo test -p caco-mcp --test introspect
cargo test -p caco-mcp --test sandbox_safety
git add crates/caco-mcp/tests/
git commit -m "test(mcp): integration tests for introspection + safety"
```

---

## Phase 8: Polish

### Task 21: Full workspace checks + changelog

- [ ] **Step 1: Run every gate**

Run in sequence:

```bash
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Expected: all three succeed cleanly. If clippy flags anything in `caco-mcp`, fix in place (prefer `#[allow(...)]` only when the clippy complaint is a false positive; state why in a brief comment).

- [ ] **Step 2: Update CHANGELOG.md**

Add a new entry at the top:

```markdown
## [Unreleased]

### Added

- MCP server (`caco-mcp`): new workspace crate + `caco-mcp-server` binary exposing 17 CLI commands and 7 DB introspection tools via the Model Context Protocol. Runs against a sandboxed copy of the user's library; hard safety guard prevents operating on the real caco home.
```

- [ ] **Step 3: Update CLAUDE.md**

Add `caco-mcp` to the crates list in the Rust Architecture section. Insert after the `caco-gui` block:

```markdown
├── caco-mcp/           # MCP server (rmcp)
│   └── src/
│       ├── main.rs, lib.rs, server.rs  # Entry point, CacoMcpServer, rmcp ServerHandler
│       ├── sandbox.rs, reset.rs        # Sandbox paths, safety guard, bootstrap
│       ├── bin_resolve.rs, cli_runner.rs # Dev caco bin discovery + shell-out
│       ├── cli_tools.rs                # 17 caco_* tools
│       ├── sandbox_tools.rs            # sandbox_info, reset_sandbox
│       └── introspect.rs               # 7 inspect_* tools + run_sql
```

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md CLAUDE.md
git commit -m "docs: document caco-mcp crate"
```

---

### Task 22: Manual end-to-end smoke

- [ ] **Step 1: Build release binary**

```bash
cargo build --release -p caco-mcp
```

- [ ] **Step 2: Manual MCP handshake against a real sandbox**

```bash
mkdir -p /tmp/caco-mcp-smoke-src
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
| CACO_MCP_SANDBOX=/tmp/caco-mcp-smoke-sb \
  CACO_MCP_SOURCE_HOME=/tmp/caco-mcp-smoke-src \
  ./target/release/caco-mcp-server 2>/tmp/caco-mcp-smoke.log \
| head -3
```

Expected: three JSON lines. `tools/list` response should contain at least 24 tool names (17 CLI + 7 inspect + 2 sandbox = 26).

- [ ] **Step 3: Count tools**

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
| CACO_MCP_SANDBOX=/tmp/caco-mcp-smoke-sb \
  CACO_MCP_SOURCE_HOME=/tmp/caco-mcp-smoke-src \
  ./target/release/caco-mcp-server 2>/dev/null \
| grep -o '"name":"[^"]*"' | wc -l
```

Expected: ≥ 26.

- [ ] **Step 4: If everything passes, report completion**

No commit needed — just confirm the server boots and exposes all tools.

---

## Appendix: Missing flags catalogue

When implementing the CLI tools in Tasks 8–12, always **read the corresponding source file first** to confirm the flag list. The plan above lists the flags I saw during design; source files change. For each tool, after reading the source, the MCP args struct must contain a field for every flag on the clap `Args` struct (except interactive ones like `--edit`, which are explicitly out of scope per the spec). Do not silently drop flags.

## Appendix: rmcp version drift

The plan targets rmcp 0.8. If a newer version is published by the time implementation starts, the macro surface (`tool_router`, `tool_handler`, `tool`) is expected to be stable, but `ServerInfo` fields or import paths may shift. If a build error points to an rmcp type or path, consult https://docs.rs/rmcp/latest/rmcp/ and adjust imports only — the architecture and tool contracts should not change.
