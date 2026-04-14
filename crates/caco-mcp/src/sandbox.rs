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
