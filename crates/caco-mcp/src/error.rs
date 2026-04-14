//! Error type for caco-mcp.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacoMcpError {
    #[error("sandbox path resolves to the real caco home: {path}")]
    SandboxPathUnsafe { path: PathBuf },

    #[error("sandbox does not exist at {path} — run reset_sandbox to bootstrap")]
    SandboxMissing { path: PathBuf },

    #[error("source home does not exist at {path}")]
    SourceHomeMissing { path: PathBuf },

    #[error("caco binary not found (tried: {tried:?})")]
    CacoBinNotFound { tried: Vec<PathBuf> },

    #[error("caco binary failed to spawn: {0}")]
    CacoBinSpawn(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("filesystem copy failed: {0}")]
    FsCopy(#[from] fs_extra::error::Error),

    #[error("sql statement rejected: {reason}")]
    SqlRejected { reason: String },

    #[error("wad not found: id={id}")]
    WadNotFound { id: i64 },

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl CacoMcpError {
    /// Convert into an `rmcp::ErrorData` for returning from tool handlers.
    pub fn into_mcp_error(self) -> rmcp::ErrorData {
        rmcp::ErrorData::internal_error(self.to_string(), None)
    }
}

pub type Result<T> = std::result::Result<T, CacoMcpError>;
