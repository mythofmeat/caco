//! MCP tools that shell out to the caco CLI.

use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use crate::cli_runner::{CliResult, CliRunner};
use crate::cli_tools_macros::{push_flag, push_opt};
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

// ---------- caco_info ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct InfoArgs {
    /// Query terms to identify the WAD (e.g. "id:42" or a title fragment).
    #[serde(default)]
    pub query: Vec<String>,
    /// Output format: "table", "plain", or "json". Defaults to "json".
    #[serde(default)]
    pub output: Option<String>,
    /// Show per-map level stats.
    #[serde(default)]
    pub levelstats: bool,
    /// Show live playtime from an active session.
    #[serde(default)]
    pub live: bool,
    /// Override beaten count (e.g. "+1" or "-1").
    #[serde(default)]
    pub beaten: Option<String>,
    /// Plain-text output mode (no colour).
    #[serde(default)]
    pub plain: bool,
}

impl InfoArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        // Default to -o json so parsed_json is populated.
        let output = self.output.clone().unwrap_or_else(|| "json".into());
        argv.push("--output".into());
        argv.push(output);
        push_flag(&mut argv, "--levelstats", self.levelstats);
        push_flag(&mut argv, "--live", self.live);
        push_opt(&mut argv, "--beaten", self.beaten.as_ref());
        push_flag(&mut argv, "--plain", self.plain);
        argv.extend(self.query.clone());
        argv
    }
}

// ---------- caco_random ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct RandomArgs {
    /// Optional query to restrict the pool (e.g. "status:unplayed").
    #[serde(default)]
    pub query: Vec<String>,
    /// Print full metadata for the selected WAD.
    #[serde(default)]
    pub info: bool,
}

impl RandomArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        push_flag(&mut argv, "--info", self.info);
        argv.extend(self.query.clone());
        argv
    }
}

// ---------- caco_trash ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct TrashArgs {
    /// Query terms to identify WADs to trash or restore.
    #[serde(default)]
    pub query: Vec<String>,
    /// Output format: "table", "plain", or "json". Defaults to "json".
    #[serde(default)]
    pub output: Option<String>,
    /// List trashed WADs instead of trashing.
    #[serde(default)]
    pub list: bool,
    /// Restore trashed WADs matching the query.
    #[serde(default)]
    pub restore: bool,
    /// Permanently delete (purge) rather than soft-delete.
    #[serde(default)]
    pub purge: bool,
    /// Trash/restore a registered IWAD by family name (e.g. "doom2").
    #[serde(default)]
    pub iwad: Option<String>,
    /// Trash/restore a registered id24 WAD by name.
    #[serde(default)]
    pub id24: Option<String>,
    /// Dry run: show what would be trashed without doing it.
    #[serde(default)]
    pub dry_run: bool,
    /// Skip confirmation prompt (required in MCP context — stdin is not a tty).
    #[serde(default)]
    pub yes: bool,
}

impl TrashArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        // Default to -o json so parsed_json is populated.
        let output = self.output.clone().unwrap_or_else(|| "json".into());
        argv.push("--output".into());
        argv.push(output);
        push_flag(&mut argv, "--list", self.list);
        push_flag(&mut argv, "--restore", self.restore);
        push_flag(&mut argv, "--purge", self.purge);
        push_opt(&mut argv, "--iwad", self.iwad.as_ref());
        push_opt(&mut argv, "--id24", self.id24.as_ref());
        push_flag(&mut argv, "--dry-run", self.dry_run);
        push_flag(&mut argv, "--yes", self.yes);
        argv.extend(self.query.clone());
        argv
    }
}

// ---------- caco_enrich ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct EnrichArgs {
    /// Query terms to select WADs to enrich (empty = all eligible WADs).
    #[serde(default)]
    pub query: Vec<String>,
    /// Also auto-detect and fill missing complevel.
    #[serde(default)]
    pub complevel: bool,
    /// Dry run: show what would be enriched without writing to the DB.
    #[serde(default)]
    pub dry_run: bool,
}

impl EnrichArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        push_flag(&mut argv, "--complevel", self.complevel);
        push_flag(&mut argv, "--dry-run", self.dry_run);
        argv.extend(self.query.clone());
        argv
    }
}

// ---------- tool router ----------

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

    #[tool(
        name = "caco_info",
        description = "Show WAD metadata and stats. Mirrors `caco info`. Defaults to JSON output."
    )]
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

    #[tool(
        name = "caco_random",
        description = "Pick a random WAD from the library, optionally filtered by query. \
                       Mirrors `caco random`."
    )]
    pub async fn caco_random(
        &self,
        Parameters(args): Parameters<RandomArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["random".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_trash",
        description = "Soft-delete, restore, list, or purge WADs. Mirrors `caco trash`. \
                       Pass `yes: true` to skip the confirmation prompt (required in MCP context). \
                       Defaults to JSON output."
    )]
    pub async fn caco_trash(
        &self,
        Parameters(args): Parameters<TrashArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["trash".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_enrich",
        description = "Fetch missing metadata from Doom Wiki and optionally auto-detect complevel. \
                       Mirrors `caco enrich`."
    )]
    pub async fn caco_enrich(
        &self,
        Parameters(args): Parameters<EnrichArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["enrich".into()];
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

    #[test]
    fn info_default_uses_json_output() {
        let args = InfoArgs::default();
        let argv = args.to_argv();
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "json"));
        assert!(!argv.contains(&"--levelstats".to_string()));
        assert!(!argv.contains(&"--live".to_string()));
        assert!(!argv.contains(&"--plain".to_string()));
    }

    #[test]
    fn info_flags_render() {
        let args = InfoArgs {
            query: vec!["id:7".into()],
            output: None,
            levelstats: true,
            live: true,
            beaten: Some("+1".into()),
            plain: true,
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--levelstats".to_string()));
        assert!(argv.contains(&"--live".to_string()));
        assert!(argv.contains(&"--plain".to_string()));
        assert!(argv.windows(2).any(|w| w[0] == "--beaten" && w[1] == "+1"));
        assert!(argv.contains(&"id:7".to_string()));
    }

    #[test]
    fn random_default_renders() {
        let args = RandomArgs::default();
        let argv = args.to_argv();
        assert!(!argv.contains(&"--info".to_string()));
    }

    #[test]
    fn random_flags_render() {
        let args = RandomArgs {
            query: vec!["status:unplayed".into()],
            info: true,
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--info".to_string()));
        assert!(argv.contains(&"status:unplayed".to_string()));
    }

    #[test]
    fn trash_default_uses_json_output() {
        let args = TrashArgs::default();
        let argv = args.to_argv();
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "json"));
        assert!(!argv.contains(&"--dry-run".to_string()));
        assert!(!argv.contains(&"--yes".to_string()));
    }

    #[test]
    fn trash_flags_render() {
        let args = TrashArgs {
            query: vec!["id:3".into()],
            output: Some("plain".into()),
            list: true,
            restore: false,
            purge: true,
            iwad: Some("doom2".into()),
            id24: None,
            dry_run: true,
            yes: true,
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--list".to_string()));
        assert!(!argv.contains(&"--restore".to_string()));
        assert!(argv.contains(&"--purge".to_string()));
        assert!(argv.windows(2).any(|w| w[0] == "--iwad" && w[1] == "doom2"));
        assert!(!argv.contains(&"--id24".to_string()));
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:3".to_string()));
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "plain"));
    }

    #[test]
    fn enrich_default_renders() {
        let args = EnrichArgs::default();
        let argv = args.to_argv();
        assert!(!argv.contains(&"--complevel".to_string()));
        assert!(!argv.contains(&"--dry-run".to_string()));
    }

    #[test]
    fn enrich_flags_render() {
        let args = EnrichArgs {
            query: vec!["status:unplayed".into()],
            complevel: true,
            dry_run: true,
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--complevel".to_string()));
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(argv.contains(&"status:unplayed".to_string()));
    }
}
