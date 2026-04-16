pub mod client;
pub mod models;
pub mod parser;

pub use client::DoomwikiClient;
pub use models::{SearchResult, WikiEntry};
pub use parser::WikitextParser;

/// Extract the page title from a Doom Wiki article URL.
///
/// Accepts forms such as:
/// - `https://doomwiki.org/wiki/Sunder`
/// - `https://www.doomwiki.org/wiki/DBP31:_Santa%27s_Outback_Bender`
///
/// The returned title is percent-decoded and has underscores converted to
/// spaces (MediaWiki's canonical title form). Returns `None` if the URL
/// does not look like a Doom Wiki `/wiki/<title>` URL.
pub fn extract_doomwiki_title_from_url(url: &str) -> Option<String> {
    if !url.contains("doomwiki.org") {
        return None;
    }
    let after = url.split("/wiki/").nth(1)?;
    let raw = after.split(['#', '?']).next().unwrap_or("");
    if raw.is_empty() {
        return None;
    }
    let decoded = urlencoding::decode(raw).ok()?;
    let normalized = decoded.replace('_', " ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_title() {
        assert_eq!(
            extract_doomwiki_title_from_url("https://doomwiki.org/wiki/Sunder"),
            Some("Sunder".to_string()),
        );
    }

    #[test]
    fn extracts_title_with_underscores_and_percent_encoding() {
        assert_eq!(
            extract_doomwiki_title_from_url(
                "https://doomwiki.org/wiki/DBP31:_Santa%27s_Outback_Bender"
            ),
            Some("DBP31: Santa's Outback Bender".to_string()),
        );
    }

    #[test]
    fn extracts_title_from_www_subdomain() {
        assert_eq!(
            extract_doomwiki_title_from_url("https://www.doomwiki.org/wiki/Scythe"),
            Some("Scythe".to_string()),
        );
    }

    #[test]
    fn strips_url_fragment() {
        assert_eq!(
            extract_doomwiki_title_from_url("https://doomwiki.org/wiki/Scythe#Reception"),
            Some("Scythe".to_string()),
        );
    }

    #[test]
    fn strips_url_query() {
        assert_eq!(
            extract_doomwiki_title_from_url("https://doomwiki.org/wiki/Scythe?action=history"),
            Some("Scythe".to_string()),
        );
    }

    #[test]
    fn returns_none_for_non_doomwiki_url() {
        assert_eq!(
            extract_doomwiki_title_from_url("https://example.com/wiki/Sunder"),
            None,
        );
    }

    #[test]
    fn returns_none_for_api_url() {
        assert_eq!(
            extract_doomwiki_title_from_url("https://doomwiki.org/w/api.php?action=query"),
            None,
        );
    }

    #[test]
    fn returns_none_for_empty_title() {
        assert_eq!(
            extract_doomwiki_title_from_url("https://doomwiki.org/wiki/"),
            None,
        );
    }
}
