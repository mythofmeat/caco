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
pub struct EnrichReport {
    pub examined: usize,
    pub findings: Vec<String>,
    pub wiki_lookups: u32,
    /// Why the wiki was only asked once, when it refused.
    pub wiki_blocked: Option<String>,
    pub cancelled: bool,
    pub dry_run: bool,
}

pub enum AppMessage {
    Notify(Notification),
    PlayFinished {
        outcome: Result<PlayResult, String>,
    },
    /// The WAD's file is not on this machine and caco could not fetch it.
    /// Triggers the "WAD Unavailable" link dialog, which says where the file
    /// was last seen.
    PlayUnavailable {
        wad_id: i64,
    },
    /// The WAD's file plays differently from the one it was last played with,
    /// so the launch stopped to ask. Opens the "File Changed" dialog.
    PlayFileChanged {
        wad_id: i64,
        title: String,
        change: caco_core::identity::ContentChange,
    },
    /// The startup check of every recorded WAD file wrote something back.
    /// Only sent when it did, so the library reloads only when it is stale.
    FilesChecked(caco_core::presence::CheckReport),
    /// The startup hashing of WAD files new to caco or replaced since it last
    /// looked recorded something. Silent, since a changed file is only worth
    /// a word when it is about to be played, but the library reloads: a file
    /// that plays differently has lost its map analysis.
    FilesIdentified,
    /// An enrichment run moved on to another WAD. `done` counts WADs already
    /// finished, so `done / total` drives a progress bar directly.
    EnrichProgress {
        done: usize,
        total: usize,
        title: String,
    },
    EnrichComplete(Result<EnrichReport, String>),
    /// A sourceport build moved on to a new stage (fetch, configure, ...).
    SourceportBuildStep(String),
    /// One line of output from the running build command. Sent per line
    /// rather than buffered so a four-minute compile shows progress.
    SourceportBuildLine(String),
    /// The build finished: `Ok` carries the message to show, `Err` the
    /// failure with the tail of the output that produced it.
    SourceportBuildComplete(Result<String, String>),
    /// Result of an explicit, on-demand branch/tag lookup from the Sourceports dialog.
    SourceportVersionsLoaded {
        sourceport: String,
        outcome: Result<caco_core::sourceports::RemoteRefs, String>,
    },
    /// Names of built sourceports whose remote has moved past the installed
    /// commit. Only sent when the list is non-empty — a startup check that
    /// finds nothing says nothing.
    SourceportUpdatesAvailable(Vec<String>),
    SearchComplete(SearchSource, Result<Vec<SearchResultEntry>, String>),
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
