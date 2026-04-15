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

// ---------- caco_saves ----------

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum SavesArgs {
    /// List save files for a WAD.
    List {
        #[serde(default)]
        query: Vec<String>,
        #[serde(default)]
        plain: bool,
        #[serde(default)]
        yes: bool,
    },
    /// Backup the WAD data directory.
    Backup {
        #[serde(default)]
        query: Vec<String>,
        #[serde(default)]
        yes: bool,
    },
    /// Restore from a backup.
    Restore {
        #[serde(default)]
        query: Vec<String>,
        /// Specific backup filename (latest if omitted).
        #[serde(default)]
        backup: Option<String>,
        #[serde(default)]
        yes: bool,
    },
    /// Delete save files only.
    Clean {
        #[serde(default)]
        query: Vec<String>,
        #[serde(default)]
        dry_run: bool,
        #[serde(default)]
        yes: bool,
    },
    /// List existing backups.
    Backups {
        #[serde(default)]
        query: Vec<String>,
        #[serde(default)]
        plain: bool,
        #[serde(default)]
        yes: bool,
    },
}

impl SavesArgs {
    fn to_argv(&self) -> Vec<String> {
        match self {
            SavesArgs::List { query, plain, yes } => {
                let mut argv = vec!["list".into()];
                push_flag(&mut argv, "--plain", *plain);
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
            SavesArgs::Backup { query, yes } => {
                let mut argv = vec!["backup".into()];
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
            SavesArgs::Restore { query, backup, yes } => {
                let mut argv = vec!["restore".into()];
                push_opt(&mut argv, "--backup", backup.as_ref());
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
            SavesArgs::Clean { query, dry_run, yes } => {
                let mut argv = vec!["clean".into()];
                push_flag(&mut argv, "--dry-run", *dry_run);
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
            SavesArgs::Backups { query, plain, yes } => {
                let mut argv = vec!["backups".into()];
                push_flag(&mut argv, "--plain", *plain);
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
        }
    }
}

// ---------- caco_demos ----------

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum DemosArgs {
    /// List demo files for a WAD.
    List {
        #[serde(default)]
        query: Vec<String>,
        #[serde(default)]
        plain: bool,
        #[serde(default)]
        yes: bool,
    },
    /// Play back a demo. Spawns a sourceport child process; may block while the
    /// player is open. Stdin is null in MCP context.
    Play {
        #[serde(default)]
        query: Vec<String>,
        /// Specific demo filename (most recent if omitted).
        #[serde(default)]
        demo: Option<String>,
        /// Sourceport to use.
        #[serde(default)]
        sourceport: Option<String>,
        #[serde(default)]
        yes: bool,
    },
    /// Delete demo files.
    Clean {
        #[serde(default)]
        query: Vec<String>,
        #[serde(default)]
        dry_run: bool,
        #[serde(default)]
        yes: bool,
    },
}

impl DemosArgs {
    fn to_argv(&self) -> Vec<String> {
        match self {
            DemosArgs::List { query, plain, yes } => {
                let mut argv = vec!["list".into()];
                push_flag(&mut argv, "--plain", *plain);
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
            DemosArgs::Play { query, demo, sourceport, yes } => {
                let mut argv = vec!["play".into()];
                push_opt(&mut argv, "--demo", demo.as_ref());
                push_opt(&mut argv, "--sourceport", sourceport.as_ref());
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
            DemosArgs::Clean { query, dry_run, yes } => {
                let mut argv = vec!["clean".into()];
                push_flag(&mut argv, "--dry-run", *dry_run);
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
        }
    }
}

// ---------- caco_profile ----------

/// Profile actions exposed via MCP. The CLI's `edit` action is intentionally
/// omitted because it spawns $EDITOR interactively, which is incompatible with
/// MCP. Use `path` to locate the file and edit it out-of-band if needed.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum ProfileArgs {
    /// List config profiles.
    Ls {
        /// Filter by sourceport.
        #[serde(default)]
        sourceport: Option<String>,
    },
    /// Create a new profile.
    Create {
        /// Profile name.
        name: String,
        #[serde(default)]
        sourceport: Option<String>,
        /// Copy from existing profile.
        #[serde(default)]
        from: Option<String>,
    },
    /// Copy a profile.
    Cp {
        /// Source profile name.
        source: String,
        /// Destination profile name.
        dest: String,
        #[serde(default)]
        sourceport: Option<String>,
    },
    /// Delete a profile.
    Rm {
        name: String,
        #[serde(default)]
        sourceport: Option<String>,
        #[serde(default)]
        yes: bool,
    },
    /// Print absolute path to profile file.
    Path {
        name: String,
        #[serde(default)]
        sourceport: Option<String>,
    },
}

impl ProfileArgs {
    fn to_argv(&self) -> Vec<String> {
        match self {
            ProfileArgs::Ls { sourceport } => {
                let mut argv = vec!["ls".into()];
                push_opt(&mut argv, "--sourceport", sourceport.as_ref());
                argv
            }
            ProfileArgs::Create { name, sourceport, from } => {
                let mut argv = vec!["create".into()];
                push_opt(&mut argv, "--sourceport", sourceport.as_ref());
                push_opt(&mut argv, "--from", from.as_ref());
                argv.push(name.clone());
                argv
            }
            ProfileArgs::Cp { source, dest, sourceport } => {
                let mut argv = vec!["cp".into()];
                push_opt(&mut argv, "--sourceport", sourceport.as_ref());
                argv.push(source.clone());
                argv.push(dest.clone());
                argv
            }
            ProfileArgs::Rm { name, sourceport, yes } => {
                let mut argv = vec!["rm".into()];
                push_opt(&mut argv, "--sourceport", sourceport.as_ref());
                push_flag(&mut argv, "--yes", *yes);
                argv.push(name.clone());
                argv
            }
            ProfileArgs::Path { name, sourceport } => {
                let mut argv = vec!["path".into()];
                push_opt(&mut argv, "--sourceport", sourceport.as_ref());
                argv.push(name.clone());
                argv
            }
        }
    }
}

// ---------- caco_companion ----------

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum CompanionArgs {
    /// Register a companion file and link to a WAD.
    Add {
        #[serde(default)]
        query: Vec<String>,
        /// Path to companion file.
        file: String,
    },
    /// Unlink a companion file from a WAD.
    Rm {
        #[serde(default)]
        query: Vec<String>,
        /// Companion filename to remove.
        file: String,
        #[serde(default)]
        yes: bool,
    },
    /// Enable a disabled companion file.
    Enable {
        #[serde(default)]
        query: Vec<String>,
        file: String,
    },
    /// Disable a companion file without removing.
    Disable {
        #[serde(default)]
        query: Vec<String>,
        file: String,
    },
    /// List companion files.
    Ls {
        #[serde(default)]
        query: Vec<String>,
        #[serde(default)]
        plain: bool,
    },
}

impl CompanionArgs {
    fn to_argv(&self) -> Vec<String> {
        match self {
            CompanionArgs::Add { query, file } => {
                let mut argv = vec!["add".into()];
                argv.push("--file".into());
                argv.push(file.clone());
                argv.extend(query.clone());
                argv
            }
            CompanionArgs::Rm { query, file, yes } => {
                let mut argv = vec!["rm".into()];
                argv.push("--file".into());
                argv.push(file.clone());
                push_flag(&mut argv, "--yes", *yes);
                argv.extend(query.clone());
                argv
            }
            CompanionArgs::Enable { query, file } => {
                let mut argv = vec!["enable".into()];
                argv.push("--file".into());
                argv.push(file.clone());
                argv.extend(query.clone());
                argv
            }
            CompanionArgs::Disable { query, file } => {
                let mut argv = vec!["disable".into()];
                argv.push("--file".into());
                argv.push(file.clone());
                argv.extend(query.clone());
                argv
            }
            CompanionArgs::Ls { query, plain } => {
                let mut argv = vec!["ls".into()];
                push_flag(&mut argv, "--plain", *plain);
                argv.extend(query.clone());
                argv
            }
        }
    }
}

// ---------- caco_collection ----------

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum CollectionArgs {
    /// Create a new smart collection.
    Add {
        /// Collection name.
        name: String,
        /// Query terms (same syntax as `caco ls`).
        #[serde(default)]
        query: Vec<String>,
        /// Sort field.
        #[serde(default)]
        sort: Option<String>,
        /// Sort descending.
        #[serde(default)]
        desc: bool,
    },
    /// Delete a smart collection.
    Rm {
        name: String,
    },
    /// List all smart collections.
    Ls {
        /// Output format: "table", "plain", or "json". Defaults to "json"
        /// MCP-side so `parsed_json` is populated.
        #[serde(default)]
        output: Option<String>,
    },
    /// Run a collection's query and show results.
    Run {
        /// Collection name.
        name: String,
        /// Output format: "table", "plain", or "json". Defaults to "json"
        /// MCP-side so `parsed_json` is populated.
        #[serde(default)]
        output: Option<String>,
    },
}

impl CollectionArgs {
    fn to_argv(&self) -> Vec<String> {
        match self {
            CollectionArgs::Add { name, query, sort, desc } => {
                let mut argv = vec!["add".into()];
                push_opt(&mut argv, "--sort", sort.as_ref());
                push_flag(&mut argv, "--desc", *desc);
                argv.push(name.clone());
                argv.extend(query.clone());
                argv
            }
            CollectionArgs::Rm { name } => {
                vec!["rm".into(), name.clone()]
            }
            CollectionArgs::Ls { output } => {
                let out = output.clone().unwrap_or_else(|| "json".into());
                vec!["ls".into(), "--output".into(), out]
            }
            CollectionArgs::Run { name, output } => {
                let out = output.clone().unwrap_or_else(|| "json".into());
                vec!["run".into(), "--output".into(), out, name.clone()]
            }
        }
    }
}

// ---------- caco_import ----------

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum ImportSource {
    /// idgames archive. `query` may be a numeric ID (direct fetch, no picker),
    /// a text query (invokes interactive picker — won't work over MCP), or a
    /// path to a saved JSON response (picker still required).
    Idgames { query: String },
    /// Doom Wiki search. Always invokes the interactive picker — won't work
    /// over MCP. A `.json` path also routes to JSON import (still picker).
    Doomwiki { query: String },
    /// Doomworld forum thread URL — direct fetch, no picker.
    Doomworld { url: String },
    /// Direct URL import. The URL is also used as the title fallback when
    /// `title` is not set.
    Url { url: String },
    /// Direct local file import — no picker.
    Local { path: String },
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ImportArgs {
    /// Source-discriminated input (idgames/doomwiki/doomworld/url/local).
    pub source: ImportSource,
    /// Title override (-t / --title).
    #[serde(default)]
    pub title: Option<String>,
    /// Author override (-a / --author).
    #[serde(default)]
    pub author: Option<String>,
    /// Year override.
    #[serde(default)]
    pub year: Option<i32>,
    /// Tags to attach (repeatable).
    #[serde(default)]
    pub tag: Vec<String>,
    /// Description (used by --url imports).
    #[serde(default)]
    pub description: Option<String>,
    /// Force import even if duplicate (-f / --force).
    #[serde(default)]
    pub force: bool,
    /// Multi-select from search results — REQUIRES fzf, non-functional over MCP.
    #[serde(default)]
    pub multi: bool,
    /// Force LLM-powered metadata extraction (Doomworld only).
    #[serde(default)]
    pub smart: bool,
    /// Disable auto-LLM extraction (when `[llm]` is configured).
    #[serde(default)]
    pub no_smart: bool,
    /// LLM backend override.
    #[serde(default)]
    pub llm_backend: Option<String>,
    /// LLM model override.
    #[serde(default)]
    pub llm_model: Option<String>,
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
                // Render URL twice: once as the --url flag value (which selects
                // URL import mode), and once as the positional source (which
                // becomes the title fallback when --title is not set).
                argv.push("--url".into());
                argv.push(url.clone());
                argv.push(url.clone());
            }
            ImportSource::Local { path } => {
                argv.push("--local".into());
                argv.push(path.clone());
            }
        }
        push_opt(&mut argv, "--title", self.title.as_ref());
        push_opt(&mut argv, "--author", self.author.as_ref());
        push_opt(&mut argv, "--year", self.year.as_ref());
        push_multi(&mut argv, "--tag", &self.tag);
        push_opt(&mut argv, "--description", self.description.as_ref());
        push_flag(&mut argv, "--force", self.force);
        push_flag(&mut argv, "--multi", self.multi);
        push_flag(&mut argv, "--smart", self.smart);
        push_flag(&mut argv, "--no-smart", self.no_smart);
        push_opt(&mut argv, "--llm-backend", self.llm_backend.as_ref());
        push_opt(&mut argv, "--llm-model", self.llm_model.as_ref());
        argv
    }
}

// ---------- caco_gc ----------

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct GcArgs {
    /// Preview cleanup without deleting.
    #[serde(default)]
    pub dry_run: bool,
    /// Preserve save files (.dsg/.zds) in data dirs.
    #[serde(default)]
    pub keep_saves: bool,
    /// Preserve demo files (.lmp) in data dirs.
    #[serde(default)]
    pub keep_demos: bool,
    /// Skip data directory cleanup entirely.
    #[serde(default)]
    pub keep_data: bool,
    /// Skip cache file cleanup entirely.
    #[serde(default)]
    pub keep_cache: bool,
    /// Skip companion file cleanup.
    #[serde(default)]
    pub keep_companions: bool,
    /// Only clean orphaned data dirs, backups, and companion files.
    #[serde(default)]
    pub orphans_only: bool,
    /// Query terms to mark WAD(s) as GC-ignored. Repeatable; each entry
    /// becomes a separate `--ignore <value>` pair.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Query terms to remove GC-ignore from WAD(s). Repeatable; each entry
    /// becomes a separate `--unignore <value>` pair.
    #[serde(default)]
    pub unignore: Vec<String>,
}

impl GcArgs {
    fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        // Always inject -y: gc has interactive y/n/i prompts that we must
        // force non-interactive. There is no MCP equivalent of the interactive
        // `i` (ignore-this-WAD) response — use the `ignore` parameter instead.
        argv.push("-y".into());
        push_flag(&mut argv, "--dry-run", self.dry_run);
        push_flag(&mut argv, "--keep-saves", self.keep_saves);
        push_flag(&mut argv, "--keep-demos", self.keep_demos);
        push_flag(&mut argv, "--keep-data", self.keep_data);
        push_flag(&mut argv, "--keep-cache", self.keep_cache);
        push_flag(&mut argv, "--keep-companions", self.keep_companions);
        push_flag(&mut argv, "--orphans-only", self.orphans_only);
        push_multi(&mut argv, "--ignore", &self.ignore);
        push_multi(&mut argv, "--unignore", &self.unignore);
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

    #[tool(
        name = "caco_saves",
        description = "List, backup, restore, or clean WAD save files. Mirrors `caco saves`. \
                       Pass `{\"action\": \"list\"|\"backup\"|\"restore\"|\"clean\"|\"backups\", ...}`. \
                       Set `yes: true` to auto-select the first matching WAD; otherwise the \
                       underlying CLI uses an interactive picker that does not work over MCP. \
                       For `clean` and `restore`, `yes` also skips the destructive-confirmation prompt. \
                       Output is plain text (or TSV with `plain: true`); `parsed_json` is null."
    )]
    pub async fn caco_saves(
        &self,
        Parameters(args): Parameters<SavesArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["saves".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_demos",
        description = "List, play back, or clean WAD demo files. Mirrors `caco demos`. \
                       Pass `{\"action\": \"list\"|\"play\"|\"clean\", ...}`. \
                       The `play` action spawns a sourceport child process and may block \
                       while the player window is open. Set `yes: true` to auto-select the \
                       first matching WAD; otherwise the CLI uses an interactive picker. \
                       Output is plain text (or TSV with `plain: true`); `parsed_json` is null."
    )]
    pub async fn caco_demos(
        &self,
        Parameters(args): Parameters<DemosArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["demos".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_profile",
        description = "Manage sourceport config profiles. Mirrors `caco profile`. \
                       Pass `{\"action\": \"ls\"|\"create\"|\"cp\"|\"rm\"|\"path\", ...}`. \
                       The `edit` action is intentionally not exposed; it spawns $EDITOR \
                       interactively, incompatible with MCP. Use `path` to locate the file \
                       and edit it out-of-band if needed. \
                       Output is plain text; `parsed_json` is null."
    )]
    pub async fn caco_profile(
        &self,
        Parameters(args): Parameters<ProfileArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["profile".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_companion",
        description = "Manage companion files (DEH/BEX/WAD attachments) for WADs. Mirrors \
                       `caco companion`. Pass `{\"action\": \"add\"|\"rm\"|\"enable\"|\
                       \"disable\"|\"ls\", ...}`. \
                       All variants resolve a single WAD via the CLI's interactive picker \
                       (`ResolveMode::Pick`), which IGNORES `--yes` and does not work reliably \
                       over MCP. (`ls` skips the picker only when called with an empty query.) \
                       Use a precise query like `id:N` or a sufficiently specific title to avoid \
                       the picker. \
                       Output is plain text; `parsed_json` is null."
    )]
    pub async fn caco_companion(
        &self,
        Parameters(args): Parameters<CompanionArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["companion".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_collection",
        description = "Manage smart collections (saved queries). Mirrors `caco collection`. \
                       Pass `{\"action\": \"add\"|\"rm\"|\"ls\"|\"run\", ...}`. \
                       `ls` and `run` default to JSON output MCP-side so `parsed_json` is \
                       populated; override via `output: \"table\"` or `\"plain\"`."
    )]
    pub async fn caco_collection(
        &self,
        Parameters(args): Parameters<CollectionArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["collection".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_import",
        description = "Import a WAD from idgames, Doom Wiki, Doomworld, a URL, or a local \
                       file. Mirrors `caco import`. Pass `source` as a discriminated object: \
                       `{\"source\": \"idgames\", \"query\": \"18184\"}`, \
                       `{\"source\": \"doomwiki\", \"query\": \"Sunder\"}`, \
                       `{\"source\": \"doomworld\", \"url\": \"...\"}`, \
                       `{\"source\": \"url\", \"url\": \"...\"}`, or \
                       `{\"source\": \"local\", \"path\": \"...\"}`. \
                       MCP LIMITATIONS: `idgames` with a non-numeric query (text search) \
                       invokes an interactive picker and will not work over MCP — use a \
                       numeric idgames ID for direct fetch. `doomwiki` ALWAYS invokes the \
                       picker and will not work over MCP. `idgames`/`doomwiki` with a `.json` \
                       file path also invokes the picker. `doomworld`, `url`, `local`, and \
                       `idgames` with a numeric ID all work directly. `multi: true` requires \
                       fzf and is non-functional over MCP."
    )]
    pub async fn caco_import(
        &self,
        Parameters(args): Parameters<ImportArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["import".into()];
        argv.extend(args.to_argv());
        let runner = CliRunner { bin: &self.caco_bin, paths: &self.paths };
        let result = runner.run(argv).await.map_err(|e| e.into_mcp_error())?;
        Ok(Json(result))
    }

    #[tool(
        name = "caco_gc",
        description = "Garbage-collect data for completed/abandoned WADs. Mirrors `caco gc`. \
                       The CLI's interactive y/n/i prompts are forced non-interactive by \
                       always injecting `-y`. There is no MCP equivalent of the interactive \
                       `i` (ignore-this-WAD) response — use the `ignore` parameter ahead of \
                       time to mark WADs as GC-ignored \
                       (e.g. `ignore: [\"status:abandoned\"]` or `ignore: [\"id:5\", \"id:7\"]`). \
                       `ignore` and `unignore` are repeatable: multiple values produce \
                       multiple `--ignore` / `--unignore` flag pairs. Use `dry_run: true` to \
                       preview without deleting."
    )]
    pub async fn caco_gc(
        &self,
        Parameters(args): Parameters<GcArgs>,
    ) -> Result<Json<CliResult>, rmcp::ErrorData> {
        let mut argv = vec!["gc".into()];
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

    // ---------- caco_saves tests ----------

    #[test]
    fn saves_list_default_renders() {
        let args = SavesArgs::List { query: vec![], plain: false, yes: false };
        let argv = args.to_argv();
        assert_eq!(argv[0], "list");
        assert!(!argv.contains(&"--plain".to_string()));
        assert!(!argv.contains(&"--yes".to_string()));
    }

    #[test]
    fn saves_list_flags_render() {
        let args = SavesArgs::List {
            query: vec!["id:5".into()],
            plain: true,
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "list");
        assert!(argv.contains(&"--plain".to_string()));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:5".to_string()));
    }

    #[test]
    fn saves_backup_renders() {
        let args = SavesArgs::Backup {
            query: vec!["id:7".into()],
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "backup");
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:7".to_string()));
    }

    #[test]
    fn saves_restore_renders() {
        let args = SavesArgs::Restore {
            query: vec!["id:9".into()],
            backup: Some("backup_2025.tar.gz".into()),
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "restore");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--backup" && w[1] == "backup_2025.tar.gz"));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:9".to_string()));
    }

    #[test]
    fn saves_restore_omits_absent_backup() {
        let args = SavesArgs::Restore {
            query: vec!["id:9".into()],
            backup: None,
            yes: false,
        };
        let argv = args.to_argv();
        assert!(!argv.contains(&"--backup".to_string()));
    }

    #[test]
    fn saves_clean_renders() {
        let args = SavesArgs::Clean {
            query: vec!["id:1".into()],
            dry_run: true,
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "clean");
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:1".to_string()));
    }

    #[test]
    fn saves_backups_renders() {
        let args = SavesArgs::Backups {
            query: vec!["id:2".into()],
            plain: true,
            yes: false,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "backups");
        assert!(argv.contains(&"--plain".to_string()));
        assert!(!argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:2".to_string()));
    }

    // ---------- caco_demos tests ----------

    #[test]
    fn demos_list_renders() {
        let args = DemosArgs::List {
            query: vec!["id:3".into()],
            plain: true,
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "list");
        assert!(argv.contains(&"--plain".to_string()));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:3".to_string()));
    }

    #[test]
    fn demos_play_renders() {
        let args = DemosArgs::Play {
            query: vec!["id:4".into()],
            demo: Some("run1.lmp".into()),
            sourceport: Some("dsda-doom".into()),
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "play");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--demo" && w[1] == "run1.lmp"));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--sourceport" && w[1] == "dsda-doom"));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:4".to_string()));
    }

    #[test]
    fn demos_play_omits_absent_options() {
        let args = DemosArgs::Play {
            query: vec!["id:4".into()],
            demo: None,
            sourceport: None,
            yes: false,
        };
        let argv = args.to_argv();
        assert!(!argv.contains(&"--demo".to_string()));
        assert!(!argv.contains(&"--sourceport".to_string()));
        assert!(!argv.contains(&"--yes".to_string()));
    }

    #[test]
    fn demos_clean_renders() {
        let args = DemosArgs::Clean {
            query: vec!["id:5".into()],
            dry_run: true,
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "clean");
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:5".to_string()));
    }

    // ---------- caco_profile tests ----------

    #[test]
    fn profile_ls_default_renders() {
        let args = ProfileArgs::Ls { sourceport: None };
        let argv = args.to_argv();
        assert_eq!(argv[0], "ls");
        assert!(!argv.contains(&"--sourceport".to_string()));
    }

    #[test]
    fn profile_ls_with_sourceport() {
        let args = ProfileArgs::Ls {
            sourceport: Some("dsda-doom".into()),
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "ls");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--sourceport" && w[1] == "dsda-doom"));
    }

    #[test]
    fn profile_create_renders() {
        let args = ProfileArgs::Create {
            name: "speedrun".into(),
            sourceport: Some("dsda-doom".into()),
            from: Some("default".into()),
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "create");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--sourceport" && w[1] == "dsda-doom"));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--from" && w[1] == "default"));
        // positional name appears AFTER the flags
        assert_eq!(argv.last().unwrap(), "speedrun");
    }

    #[test]
    fn profile_create_omits_absent_options() {
        let args = ProfileArgs::Create {
            name: "fresh".into(),
            sourceport: None,
            from: None,
        };
        let argv = args.to_argv();
        assert!(!argv.contains(&"--sourceport".to_string()));
        assert!(!argv.contains(&"--from".to_string()));
        assert_eq!(argv.last().unwrap(), "fresh");
    }

    #[test]
    fn profile_cp_renders() {
        let args = ProfileArgs::Cp {
            source: "src".into(),
            dest: "dst".into(),
            sourceport: Some("zdoom".into()),
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "cp");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--sourceport" && w[1] == "zdoom"));
        // src and dst preserved in order at the end
        let tail: Vec<&str> = argv.iter().rev().take(2).map(String::as_str).collect();
        assert_eq!(tail, vec!["dst", "src"]);
    }

    #[test]
    fn profile_rm_renders() {
        let args = ProfileArgs::Rm {
            name: "old".into(),
            sourceport: Some("woof".into()),
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "rm");
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--sourceport" && w[1] == "woof"));
        assert_eq!(argv.last().unwrap(), "old");
    }

    #[test]
    fn profile_path_renders() {
        let args = ProfileArgs::Path {
            name: "default".into(),
            sourceport: None,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "path");
        assert!(!argv.contains(&"--sourceport".to_string()));
        assert_eq!(argv.last().unwrap(), "default");
    }

    // Verify the Edit variant does not exist on ProfileArgs by attempting a
    // deserialization of `{"action": "edit", ...}`. Should fail with an
    // unknown-variant error.
    #[test]
    fn profile_edit_variant_is_excluded() {
        let json = r#"{"action": "edit", "name": "foo"}"#;
        let result: Result<ProfileArgs, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "ProfileArgs must NOT accept an 'edit' action"
        );
    }

    // ---------- caco_companion tests ----------

    #[test]
    fn companion_add_renders() {
        let args = CompanionArgs::Add {
            query: vec!["id:1".into()],
            file: "/tmp/brutal.deh".into(),
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "add");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--file" && w[1] == "/tmp/brutal.deh"));
        assert!(argv.contains(&"id:1".to_string()));
    }

    #[test]
    fn companion_rm_renders() {
        let args = CompanionArgs::Rm {
            query: vec!["id:2".into()],
            file: "old.deh".into(),
            yes: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "rm");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--file" && w[1] == "old.deh"));
        assert!(argv.contains(&"--yes".to_string()));
        assert!(argv.contains(&"id:2".to_string()));
    }

    #[test]
    fn companion_enable_renders() {
        let args = CompanionArgs::Enable {
            query: vec!["id:3".into()],
            file: "fix.wad".into(),
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "enable");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--file" && w[1] == "fix.wad"));
        assert!(argv.contains(&"id:3".to_string()));
    }

    #[test]
    fn companion_disable_renders() {
        let args = CompanionArgs::Disable {
            query: vec!["id:4".into()],
            file: "fix.wad".into(),
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "disable");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--file" && w[1] == "fix.wad"));
        assert!(argv.contains(&"id:4".to_string()));
    }

    #[test]
    fn companion_ls_renders() {
        let args = CompanionArgs::Ls {
            query: vec!["id:5".into()],
            plain: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "ls");
        assert!(argv.contains(&"--plain".to_string()));
        assert!(argv.contains(&"id:5".to_string()));
    }

    #[test]
    fn companion_ls_default_no_plain() {
        let args = CompanionArgs::Ls { query: vec![], plain: false };
        let argv = args.to_argv();
        assert_eq!(argv[0], "ls");
        assert!(!argv.contains(&"--plain".to_string()));
    }

    // ---------- caco_collection tests ----------

    #[test]
    fn collection_add_renders() {
        let args = CollectionArgs::Add {
            name: "favs".into(),
            query: vec!["status:completed".into(), "tag:fun".into()],
            sort: Some("playtime".into()),
            desc: true,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "add");
        // --sort and --desc render BEFORE the positional name
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--sort" && w[1] == "playtime"));
        assert!(argv.contains(&"--desc".to_string()));
        // name precedes query terms
        let name_pos = argv.iter().position(|s| s == "favs").unwrap();
        let q1_pos = argv.iter().position(|s| s == "status:completed").unwrap();
        let q2_pos = argv.iter().position(|s| s == "tag:fun").unwrap();
        assert!(name_pos < q1_pos);
        assert!(q1_pos < q2_pos);
    }

    #[test]
    fn collection_add_omits_absent_options() {
        let args = CollectionArgs::Add {
            name: "minimal".into(),
            query: vec![],
            sort: None,
            desc: false,
        };
        let argv = args.to_argv();
        assert_eq!(argv, vec!["add", "minimal"]);
    }

    #[test]
    fn collection_rm_renders() {
        let args = CollectionArgs::Rm { name: "favs".into() };
        let argv = args.to_argv();
        assert_eq!(argv, vec!["rm", "favs"]);
    }

    #[test]
    fn collection_ls_default_uses_json_output() {
        let args = CollectionArgs::Ls { output: None };
        let argv = args.to_argv();
        assert_eq!(argv[0], "ls");
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "json"));
    }

    #[test]
    fn collection_ls_custom_output() {
        let args = CollectionArgs::Ls {
            output: Some("plain".into()),
        };
        let argv = args.to_argv();
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "plain"));
    }

    #[test]
    fn collection_run_default_uses_json_output() {
        let args = CollectionArgs::Run {
            name: "favs".into(),
            output: None,
        };
        let argv = args.to_argv();
        assert_eq!(argv[0], "run");
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "json"));
        // positional name at the end
        assert_eq!(argv.last().unwrap(), "favs");
    }

    #[test]
    fn collection_run_custom_output() {
        let args = CollectionArgs::Run {
            name: "favs".into(),
            output: Some("table".into()),
        };
        let argv = args.to_argv();
        assert!(argv.windows(2).any(|w| w[0] == "--output" && w[1] == "table"));
        assert_eq!(argv.last().unwrap(), "favs");
    }

    // ---------- caco_import tests ----------

    fn import_args(source: ImportSource) -> ImportArgs {
        ImportArgs {
            source,
            title: None,
            author: None,
            year: None,
            tag: vec![],
            description: None,
            force: false,
            multi: false,
            smart: false,
            no_smart: false,
            llm_backend: None,
            llm_model: None,
        }
    }

    #[test]
    fn import_idgames_renders() {
        let args = import_args(ImportSource::Idgames {
            query: "18184".into(),
        });
        let argv = args.to_argv();
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--idgames" && w[1] == "18184"));
        // No other source flags
        assert!(!argv.contains(&"--doomwiki".to_string()));
        assert!(!argv.contains(&"--doomworld".to_string()));
        assert!(!argv.contains(&"--url".to_string()));
        assert!(!argv.contains(&"--local".to_string()));
        // Default ImportArgs should not render any metadata flags.
        assert!(!argv.contains(&"--force".to_string()));
        assert!(!argv.contains(&"--multi".to_string()));
        assert!(!argv.contains(&"--smart".to_string()));
        assert!(!argv.contains(&"--no-smart".to_string()));
        assert!(!argv.contains(&"--title".to_string()));
        assert!(!argv.contains(&"--author".to_string()));
    }

    #[test]
    fn import_doomwiki_renders() {
        let args = import_args(ImportSource::Doomwiki {
            query: "Sunder".into(),
        });
        let argv = args.to_argv();
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--doomwiki" && w[1] == "Sunder"));
        assert!(!argv.contains(&"--idgames".to_string()));
    }

    #[test]
    fn import_doomworld_renders() {
        let args = import_args(ImportSource::Doomworld {
            url: "https://www.doomworld.com/forum/topic/12345/".into(),
        });
        let argv = args.to_argv();
        assert!(argv.windows(2).any(|w| {
            w[0] == "--doomworld"
                && w[1] == "https://www.doomworld.com/forum/topic/12345/"
        }));
    }

    #[test]
    fn import_url_renders_url_twice() {
        let url = "https://example.com/cool.zip";
        let args = import_args(ImportSource::Url { url: url.into() });
        let argv = args.to_argv();
        // --url <url> is present
        assert!(argv.windows(2).any(|w| w[0] == "--url" && w[1] == url));
        // URL also appears as the positional source for title fallback —
        // that means the URL string appears at least TWICE in the argv.
        let url_count = argv.iter().filter(|s| s.as_str() == url).count();
        assert_eq!(
            url_count, 2,
            "URL must render twice (once as --url value, once as positional), got {url_count}: {argv:?}"
        );
    }

    #[test]
    fn import_local_renders() {
        let args = import_args(ImportSource::Local {
            path: "/tmp/cool.wad".into(),
        });
        let argv = args.to_argv();
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--local" && w[1] == "/tmp/cool.wad"));
    }

    #[test]
    fn import_metadata_flags_render() {
        let args = ImportArgs {
            source: ImportSource::Idgames {
                query: "18184".into(),
            },
            title: Some("Cool WAD".into()),
            author: Some("DoomGuy".into()),
            year: Some(1994),
            tag: vec!["fun".into(), "hard".into()],
            description: Some("A great wad".into()),
            force: true,
            multi: true,
            smart: true,
            no_smart: false,
            llm_backend: Some("anthropic".into()),
            llm_model: Some("claude-3-5".into()),
        };
        let argv = args.to_argv();
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--title" && w[1] == "Cool WAD"));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--author" && w[1] == "DoomGuy"));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--year" && w[1] == "1994"));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--description" && w[1] == "A great wad"));
        assert!(argv.contains(&"--force".to_string()));
        assert!(argv.contains(&"--multi".to_string()));
        assert!(argv.contains(&"--smart".to_string()));
        assert!(!argv.contains(&"--no-smart".to_string()));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--llm-backend" && w[1] == "anthropic"));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--llm-model" && w[1] == "claude-3-5"));
        // Two --tag pairs in argv
        let tag_positions: Vec<_> = argv
            .iter()
            .enumerate()
            .filter(|(_, v)| v.as_str() == "--tag")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(tag_positions.len(), 2);
        assert_eq!(argv[tag_positions[0] + 1], "fun");
        assert_eq!(argv[tag_positions[1] + 1], "hard");
    }

    #[test]
    fn import_metadata_flags_default_omit() {
        let args = import_args(ImportSource::Local {
            path: "/tmp/cool.wad".into(),
        });
        let argv = args.to_argv();
        // None of the optional metadata flags should be in argv.
        for flag in [
            "--title",
            "--author",
            "--year",
            "--description",
            "--force",
            "--multi",
            "--smart",
            "--no-smart",
            "--llm-backend",
            "--llm-model",
            "--tag",
        ] {
            assert!(
                !argv.contains(&flag.to_string()),
                "default ImportArgs should not render {flag}, got {argv:?}"
            );
        }
    }

    #[test]
    fn import_no_smart_renders() {
        let args = ImportArgs {
            no_smart: true,
            ..import_args(ImportSource::Doomworld {
                url: "https://www.doomworld.com/forum/topic/1/".into(),
            })
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--no-smart".to_string()));
        assert!(!argv.contains(&"--smart".to_string()));
    }

    // ---------- caco_gc tests ----------

    #[test]
    fn gc_default_injects_yes() {
        let args = GcArgs::default();
        let argv = args.to_argv();
        assert!(
            argv.contains(&"-y".to_string()),
            "gc must always inject -y to force non-interactive, got {argv:?}"
        );
        // No other flags set by default.
        assert!(!argv.contains(&"--dry-run".to_string()));
        assert!(!argv.contains(&"--keep-saves".to_string()));
        assert!(!argv.contains(&"--keep-demos".to_string()));
        assert!(!argv.contains(&"--keep-data".to_string()));
        assert!(!argv.contains(&"--keep-cache".to_string()));
        assert!(!argv.contains(&"--keep-companions".to_string()));
        assert!(!argv.contains(&"--orphans-only".to_string()));
        assert!(!argv.contains(&"--ignore".to_string()));
        assert!(!argv.contains(&"--unignore".to_string()));
    }

    #[test]
    fn gc_keep_flags_render() {
        let args = GcArgs {
            keep_saves: true,
            keep_demos: true,
            keep_data: true,
            keep_cache: true,
            keep_companions: true,
            ..GcArgs::default()
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"-y".to_string()));
        assert!(argv.contains(&"--keep-saves".to_string()));
        assert!(argv.contains(&"--keep-demos".to_string()));
        assert!(argv.contains(&"--keep-data".to_string()));
        assert!(argv.contains(&"--keep-cache".to_string()));
        assert!(argv.contains(&"--keep-companions".to_string()));
    }

    #[test]
    fn gc_ignore_renders_multi() {
        let args = GcArgs {
            ignore: vec!["status:abandoned".into(), "tag:fun".into()],
            ..GcArgs::default()
        };
        let argv = args.to_argv();
        let positions: Vec<_> = argv
            .iter()
            .enumerate()
            .filter(|(_, v)| v.as_str() == "--ignore")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            positions.len(),
            2,
            "ignore must produce two --ignore flag pairs, got {argv:?}"
        );
        assert_eq!(argv[positions[0] + 1], "status:abandoned");
        assert_eq!(argv[positions[1] + 1], "tag:fun");
    }

    #[test]
    fn gc_unignore_renders_multi() {
        let args = GcArgs {
            unignore: vec!["id:5".into(), "id:7".into()],
            ..GcArgs::default()
        };
        let argv = args.to_argv();
        let positions: Vec<_> = argv
            .iter()
            .enumerate()
            .filter(|(_, v)| v.as_str() == "--unignore")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 2);
        assert_eq!(argv[positions[0] + 1], "id:5");
        assert_eq!(argv[positions[1] + 1], "id:7");
    }

    #[test]
    fn gc_orphans_only_renders() {
        let args = GcArgs {
            orphans_only: true,
            ..GcArgs::default()
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--orphans-only".to_string()));
        assert!(argv.contains(&"-y".to_string()));
    }

    #[test]
    fn gc_dry_run_renders() {
        let args = GcArgs {
            dry_run: true,
            ..GcArgs::default()
        };
        let argv = args.to_argv();
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(argv.contains(&"-y".to_string()));
    }
}
