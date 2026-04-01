use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Status enum
// ---------------------------------------------------------------------------

/// Play status for a WAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    ToPlay,
    Backlog,
    Playing,
    Finished,
    Abandoned,
    AwaitingUpdate,
}

impl Status {
    pub const ALL: &[Status] = &[
        Status::ToPlay,
        Status::Backlog,
        Status::Playing,
        Status::Finished,
        Status::Abandoned,
        Status::AwaitingUpdate,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::ToPlay => "to-play",
            Status::Backlog => "backlog",
            Status::Playing => "playing",
            Status::Finished => "finished",
            Status::Abandoned => "abandoned",
            Status::AwaitingUpdate => "awaiting-update",
        }
    }

    /// Parse a status string, supporting shortcuts.
    pub fn parse(s: &str) -> Option<Status> {
        // Try exact match first
        if let Ok(st) = s.parse::<Status>() {
            return Some(st);
        }
        // Try shortcut
        STATUS_SHORTCUTS
            .get(s.to_lowercase().as_str())
            .and_then(|full| full.parse().ok())
    }

    /// Display name (e.g., "To Play", "Awaiting Update").
    pub fn display_name(self) -> &'static str {
        match self {
            Status::ToPlay => "To Play",
            Status::Backlog => "Backlog",
            Status::Playing => "Playing",
            Status::Finished => "Finished",
            Status::Abandoned => "Abandoned",
            Status::AwaitingUpdate => "Awaiting Update",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Status {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "to-play" => Ok(Status::ToPlay),
            "backlog" => Ok(Status::Backlog),
            "playing" => Ok(Status::Playing),
            "finished" => Ok(Status::Finished),
            "abandoned" => Ok(Status::Abandoned),
            "awaiting-update" => Ok(Status::AwaitingUpdate),
            _ => Err(crate::Error::InvalidStatus(s.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// PlayState enum
// ---------------------------------------------------------------------------

/// Objective play state for a WAD, derived from playthrough records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayState {
    Unplayed,
    Started,
    Completed,
}

impl PlayState {
    pub const ALL: &[PlayState] = &[
        PlayState::Unplayed,
        PlayState::Started,
        PlayState::Completed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PlayState::Unplayed => "unplayed",
            PlayState::Started => "started",
            PlayState::Completed => "completed",
        }
    }

    /// Parse a play state string, supporting shortcuts.
    pub fn parse(s: &str) -> Option<PlayState> {
        if let Ok(ps) = s.parse::<PlayState>() {
            return Some(ps);
        }
        PLAY_STATE_SHORTCUTS
            .get(s.to_lowercase().as_str())
            .and_then(|full| full.parse().ok())
    }

    pub fn display_name(self) -> &'static str {
        match self {
            PlayState::Unplayed => "Unplayed",
            PlayState::Started => "Started",
            PlayState::Completed => "Completed",
        }
    }
}

impl fmt::Display for PlayState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PlayState {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unplayed" => Ok(PlayState::Unplayed),
            "started" => Ok(PlayState::Started),
            "completed" => Ok(PlayState::Completed),
            _ => Err(crate::Error::InvalidPlayState(s.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Intent enum
// ---------------------------------------------------------------------------

/// User's organizational intent for a WAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Intent {
    Inbox,
    Queued,
    Shelved,
    Dropped,
}

impl Intent {
    pub const ALL: &[Intent] = &[
        Intent::Inbox,
        Intent::Queued,
        Intent::Shelved,
        Intent::Dropped,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Intent::Inbox => "inbox",
            Intent::Queued => "queued",
            Intent::Shelved => "shelved",
            Intent::Dropped => "dropped",
        }
    }

    /// Parse an intent string, supporting shortcuts.
    pub fn parse(s: &str) -> Option<Intent> {
        if let Ok(i) = s.parse::<Intent>() {
            return Some(i);
        }
        INTENT_SHORTCUTS
            .get(s.to_lowercase().as_str())
            .and_then(|full| full.parse().ok())
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Intent::Inbox => "Inbox",
            Intent::Queued => "Queued",
            Intent::Shelved => "Shelved",
            Intent::Dropped => "Dropped",
        }
    }
}

impl fmt::Display for Intent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Intent {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inbox" => Ok(Intent::Inbox),
            "queued" => Ok(Intent::Queued),
            "shelved" => Ok(Intent::Shelved),
            "dropped" => Ok(Intent::Dropped),
            _ => Err(crate::Error::InvalidIntent(s.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Availability enum
// ---------------------------------------------------------------------------

/// File availability state for a WAD (system-managed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Availability {
    Cached,
    Downloadable,
    Unavailable,
}

impl Availability {
    pub const ALL: &[Availability] = &[
        Availability::Cached,
        Availability::Downloadable,
        Availability::Unavailable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Availability::Cached => "cached",
            Availability::Downloadable => "downloadable",
            Availability::Unavailable => "unavailable",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Availability::Cached => "Cached",
            Availability::Downloadable => "Downloadable",
            Availability::Unavailable => "Unavailable",
        }
    }
}

impl fmt::Display for Availability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Availability {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cached" => Ok(Availability::Cached),
            "downloadable" => Ok(Availability::Downloadable),
            "unavailable" => Ok(Availability::Unavailable),
            _ => Err(crate::Error::InvalidAvailability(s.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// SourceType enum
// ---------------------------------------------------------------------------

/// Where the WAD can be obtained from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceType {
    Idgames,
    Doomwiki,
    Doomworld,
    Url,
    Local,
}

impl SourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::Idgames => "idgames",
            SourceType::Doomwiki => "doomwiki",
            SourceType::Doomworld => "doomworld",
            SourceType::Url => "url",
            SourceType::Local => "local",
        }
    }
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SourceType {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "idgames" => Ok(SourceType::Idgames),
            "doomwiki" => Ok(SourceType::Doomwiki),
            "doomworld" => Ok(SourceType::Doomworld),
            "url" => Ok(SourceType::Url),
            "local" => Ok(SourceType::Local),
            _ => Err(crate::Error::InvalidSourceType(s.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// WadRecord
// ---------------------------------------------------------------------------

/// A WAD row with attached tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WadRecord {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub year: Option<i32>,
    pub description: Option<String>,
    pub status: String,
    pub play_state: String,
    pub intent: String,
    pub availability: String,
    pub rating: Option<i32>,
    pub notes: Option<String>,
    pub source_type: String,
    pub source_id: Option<String>,
    pub source_url: Option<String>,
    pub idgames_id: Option<String>,
    pub filename: Option<String>,
    pub cached_path: Option<String>,
    pub custom_iwad: Option<String>,
    pub custom_sourceport: Option<String>,
    pub custom_args: Option<String>,
    pub companion_files: Option<String>,
    pub custom_config: Option<String>,
    pub version: Option<String>,
    pub complevel: Option<i32>,
    pub zdoom_required: Option<i32>,
    pub stats_snapshot: Option<String>,
    pub gc_ignore: bool,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<String>,
}

impl WadRecord {
    /// Build a `WadRecord` from a `rusqlite::Row`.
    ///
    /// Expects all wad columns to be present (SELECT *). Tags must be
    /// attached separately via `attach_tags`.
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(WadRecord {
            id: row.get("id")?,
            title: row.get("title")?,
            author: row.get("author")?,
            year: row.get("year")?,
            description: row.get("description")?,
            status: row.get("status")?,
            play_state: row.get::<_, Option<String>>("play_state")?.unwrap_or_else(|| "unplayed".to_string()),
            intent: row.get::<_, Option<String>>("intent")?.unwrap_or_else(|| "inbox".to_string()),
            availability: row.get::<_, Option<String>>("availability")?.unwrap_or_else(|| "unavailable".to_string()),
            rating: row.get("rating")?,
            notes: row.get("notes")?,
            source_type: row.get("source_type")?,
            source_id: row.get("source_id")?,
            source_url: row.get("source_url")?,
            idgames_id: row.get("idgames_id")?,
            filename: row.get("filename")?,
            cached_path: row.get("cached_path")?,
            custom_iwad: row.get("custom_iwad")?,
            custom_sourceport: row.get("custom_sourceport")?,
            custom_args: row.get("custom_args")?,
            companion_files: row.get("companion_files")?,
            custom_config: row.get("custom_config")?,
            version: row.get("version")?,
            complevel: row.get("complevel")?,
            zdoom_required: row.get("zdoom_required")?,
            stats_snapshot: row.get("stats_snapshot")?,
            gc_ignore: row.get::<_, i64>("gc_ignore").unwrap_or(0) != 0,
            deleted_at: row.get("deleted_at")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            tags: Vec::new(),
        })
    }

    /// Get the parsed status enum.
    pub fn status_enum(&self) -> Option<Status> {
        self.status.parse().ok()
    }

    /// Get the parsed play state enum.
    pub fn play_state_enum(&self) -> Option<PlayState> {
        self.play_state.parse().ok()
    }

    /// Get the parsed intent enum.
    pub fn intent_enum(&self) -> Option<Intent> {
        self.intent.parse().ok()
    }

    /// Get the parsed availability enum.
    pub fn availability_enum(&self) -> Option<Availability> {
        self.availability.parse().ok()
    }

    /// Get the parsed source type enum.
    pub fn source_type_enum(&self) -> Option<SourceType> {
        self.source_type.parse().ok()
    }
}

// ---------------------------------------------------------------------------
// Query parser types
// ---------------------------------------------------------------------------

/// A single query term (field:value or free text).
#[derive(Debug, Clone)]
pub struct QueryTerm {
    /// `None` for free-text search.
    pub field: Option<String>,
    pub value: String,
    pub negated: bool,
}

/// A group of terms joined by AND (implicit).
#[derive(Debug, Clone, Default)]
pub struct AndGroup {
    pub terms: Vec<QueryTerm>,
}

/// Complete parsed query with OR groups.
///
/// Structure: (term1 AND term2) OR (term3 AND term4)
#[derive(Debug, Clone, Default)]
pub struct ParsedQuery {
    pub or_groups: Vec<AndGroup>,
}

impl ParsedQuery {
    pub fn is_empty(&self) -> bool {
        self.or_groups.is_empty() || self.or_groups.iter().all(|g| g.terms.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Status shortcuts for query parsing.
pub static STATUS_SHORTCUTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("t", "to-play"),
        ("toplay", "to-play"),
        ("tp", "to-play"),
        ("b", "backlog"),
        ("back", "backlog"),
        ("p", "playing"),
        ("play", "playing"),
        ("f", "finished"),
        ("fin", "finished"),
        ("done", "finished"),
        ("a", "abandoned"),
        ("drop", "abandoned"),
        ("dropped", "abandoned"),
        ("w", "awaiting-update"),
        ("waiting", "awaiting-update"),
        ("wip", "awaiting-update"),
        ("au", "awaiting-update"),
        ("await", "awaiting-update"),
    ])
});

/// Play state shortcuts for query parsing.
pub static PLAY_STATE_SHORTCUTS: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| {
        HashMap::from([
            ("u", "unplayed"),
            ("s", "started"),
            ("c", "completed"),
            ("done", "completed"),
        ])
    });

/// Intent shortcuts for query parsing.
pub static INTENT_SHORTCUTS: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| {
        HashMap::from([
            ("i", "inbox"),
            ("q", "queued"),
            ("sh", "shelved"),
            ("d", "dropped"),
        ])
    });

/// OR separator for query syntax (space-comma-space).
pub const OR_SEPARATOR: &str = " , ";

/// Fields allowed in `update_wad()` — guards against SQL column-name injection.
pub static ALLOWED_UPDATE_FIELDS: LazyLock<std::collections::HashSet<&'static str>> =
    LazyLock::new(|| {
        [
            "title",
            "author",
            "year",
            "description",
            "status",
            "rating",
            "notes",
            "source_url",
            "filename",
            "cached_path",
            "custom_iwad",
            "custom_sourceport",
            "custom_args",
            "companion_files",
            "custom_config",
            "version",
            "complevel",
            "zdoom_required",
            "idgames_id",
            "deleted_at",
            "stats_snapshot",
            "gc_ignore",
            "play_state",
            "intent",
            "availability",
        ]
        .into_iter()
        .collect()
    });

/// Status metadata entry: (display_name, hex_color, rich_color, css_class).
pub type StatusMeta = (&'static str, &'static str, &'static str, &'static str);

/// Canonical status metadata.
pub static STATUS_METADATA: LazyLock<HashMap<&'static str, StatusMeta>> =
    LazyLock::new(|| {
        HashMap::from([
            ("to-play",         ("To Play",         "#3366cc", "dodger_blue1", "status-to-play")),
            ("backlog",         ("Backlog",          "#cccc33", "yellow",       "status-backlog")),
            ("playing",         ("Playing",          "#33cc33", "green1",       "status-playing")),
            ("finished",        ("Finished",         "#808080", "grey50",       "status-finished")),
            ("abandoned",       ("Abandoned",        "#cc3333", "red",          "status-abandoned")),
            ("awaiting-update", ("Awaiting Update",  "#cc33cc", "magenta",      "status-awaiting-update")),
        ])
    });

/// Play state metadata: (display_name, hex_color, rich_color, css_class).
pub static PLAY_STATE_METADATA: LazyLock<HashMap<&'static str, StatusMeta>> =
    LazyLock::new(|| {
        HashMap::from([
            ("unplayed",  ("Unplayed",  "#3366cc", "dodger_blue1", "play-unplayed")),
            ("started",   ("Started",   "#33cc33", "green1",       "play-started")),
            ("completed", ("Completed", "#808080", "grey50",       "play-completed")),
        ])
    });

/// Intent metadata: (display_name, hex_color, rich_color, css_class).
pub static INTENT_METADATA: LazyLock<HashMap<&'static str, StatusMeta>> =
    LazyLock::new(|| {
        HashMap::from([
            ("inbox",   ("Inbox",   "#cccc33", "yellow",       "intent-inbox")),
            ("queued",  ("Queued",  "#3366cc", "dodger_blue1", "intent-queued")),
            ("shelved", ("Shelved", "#808080", "grey50",       "intent-shelved")),
            ("dropped", ("Dropped", "#cc3333", "red",          "intent-dropped")),
        ])
    });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_as_str_roundtrip() {
        for &st in Status::ALL {
            let s = st.as_str();
            let parsed: Status = s.parse().unwrap();
            assert_eq!(parsed, st);
        }
    }

    #[test]
    fn test_status_shortcuts() {
        assert_eq!(Status::parse("t"), Some(Status::ToPlay));
        assert_eq!(Status::parse("tp"), Some(Status::ToPlay));
        assert_eq!(Status::parse("p"), Some(Status::Playing));
        assert_eq!(Status::parse("f"), Some(Status::Finished));
        assert_eq!(Status::parse("done"), Some(Status::Finished));
        assert_eq!(Status::parse("drop"), Some(Status::Abandoned));
        assert_eq!(Status::parse("w"), Some(Status::AwaitingUpdate));
        assert_eq!(Status::parse("wip"), Some(Status::AwaitingUpdate));
        assert_eq!(Status::parse("au"), Some(Status::AwaitingUpdate));
    }

    #[test]
    fn test_status_display() {
        assert_eq!(Status::ToPlay.to_string(), "to-play");
        assert_eq!(Status::AwaitingUpdate.to_string(), "awaiting-update");
    }

    #[test]
    fn test_status_display_name() {
        assert_eq!(Status::ToPlay.display_name(), "To Play");
        assert_eq!(Status::AwaitingUpdate.display_name(), "Awaiting Update");
    }

    #[test]
    fn test_source_type_roundtrip() {
        for st in [
            SourceType::Idgames,
            SourceType::Doomwiki,
            SourceType::Doomworld,
            SourceType::Url,
            SourceType::Local,
        ] {
            let s = st.as_str();
            let parsed: SourceType = s.parse().unwrap();
            assert_eq!(parsed, st);
        }
    }

    #[test]
    fn test_invalid_status() {
        assert!("invalid".parse::<Status>().is_err());
        assert!(Status::parse("invalid").is_none());
    }

    #[test]
    fn test_invalid_source_type() {
        assert!("invalid".parse::<SourceType>().is_err());
    }

    #[test]
    fn test_parsed_query_empty() {
        let q = ParsedQuery::default();
        assert!(q.is_empty());

        let q2 = ParsedQuery {
            or_groups: vec![AndGroup { terms: vec![] }],
        };
        assert!(q2.is_empty());
    }

    #[test]
    fn test_allowed_update_fields() {
        assert!(ALLOWED_UPDATE_FIELDS.contains("title"));
        assert!(ALLOWED_UPDATE_FIELDS.contains("status"));
        assert!(ALLOWED_UPDATE_FIELDS.contains("play_state"));
        assert!(ALLOWED_UPDATE_FIELDS.contains("intent"));
        assert!(ALLOWED_UPDATE_FIELDS.contains("availability"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("id"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("created_at"));
    }

    // -----------------------------------------------------------------------
    // PlayState tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_play_state_as_str_roundtrip() {
        for &ps in PlayState::ALL {
            let s = ps.as_str();
            let parsed: PlayState = s.parse().unwrap();
            assert_eq!(parsed, ps);
        }
    }

    #[test]
    fn test_play_state_shortcuts() {
        assert_eq!(PlayState::parse("u"), Some(PlayState::Unplayed));
        assert_eq!(PlayState::parse("s"), Some(PlayState::Started));
        assert_eq!(PlayState::parse("c"), Some(PlayState::Completed));
        assert_eq!(PlayState::parse("done"), Some(PlayState::Completed));
    }

    #[test]
    fn test_play_state_display() {
        assert_eq!(PlayState::Unplayed.to_string(), "unplayed");
        assert_eq!(PlayState::Completed.to_string(), "completed");
    }

    #[test]
    fn test_play_state_display_name() {
        assert_eq!(PlayState::Unplayed.display_name(), "Unplayed");
        assert_eq!(PlayState::Completed.display_name(), "Completed");
    }

    #[test]
    fn test_invalid_play_state() {
        assert!("invalid".parse::<PlayState>().is_err());
        assert!(PlayState::parse("invalid").is_none());
    }

    // -----------------------------------------------------------------------
    // Intent tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_intent_as_str_roundtrip() {
        for &i in Intent::ALL {
            let s = i.as_str();
            let parsed: Intent = s.parse().unwrap();
            assert_eq!(parsed, i);
        }
    }

    #[test]
    fn test_intent_shortcuts() {
        assert_eq!(Intent::parse("i"), Some(Intent::Inbox));
        assert_eq!(Intent::parse("q"), Some(Intent::Queued));
        assert_eq!(Intent::parse("sh"), Some(Intent::Shelved));
        assert_eq!(Intent::parse("d"), Some(Intent::Dropped));
    }

    #[test]
    fn test_intent_display() {
        assert_eq!(Intent::Inbox.to_string(), "inbox");
        assert_eq!(Intent::Dropped.to_string(), "dropped");
    }

    #[test]
    fn test_invalid_intent() {
        assert!("invalid".parse::<Intent>().is_err());
        assert!(Intent::parse("invalid").is_none());
    }

    // -----------------------------------------------------------------------
    // Availability tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_availability_as_str_roundtrip() {
        for &a in Availability::ALL {
            let s = a.as_str();
            let parsed: Availability = s.parse().unwrap();
            assert_eq!(parsed, a);
        }
    }

    #[test]
    fn test_availability_display() {
        assert_eq!(Availability::Cached.to_string(), "cached");
        assert_eq!(Availability::Unavailable.to_string(), "unavailable");
    }

    #[test]
    fn test_invalid_availability() {
        assert!("invalid".parse::<Availability>().is_err());
    }
}
