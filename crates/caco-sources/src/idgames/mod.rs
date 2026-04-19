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

/// Extract the idgames archive file path from a path-style mirror URL.
///
/// The Doom Wiki's `{{ig|file=...}}` template and doomworld's own idgames
/// frontend both render paths like `https://www.doomworld.com/idgames/<path>`.
/// These URLs don't carry a numeric `?id=N`, but the path can be passed to
/// the idgames API as the `file` parameter to look up the entry.
///
/// The idgames API's `file=` lookup requires an actual filename, not a
/// directory/slug path — so if the final segment has no extension we append
/// `.zip`, which is the universal archive format used by the idgames archive.
///
/// Accepts forms such as:
/// - `https://www.doomworld.com/idgames/levels/doom2/megawads/scythe.zip`
///   → `levels/doom2/megawads/scythe.zip`
/// - `https://www.doomworld.com/idgames/levels/doom2/Ports/v-z/witchinghour`
///   → `levels/doom2/Ports/v-z/witchinghour.zip`
///
/// Returns `None` for URLs that already carry a query string (those should
/// use [`extract_idgames_id_from_url`] instead) or that don't sit under the
/// `/idgames/` archive prefix.
pub fn extract_idgames_file_path_from_url(url: &str) -> Option<String> {
    if !url.contains("doomworld.com/idgames") {
        return None;
    }
    if url.contains('?') {
        return None;
    }
    let after = url.split("/idgames/").nth(1)?.trim_matches('/');
    if after.is_empty() {
        return None;
    }
    let last_segment = after.rsplit('/').next().unwrap_or("");
    if last_segment.contains('.') {
        Some(after.to_string())
    } else {
        Some(format!("{after}.zip"))
    }
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
            extract_idgames_id_from_url("https://www.doomworld.com/idgames/index.php?id=18184"),
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
            extract_idgames_id_from_url("https://www.doomworld.com/idgames/?foo=bar&id=42&baz=qux"),
            Some(42),
        );
    }

    #[test]
    fn returns_none_for_forum_url() {
        assert_eq!(
            extract_idgames_id_from_url("https://www.doomworld.com/forum/topic/123-something/"),
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

    #[test]
    fn extracts_file_path_from_zip_url() {
        assert_eq!(
            extract_idgames_file_path_from_url(
                "https://www.doomworld.com/idgames/levels/doom2/megawads/scythe.zip"
            ),
            Some("levels/doom2/megawads/scythe.zip".to_string()),
        );
    }

    #[test]
    fn extracts_file_path_from_slug_url() {
        // Slug URLs from doomworld's frontend have no extension; the idgames
        // API's `file=` param requires an actual filename, so we append
        // `.zip` (the universal archive format).
        assert_eq!(
            extract_idgames_file_path_from_url(
                "https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/sunlust"
            ),
            Some("levels/doom2/Ports/megawads/sunlust.zip".to_string()),
        );
    }

    #[test]
    fn extracts_file_path_appends_zip_for_slug_with_dotted_dir() {
        // Dots may appear in directory names (`v-z` has none, but `Level-1.2`
        // would) — we only look at the final segment to decide whether to
        // append `.zip`.
        assert_eq!(
            extract_idgames_file_path_from_url(
                "https://www.doomworld.com/idgames/levels/doom2/Ports/v-z/witchinghour"
            ),
            Some("levels/doom2/Ports/v-z/witchinghour.zip".to_string()),
        );
    }

    #[test]
    fn extracts_file_path_without_www() {
        assert_eq!(
            extract_idgames_file_path_from_url(
                "https://doomworld.com/idgames/levels/doom2/megawads/scythe.zip"
            ),
            Some("levels/doom2/megawads/scythe.zip".to_string()),
        );
    }

    #[test]
    fn file_path_returns_none_for_query_url() {
        assert_eq!(
            extract_idgames_file_path_from_url("https://www.doomworld.com/idgames/?id=18184"),
            None,
        );
    }

    #[test]
    fn file_path_returns_none_for_non_idgames_url() {
        assert_eq!(
            extract_idgames_file_path_from_url("https://example.com/idgames/foo.zip"),
            None,
        );
    }

    #[test]
    fn file_path_returns_none_for_archive_root() {
        assert_eq!(
            extract_idgames_file_path_from_url("https://www.doomworld.com/idgames/"),
            None,
        );
    }
}
