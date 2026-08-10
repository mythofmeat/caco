use std::time::Instant;

use caco_core::player::PlayResult;
use caco_core::wad_analysis::WadAnalysis;
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
        Self {
            text,
            severity: Severity::Info,
            created_at: Instant::now(),
        }
    }

    pub fn warning(text: String) -> Self {
        Self {
            text,
            severity: Severity::Warning,
            created_at: Instant::now(),
        }
    }

    pub fn error(text: String) -> Self {
        Self {
            text,
            severity: Severity::Error,
            created_at: Instant::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().as_secs() >= 3
    }
}

// ---------------------------------------------------------------------------
// AppMessage (for background thread communication)
// ---------------------------------------------------------------------------

/// What an enrichment run produced.
///
/// Findings arrive pre-rendered as display lines: the enrich service's own
/// types stay in caco-sources, and the dialog only ever shows this list.
#[derive(Debug, Clone)]
pub enum EnrichReport {
    Wads {
        examined: usize,
        findings: Vec<String>,
        wiki_lookups: u32,
        cancelled: bool,
        dry_run: bool,
    },
    Cacowards {
        year: i64,
        scraped: usize,
        upserted: usize,
        linked: usize,
        previews: Vec<String>,
        dry_run: bool,
    },
}

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
    /// An enrichment run moved on to another WAD. `done` counts WADs already
    /// finished, so `done / total` drives a progress bar directly.
    EnrichProgress {
        done: usize,
        total: usize,
        title: String,
    },
    EnrichComplete(Result<EnrichReport, String>),
    /// A sourceport build moved on to a new stage (fetch, configure, ...).
    PortBuildStep(String),
    /// One line of output from the running build command. Sent per line
    /// rather than buffered so a four-minute compile shows progress.
    PortBuildLine(String),
    /// The build finished: `Ok` carries the message to show, `Err` the
    /// failure with the tail of the output that produced it.
    PortBuildComplete(Result<String, String>),
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
    /// A background re-analysis pass refreshed the cached `wad_analysis` row
    /// for one or more WADs. Carries the freshly produced analyses so the
    /// UI doesn't have to re-query the DB on the next frame.
    AnalysesRefreshed(Vec<(i64, WadAnalysis)>),
}
