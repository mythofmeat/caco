use caco_sources::import_service::ImportResult;

/// Severity level for notifications.
#[derive(Clone, Debug)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(&self) -> &str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// Identifies which screen to push.
#[derive(Clone, Debug)]
pub enum ScreenId {
    WadDetail(i64),
    WadEdit(i64),
    Sessions(i64),
    ConfirmDelete(i64),
    Stats,
    WadStats(i64),
    Cache,
    Resources,
}

/// Result returned when a screen pops.
#[derive(Clone, Debug)]
pub enum ScreenResult {
    /// Edit was saved.
    Saved,
    /// Deletion was confirmed for this WAD ID.
    Confirmed(i64),
    /// User cancelled.
    Cancelled,
}

/// Search result from a background thread (idgames or doomwiki).
#[derive(Clone, Debug)]
pub enum SearchSource {
    Idgames,
    Doomwiki,
}

/// Messages sent from screens/widgets/background threads to the App.
pub enum AppMessage {
    // Navigation
    PushScreen(ScreenId),
    PopScreen(ScreenResult),
    Quit,

    // Data mutations
    WadUpdated(i64),
    WadImported(i64),
    WadDeleted(i64),
    RefreshLibrary,

    // Notifications
    Notify(String, Severity),

    // Play
    PlayWad(i64),

    // Background results
    SearchComplete(SearchSource, Vec<SearchResultEntry>),
    ImportComplete(std::result::Result<ImportResult, String>),
}

/// Generic search result entry from background search threads.
#[derive(Clone, Debug)]
pub struct SearchResultEntry {
    pub title: String,
    pub author: Option<String>,
    pub extra: String,
    pub description: Option<String>,
    /// Source-specific ID for import.
    pub source_id: String,
    /// Source-specific data for preview.
    pub source_data: SearchSourceData,
}

/// Source-specific data attached to search results.
#[derive(Clone, Debug)]
pub enum SearchSourceData {
    Idgames {
        id: i64,
        rating: Option<f64>,
        date: Option<String>,
        filename: Option<String>,
    },
    Doomwiki {
        year: Option<i32>,
        iwad: Option<String>,
        port: Option<String>,
    },
}
