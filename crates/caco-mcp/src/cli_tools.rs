//! MCP tools that shell out to the caco CLI.

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
