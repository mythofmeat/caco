use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use super::mirrors::mirror_file_matches;
use crate::identity::{FileIdentity, Hashes};

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
/// there is anywhere to try.
///
/// Derived from `cached_path`, `source_url` and what the mirrors hold, never
/// stored. It used to be a column maintained on write, which drifted the moment
/// anything nulled `cached_path` with raw SQL — `clear_cached_path` did exactly
/// that, so every cache eviction left a row still claiming to be cached. Deriving
/// it makes that unrepresentable.
///
/// Neither half looks at the filesystem, so `Cached` is only as true as
/// `cached_path`. A file can go without caco deleting it — a data dir synced
/// from a machine that has it, an unmounted drive — and [`crate::presence`] is
/// what notices: it runs at startup and before every launch, and clears the
/// path of a file that has gone.
///
/// Answers a different question from [`Retrievability`]: this one changes
/// every time a file lands in or leaves the cache, while retrievability is a
/// fact about where the WAD came from. Note that `Downloadable` only promises
/// there is somewhere to try, not that fetching it will work — see
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

    /// Classify from what decides it. An empty string counts as absent — blank
    /// text fields arrive from the GUI as `Some("")`.
    pub fn derive(cached_path: Option<&str>, source_url: Option<&str>, mirrored: bool) -> Self {
        let present = |v: Option<&str>| v.is_some_and(|s| !s.is_empty());
        if present(cached_path) {
            Availability::Cached
        } else if present(source_url) || mirrored {
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
            Availability::Downloadable => concat!(
                "(COALESCE(wads.cached_path, '') = '' AND (COALESCE(wads.source_url, '') <> '' ",
                "OR EXISTS (",
                mirror_file_matches!(),
                ")))"
            ),
            Availability::Unavailable => concat!(
                "(COALESCE(wads.cached_path, '') = '' AND COALESCE(wads.source_url, '') = '' ",
                "AND NOT EXISTS (",
                mirror_file_matches!(),
                "))"
            ),
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

/// The SQL spelling of [`Retrievability::Automatic`]. A macro so that the
/// predicates built on it can be composed at compile time.
macro_rules! automatic_sql {
    () => {
        concat!(
            "(wads.source_type = 'idgames' OR COALESCE(wads.idgames_id, '') <> '' OR EXISTS (",
            mirror_file_matches!(),
            "))"
        )
    };
}

/// Whether caco can fetch a WAD's file again on its own.
///
/// Deliberately derived from source metadata and what the mirrors hold rather
/// than stored, and never from where the file currently sits on disk. Placement is a *consequence*
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
    /// Re-downloadable from idgames, which is archival and stable, or from a
    /// mirror the user configured, which holds what they chose to keep.
    Automatic,
    /// Only obtainable by hand. `source_url` may still be set — a Doomworld
    /// thread or a one-off file host — but those links rot, sit behind a WAF,
    /// or point at storage that has since been emptied, so they are not
    /// something caco can promise to re-fetch.
    Manual,
}

impl Retrievability {
    pub const ALL: &[Retrievability] = &[Retrievability::Automatic, Retrievability::Manual];

    /// SQL predicate matching [`Retrievability::Automatic`] rows.
    ///
    /// Must stay in lockstep with [`Retrievability::derive`];
    /// `test_retrievability_sql_matches_rust` asserts they agree.
    pub const AUTOMATIC_SQL: &str = automatic_sql!();

    /// SQL predicate matching WADs that are `Manual` *and* have no local copy
    /// — the set `WadRecord::is_lost` describes. Not a third variant: it is an
    /// intersection with [`Availability`], exposed because "what can I no
    /// longer play" is the question worth one click.
    pub const LOST_SQL: &str = concat!(
        "(NOT ",
        automatic_sql!(),
        " AND COALESCE(wads.cached_path, '') = '')"
    );

    /// Classify from what decides it.
    pub fn derive(source_type: SourceType, idgames_id: Option<&str>, mirrored: bool) -> Self {
        if source_type == SourceType::Idgames
            || idgames_id.is_some_and(|id| !id.is_empty())
            || mirrored
        {
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

/// What every query [`WadRecord::from_row`] reads has to select: the row, and
/// whether a mirror holds its file. A required column, so that a query forgetting
/// it fails at once rather than reading every WAD as held by nothing.
pub const WAD_COLUMNS: &str = concat!("wads.*, EXISTS (", mirror_file_matches!(), ") AS mirrored");

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
    /// The WAD's file, as of the last time caco looked. Kept honest by
    /// [`crate::presence`]; see [`Availability`].
    pub cached_path: Option<String>,
    /// Where the file was the last time caco found it, set only while
    /// `cached_path` is empty: the answer to "where did it go" for a file that
    /// is on another machine or a drive that is not mounted. History, not a
    /// copy of anything derivable — `crate::presence` is its only writer, and
    /// setting `cached_path` through [`crate::db::WadUpdate`] clears it.
    pub last_known_path: Option<String>,
    /// What the file is, as of the last time caco hashed it. Kept when the
    /// file goes, since it is what a download is checked against and what
    /// names the file to get it back. `crate::identity` is its only writer.
    pub identity: Option<FileIdentity>,
    /// The file's modification time when `identity` was taken. With the size,
    /// the stamp that says whether the file has been replaced since.
    pub file_mtime: Option<i64>,
    /// Whether a mirror holds this WAD's file, as of the last time each index was
    /// read. Not a column of `wads`: see [`crate::db::mirrors`].
    #[serde(default)]
    pub mirrored: bool,
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
        Retrievability::derive(self.source_type, self.idgames_id.as_deref(), self.mirrored)
    }

    /// Whether idgames can give this WAD's file back, whatever a mirror holds.
    pub fn on_idgames(&self) -> bool {
        Retrievability::derive(self.source_type, self.idgames_id.as_deref(), false).is_automatic()
    }

    /// Whether this WAD's file is on the machine, and if not whether there is
    /// a URL to try.
    pub fn availability(&self) -> Availability {
        Availability::derive(
            self.cached_path.as_deref(),
            self.source_url.as_deref(),
            self.mirrored,
        )
    }

    /// Manual-only with no local copy: caco cannot fetch this WAD, and the
    /// library is listing something it cannot actually launch.
    ///
    /// The intersection of the two axes rather than a state of its own — a
    /// `Manual` WAD becomes lost and unlost purely by its file coming and
    /// going, which is what `Availability` already tracks. When it went, as
    /// opposed to never having had one, `last_known_path` says where it was.
    pub fn is_lost(&self) -> bool {
        self.retrievability() == Retrievability::Manual
            && self.availability() != Availability::Cached
    }

    /// Build a `WadRecord` from a `rusqlite::Row`.
    ///
    /// Expects [`WAD_COLUMNS`] to have been selected. Tags must be attached
    /// separately via `attach_tags`.
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
            last_known_path: row.get("last_known_path")?,
            identity: identity_from_row(row)?,
            file_mtime: row.get("file_mtime")?,
            mirrored: row.get("mirrored")?,
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

/// The identity columns, which `crate::identity` writes together or not at all.
fn identity_from_row(row: &rusqlite::Row) -> rusqlite::Result<Option<FileIdentity>> {
    let size: Option<i64> = row.get("file_size")?;
    let sha256: Option<String> = row.get("file_sha256")?;
    let sha1: Option<String> = row.get("file_sha1")?;
    let content_sha256: Option<String> = row.get("content_sha256")?;
    Ok(match (size, sha256, sha1, content_sha256) {
        (Some(size), Some(sha256), Some(sha1), Some(content_sha256)) => Some(FileIdentity {
            file: Hashes {
                size: u64::try_from(size).unwrap_or_default(),
                sha256,
                sha1,
            },
            content_sha256,
        }),
        _ => None,
    })
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
            Retrievability::derive(SourceType::Idgames, None, false),
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
                Retrievability::derive(src, Some("12345"), false),
                Retrievability::Automatic,
                "{src} with an idgames id should be automatic"
            );
            assert_eq!(
                Retrievability::derive(src, None, false),
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
            Retrievability::derive(SourceType::Doomworld, Some(""), false),
            Retrievability::Manual
        );
    }

    /// A copy on the user's own mirror makes a manual WAD fetchable, whatever it
    /// was imported from.
    #[test]
    fn test_retrievability_a_mirror_promotes_any_source() {
        for src in [
            SourceType::Doomwiki,
            SourceType::Doomworld,
            SourceType::Url,
            SourceType::Local,
        ] {
            assert_eq!(
                Retrievability::derive(src, None, true),
                Retrievability::Automatic,
                "{src} on a mirror should be automatic"
            );
        }
    }

    /// `Downloadable` means there is somewhere to try, which a mirror is, whether
    /// or not there is a URL.
    #[test]
    fn test_availability_a_mirror_is_somewhere_to_try() {
        assert_eq!(
            Availability::derive(None, None, true),
            Availability::Downloadable
        );
        assert_eq!(
            Availability::derive(Some(""), Some(""), true),
            Availability::Downloadable
        );
        assert_eq!(
            Availability::derive(None, None, false),
            Availability::Unavailable
        );
        assert_eq!(
            Availability::derive(Some("/keep/x.wad"), None, true),
            Availability::Cached
        );
    }
}
