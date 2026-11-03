use std::io;
use std::path::PathBuf;

/// Central error type for caco-core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("WAD not found: id {0}")]
    WadNotFound(i64),

    #[error("invalid field: {0}")]
    InvalidField(String),

    #[error("invalid fields: {}", .0.join(", "))]
    InvalidFields(Vec<String>),

    #[error("invalid status: {0}")]
    InvalidStatus(String),

    #[error("invalid availability: {0}")]
    InvalidAvailability(String),

    #[error("invalid retrievability: {0}")]
    InvalidRetrievability(String),

    #[error("invalid source type: {0}")]
    InvalidSourceType(String),

    #[error("duplicate WAD: {0}")]
    DuplicateWad(String),

    #[error("file not found: {0}")]
    FileNotFound(String),

    /// A WAD's file is not on this machine. Its own variant because callers
    /// act on it — the GUI offers to link a copy — where any other failure to
    /// launch is only reported.
    #[error("'{title}' is not on this machine{}", last_seen_suffix(.last_seen))]
    WadFileMissing {
        title: String,
        /// Where the file was the last time caco found it, if it ever had one.
        last_seen: Option<PathBuf>,
    },

    /// A WAD's file plays differently from the one it was last played with.
    /// Its own variant because the GUI asks before launching: saves and demos
    /// made with one version of a WAD often do not work with another.
    #[error("'{title}' has changed since it was last played")]
    WadFileChanged {
        title: String,
        change: crate::identity::ContentChange,
    },

    #[error("invalid WAD format: {0}")]
    InvalidWadFormat(String),

    #[error("migration failed: {0}")]
    MigrationFailed(String),

    #[error("profile '{name}' not found for '{sourceport}'")]
    ProfileNotFound { sourceport: String, name: String },

    #[error("profile '{name}' already exists for '{sourceport}'")]
    ProfileExists { sourceport: String, name: String },

    #[error("companion file not found: {0}")]
    CompanionNotFound(String),

    #[error("no sourceport specified and no default configured")]
    NoSourceport,

    #[error("no build recipe for sourceport '{0}'")]
    PortNotFound(String),

    #[error("required tool not found on PATH: {0}")]
    MissingTool(String),

    #[error("{step} failed for '{sourceport}': {detail}")]
    PortBuild {
        sourceport: String,
        step: &'static str,
        detail: String,
    },

    #[error("build of '{0}' was cancelled")]
    PortBuildCancelled(String),
}

fn last_seen_suffix(last_seen: &Option<PathBuf>) -> String {
    last_seen
        .as_ref()
        .map(|path| format!(" (last seen at {})", path.display()))
        .unwrap_or_default()
}

/// Convenience alias used throughout caco-core.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wad_file_missing_says_where_it_was_last_seen() {
        let seen = Error::WadFileMissing {
            title: "My House".to_string(),
            last_seen: Some(PathBuf::from("/mnt/nas/myhouse.pk3")),
        };
        assert_eq!(
            seen.to_string(),
            "'My House' is not on this machine (last seen at /mnt/nas/myhouse.pk3)"
        );

        let never = Error::WadFileMissing {
            title: "My House".to_string(),
            last_seen: None,
        };
        assert_eq!(never.to_string(), "'My House' is not on this machine");
    }
}
