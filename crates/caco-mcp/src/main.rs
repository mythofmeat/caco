//! caco-mcp-server binary entry point.

use std::path::PathBuf;

use clap::Parser;
use rmcp::ServiceExt;
use rmcp::transport::io::stdio;
use tracing_subscriber::EnvFilter;

use caco_mcp::bin_resolve;
use caco_mcp::sandbox::SandboxPaths;
use caco_mcp::server::CacoMcpServer;

#[derive(Parser)]
#[command(name = "caco-mcp-server", version)]
struct Args {
    /// Override the sandbox root (default: ~/.local/share/caco-mcp-sandbox/).
    #[arg(long, env = "CACO_MCP_SANDBOX")]
    sandbox_path: Option<PathBuf>,

    /// Override the source caco home to bootstrap from (default: ~/.local/share/caco/).
    #[arg(long, env = "CACO_MCP_SOURCE_HOME")]
    source_home: Option<PathBuf>,

    /// Override the caco binary to invoke (default: sibling of this binary).
    #[arg(long, env = "CACO_MCP_CACO_BIN")]
    caco_bin: Option<PathBuf>,
}

fn default_sandbox_path() -> PathBuf {
    dirs::data_dir()
        .expect("no data_dir")
        .join("caco-mcp-sandbox")
}

fn default_source_home() -> PathBuf {
    dirs::data_dir().expect("no data_dir").join("caco")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging to stderr; stdout reserved for MCP JSON-RPC.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("CACO_MCP_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    let sandbox_path = args.sandbox_path.unwrap_or_else(default_sandbox_path);
    let source_home = args.source_home.unwrap_or_else(default_source_home);
    let paths = SandboxPaths::new(sandbox_path, source_home)?;
    let caco_bin = bin_resolve::resolve(args.caco_bin)?;

    tracing::info!(
        sandbox = ?paths.sandbox,
        source_home = ?paths.source_home,
        "starting caco-mcp-server"
    );

    let server = CacoMcpServer::new(paths, caco_bin);
    let (r, w) = stdio();
    let service = server.serve((r, w)).await?;
    service.waiting().await?;
    Ok(())
}
