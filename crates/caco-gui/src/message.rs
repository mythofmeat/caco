use std::time::Instant;

use caco_core::player::PlayResult;
use caco_sources::import_service::ImportResult;

use crate::import::state::{SearchResultEntry, SearchSource};

// ---------------------------------------------------------------------------
// Severity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Notification {
    pub text: String,
    pub severity: Severity,
    pub created_at: Instant,
}

impl Notification {
    pub fn info(text: String) -> Self {
        Self { text, severity: Severity::Info, created_at: Instant::now() }
    }

    pub fn warning(text: String) -> Self {
        Self { text, severity: Severity::Warning, created_at: Instant::now() }
    }

    pub fn error(text: String) -> Self {
        Self { text, severity: Severity::Error, created_at: Instant::now() }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().as_secs() >= 3
    }
}

// ---------------------------------------------------------------------------
// AppMessage (for background thread communication)
// ---------------------------------------------------------------------------

pub enum AppMessage {
    Notify(Notification),
    PlayFinished {
        wad_id: i64,
        outcome: Result<PlayResult, String>,
    },
    /// WAD could not be played because no downloadable source was available.
    /// Triggers the "WAD Unavailable" link dialog.
    PlayUnavailable {
        wad_id: i64,
    },
    SearchComplete(SearchSource, Vec<SearchResultEntry>),
    ImportComplete(Result<ImportResult, String>),
    ThumbnailReady {
        wad_id: i64,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    ThumbnailFailed {
        wad_id: i64,
    },
}
