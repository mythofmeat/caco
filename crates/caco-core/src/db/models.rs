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
    pub stats_snapshot: Option<String>,
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
            stats_snapshot: row.get("stats_snapshot")?,
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
            "idgames_id",
            "deleted_at",
            "stats_snapshot",
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
        assert!(!ALLOWED_UPDATE_FIELDS.contains("id"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("created_at"));
    }
}
