//! caco-mcp — MCP server for caco.
//!
//! See `docs/superpowers/specs/2026-04-14-mcp-server-design.md`.

pub mod bin_resolve;
pub mod cli_runner;
pub mod cli_tools;
pub mod cli_tools_macros;
pub mod error;
pub mod introspect;
pub mod reset;
pub mod sandbox;
pub mod sandbox_tools;
pub mod schema_transform;
pub mod server;
