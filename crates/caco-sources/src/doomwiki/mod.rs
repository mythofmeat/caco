pub mod client;
pub mod models;
pub mod parser;

pub use client::DoomwikiClient;
pub use models::WikiEntry;
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

/// A Doom Wiki page title in one spelling, so two can be compared.
///
/// MediaWiki treats underscores as spaces and ignores the case of a title's first
/// letter, and which one you get depends on where the title came from: a URL, a
/// `[[link]]`, or a curated `doomwiki =` value.
pub fn canonical_page(page: &str) -> String {
    let spaced = page.replace('_', " ");
    let mut chars = spaced.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_titles_compare_however_they_were_spelled() {
        for (a, b) in [
            ("TNT:_Overcharged", "TNT: Overcharged"),
            ("lilith", "Lilith"),
            (" Gossip ", "Gossip"),
            (
                "Realm_of_Chaos_2:_The_Revenge_of_'96!",
                "Realm of Chaos 2: The Revenge of '96!",
            ),
        ] {
            assert_eq!(canonical_page(a), canonical_page(b));
        }
        // Only the first letter is folded: MediaWiki keeps the rest.
        assert_ne!(canonical_page("iD Software"), canonical_page("id Software"));
    }

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
