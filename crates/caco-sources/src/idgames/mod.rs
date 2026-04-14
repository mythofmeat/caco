pub mod client;
pub mod models;

pub use client::{IdgamesClient, MIRRORS};
pub use models::{ApiInfo, Directory, FileEntry, Review, Vote};

/// Extract a numeric idgames ID from a `doomworld.com/idgames/...?id=N` URL.
///
/// Accepts forms such as:
/// - `https://www.doomworld.com/idgames/?id=18184`
/// - `https://www.doomworld.com/idgames/index.php?id=18184`
/// - `https://doomworld.com/idgames/?id=18184`
///
/// Returns `None` if the URL does not reference the idgames section or
/// the `id=` query parameter is missing / non-numeric.
pub fn extract_idgames_id_from_url(url: &str) -> Option<i64> {
    if !url.contains("doomworld.com/idgames") {
        return None;
    }
    let query = url.split_once('?').map(|(_, q)| q)?;
    for param in query.split('&') {
        if let Some(value) = param.strip_prefix("id=") {
            return value.parse::<i64>().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_id_from_basic_url() {
        assert_eq!(
            extract_idgames_id_from_url("https://www.doomworld.com/idgames/?id=18184"),
            Some(18184),
        );
    }

    #[test]
    fn extracts_id_from_index_php_url() {
        assert_eq!(
            extract_idgames_id_from_url(
                "https://www.doomworld.com/idgames/index.php?id=18184"
            ),
            Some(18184),
        );
    }

    #[test]
    fn extracts_id_without_www() {
        assert_eq!(
            extract_idgames_id_from_url("https://doomworld.com/idgames/?id=18184"),
            Some(18184),
        );
    }

    #[test]
    fn extracts_id_with_extra_query_params() {
        assert_eq!(
            extract_idgames_id_from_url(
                "https://www.doomworld.com/idgames/?foo=bar&id=42&baz=qux"
            ),
            Some(42),
        );
    }

    #[test]
    fn returns_none_for_forum_url() {
        assert_eq!(
            extract_idgames_id_from_url(
                "https://www.doomworld.com/forum/topic/123-something/"
            ),
            None,
        );
    }

    #[test]
    fn returns_none_for_non_doomworld_url() {
        assert_eq!(
            extract_idgames_id_from_url("https://example.com/idgames/?id=1"),
            None,
        );
    }

    #[test]
    fn returns_none_for_missing_id_param() {
        assert_eq!(
            extract_idgames_id_from_url("https://www.doomworld.com/idgames/"),
            None,
        );
    }

    #[test]
    fn returns_none_for_non_numeric_id() {
        assert_eq!(
            extract_idgames_id_from_url("https://www.doomworld.com/idgames/?id=abc"),
            None,
        );
    }
}
