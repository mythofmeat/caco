//! Getting a WAD's file through the user's own browser, which passes the challenges
//! caco's client cannot.
//!
//! What is here is what can be decided without a thread or a clock: which page to open
//! for a WAD, what a file in the Downloads folder has to look like to be worth asking
//! about, and when one has finished arriving. The GUI's `get` module runs [`Arrivals`]
//! on a worker and asks the questions.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::db::{CacowardRecord, WadRecord};
use crate::source_url::{doomworld_topic_url, idgames_id_url};

/// What a WAD's file arrives as, by extension. A browser's name for a download it has
/// not finished (`.part`, `.crdownload`, `.download`) is not on this list, so a file
/// is only ever seen under its final name.
pub const WAD_SHAPED: &[&str] = &["wad", "pk3", "pk7", "zip", "7z"];

/// Whether a file with this name could be a WAD's. A hidden file is some tool's
/// temporary, whatever it ends in.
pub fn is_wad_shaped(name: &OsStr) -> bool {
    let name = Path::new(name);
    let hidden = name.to_string_lossy().starts_with('.');
    !hidden && has_wad_shaped_extension(name)
}

fn has_wad_shaped_extension(name: &Path) -> bool {
    name.extension().and_then(OsStr::to_str).is_some_and(|ext| {
        WAD_SHAPED
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

// ---------------------------------------------------------------------------
// Which page to open
// ---------------------------------------------------------------------------

/// Where a person can get this WAD's file, best first: a link straight to a file, which
/// the browser starts downloading at once, then the share pages the source named, then
/// the page the WAD came from. Anything further down still matters, because a share
/// link can have rotted since it was scraped.
pub fn pages_for_wad(wad: &WadRecord) -> Vec<String> {
    let scraped: Vec<String> = wad
        .download_urls
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default();
    let (files, others): (Vec<String>, Vec<String>) =
        scraped.into_iter().partition(|url| links_to_a_file(url));
    web_pages(
        files
            .into_iter()
            .chain(others)
            .chain(wad.source_url.clone()),
    )
}

/// Where a person can get a Cacoward entry's file, best first. idgames, because that
/// page is one click from the file. Then the forum thread, which is where a
/// Doomworld-only release's download links are, then the wiki, which knows about the
/// WAD but rarely where to get it. Last the year's own page, which every entry has.
pub fn pages_for_cacoward(record: &CacowardRecord, year_page: Option<&str>) -> Vec<String> {
    web_pages(
        [
            record.idgames_id.map(idgames_id_url),
            record.doomworld_id.map(doomworld_topic_url),
            record.doomwiki_url(),
            year_page.map(str::to_string),
        ]
        .into_iter()
        .flatten(),
    )
}

/// Whether a URL's path ends in a file a WAD could be, rather than in a page about one.
fn links_to_a_file(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/')
        .next()
        .is_some_and(|name| has_wad_shaped_extension(Path::new(name)))
}

/// Only what a browser should be handed, once each. A `source_url` can be a file's path
/// on this machine, and a `download_urls` entry is whatever a forum post said.
fn web_pages(urls: impl Iterator<Item = String>) -> Vec<String> {
    let mut pages: Vec<String> = Vec::new();
    for url in urls {
        let url = url.trim();
        if is_web_url(url) && !pages.iter().any(|page| page == url) {
            pages.push(url.to_string());
        }
    }
    pages
}

fn is_web_url(url: &str) -> bool {
    let scheme = |prefix: &str| {
        url.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };
    scheme("https://") || scheme("http://")
}

/// The site a URL is on, to name it to the user: `doomworld.com`, not the whole URL.
pub fn page_host(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    host.strip_prefix("www.").unwrap_or(host)
}

// ---------------------------------------------------------------------------
// Which pending Get a file belongs to
// ---------------------------------------------------------------------------

/// Below this, two names only have a letter or two in common, which says nothing.
const NAMES_ARE_RELATED: f32 = 0.3;

/// How much a file's name looks like a name it could be expected to have, from 0 to 1,
/// with 0 meaning it says nothing either way.
///
/// Only ever breaks a tie between Gets waiting at once. The user just asked for these
/// WADs, so a poor score is no reason not to ask, and the question is what catches a
/// wrong guess.
pub fn name_likeness(filename: &str, expected: &str) -> f32 {
    let a = comparable(strip_extension(filename));
    let b = comparable(strip_extension(expected));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let likeness = dice_coefficient(&a, &b);
    if likeness < NAMES_ARE_RELATED {
        0.0
    } else {
        likeness
    }
}

/// A name without its extension if that is one a WAD's file has, which keeps the
/// `2` of `Scythe 2` and the `.5` of `Sunlust v1.5` out of it.
fn strip_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if has_wad_shaped_extension(Path::new(name)) => stem,
        _ => name,
    }
}

/// `AftercareSessions` and `Aftercare Sessions` are the same name to this.
fn comparable(name: &str) -> Vec<char> {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// How many adjacent pairs of letters two names share, as a share of those they have,
/// which goes by neither order nor length the way an edit distance does. A release's
/// file adding `_v1.2` to its title should still look like the title.
fn dice_coefficient(a: &[char], b: &[char]) -> f32 {
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let mut pairs: HashMap<(char, char), i32> = HashMap::new();
    for pair in a.windows(2) {
        *pairs.entry((pair[0], pair[1])).or_default() += 1;
    }
    let mut shared = 0;
    for pair in b.windows(2) {
        if let Some(count) = pairs.get_mut(&(pair[0], pair[1]))
            && *count > 0
        {
            *count -= 1;
            shared += 1;
        }
    }
    (2 * shared) as f32 / (a.len() + b.len() - 2) as f32
}

// ---------------------------------------------------------------------------
// Noticing a file has arrived
// ---------------------------------------------------------------------------

/// A file that has finished arriving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub path: PathBuf,
    pub size: u64,
}

/// What a file looked like, which is how to tell that it has changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// The WAD-shaped files directly in a directory.
pub type Listing = HashMap<PathBuf, Stamp>;

/// Read a directory for [`Arrivals::observe`]. Only a WAD-shaped name is looked at any
/// further, so this costs one read however full the Downloads folder is.
pub fn list_wad_shaped(dir: &Path) -> io::Result<Listing> {
    let mut listing = Listing::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        if !is_wad_shaped(&entry.file_name()) {
            continue;
        }
        // A file gone since the directory was read has not arrived, a directory called
        // `something.zip` never will, and a link to a file somewhere else is not a
        // download. Which is what this asks: unlike `fs::metadata`, it does not follow one.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_file() {
            let stamp = Stamp {
                size: meta.len(),
                modified: meta.modified().ok(),
            };
            listing.insert(entry.path(), stamp);
        }
    }
    Ok(listing)
}

/// Tells which files have arrived in a directory since it was first looked at, from
/// successive listings of it.
///
/// A file has arrived when it is new, or different from what was there, and has not
/// changed for `settle`. A browser only renames its finished download to the final
/// name, which would be enough on its own, but anything else writing there does not.
/// Firefox also holds the final name with an empty file for as long as a download
/// runs, so an empty file has not arrived, however long it has sat there.
#[derive(Debug)]
pub struct Arrivals {
    settle: Duration,
    /// What was there to begin with and what has been reported since, as it was then.
    known: Listing,
    /// Everything else, and when it was last seen to change.
    settling: HashMap<PathBuf, (Stamp, Instant)>,
}

impl Arrivals {
    /// Start from what is in the directory now, none of which has arrived.
    pub fn new(baseline: Listing, settle: Duration) -> Self {
        Self {
            settle,
            known: baseline,
            settling: HashMap::new(),
        }
    }

    /// Take a new listing, and return the files it shows to have arrived, each once.
    pub fn observe(&mut self, listing: &Listing, now: Instant) -> Vec<Arrival> {
        // A file that went and came back, under the same name, is a new one.
        self.known.retain(|path, _| listing.contains_key(path));
        self.settling.retain(|path, _| listing.contains_key(path));

        let mut arrived = Vec::new();
        for (path, stamp) in listing {
            if self.known.get(path) == Some(stamp) {
                continue;
            }
            match self.settling.get(path) {
                Some((seen, since)) if seen == stamp => {
                    if stamp.size > 0 && now.duration_since(*since) >= self.settle {
                        self.settling.remove(path);
                        self.known.insert(path.clone(), *stamp);
                        arrived.push(Arrival {
                            path: path.clone(),
                            size: stamp.size,
                        });
                    }
                }
                _ => {
                    self.settling.insert(path.clone(), (*stamp, now));
                }
            }
        }
        // A map has no order of its own, and two files landing together should not be
        // asked about in a different order each time.
        arrived.sort_by(|a, b| a.path.cmp(&b.path));
        arrived
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, NewCacoward, NewWad, SourceType, WadUpdate};

    fn shaped(name: &str) -> bool {
        is_wad_shaped(OsStr::new(name))
    }

    #[test]
    fn test_what_counts_as_wad_shaped() {
        for name in ["a.wad", "a.pk3", "a.pk7", "a.zip", "a.7z", "A.WAD", "b.Zip"] {
            assert!(shaped(name), "{name}");
        }
        for name in [
            // Not a WAD's file.
            "a.txt",
            "a.exe",
            "wad",
            "zip",
            // What browsers call a download they have not finished.
            "a.zip.part",
            "a.zip.crdownload",
            "Unconfirmed 12345.crdownload",
            "a.pk3.download",
            // Some tool's temporary.
            ".a.zip",
            ".com.google.Chrome.x1y2z3",
        ] {
            assert!(!shaped(name), "{name}");
        }
    }

    fn wad() -> (rusqlite::Connection, i64) {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        let id = db::add_wad(&conn, &NewWad::new("Road", SourceType::Doomworld)).unwrap();
        (conn, id)
    }

    fn pages(urls: Option<&str>, source_url: Option<&str>) -> Vec<String> {
        let (conn, id) = wad();
        let update = WadUpdate::new()
            .set_text("download_urls", urls.map(str::to_string))
            .set_text("source_url", source_url.map(str::to_string));
        db::update_wad(&conn, id, &update).unwrap();
        pages_for_wad(&db::get_wad(&conn, id, false).unwrap().unwrap())
    }

    const THREAD: &str = "https://www.doomworld.com/forum/topic/151858-road/";

    #[test]
    fn test_a_link_to_a_file_comes_before_a_share_page_and_the_thread() {
        let urls = r#"["https://drive.google.com/file/d/abc/view",
                       "https://example.com/files/road.zip?dl=1"]"#;
        assert_eq!(
            pages(Some(urls), Some(THREAD)),
            [
                "https://example.com/files/road.zip?dl=1",
                "https://drive.google.com/file/d/abc/view",
                THREAD,
            ]
        );
    }

    /// `.rar` is a file, but not one that would be noticed arriving, or could be played.
    #[test]
    fn test_only_a_file_a_wad_could_be_counts_as_a_link_to_one() {
        let urls = r#"["https://example.com/road.rar", "https://example.com/road.pk3"]"#;
        assert_eq!(
            pages(Some(urls), None),
            [
                "https://example.com/road.pk3",
                "https://example.com/road.rar"
            ]
        );
    }

    #[test]
    fn test_without_download_links_the_page_it_came_from_is_the_page() {
        assert_eq!(pages(None, Some(THREAD)), [THREAD]);
        assert_eq!(pages(Some("[]"), Some(THREAD)), [THREAD]);
        // What an old row could have, which should not cost it the page it does have.
        assert_eq!(pages(Some("not json"), Some(THREAD)), [THREAD]);
    }

    /// A local import's `source_url` is a path on this machine, and a forum post can say
    /// anything. Neither goes to a browser.
    #[test]
    fn test_only_web_pages_are_offered() {
        assert_eq!(
            pages(
                Some(r#"["javascript:alert(1)", "file:///etc/passwd", "-x", ""]"#),
                Some("/home/me/doom/road.wad")
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            pages(None, Some("HTTPS://Example.com/x")),
            ["HTTPS://Example.com/x"]
        );
    }

    #[test]
    fn test_a_page_named_twice_is_offered_once() {
        let urls = format!(r#"["{THREAD}", "{THREAD}"]"#);
        assert_eq!(pages(Some(&urls), Some(THREAD)), [THREAD]);
    }

    fn entry(entry: NewCacoward) -> CacowardRecord {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        let id = db::upsert_cacoward(
            &conn,
            &NewCacoward {
                key: "k".to_string(),
                year: 2025,
                category: "winner".to_string(),
                page_order: 1,
                wad_title: "Road".to_string(),
                ..entry
            },
        )
        .unwrap();
        db::get_cacoward(&conn, id).unwrap().unwrap()
    }

    const YEAR: &str = "https://www.doomworld.com/cacowards/2025/";

    #[test]
    fn test_a_cacoward_entrys_pages_go_best_first() {
        let record = entry(NewCacoward {
            idgames_id: Some(19951),
            doomworld_id: Some(151858),
            doomwiki_page: Some("Lonesome_Road".to_string()),
            ..Default::default()
        });
        assert_eq!(
            pages_for_cacoward(&record, Some(YEAR)),
            [
                "https://www.doomworld.com/idgames/?id=19951",
                "https://www.doomworld.com/forum/topic/151858/",
                "https://doomwiki.org/wiki/Lonesome_Road",
                YEAR,
            ]
        );
    }

    /// What #53 was for: an entry that is not on idgames knows which thread to open.
    #[test]
    fn test_a_doomworld_only_entry_opens_its_thread() {
        let record = entry(NewCacoward {
            doomworld_id: Some(151858),
            ..Default::default()
        });
        assert_eq!(
            pages_for_cacoward(&record, Some(YEAR)),
            ["https://www.doomworld.com/forum/topic/151858/", YEAR]
        );
    }

    #[test]
    fn test_an_entry_naming_nothing_still_has_its_year() {
        let record = entry(NewCacoward::default());
        assert_eq!(pages_for_cacoward(&record, Some(YEAR)), [YEAR]);
        assert_eq!(pages_for_cacoward(&record, None), Vec::<String>::new());
    }

    #[test]
    fn test_page_host_names_the_site() {
        assert_eq!(page_host(THREAD), "doomworld.com");
        assert_eq!(
            page_host("https://drive.google.com/file/d/x"),
            "drive.google.com"
        );
        assert_eq!(page_host("http://example.com?x=1"), "example.com");
        assert_eq!(page_host("https://example.com"), "example.com");
    }

    #[test]
    fn test_name_likeness_goes_by_the_letters_in_the_name() {
        // The issue's example, and the usual shape of a release.
        assert_eq!(
            name_likeness("AftercareSessions.wad", "Aftercare Sessions"),
            1.0
        );
        assert_eq!(name_likeness("scythe2.zip", "Scythe 2"), 1.0);
        assert_eq!(name_likeness("SCYTHE2.ZIP", "scythe2.zip"), 1.0);

        let release = name_likeness("LonesomeRoad_v1.2.pk3", "Lonesome Road");
        assert!(release > 0.5, "{release}");

        assert_eq!(name_likeness("invoice.zip", "Lonesome Road"), 0.0);
    }

    /// `attachment` and `Lonesome Road` have `me` in common, and that must not outrank
    /// whichever Get the user pressed last.
    #[test]
    fn test_a_stray_pair_of_letters_says_nothing() {
        assert_eq!(name_likeness("attachment.7z", "Lonesome Road"), 0.0);
    }

    #[test]
    fn test_name_likeness_of_nothing_is_nothing() {
        assert_eq!(name_likeness("", "Road"), 0.0);
        assert_eq!(name_likeness("road.zip", ""), 0.0);
        assert_eq!(name_likeness(".zip", "!!!"), 0.0);
        assert_eq!(name_likeness("a.zip", "ab"), 0.0);
    }

    /// Only the extension goes, or `Sunlust v1.5` would lose its version to it.
    #[test]
    fn test_name_likeness_keeps_a_dot_that_is_not_an_extension() {
        assert_eq!(name_likeness("sunlust1.5.zip", "Sunlust 1.5"), 1.0);
    }

    // -- Arrivals --

    const SETTLE: Duration = Duration::from_secs(2);

    fn stamp(size: u64) -> Stamp {
        Stamp {
            size,
            modified: None,
        }
    }

    fn listing(files: &[(&str, u64)]) -> Listing {
        files
            .iter()
            .map(|(name, size)| (PathBuf::from(name), stamp(*size)))
            .collect()
    }

    /// Feeds listings into an [`Arrivals`] at the times given.
    struct Clock {
        arrivals: Arrivals,
        start: Instant,
    }

    impl Clock {
        fn new(baseline: &[(&str, u64)]) -> Self {
            Self {
                arrivals: Arrivals::new(listing(baseline), SETTLE),
                start: Instant::now(),
            }
        }

        fn at(&mut self, seconds: u64, files: &[(&str, u64)]) -> Vec<String> {
            let now = self.start + Duration::from_secs(seconds);
            self.arrivals
                .observe(&listing(files), now)
                .into_iter()
                .map(|arrival| arrival.path.display().to_string())
                .collect()
        }
    }

    #[test]
    fn test_a_new_file_arrives_once_it_has_held_still() {
        let mut clock = Clock::new(&[]);
        assert_eq!(clock.at(0, &[("road.zip", 10)]), Vec::<String>::new());
        assert_eq!(clock.at(1, &[("road.zip", 10)]), Vec::<String>::new());
        assert_eq!(clock.at(2, &[("road.zip", 10)]), ["road.zip"]);
    }

    #[test]
    fn test_an_arrival_is_only_reported_once() {
        let mut clock = Clock::new(&[]);
        clock.at(0, &[("road.zip", 10)]);
        assert_eq!(clock.at(2, &[("road.zip", 10)]), ["road.zip"]);
        assert_eq!(clock.at(3, &[("road.zip", 10)]), Vec::<String>::new());
        assert_eq!(clock.at(30, &[("road.zip", 10)]), Vec::<String>::new());
    }

    #[test]
    fn test_what_was_already_there_has_not_arrived() {
        let mut clock = Clock::new(&[("old.zip", 10)]);
        assert_eq!(clock.at(0, &[("old.zip", 10)]), Vec::<String>::new());
        assert_eq!(clock.at(30, &[("old.zip", 10)]), Vec::<String>::new());
    }

    #[test]
    fn test_a_file_still_growing_has_not_arrived() {
        let mut clock = Clock::new(&[]);
        clock.at(0, &[("road.zip", 10)]);
        assert_eq!(clock.at(2, &[("road.zip", 20)]), Vec::<String>::new());
        assert_eq!(clock.at(4, &[("road.zip", 30)]), Vec::<String>::new());
        assert_eq!(clock.at(5, &[("road.zip", 30)]), Vec::<String>::new());
        assert_eq!(clock.at(6, &[("road.zip", 30)]), ["road.zip"]);
    }

    /// Firefox holds the final name with an empty file while its `.part` grows, and
    /// renames over it at the end, so the empty file has to wait however long that is.
    #[test]
    fn test_firefoxs_empty_placeholder_has_not_arrived() {
        let mut clock = Clock::new(&[]);
        clock.at(0, &[("road.zip", 0)]);
        assert_eq!(clock.at(600, &[("road.zip", 0)]), Vec::<String>::new());
        assert_eq!(clock.at(601, &[("road.zip", 90)]), Vec::<String>::new());
        assert_eq!(clock.at(603, &[("road.zip", 90)]), ["road.zip"]);
    }

    /// A browser told to replace a file writes the same name again.
    #[test]
    fn test_a_file_replaced_by_a_new_one_has_arrived() {
        let mut clock = Clock::new(&[("road.zip", 10)]);
        clock.at(0, &[("road.zip", 11)]);
        assert_eq!(clock.at(2, &[("road.zip", 11)]), ["road.zip"]);
    }

    #[test]
    fn test_a_file_that_goes_before_it_settles_has_not_arrived() {
        let mut clock = Clock::new(&[]);
        clock.at(0, &[("road.zip", 10)]);
        assert_eq!(clock.at(1, &[]), Vec::<String>::new());
        assert_eq!(clock.at(5, &[]), Vec::<String>::new());
    }

    /// Linking moves the file away, and a later download can have the same name.
    #[test]
    fn test_a_file_that_went_and_came_back_is_a_new_one() {
        let mut clock = Clock::new(&[("road.zip", 10)]);
        clock.at(0, &[]);
        clock.at(1, &[("road.zip", 10)]);
        assert_eq!(clock.at(3, &[("road.zip", 10)]), ["road.zip"]);
    }

    #[test]
    fn test_files_arriving_together_come_in_a_steady_order() {
        let mut clock = Clock::new(&[]);
        let both = [("b.zip", 1), ("a.zip", 1), ("c.wad", 1)];
        clock.at(0, &both);
        assert_eq!(clock.at(2, &both), ["a.zip", "b.zip", "c.wad"]);
    }

    #[test]
    fn test_listing_a_directory_finds_wad_shaped_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| fs::write(dir.path().join(name), bytes).unwrap();
        write("road.zip", b"12345");
        write("ROAD2.PK3", b"123");
        write("road3.zip.part", b"123");
        write("notes.txt", b"123");
        write(".hidden.wad", b"123");
        fs::create_dir(dir.path().join("folder.zip")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("notes.txt"), dir.path().join("link.wad"))
            .unwrap();

        let found = list_wad_shaped(dir.path()).unwrap();

        let mut names: Vec<(String, u64)> = found
            .iter()
            .map(|(path, stamp)| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, stamp.size)
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            [("ROAD2.PK3".to_string(), 3), ("road.zip".to_string(), 5)]
        );
    }

    #[test]
    fn test_listing_a_directory_that_is_not_there_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let missing = list_wad_shaped(&dir.path().join("nope")).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    }
}
