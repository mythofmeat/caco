pub mod client;
pub mod models;
pub mod parser;

pub use caco_core::source_url::{canonical_page, extract_doomwiki_title_from_url};
pub use client::DoomwikiClient;
pub use models::WikiEntry;
pub use parser::WikitextParser;
