//! MCP tools for sandbox lifecycle.

use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use crate::reset::{ResetOptions, reset_sandbox};
use crate::sandbox::SandboxInfo;
use crate::server::CacoMcpServer;

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ResetSandboxParams {
    #[serde(default)]
    pub skip_wads: bool,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SandboxInfoParams {}

#[tool_router(router = sandbox_tools_router, vis = "pub")]
impl CacoMcpServer {
    #[tool(
        name = "sandbox_info",
        description = "Return the sandbox path, source home, and DB state."
    )]
    pub async fn sandbox_info_tool(&self, _p: Parameters<SandboxInfoParams>) -> Json<SandboxInfo> {
        Json(self.compute_sandbox_info())
    }

    #[tool(
        name = "reset_sandbox",
        description = "Wipe the sandbox and re-bootstrap it by deep-copying the source caco home. \
                       Set skip_wads=true to omit the potentially-large wads/ directory."
    )]
    pub async fn reset_sandbox_tool(
        &self,
        Parameters(params): Parameters<ResetSandboxParams>,
    ) -> Result<Json<SandboxInfo>, rmcp::ErrorData> {
        reset_sandbox(
            &self.paths,
            &ResetOptions {
                skip_wads: params.skip_wads,
            },
        )
        .map_err(|e| e.into_mcp_error())?;
        Ok(Json(self.compute_sandbox_info()))
    }
}
