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

#[derive(Deserialize, schemars::JsonSchema)]
pub struct InspectWadArgs {
    pub id: i64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct InspectedWad {
    pub record: serde_json::Value,
    pub tags: Vec<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct InspectSessionsArgs {
    #[serde(default)]
    pub wad_id: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct InspectCompanionsArgs {
    #[serde(default)]
    pub wad_id: Option<i64>,
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
            .query_row("SELECT * FROM wads WHERE id = ?1", [args.id], row_to_json)
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

    #[tool(
        name = "inspect_sessions",
        description = "Return session log rows. Filterable by wad_id. Default limit 100, max 10000."
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
        let mut stmt = conn
            .prepare(sql)
            .map_err(CacoMcpError::from)
            .map_err(|e| e.into_mcp_error())?;
        let rows: Vec<serde_json::Value> = stmt
            .query_map(
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                row_to_json,
            )
            .map_err(CacoMcpError::from)
            .map_err(|e| e.into_mcp_error())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(Json(rows))
    }

    #[tool(
        name = "inspect_companions",
        description = "Return companion registry rows. When `wad_id` is set, joins `wad_companions` \
                       and returns only companions attached to that WAD, ordered by load_order."
    )]
    pub fn inspect_companions(
        &self,
        Parameters(args): Parameters<InspectCompanionsArgs>,
    ) -> std::result::Result<Json<Vec<serde_json::Value>>, rmcp::ErrorData> {
        let conn = open_ro(&self.paths).map_err(|e| e.into_mcp_error())?;
        let rows: Vec<serde_json::Value> = match args.wad_id {
            Some(wid) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT c.*, wc.wad_id, wc.enabled, wc.load_order \
                         FROM companion_files_registry c \
                         JOIN wad_companions wc ON wc.companion_id = c.id \
                         WHERE wc.wad_id = ?1 \
                         ORDER BY wc.load_order",
                    )
                    .map_err(CacoMcpError::from)
                    .map_err(|e| e.into_mcp_error())?;
                stmt.query_map([wid], row_to_json)
                    .map_err(CacoMcpError::from)
                    .map_err(|e| e.into_mcp_error())?
                    .filter_map(|r| r.ok())
                    .collect()
            }
            None => {
                let mut stmt = conn
                    .prepare("SELECT * FROM companion_files_registry ORDER BY id")
                    .map_err(CacoMcpError::from)
                    .map_err(|e| e.into_mcp_error())?;
                stmt.query_map([], row_to_json)
                    .map_err(CacoMcpError::from)
                    .map_err(|e| e.into_mcp_error())?
                    .filter_map(|r| r.ok())
                    .collect()
            }
        };
        Ok(Json(rows))
    }

    #[tool(
        name = "inspect_iwads",
        description = "Return all registered IWADs, ordered by family then variant. Note: \
                       priority is resolved in code against the user config, not stored in \
                       the iwads table — rows here are the raw registry."
    )]
    pub fn inspect_iwads(
        &self,
        _p: Parameters<EmptyArgs>,
    ) -> std::result::Result<Json<Vec<serde_json::Value>>, rmcp::ErrorData> {
        let conn = open_ro(&self.paths).map_err(|e| e.into_mcp_error())?;
        let mut stmt = conn
            .prepare("SELECT * FROM iwads ORDER BY family, variant")
            .map_err(CacoMcpError::from)
            .map_err(|e| e.into_mcp_error())?;
        let rows: Vec<serde_json::Value> = stmt
            .query_map([], row_to_json)
            .map_err(CacoMcpError::from)
            .map_err(|e| e.into_mcp_error())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(Json(rows))
    }

    #[tool(
        name = "inspect_id24",
        description = "Return all registered id24 WADs from the `id24_wads` table, ordered by name."
    )]
    pub fn inspect_id24(
        &self,
        _p: Parameters<EmptyArgs>,
    ) -> std::result::Result<Json<Vec<serde_json::Value>>, rmcp::ErrorData> {
        let conn = open_ro(&self.paths).map_err(|e| e.into_mcp_error())?;
        let mut stmt = conn
            .prepare("SELECT * FROM id24_wads ORDER BY name")
            .map_err(CacoMcpError::from)
            .map_err(|e| e.into_mcp_error())?;
        let rows: Vec<serde_json::Value> = stmt
            .query_map([], row_to_json)
            .map_err(CacoMcpError::from)
            .map_err(|e| e.into_mcp_error())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(Json(rows))
    }
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
