pub mod app;
pub mod dialogs;
pub mod filter_query;
pub mod import;
pub mod message;
pub mod panels;
pub mod persist;
pub mod relative_time;
pub mod state;
pub mod theme;
pub mod thumbnails;
pub mod wiki_scraper;
pub mod workers;

/// The version this binary reports: the workspace `version` in `Cargo.toml`.
/// Carries no leading `v` — call sites that want one add it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
