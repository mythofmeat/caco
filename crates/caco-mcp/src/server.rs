//! Top-level MCP server for caco.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool_handler};

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
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
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
