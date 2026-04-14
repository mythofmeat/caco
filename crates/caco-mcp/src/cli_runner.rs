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
