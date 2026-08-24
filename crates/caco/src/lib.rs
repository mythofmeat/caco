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

/// The version this binary reports, derived from `git describe --tags` at
/// compile time by `build.rs`. Carries no leading `v` — call sites that want
/// one add it — and reads `4.0.5-9-g2243dac` on an untagged commit.
pub const VERSION: &str = env!("CACO_VERSION");
