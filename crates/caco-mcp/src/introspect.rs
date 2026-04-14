//! Direct-DB introspection tools.
//!
//! Stubs will be filled in by later tasks.

use rmcp::tool_router;

use crate::server::CacoMcpServer;

#[tool_router(router = introspect_router, vis = "pub")]
impl CacoMcpServer {}
