/// Error type for source adapters (idgames, doomwiki, etc.).
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error: {0}")]
    Api(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("core error: {0}")]
    Core(#[from] caco_core::Error),

    #[error("import error: {0}")]
    Import(String),

    /// API blocked by WAF challenge (Cloudflare, AWS WAF, etc.).
    ///
    /// `api_name` identifies the API ("idgames" or "doomwiki")
    /// and `message` has user-facing details.
    #[error("{message}")]
    WafBlocked {
        api_name: String,
        message: String,
    },
}

/// Convenience alias used throughout caco-sources.
pub type Result<T> = std::result::Result<T, SourceError>;
