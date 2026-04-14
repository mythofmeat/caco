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
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let candidate = parent.join(if cfg!(windows) { "caco.exe" } else { "caco" });
        tried.push(candidate.clone());
        if candidate.is_file() {
            return Ok(CacoBin::Path(candidate));
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
        if let Ok(contents) = std::fs::read_to_string(&manifest)
            && contents.contains("[workspace]")
        {
            return Some(cur);
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
