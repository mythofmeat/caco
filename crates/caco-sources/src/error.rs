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

    /// A mirror could not do what it was asked. Names the mirror, because a library
    /// can have several and the words are all the user gets.
    #[error("Mirror {mirror}: {message}")]
    Mirror { mirror: String, message: String },

    /// A WAD reads as fetchable only because of a mirror no longer configured, which
    /// the next sync forgets.
    #[error("No configured mirror has {title}")]
    NoMirrorHas { title: String },

    /// Nothing says where a WAD from idgames is in the archive, or no mirror had it there.
    #[error("Could not find {title} on any idgames mirror")]
    NotOnIdgames { title: String },

    /// A WAF answered with a challenge instead of the site's response, as
    /// recognised by [`crate::http::waf_challenge`].
    ///
    /// Worded here rather than by each client, so the same event cannot read
    /// differently depending on which client hit it, and the WAF it names is the
    /// one that answered. It does not promise that waiting helps: a challenge can
    /// be a site's standing policy for non-browser clients, not a passing limit.
    #[error(
        "{site} blocked the request ({kind} challenge). Retrying may not help; \
         add the WAD with the URL or local form instead."
    )]
    WafBlocked { site: &'static str, kind: WafKind },
}

/// Convenience alias used throughout caco-sources.
pub type Result<T> = std::result::Result<T, SourceError>;
