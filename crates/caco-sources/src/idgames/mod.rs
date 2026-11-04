//! The idgames archive.
//!
//! Its files have always come from mirrors, which are not behind anything. What is in
//! them came from `www.doomworld.com`, which is behind Cloudflare, and shares a host with
//! paths that answer with a challenge. So caco keeps its own index of the archive
//! ([`index`], kept up to date by [`sync`]), built from what the mirrors publish, and asks
//! the API only for what it does not know yet: [`archive`] is where that is decided, and
//! nothing outside this module has to know there are two places to ask.

pub mod archive;
pub mod client;
pub mod index;
pub mod listing;
pub mod models;
pub mod sync;
#[cfg(test)]
pub(crate) mod test_archive;
pub mod textfile;

pub use archive::{Archive, ArchiveSearch, IdgamesRef};
pub use client::{IdgamesClient, MIRRORS};
pub use index::{Index, IndexStatus};
pub use models::{FileEntry, Review};

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

/// The public mirrors as mirrors with idgames' layout, which is all they are, starting
/// with the one in `first`: `download_mirror` in the config. Named by address, for when
/// one has to be blamed for something.
pub fn public_mirrors(first: i64) -> Vec<caco_core::config::MirrorConfig> {
    let first = first.max(0) as usize % MIRRORS.len();
    (0..MIRRORS.len())
        .map(|offset| MIRRORS[(first + offset) % MIRRORS.len()])
        .map(|url| caco_core::config::MirrorConfig {
            name: url.to_string(),
            url: Some(url.to_string()),
            layout: Some(caco_core::config::MirrorLayout::IDGAMES.to_string()),
            ..Default::default()
        })
        .collect()
}

/// Where a file is in the archive, from a WAD's `source_url` and `filename`, for a WAD the
/// index cannot place by its id.
///
/// `source_url` looks like `https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/sunlust`:
/// the last part is a slug for the upload, and what comes before it is the directory.
pub fn archive_path_from_url(source_url: &str, filename: &str) -> Option<String> {
    if filename.is_empty() {
        return None;
    }
    let (_, after) = source_url.split_once("/idgames/")?;
    if after.contains('?') {
        return None;
    }
    let (dir, _slug) = after.trim_end_matches('/').rsplit_once('/')?;
    Some(format!("{dir}/{filename}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_public_mirrors_start_from_the_one_picked() {
        let names = |first| -> Vec<String> {
            public_mirrors(first)
                .into_iter()
                .map(|mirror| mirror.name)
                .collect()
        };
        assert_eq!(names(0)[0], MIRRORS[0]);
        assert_eq!(names(1)[0], MIRRORS[1]);
        assert_eq!(names(1).last().unwrap(), MIRRORS[0]);
        assert_eq!(names(0).len(), MIRRORS.len());
        // Out of range wraps round, as a setting from when there were more of them does.
        assert_eq!(names(MIRRORS.len() as i64)[0], MIRRORS[0]);
        assert_eq!(names(-1)[0], MIRRORS[0]);
        assert!(
            public_mirrors(0)
                .iter()
                .all(|mirror| mirror.is_idgames_copy())
        );
    }

    #[test]
    fn test_a_files_place_in_the_archive_comes_from_its_page_and_its_name() {
        assert_eq!(
            archive_path_from_url(
                "https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/sunlust",
                "sunlust.zip"
            )
            .as_deref(),
            Some("levels/doom2/Ports/megawads/sunlust.zip")
        );
    }

    #[test]
    fn test_a_page_that_does_not_say_where_it_is_places_nothing() {
        let by_id = "https://www.doomworld.com/idgames/?id=18184";
        assert_eq!(archive_path_from_url(by_id, "sunlust.zip"), None);
        let forum = "https://www.doomworld.com/forum/topic/1/";
        assert_eq!(archive_path_from_url(forum, "sunlust.zip"), None);
        let at_root = "https://www.doomworld.com/idgames/sunlust";
        assert_eq!(archive_path_from_url(at_root, "sunlust.zip"), None);
        let page = "https://www.doomworld.com/idgames/levels/doom2/sunlust";
        assert_eq!(archive_path_from_url(page, ""), None);
    }

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
