use std::time::{Duration, Instant};

use caco_core::player::PlayResult;
use caco_core::wad_analysis::WadAnalysis;
use caco_sources::import_service::ImportResult;

use crate::import::state::{ImportOrigin, SearchResults, SearchSource};
use crate::state::ActionRequest;

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

/// What a notification offers to do about what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAction {
    /// Take a WAD back out of the Trash.
    UndoDelete(i64),
    /// Show a WAD in the library.
    ShowWad(i64),
    OpenSourceports,
    OpenSettings,
}

impl NoticeAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::UndoDelete(_) => "Undo",
            Self::ShowWad(_) => "Show",
            Self::OpenSourceports => "Open Sourceports",
            Self::OpenSettings => "Open Settings",
        }
    }

    pub fn request(self) -> ActionRequest {
        match self {
            Self::UndoDelete(wad_id) => ActionRequest::UndoDelete(wad_id),
            Self::ShowWad(wad_id) => ActionRequest::ShowWad(wad_id),
            Self::OpenSourceports => ActionRequest::Sourceports,
            Self::OpenSettings => ActionRequest::Settings,
        }
    }
}

/// Long enough to read a sentence and go back to what you were doing.
const INFO_LIFETIME: Duration = Duration::from_secs(6);
/// Long enough to read a sentence that says something went wrong, and do something about it.
const WARNING_LIFETIME: Duration = Duration::from_secs(15);
/// Long enough to look over, see the button, and press it.
const ACTION_LIFETIME: Duration = Duration::from_secs(12);

#[derive(Debug, Clone)]
pub struct Notification {
    pub text: String,
    pub severity: Severity,
    pub created_at: Instant,
    pub action: Option<NoticeAction>,
}

impl Notification {
    fn new(text: String, severity: Severity) -> Self {
        Self {
            text,
            severity,
            created_at: Instant::now(),
            action: None,
        }
    }

    pub fn info(text: String) -> Self {
        Self::new(text, Severity::Info)
    }

    pub fn warning(text: String) -> Self {
        Self::new(text, Severity::Warning)
    }

    pub fn error(text: String) -> Self {
        Self::new(text, Severity::Error)
    }

    /// Offer something to do about it.
    pub fn with_action(mut self, action: NoticeAction) -> Self {
        self.action = Some(action);
        self
    }

    /// How long this stays. They all used to go after three seconds, whatever they said and
    /// however long it was, so that what went wrong with a launch was gone by the time anybody
    /// had looked down. An error stays until it is dismissed or something replaces it.
    fn lifetime(&self) -> Option<Duration> {
        match self.severity {
            Severity::Error => None,
            Severity::Warning => Some(WARNING_LIFETIME),
            Severity::Info if self.action.is_some() => Some(ACTION_LIFETIME),
            Severity::Info => Some(INFO_LIFETIME),
        }
    }

    /// How long until this goes, and `None` if it does not go by itself.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        let age = now.saturating_duration_since(self.created_at);
        self.lifetime().map(|life| life.saturating_sub(age))
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        self.remaining(now).is_some_and(|left| left.is_zero())
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

/// What a run of the idgames index worker did.
#[derive(Debug, Clone)]
pub struct IdgamesIndexRun {
    pub report: caco_sources::idgames::sync::SyncReport,
    pub status: caco_sources::idgames::IndexStatus,
    /// How many WADs in the library got the ids they were missing.
    pub ids_filled_in: usize,
    /// Someone asked for this run, and wants to hear how it went.
    pub asked_for: bool,
}

/// What a pass of the sync worker did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRun {
    pub report: caco_core::sync::SyncReport,
    /// How many WADs turned out to have their file here already.
    pub adopted: usize,
    /// Which machine this is, to tell its own history apart from anyone else's. Blank if
    /// that could not be found out, which the report says.
    pub this_machine: String,
    /// Someone pressed Sync Now, and wants to hear how it went.
    pub asked_for: bool,
}

pub enum AppMessage {
    Notify(Notification),
    PlayFinished {
        outcome: Result<PlayResult, String>,
    },
    /// The file wasn't here, and caco has started or finished fetching it before the launch.
    PlayDownloading(bool),
    /// There is nothing to launch with, which is fixed somewhere in particular.
    PlayNeedsSetup {
        text: String,
        fix: NoticeAction,
    },
    /// The WAD's file is not on this machine and caco could not fetch it.
    /// Triggers the "WAD Unavailable" link dialog, which says where the file
    /// was last seen, and what went wrong if caco tried to fetch it.
    PlayUnavailable {
        wad_id: i64,
        reason: Option<String>,
    },
    /// The WAD's file plays differently from the one it was last played with,
    /// so the launch stopped to ask. Opens the "File Changed" dialog.
    PlayFileChanged {
        wad_id: i64,
        title: String,
        change: caco_core::identity::ContentChange,
        opts: caco_core::player::PlayOptions,
    },
    /// Another machine has saves for the WAD that this one does not, so the launch stopped
    /// to ask. Opens the "Newer Saves" dialog. `opts` is what the launch had already been
    /// told to go ahead with.
    PlayNewerSaves {
        wad_id: i64,
        title: String,
        saves: caco_core::sync::NewerSaves,
        opts: caco_core::player::PlayOptions,
    },
    /// A pass of the sync worker has something to report.
    SyncFinished(SyncRun),
    /// A file has finished arriving in the Downloads folder while a Get was waiting.
    DownloadLanded(caco_core::downloads::Arrival),
    /// The file the user said to link has been dealt with.
    GetLinked {
        target: crate::get::GetTarget,
        title: String,
        filename: String,
        outcome: Result<crate::get::GetOutcome, String>,
    },
    /// The startup check of every recorded WAD file wrote something back.
    /// Only sent when it did, so the library reloads only when it is stale.
    FilesChecked(caco_core::presence::CheckReport),
    /// The startup hashing of WAD files new to caco or replaced since it last
    /// looked recorded something. Silent, since a changed file is only worth
    /// a word when it is about to be played, but the library reloads: a file
    /// that plays differently has lost its map analysis.
    FilesIdentified,
    /// Every mirror's index has been read, or tried.
    MirrorsSynced(caco_sources::mirror::SyncReport),
    /// A test of the mirror in the setup wizard finished.
    MirrorProbed(Result<caco_sources::mirror::MirrorProbe, String>),
    /// Setting up the mirror in the wizard finished, for this config.
    MirrorSetUp {
        config: Box<caco_core::config::MirrorConfig>,
        outcome: Result<(), String>,
    },
    /// A bulk upload moved on to another WAD. `done` counts WADs already finished.
    MirrorBulkProgress {
        done: usize,
        total: usize,
        title: String,
    },
    /// A bulk upload finished, or could not start.
    MirrorBulkFinished(Result<caco_sources::mirror::UploadReport, String>),
    /// An upload to a mirror finished. Which WAD by title, since the status bar
    /// lists what is in flight by title.
    MirrorUploaded {
        title: String,
        outcome: Result<caco_sources::mirror::Uploaded, String>,
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
    SourceportBuildStep(String),
    /// One line of output from the running build command. Sent per line
    /// rather than buffered so a four-minute compile shows progress.
    SourceportBuildLine(String),
    /// The build finished: `Ok` carries the message to show, `Err` the
    /// failure with the tail of the output that produced it.
    SourceportBuildComplete(Result<String, String>),
    /// Which versions a sourceport's repository has, asked when the Sourceports
    /// dialog first shows it.
    SourceportVersionsLoaded {
        sourceport: String,
        outcome: Result<caco_core::sourceports::RemoteVersions, String>,
    },
    /// Names of built sourceports with something newer to build: a newer
    /// release, or new commits on their branch. Only sent when the list is
    /// non-empty — a startup check that finds nothing says nothing.
    SourceportUpdatesAvailable(Vec<String>),
    SearchComplete(SearchSource, Result<SearchResults, String>),
    /// The newest uploads on idgames, for the table on the Import view.
    NewestUploads(SearchResults),
    /// How far along the local index of idgames is, read every so often by the worker
    /// keeping it up to date, and once when the app starts whether or not it has work to do.
    IdgamesIndexStatus(caco_sources::idgames::IndexStatus),
    /// That worker moved on, or got further through what it was doing.
    IdgamesIndexProgress(caco_sources::idgames::sync::SyncProgress),
    /// That worker finished, or could not start.
    IdgamesIndexFinished(Result<IdgamesIndexRun, String>),
    ImportComplete(ImportOrigin, Result<ImportResult, String>),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_stays_until_it_is_dismissed() {
        let error = Notification::error("It did not start.".to_string());
        let a_day_later = error.created_at + Duration::from_secs(86_400);
        assert!(!error.is_expired(a_day_later));
        assert_eq!(error.remaining(a_day_later), None);
    }

    #[test]
    fn what_is_only_news_goes_by_itself_and_sooner_than_what_went_wrong() {
        let info = Notification::info("Settings saved".to_string());
        let warning = Notification::warning("Close it first.".to_string());
        let start = info.created_at;

        assert!(!info.is_expired(start));
        assert!(info.is_expired(start + INFO_LIFETIME));
        assert!(!warning.is_expired(start + INFO_LIFETIME));
        assert!(warning.is_expired(warning.created_at + WARNING_LIFETIME));
    }

    #[test]
    fn something_that_can_be_acted_on_stays_long_enough_to_act_on() {
        let plain = Notification::info("Moved it".to_string());
        let undoable = plain.clone().with_action(NoticeAction::UndoDelete(1));
        let after_a_while = plain.created_at + INFO_LIFETIME;
        assert!(plain.is_expired(after_a_while));
        assert!(!undoable.is_expired(after_a_while));
    }
}
