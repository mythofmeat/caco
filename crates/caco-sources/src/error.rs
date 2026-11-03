use crate::http::WafKind;

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

    /// A WAF answered with a challenge instead of the site's response, as
    /// recognised by [`crate::http::waf_challenge`].
    ///
    /// Worded here rather than by each client, so the same event cannot read
    /// differently depending on which client hit it, and the WAF it names is the
    /// one that answered.
    #[error("{site} blocked the request ({kind} challenge). Try again later or import manually.")]
    WafBlocked { site: &'static str, kind: WafKind },
}

/// Convenience alias used throughout caco-sources.
pub type Result<T> = std::result::Result<T, SourceError>;
