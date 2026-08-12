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
    Unplayed,
    InProgress,
    Completed,
    Abandoned,
}

impl Status {
    pub const ALL: &[Status] = &[
        Status::Unplayed,
        Status::InProgress,
        Status::Completed,
        Status::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Unplayed => "unplayed",
            Status::InProgress => "in-progress",
            Status::Completed => "completed",
            Status::Abandoned => "abandoned",
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

    /// Display name (e.g., "Unplayed", "In Progress").
    pub fn display_name(self) -> &'static str {
        match self {
            Status::Unplayed => "Unplayed",
            Status::InProgress => "In Progress",
            Status::Completed => "Completed",
            Status::Abandoned => "Abandoned",
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
            "unplayed" => Ok(Status::Unplayed),
            "in-progress" => Ok(Status::InProgress),
            "completed" => Ok(Status::Completed),
            "abandoned" => Ok(Status::Abandoned),
            _ => Err(crate::Error::InvalidStatus(s.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Availability enum
// ---------------------------------------------------------------------------

/// Whether a WAD's file is on this machine right now, and if not, whether
/// there is any URL to try.
///
/// Derived from `cached_path` + `source_url`, never stored. It used to be a
/// column maintained on write, which drifted the moment anything nulled
/// `cached_path` with raw SQL — `clear_cached_path` did exactly that, so
/// every cache eviction left a row still claiming to be cached. Deriving it
/// makes that unrepresentable.
///
/// Answers a different question from [`Retrievability`]: this one changes
/// every time a file lands in or leaves the cache, while retrievability is a
/// fact about where the WAD came from. Note that `Downloadable` only promises
/// a `source_url` exists, not that fetching it will work — see
/// [`Retrievability::Manual`].
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

    /// Classify from the two fields that decide it. An empty string counts as
    /// absent — blank text fields arrive from the GUI as `Some("")`.
    pub fn derive(cached_path: Option<&str>, source_url: Option<&str>) -> Self {
        let present = |v: Option<&str>| v.is_some_and(|s| !s.is_empty());
        if present(cached_path) {
            Availability::Cached
        } else if present(source_url) {
            Availability::Downloadable
        } else {
            Availability::Unavailable
        }
    }

    /// SQL predicate selecting rows of this variant.
    ///
    /// Must stay in lockstep with [`Availability::derive`];
    /// `test_availability_sql_matches_rust` asserts they agree.
    pub fn sql(self) -> &'static str {
        match self {
            Availability::Cached => "COALESCE(wads.cached_path, '') <> ''",
            Availability::Downloadable => {
                "(COALESCE(wads.cached_path, '') = '' AND COALESCE(wads.source_url, '') <> '')"
            }
            Availability::Unavailable => {
                "(COALESCE(wads.cached_path, '') = '' AND COALESCE(wads.source_url, '') = '')"
            }
        }
    }

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
// Retrievability enum
// ---------------------------------------------------------------------------

/// Whether caco can fetch a WAD's file again on its own.
///
/// Deliberately derived from source metadata rather than stored, and never
/// from where the file currently sits on disk. Placement is a *consequence*
/// of retrievability — a manual WAD belongs in the portable data dir whether
/// or not its file has been downloaded yet — so reading it back off the path
/// would make the two drift apart the moment `cache_dir` or `CACO_HOME`
/// moves.
///
/// This is orthogonal to [`Availability`], which answers "is the file here
/// right now". A WAD can be `Manual` + cached (must be kept) or `Automatic` +
/// not cached (will be re-fetched on demand).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Retrievability {
    /// Re-downloadable from idgames, which is archival and stable.
    Automatic,
    /// Only obtainable by hand. `source_url` may still be set — a Doomworld
    /// thread or a one-off file host — but those links rot, sit behind
    /// Cloudflare, or point at storage that has since been emptied, so they
    /// are not something caco can promise to re-fetch.
    Manual,
}

impl Retrievability {
    pub const ALL: &[Retrievability] = &[Retrievability::Automatic, Retrievability::Manual];

    /// SQL predicate matching [`Retrievability::Automatic`] rows.
    ///
    /// Must stay in lockstep with [`Retrievability::derive`];
    /// `test_retrievability_sql_matches_rust` asserts they agree.
    pub const AUTOMATIC_SQL: &str =
        "(wads.source_type = 'idgames' OR COALESCE(wads.idgames_id, '') <> '')";

    /// SQL predicate matching WADs that are `Manual` *and* have no local copy
    /// — the set `WadRecord::is_lost` describes. Not a third variant: it is an
    /// intersection with [`Availability`], exposed because "what can I no
    /// longer play" is the question worth one click.
    pub const LOST_SQL: &str = "(NOT (wads.source_type = 'idgames' \
         OR COALESCE(wads.idgames_id, '') <> '') \
         AND COALESCE(wads.cached_path, '') = '')";

    /// Classify from the two fields that decide it.
    pub fn derive(source_type: SourceType, idgames_id: Option<&str>) -> Self {
        if source_type == SourceType::Idgames || idgames_id.is_some_and(|id| !id.is_empty()) {
            Retrievability::Automatic
        } else {
            Retrievability::Manual
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Retrievability::Automatic => "automatic",
            Retrievability::Manual => "manual",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Retrievability::Automatic => "Automatic",
            Retrievability::Manual => "Manual",
        }
    }

    /// True when caco can fetch the file again unattended.
    pub fn is_automatic(self) -> bool {
        self == Retrievability::Automatic
    }
}

impl fmt::Display for Retrievability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Retrievability {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "automatic" | "auto" | "a" => Ok(Retrievability::Automatic),
            "manual" | "m" => Ok(Retrievability::Manual),
            _ => Err(crate::Error::InvalidRetrievability(s.to_string())),
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
    pub status: Status,
    pub rating: Option<i32>,
    pub notes: Option<String>,
    pub source_type: SourceType,
    pub source_id: Option<String>,
    pub source_url: Option<String>,
    pub idgames_id: Option<String>,
    pub filename: Option<String>,
    pub cached_path: Option<String>,
    pub custom_iwad: Option<String>,
    pub custom_sourceport: Option<String>,
    pub required_sourceport_family: Option<String>,
    pub custom_args: Option<String>,
    pub companion_files: Option<String>,
    pub custom_config: Option<String>,
    pub version: Option<String>,
    pub complevel: Option<i32>,
    pub zdoom_required: Option<i32>,
    /// JSON-encoded array of candidate download URLs scraped from the source
    /// (primarily Doomworld threads).
    pub download_urls: Option<String>,
    pub stats_snapshot: Option<String>,
    pub gc_ignore: bool,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<String>,
}

impl WadRecord {
    /// Whether caco can fetch this WAD's file again on its own.
    pub fn retrievability(&self) -> Retrievability {
        Retrievability::derive(self.source_type, self.idgames_id.as_deref())
    }

    /// Whether this WAD's file is on the machine, and if not whether there is
    /// a URL to try.
    pub fn availability(&self) -> Availability {
        Availability::derive(self.cached_path.as_deref(), self.source_url.as_deref())
    }

    /// Manual-only with no local copy: caco cannot fetch this WAD, and the
    /// library is listing something it cannot actually launch.
    ///
    /// The intersection of the two axes rather than a state of its own — a
    /// `Manual` WAD becomes lost and unlost purely by its file coming and
    /// going, which is what `Availability` already tracks.
    pub fn is_lost(&self) -> bool {
        self.retrievability() == Retrievability::Manual
            && self.availability() != Availability::Cached
    }

    /// Build a `WadRecord` from a `rusqlite::Row`.
    ///
    /// Expects all wad columns to be present (SELECT *). Tags must be
    /// attached separately via `attach_tags`.
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let status_raw: String = row.get("status")?;
        let status = status_raw.parse().unwrap_or(Status::Unplayed);

        let source_type_raw: String = row.get("source_type")?;
        let source_type = source_type_raw.parse().unwrap_or(SourceType::Local);

        Ok(WadRecord {
            id: row.get("id")?,
            title: row.get("title")?,
            author: row.get("author")?,
            year: row.get("year")?,
            description: row.get("description")?,
            status,
            rating: row.get("rating")?,
            notes: row.get("notes")?,
            source_type,
            source_id: row.get("source_id")?,
            source_url: row.get("source_url")?,
            idgames_id: row.get("idgames_id")?,
            filename: row.get("filename")?,
            cached_path: row.get("cached_path")?,
            custom_iwad: row.get("custom_iwad")?,
            custom_sourceport: row.get("custom_sourceport")?,
            required_sourceport_family: row.get("required_sourceport_family").ok(),
            custom_args: row.get("custom_args")?,
            companion_files: row.get("companion_files")?,
            custom_config: row.get("custom_config")?,
            version: row.get("version")?,
            complevel: row.get("complevel")?,
            zdoom_required: row.get("zdoom_required")?,
            download_urls: row.get("download_urls").ok(),
            stats_snapshot: row.get("stats_snapshot")?,
            gc_ignore: row.get::<_, i64>("gc_ignore").unwrap_or(0) != 0,
            deleted_at: row.get("deleted_at")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            tags: Vec::new(),
        })
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
        ("u", "unplayed"),
        ("ip", "in-progress"),
        ("inp", "in-progress"),
        ("playing", "in-progress"),
        ("p", "in-progress"),
        ("c", "completed"),
        ("done", "completed"),
        ("finished", "completed"),
        ("f", "completed"),
        ("a", "abandoned"),
        ("dropped", "abandoned"),
        ("d", "abandoned"),
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
            "required_sourceport_family",
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
            "download_urls",
        ]
        .into_iter()
        .collect()
    });

/// Status metadata entry: (display_name, hex_color, rich_color, css_class).
pub type StatusMeta = (&'static str, &'static str, &'static str, &'static str);

/// Canonical status metadata.
pub static STATUS_METADATA: LazyLock<HashMap<&'static str, StatusMeta>> = LazyLock::new(|| {
    HashMap::from([
        (
            "unplayed",
            ("Unplayed", "#3366cc", "dodger_blue1", "status-unplayed"),
        ),
        (
            "in-progress",
            ("In Progress", "#33cc33", "green1", "status-in-progress"),
        ),
        (
            "completed",
            ("Completed", "#808080", "grey50", "status-completed"),
        ),
        (
            "abandoned",
            ("Abandoned", "#cc3333", "red", "status-abandoned"),
        ),
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
        assert_eq!(Status::parse("u"), Some(Status::Unplayed));
        assert_eq!(Status::parse("ip"), Some(Status::InProgress));
        assert_eq!(Status::parse("inp"), Some(Status::InProgress));
        assert_eq!(Status::parse("playing"), Some(Status::InProgress));
        assert_eq!(Status::parse("p"), Some(Status::InProgress));
        assert_eq!(Status::parse("c"), Some(Status::Completed));
        assert_eq!(Status::parse("done"), Some(Status::Completed));
        assert_eq!(Status::parse("finished"), Some(Status::Completed));
        assert_eq!(Status::parse("f"), Some(Status::Completed));
        assert_eq!(Status::parse("a"), Some(Status::Abandoned));
        assert_eq!(Status::parse("dropped"), Some(Status::Abandoned));
        assert_eq!(Status::parse("d"), Some(Status::Abandoned));
    }

    #[test]
    fn test_status_display() {
        assert_eq!(Status::Unplayed.to_string(), "unplayed");
        assert_eq!(Status::InProgress.to_string(), "in-progress");
        assert_eq!(Status::Completed.to_string(), "completed");
        assert_eq!(Status::Abandoned.to_string(), "abandoned");
    }

    #[test]
    fn test_status_display_name() {
        assert_eq!(Status::Unplayed.display_name(), "Unplayed");
        assert_eq!(Status::InProgress.display_name(), "In Progress");
        assert_eq!(Status::Completed.display_name(), "Completed");
        assert_eq!(Status::Abandoned.display_name(), "Abandoned");
    }

    #[test]
    fn test_invalid_status() {
        assert!("invalid".parse::<Status>().is_err());
        assert!(Status::parse("invalid").is_none());
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
        assert!(ALLOWED_UPDATE_FIELDS.contains("required_sourceport_family"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("play_state"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("intent"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("id"));
        assert!(!ALLOWED_UPDATE_FIELDS.contains("created_at"));
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

    // -----------------------------------------------------------------------
    // Retrievability tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_retrievability_as_str_roundtrip() {
        for &r in Retrievability::ALL {
            let s = r.as_str();
            let parsed: Retrievability = s.parse().unwrap();
            assert_eq!(parsed, r);
        }
    }

    #[test]
    fn test_retrievability_parse_shortcuts() {
        for s in ["automatic", "auto", "a", "AUTO", "Automatic"] {
            assert_eq!(
                s.parse::<Retrievability>().unwrap(),
                Retrievability::Automatic
            );
        }
        for s in ["manual", "m", "Manual"] {
            assert_eq!(s.parse::<Retrievability>().unwrap(), Retrievability::Manual);
        }
        assert!("nonsense".parse::<Retrievability>().is_err());
    }

    /// An idgames WAD is automatic regardless of whether the id was recorded.
    #[test]
    fn test_retrievability_idgames_source_is_automatic() {
        assert_eq!(
            Retrievability::derive(SourceType::Idgames, None),
            Retrievability::Automatic
        );
    }

    /// A WAD found elsewhere but mirrored on idgames is still automatic — the
    /// id is what makes it re-fetchable, not where the metadata came from.
    #[test]
    fn test_retrievability_idgames_id_promotes_other_sources() {
        for src in [
            SourceType::Doomwiki,
            SourceType::Doomworld,
            SourceType::Url,
            SourceType::Local,
        ] {
            assert_eq!(
                Retrievability::derive(src, Some("12345")),
                Retrievability::Automatic,
                "{src} with an idgames id should be automatic"
            );
            assert_eq!(
                Retrievability::derive(src, None),
                Retrievability::Manual,
                "{src} without an idgames id should be manual"
            );
        }
    }

    /// An empty id is not an id — blank text fields come back from the GUI's
    /// edit dialog as `Some("")` rather than `None`.
    #[test]
    fn test_retrievability_empty_idgames_id_is_manual() {
        assert_eq!(
            Retrievability::derive(SourceType::Doomworld, Some("")),
            Retrievability::Manual
        );
    }
}
