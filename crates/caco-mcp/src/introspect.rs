//! Direct-DB introspection tools.

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

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
