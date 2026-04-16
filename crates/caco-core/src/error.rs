use std::io;

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

    #[error("invalid source type: {0}")]
    InvalidSourceType(String),

    #[error("duplicate WAD: {0}")]
    DuplicateWad(String),

    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("invalid WAD format: {0}")]
    InvalidWadFormat(String),

    #[error("migration failed: {0}")]
    MigrationFailed(String),
}

/// Convenience alias used throughout caco-core.
pub type Result<T> = std::result::Result<T, Error>;
