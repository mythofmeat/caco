//! MCP tools that shell out to the caco CLI.
//!
//! Stubs will be filled in by later tasks.

use rmcp::tool_router;

use crate::server::CacoMcpServer;

#[tool_router(router = cli_tools_router, vis = "pub")]
impl CacoMcpServer {}
