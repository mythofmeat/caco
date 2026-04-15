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
    pub record: serde_json::Map<String, serde_json::Value>,
    pub tags: Vec<String>,
}

/// Wrapper returned by `inspect_*` tools whose payload is a list of DB rows.
/// MCP clients (Claude Code's Zod validator in particular) reject tool
/// output schemas whose root isn't `type: "object"`, so every list-returning
/// tool packages its rows here.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InspectRows {
    pub rows: Vec<serde_json::Value>,
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

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RunSqlArgs {
    pub sql: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RunSqlResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub truncated: bool,
}

pub const RUN_SQL_ROW_LIMIT: usize = 10_000;

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
        let record_val: serde_json::Value = conn
            .query_row("SELECT * FROM wads WHERE id = ?1", [args.id], row_to_json)
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CacoMcpError::WadNotFound { id: args.id }.into_mcp_error()
                }
                other => CacoMcpError::from(other).into_mcp_error(),
            })?;
        let record = match record_val {
            serde_json::Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        let mut stmt = conn
            .prepare("SELECT tag FROM tags WHERE wad_id = ?1 ORDER BY tag")
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
    ) -> std::result::Result<Json<InspectRows>, rmcp::ErrorData> {
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
        Ok(Json(InspectRows { rows }))
    }

    #[tool(
        name = "inspect_companions",
        description = "Return companion registry rows. When `wad_id` is set, joins `wad_companions` \
                       and returns only companions attached to that WAD, ordered by load_order."
    )]
    pub fn inspect_companions(
        &self,
        Parameters(args): Parameters<InspectCompanionsArgs>,
    ) -> std::result::Result<Json<InspectRows>, rmcp::ErrorData> {
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
        Ok(Json(InspectRows { rows }))
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
    ) -> std::result::Result<Json<InspectRows>, rmcp::ErrorData> {
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
        Ok(Json(InspectRows { rows }))
    }

    #[tool(
        name = "inspect_id24",
        description = "Return all registered id24 WADs from the `id24_wads` table, ordered by name."
    )]
    pub fn inspect_id24(
        &self,
        _p: Parameters<EmptyArgs>,
    ) -> std::result::Result<Json<InspectRows>, rmcp::ErrorData> {
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
        Ok(Json(InspectRows { rows }))
    }

    #[tool(
        name = "run_sql",
        description = "Run a read-only SELECT against the sandbox DB. Rejects writes and \
                       multi-statement input. Caps result at 10000 rows (sets `truncated: true` \
                       when the cap is hit)."
    )]
    pub fn run_sql(
        &self,
        Parameters(args): Parameters<RunSqlArgs>,
    ) -> std::result::Result<Json<RunSqlResult>, rmcp::ErrorData> {
        execute_run_sql(&self.paths, &args.sql)
            .map(Json)
            .map_err(|e| e.into_mcp_error())
    }
}

/// Run a read-only SELECT against the sandbox DB.
///
/// Guards: connection opened read-only, `prepare()`'d statement must be
/// `readonly()`, and input containing a non-terminal `;` is rejected so that
/// piggybacked statements can't slip through.
pub fn execute_run_sql(paths: &SandboxPaths, sql: &str) -> Result<RunSqlResult> {
    if has_trailing_statement(sql) {
        return Err(CacoMcpError::SqlRejected {
            reason: "multiple statements not allowed".into(),
        });
    }
    let conn =
        Connection::open_with_flags(paths.db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let stmt = conn.prepare(sql)?;
    if !stmt.readonly() {
        return Err(CacoMcpError::SqlRejected {
            reason: "statement is not read-only".into(),
        });
    }
    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let n = columns.len();
    let mut stmt = stmt;
    let mut q = stmt.query([])?;
    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut truncated = false;
    while let Some(row) = q.next()? {
        if rows.len() >= RUN_SQL_ROW_LIMIT {
            truncated = true;
            break;
        }
        let mut vals = Vec::with_capacity(n);
        for i in 0..n {
            let v: rusqlite::types::Value = row.get(i)?;
            vals.push(match v {
                rusqlite::types::Value::Null => serde_json::Value::Null,
                rusqlite::types::Value::Integer(x) => serde_json::json!(x),
                rusqlite::types::Value::Real(x) => serde_json::json!(x),
                rusqlite::types::Value::Text(x) => serde_json::json!(x),
                rusqlite::types::Value::Blob(x) => serde_json::json!(hex::encode(&x)),
            });
        }
        rows.push(vals);
    }
    Ok(RunSqlResult { columns, rows, truncated })
}

/// Returns true if `sql` contains a `;` that is not at end-of-string
/// (ignoring trailing whitespace and trailing `;`s). This catches piggybacked
/// multi-statement input. It misses `;` inside string literals, but rusqlite's
/// `prepare()` + readonly check will catch anything effectful regardless.
fn has_trailing_statement(sql: &str) -> bool {
    let trimmed = sql.trim_end_matches(|c: char| c.is_whitespace() || c == ';');
    trimmed.contains(';')
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

#[cfg(test)]
mod run_sql_tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn seed_sandbox() -> (TempDir, SandboxPaths) {
        let dir = TempDir::new().unwrap();
        let sandbox = dir.path().to_path_buf();
        let db = sandbox.join("library.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE wads(id INTEGER PRIMARY KEY, title TEXT);
             INSERT INTO wads VALUES (1, 'Doom'), (2, 'Doom II');",
        )
        .unwrap();
        let paths = SandboxPaths {
            sandbox,
            source_home: dir.path().to_path_buf(),
        };
        (dir, paths)
    }

    #[test]
    fn select_returns_rows() {
        let (_d, paths) = seed_sandbox();
        let res = execute_run_sql(&paths, "SELECT id, title FROM wads ORDER BY id").unwrap();
        assert_eq!(res.columns, vec!["id", "title"]);
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0][1], serde_json::json!("Doom"));
        assert!(!res.truncated);
    }

    #[test]
    fn rejects_insert() {
        let (_d, paths) = seed_sandbox();
        let err =
            execute_run_sql(&paths, "INSERT INTO wads VALUES (3, 'Final Doom')").unwrap_err();
        assert!(matches!(
            err,
            CacoMcpError::SqlRejected { .. } | CacoMcpError::Database(_)
        ));
    }

    #[test]
    fn rejects_delete() {
        let (_d, paths) = seed_sandbox();
        let err = execute_run_sql(&paths, "DELETE FROM wads").unwrap_err();
        assert!(matches!(
            err,
            CacoMcpError::SqlRejected { .. } | CacoMcpError::Database(_)
        ));
    }

    #[test]
    fn rejects_update() {
        let (_d, paths) = seed_sandbox();
        let err =
            execute_run_sql(&paths, "UPDATE wads SET title = 'x' WHERE id = 1").unwrap_err();
        assert!(matches!(
            err,
            CacoMcpError::SqlRejected { .. } | CacoMcpError::Database(_)
        ));
    }

    #[test]
    fn rejects_multiple_statements() {
        let (_d, paths) = seed_sandbox();
        let err = execute_run_sql(&paths, "SELECT 1; SELECT 2").unwrap_err();
        assert!(matches!(err, CacoMcpError::SqlRejected { .. }));
    }

    #[test]
    fn allows_trailing_semicolon_and_whitespace() {
        let (_d, paths) = seed_sandbox();
        let res = execute_run_sql(&paths, "SELECT id FROM wads ;  \n").unwrap();
        assert_eq!(res.rows.len(), 2);
    }

    #[test]
    fn truncates_at_limit() {
        let (_d, paths) = seed_sandbox();
        let conn = Connection::open(paths.db_path()).unwrap();
        conn.execute_batch("CREATE TABLE big(x INTEGER);").unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        for i in 0..(RUN_SQL_ROW_LIMIT as i64 + 1) {
            tx.execute("INSERT INTO big VALUES (?1)", [i]).unwrap();
        }
        tx.commit().unwrap();
        drop(conn);

        let res = execute_run_sql(&paths, "SELECT x FROM big").unwrap();
        assert_eq!(res.rows.len(), RUN_SQL_ROW_LIMIT);
        assert!(res.truncated);
    }

    #[test]
    fn has_trailing_statement_detection() {
        assert!(!has_trailing_statement("SELECT 1"));
        assert!(!has_trailing_statement("SELECT 1;"));
        assert!(!has_trailing_statement("SELECT 1; \n"));
        assert!(has_trailing_statement("SELECT 1; SELECT 2"));
        assert!(has_trailing_statement("SELECT 1;;SELECT 2"));
    }
}
