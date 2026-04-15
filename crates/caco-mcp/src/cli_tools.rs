//! MCP tools that shell out to the caco CLI.

use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use crate::cli_runner::{CliResult, CliRunner};
use crate::cli_tools_macros::{push_flag, push_multi, push_opt};
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
    /// Filter level stats by completion timestamp (used with --levelstats).
    /// Value is an ISO-8601 timestamp prefix, e.g. "2025-01" or "2025-01-15T10:30".
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

// ---------- caco_modify ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ModifyArgs {
    /// Query terms and modifier tokens interleaved as the CLI expects them.
    /// E.g. ["id:5", "tag+hard", "beaten+1", "author=Doomer"].
    /// The first tokens match WADs; the remainder apply changes. See `caco modify --help`.
    #[serde(default)]
    pub terms: Vec<String>,
    /// Completion notes (for beaten+ actions).
    #[serde(default)]
    pub notes: Option<String>,
    /// Completion date override (ISO 8601).
    #[serde(default)]
    pub date: Option<String>,
    /// Attach a stats file (path) to the WAD.
    #[serde(default)]
    pub stats_file: Option<String>,
    /// Target completion timestamp when attaching stats (prefix match).
    #[serde(default)]
    pub beaten: Option<String>,
    /// Link a local file to the WAD cache.
    #[serde(default)]
    pub link: Option<String>,
    /// Companion files to add (paths; repeatable).
    #[serde(default)]
    pub add_files: Vec<String>,
    /// Companion files to remove (identifiers; repeatable).
    #[serde(default)]
    pub remove_files: Vec<String>,
    /// Preview changes without applying.
    #[serde(default)]
    pub dry_run: bool,
    /// Skip confirmations. REQUIRED in MCP context when the query matches multiple WADs.
    #[serde(default)]
    pub yes: bool,
}

impl ModifyArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        push_opt(&mut argv, "--notes", self.notes.as_ref());
        push_opt(&mut argv, "--date", self.date.as_ref());
        push_opt(&mut argv, "--stats-file", self.stats_file.as_ref());
        push_opt(&mut argv, "--beaten", self.beaten.as_ref());
        push_opt(&mut argv, "--link", self.link.as_ref());
        push_multi(&mut argv, "--add-file", &self.add_files);
        push_multi(&mut argv, "--remove-file", &self.remove_files);
        push_flag(&mut argv, "--dry-run", self.dry_run);
        push_flag(&mut argv, "--yes", self.yes);
        argv.extend(self.terms.clone());
        argv
    }
}

// ---------- caco_cache ----------

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum CacheArgs {
    /// List cached WADs.
    List {
        /// Plain TSV output.
        #[serde(default)]
        plain: bool,
        /// Show orphaned cache files.
        #[serde(default)]
        orphans: bool,
    },
    /// Remove cached WAD files.
    Clear {
        /// Query terms (required unless `all` is true).
        #[serde(default)]
        query: Vec<String>,
        /// Clear every cached file.
        #[serde(default)]
        all: bool,
        /// Preview changes without deleting.
        #[serde(default)]
        dry_run: bool,
        /// Skip confirmation.
        #[serde(default)]
        yes: bool,
    },
    /// Remove orphaned cache files.
    Prune {
        /// Preview changes.
        #[serde(default)]
        dry_run: bool,
        /// Skip confirmation.
        #[serde(default)]
        yes: bool,
    },
}

impl CacheArgs {
    fn to_argv(&self) -> Vec<String> {
        match self {
            CacheArgs::List { plain, orphans } => {
                let mut argv = vec!["list".into()];
                push_flag(&mut argv, "--plain", *plain);
                push_flag(&mut argv, "--orphans", *orphans);
                argv
            }
            CacheArgs::Clear { query, all, dry_run, yes } => {
                let mut argv = vec!["clear".into()];
                push_flag(&mut argv, "--all", *all);
                push_flag(&mut argv, "--dry-run", *dry_run);
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
            CacheArgs::Prune { dry_run, yes } => {
                let mut argv = vec!["prune".into()];
                push_flag(&mut argv, "--dry-run", *dry_run);
                push_flag(&mut argv, "--yes", *yes);
                argv
            }
        }
    }
}

// ---------- caco_stats ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct StatsArgs {
    /// Group activity by "month" (default) or "year".
    #[serde(default)]
    pub period: Option<String>,
    /// Number of periods to show (default: 12).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Key=value (plain) output.
    #[serde(default)]
    pub plain: bool,
}

impl StatsArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        push_opt(&mut argv, "--period", self.period.as_ref());
        push_opt(&mut argv, "--limit", self.limit.as_ref());
        push_flag(&mut argv, "--plain", self.plain);
        argv
    }
}

// ---------- caco_sessions ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SessionsArgs {
    /// Query or WAD id to select the WAD.
    #[serde(default)]
    pub query: Vec<String>,
    /// Plain TSV output.
    #[serde(default)]
    pub plain: bool,
    /// Maps to `caco sessions -y`. NOTE: has no effect in MCP context — `caco sessions`
    /// uses interactive picker mode (`ResolveMode::Pick`), which ignores `--yes`. To select
    /// a single WAD reliably, pass a precise query like `id:N` instead.
    #[serde(default)]
    pub yes: bool,
}

impl SessionsArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        push_flag(&mut argv, "--plain", self.plain);
        push_flag(&mut argv, "--yes", self.yes);
        argv.extend(self.query.clone());
        argv
    }
}

// ---------- caco_config ----------

/// READ-ONLY: prints the current config file to stdout.
/// The `--edit` flag is deliberately not exposed because it spawns $EDITOR interactively.
#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ConfigArgs {}

impl ConfigArgs {
    fn to_argv(&self) -> Vec<String> {
        Vec::new()
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
        description = "Show WAD metadata and stats. Mirrors `caco info`. Defaults to JSON output. \
                       Query must identify a single WAD — use an ID or a sufficiently specific title. \
                       Queries matching multiple WADs will fail (non-zero exit) because interactive \
                       selection is unavailable over MCP."
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
                       Mirrors `caco random`. \
                       Output is plain text in stdout (just the WAD ID; or TSV of id/title/author \
                       with `info: true`). `parsed_json` is always null for this tool."
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

    #[tool(
        name = "caco_modify",
        description = "Modify WAD metadata, tags, and completions. Mirrors `caco modify`. \
                       Pass query terms and action tokens interleaved in `terms` \
                       (e.g. [\"id:5\", \"tag+hard\", \"beaten+1\"]). \
                       When the query matches multiple WADs, set `yes: true` to apply to all \
                       without a confirmation prompt (required in MCP context — stdin is not a tty)."
    )]
    pub async fn caco_modify(
        &self,
        Parameters(args): Parameters<ModifyArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["modify".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_cache",
        description = "List, clear, or prune the WAD download cache. Mirrors `caco cache`. \
                       Pass `{\"action\": \"list\"}`, `{\"action\": \"clear\", ...}`, \
                       or `{\"action\": \"prune\", ...}`. \
                       Set `yes: true` to skip confirmation prompts (required in MCP context)."
    )]
    pub async fn caco_cache(
        &self,
        Parameters(args): Parameters<CacheArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["cache".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_stats",
        description = "Show library play statistics grouped by month or year. \
                       Mirrors `caco stats`. \
                       Omit `period` and `limit` to use CLI defaults (month, 12 periods)."
    )]
    pub async fn caco_stats(
        &self,
        Parameters(args): Parameters<StatsArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["stats".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_sessions",
        description = "Show play-session history for a WAD. Mirrors `caco sessions`. \
                       Query must identify a single WAD — use an ID like `id:N` or a \
                       sufficiently specific title. Broad queries that match multiple WADs \
                       will invoke an interactive picker that does not work reliably over MCP, \
                       even with `yes: true`."
    )]
    pub async fn caco_sessions(
        &self,
        Parameters(args): Parameters<SessionsArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["sessions".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_config",
        description = "Print the current caco config file to stdout. READ-ONLY. \
                       Mirrors `caco config` (without --edit). \
                       The `--edit` flag is deliberately not exposed because it spawns \
                       $EDITOR interactively, which is incompatible with MCP."
    )]
    pub async fn caco_config(
        &self,
        Parameters(args): Parameters<ConfigArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["config".into()];
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
            beaten: Some("2025-01".into()),
            plain: true,
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--levelstats".to_string()));
        assert!(argv.contains(&"--live".to_string()));
        assert!(argv.contains(&"--plain".to_string()));
        assert!(argv.windows(2).any(|w| w[0] == "--beaten" && w[1] == "2025-01"));
        assert!(argv.contains(&"id:7".to_string()));
    }

    #[test]
    fn random_default_renders_empty() {
        let args = RandomArgs::default();
        let argv = args.to_argv();
        assert!(argv.is_empty(), "default random args should produce empty argv, got {:?}", argv);
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

    #[test]
    fn modify_flags_render() {
        let args = ModifyArgs {
            terms: vec!["id:5".into(), "tag+hard".into()],
            notes: Some("fun wad".into()),
            date: None,
            stats_file: Some("/tmp/stats.txt".into()),
            beaten: None,
            link: None,
            add_files: vec!["brutal.deh".into(), "fix.wad".into()],
            remove_files: vec!["old.deh".into()],
            dry_run: true,
            yes: true,
        };
        let argv = args.to_argv();
        // push_opt
        assert!(argv.windows(2).any(|w| w[0] == "--notes" && w[1] == "fun wad"));
        assert!(argv.windows(2).any(|w| w[0] == "--stats-file" && w[1] == "/tmp/stats.txt"));
        // push_multi: two --add-file pairs
        let add_file_positions: Vec<_> = argv.iter().enumerate()
            .filter(|(_, v)| v.as_str() == "--add-file")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(add_file_positions.len(), 2);
        assert_eq!(argv[add_file_positions[0] + 1], "brutal.deh");
        assert_eq!(argv[add_file_positions[1] + 1], "fix.wad");
        // push_multi: one --remove-file pair
        assert!(argv.windows(2).any(|w| w[0] == "--remove-file" && w[1] == "old.deh"));
        // dry_run renders with hyphen
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(argv.contains(&"--yes".to_string()));
        // terms appended at the end
        assert!(argv.contains(&"id:5".to_string()));
        assert!(argv.contains(&"tag+hard".to_string()));
        // absent Option fields produce no flags
        assert!(!argv.contains(&"--date".to_string()));
        assert!(!argv.contains(&"--beaten".to_string()));
    }

    #[test]
    fn cache_list_default_renders() {
        let args = CacheArgs::List { plain: false, orphans: false };
        let argv = args.to_argv();
        assert_eq!(argv[0], "list");
        assert!(!argv.contains(&"--plain".to_string()));
        assert!(!argv.contains(&"--orphans".to_string()));
    }

    #[test]
    fn cache_list_flags_render() {
        let args = CacheArgs::List { plain: true, orphans: true };
        let argv = args.to_argv();
        assert_eq!(argv[0], "list");
        assert!(argv.contains(&"--plain".to_string()));
        assert!(argv.contains(&"--orphans".to_string()));
    }

    #[test]
    fn cache_clear_renders_query() {
        let args = CacheArgs::Clear {
            query: vec!["id:3".into()],
            all: false,
            dry_run: true,
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "clear");
        assert!(!argv.contains(&"--all".to_string()));
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:3".to_string()));
    }

    #[test]
    fn cache_prune_renders() {
        let args = CacheArgs::Prune { dry_run: true, yes: false };
        let argv = args.to_argv();
        assert_eq!(argv[0], "prune");
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(!argv.contains(&"--yes".to_string()));
    }

    #[test]
    fn stats_defaults_empty() {
        let args = StatsArgs::default();
        let argv = args.to_argv();
        assert!(argv.is_empty(), "default stats args should produce empty argv, got {:?}", argv);
    }

    #[test]
    fn stats_flags_render() {
        let args = StatsArgs {
            period: Some("year".into()),
            limit: Some(6),
            plain: true,
        };
        let argv = args.to_argv();
        assert!(argv.windows(2).any(|w| w[0] == "--period" && w[1] == "year"));
        assert!(argv.windows(2).any(|w| w[0] == "--limit" && w[1] == "6"));
        assert!(argv.contains(&"--plain".to_string()));
    }

    #[test]
    fn sessions_yes_renders() {
        let args = SessionsArgs {
            query: vec!["id:7".into()],
            plain: false,
            yes: true,
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--yes".to_string()));
        assert!(!argv.contains(&"--plain".to_string()));
        assert!(argv.contains(&"id:7".to_string()));
    }

    #[test]
    fn config_empty_argv() {
        assert!(ConfigArgs {}.to_argv().is_empty());
    }
}
