pub mod client;
pub mod models;
pub mod parser;

use std::sync::LazyLock;

use regex::Regex;

pub use client::DoomworldClient;
pub use models::ForumThread;
pub use parser::DoomworldParser;

static TOPIC_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"doomworld\.com/(?:forum/topic|vb/thread)/(\d+)").unwrap());

/// Extract the forum topic id from a Doomworld thread URL.
///
/// Both forms occur in real libraries, the current one and the one the Doom
/// Wiki's `{{dwforums}}` template still links, and the number is what they share:
/// - `https://www.doomworld.com/forum/topic/152740-udmf-infernal-odyssey-12-maps/`
/// - `https://www.doomworld.com/vb/thread/151858`
pub fn extract_topic_id_from_url(url: &str) -> Option<i64> {
    TOPIC_ID.captures(url)?.get(1)?.as_str().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_topic_id_from_either_url_form() {
        for (url, id) in [
            (
                "https://www.doomworld.com/forum/topic/152740-udmf-infernal-odyssey-12-maps/",
                152740,
            ),
            ("https://www.doomworld.com/forum/topic/134292", 134292),
            ("https://www.doomworld.com/vb/thread/151858", 151858),
            ("https://doomworld.com/vb/thread/151858/", 151858),
            (
                "https://www.doomworld.com/forum/topic/134292-myhousewad/?page=5",
                134292,
            ),
        ] {
            assert_eq!(extract_topic_id_from_url(url), Some(id), "{url}");
        }
    }

    #[test]
    fn other_urls_have_no_topic_id() {
        for url in [
            "https://www.doomworld.com/idgames/levels/doom2/Ports/s-u/tsdp2",
            "https://www.doomworld.com/profile/1723-scuba-steve/",
            "https://example.com/forum/topic/123-something/",
            "https://doomwiki.org/wiki/Gossip",
        ] {
            assert_eq!(extract_topic_id_from_url(url), None, "{url}");
        }
    }
}
