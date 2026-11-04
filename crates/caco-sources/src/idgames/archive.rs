//! Asking about the idgames archive: the local index when it knows, the API when it does
//! not. Everything that wants to know something about an upload comes through here, so a
//! blocked API costs only what the index has not got to yet.

use std::path::PathBuf;

use caco_core::config;
use caco_core::db::WadRecord;

use super::client::IdgamesClient;
use super::index::{Index, IndexStatus};
use super::models::FileEntry;
use super::{archive_path_from_url, extract_idgames_id_from_url};
use crate::error::Result;

/// How many results a search returns. The API gives at most 100 of what it finds, and it
/// finds less, since it searches one field at a time.
const SEARCH_LIMIT: usize = 200;

/// How to name an upload, which an entry from the index may have to do by path: the
/// listing has paths and caco's rows have ids, and the API is what connects the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdgamesRef {
    Id(i64),
    Path(String),
}

impl IdgamesRef {
    /// How to name this entry, by id if it has one.
    pub fn of(entry: &FileEntry) -> Self {
        if entry.id > 0 {
            Self::Id(entry.id)
        } else {
            Self::Path(entry.path())
        }
    }
}

/// What a search found.
#[derive(Debug, Clone)]
pub struct ArchiveSearch {
    pub entries: Vec<FileEntry>,
    /// Why this may not be everything, when that is the case.
    pub note: Option<String>,
}

/// Asking about the archive. Cheap to make, and opens the index afresh for each question,
/// so one can be shared between threads.
pub struct Archive {
    index_path: Option<PathBuf>,
    api: IdgamesClient,
}

impl Archive {
    /// The real index and the real API.
    ///
    /// Not from a unit test of this crate, which gets here through `ImportService::new`
    /// and has no way to say otherwise. Opening the index creates it, so one that could
    /// reach the real one would be writing into the developer's cache on every
    /// `cargo test`, and asking doomworld.com about its fixtures.
    pub fn open() -> Self {
        if cfg!(test) {
            return Self {
                index_path: None,
                api: IdgamesClient::with_api_url("http://127.0.0.1:1/"),
            };
        }
        Self::new(config::idgames_index_path(), IdgamesClient::new())
    }

    pub fn new(index_path: PathBuf, api: IdgamesClient) -> Self {
        Self {
            index_path: Some(index_path),
            api,
        }
    }

    /// The index, if there can be one. Nothing here needs one: without it, this is the API.
    fn index(&self) -> Option<Index> {
        self.index_path
            .as_deref()
            .and_then(|path| Index::open(path).ok())
    }

    /// What is known about the upload with this id.
    pub fn by_id(&self, id: i64) -> Result<FileEntry> {
        let known = self
            .index()
            .and_then(|index| index.by_id(id).ok())
            .flatten();
        match known {
            Some(entry) if is_described(&entry) => Ok(entry),
            known => self.api.get(Some(id), None).or_else(|e| known.ok_or(e)),
        }
    }

    /// What is known about the upload at this path in the archive.
    ///
    /// An upload whose id the index has not got is worth one question to the API, for the
    /// import this is usually about: a row with an id can be matched to everything else
    /// that goes by id. If that fails it is still an upload, known by its path, and that
    /// is enough to fetch it.
    pub fn by_path(&self, path: &str) -> Result<FileEntry> {
        let index = self.index();
        let known = index
            .as_ref()
            .and_then(|index| index.by_path(path).ok())
            .flatten();
        match known {
            Some(entry) if entry.id > 0 && is_described(&entry) => Ok(entry),
            known => match self.api.get_by_path(path) {
                Ok(entry) => {
                    // It only ever makes the next question cheaper.
                    if let Some(index) = &index {
                        let _ = index.record_entry(&entry);
                    }
                    Ok(entry)
                }
                Err(failure) => known.ok_or(failure),
            },
        }
    }

    /// What is known about an upload, however it is named.
    pub fn entry(&self, target: &IdgamesRef) -> Result<FileEntry> {
        match target {
            IdgamesRef::Id(id) => self.by_id(*id),
            IdgamesRef::Path(path) => self.by_path(path),
        }
    }

    /// Where a WAD's file is in the archive, by what the index says, then the API, then
    /// what the row itself says. `None` if it has no way of telling.
    pub fn path_of(&self, wad: &WadRecord) -> Option<String> {
        let source_url = wad.source_url.as_deref().unwrap_or("");
        let id = wad
            .idgames_id
            .as_deref()
            .and_then(|id| id.parse::<i64>().ok())
            .or_else(|| extract_idgames_id_from_url(source_url));
        id.and_then(|id| self.by_id(id).ok())
            .map(|entry| entry.path())
            .or_else(|| archive_path_from_url(source_url, wad.filename.as_deref().unwrap_or("")))
    }

    /// Uploads matching `query`, best first.
    ///
    /// By the index alone once it is complete. Until then the API knows what the index
    /// has not got to, so it is asked as well, by title and by file name, since it only
    /// looks in one field at a time. A failure there is no failure at all while the index
    /// has an answer: a search with the API behind a challenge is the case this exists for.
    pub fn search(&self, query: &str) -> Result<ArchiveSearch> {
        let index = self.index();
        let status = index
            .as_ref()
            .and_then(|index| index.status().ok())
            .unwrap_or_default();
        let local = index
            .as_ref()
            .and_then(|index| index.search(query, SEARCH_LIMIT).ok())
            .unwrap_or_default();
        if status.is_complete() {
            return Ok(ArchiveSearch {
                entries: local,
                note: None,
            });
        }

        let asked = self.ask_api(query);
        finish_search(local, asked, &status)
    }

    fn ask_api(&self, query: &str) -> Result<Vec<FileEntry>> {
        let mut found = self.api.search(query, Some("title"), None, None)?;
        found.extend(self.api.search(query, Some("filename"), None, None)?);
        Ok(found)
    }

    /// The most recent uploads, newest first. Nothing, until there is a listing.
    pub fn newest(&self, limit: usize) -> Vec<FileEntry> {
        self.index()
            .and_then(|index| index.newest(limit).ok())
            .unwrap_or_default()
    }
}

/// Whether a row says more than its file name, which is all it has until its text file has
/// been read or the API has said. It would otherwise become a WAD called `sunlust` in the
/// library, a few minutes before the index could have said what it is.
fn is_described(entry: &FileEntry) -> bool {
    !entry.author.is_empty() || !entry.description.is_empty()
}

/// What the index found together with what came of asking the API. The API knows more about
/// anything both found, since it has what the index is still waiting for.
fn finish_search(
    local: Vec<FileEntry>,
    asked: Result<Vec<FileEntry>>,
    status: &IndexStatus,
) -> Result<ArchiveSearch> {
    let from_api = match asked {
        Ok(from_api) => from_api,
        Err(failure) if local.is_empty() && status.is_empty() => return Err(failure),
        Err(failure) => {
            let note = format!(
                "From the local index, which is still being built, because idgames' own search \
                 could not be asked. {failure}"
            );
            return Ok(ArchiveSearch {
                entries: local,
                note: Some(note),
            });
        }
    };

    let mut entries = local;
    for entry in from_api {
        let path = entry.path();
        match entries.iter_mut().find(|known| known.path() == path) {
            Some(known) if known.id == 0 || !is_described(known) => *known = entry,
            Some(_) => {}
            None => entries.push(entry),
        }
    }
    Ok(ArchiveSearch {
        entries,
        note: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SourceError;
    use crate::http::WafKind;

    fn entry(path: &str, id: i64, description: &str) -> FileEntry {
        let (dir, filename) = path.rsplit_once('/').unwrap();
        serde_json::from_value(serde_json::json!({
            "id": id,
            "dir": format!("{dir}/"),
            "filename": filename,
            "description": description,
        }))
        .unwrap()
    }

    fn blocked() -> SourceError {
        SourceError::WafBlocked {
            site: "idgames",
            kind: WafKind::Cloudflare,
        }
    }

    fn building() -> IndexStatus {
        IndexStatus {
            uploads: 100,
            waiting_for_text: 90,
            ..Default::default()
        }
    }

    fn paths(search: &ArchiveSearch) -> Vec<String> {
        search.entries.iter().map(FileEntry::path).collect()
    }

    #[test]
    fn test_the_api_fills_in_what_the_index_has_not_got_to() {
        let local = vec![
            entry("levels/doom2/a.zip", 0, ""),
            entry("levels/doom2/b.zip", 2, "B"),
        ];
        let from_api = vec![
            entry("levels/doom2/a.zip", 1, "A"),
            entry("levels/doom2/b.zip", 2, "Not used"),
            entry("levels/doom2/c.zip", 3, "C"),
            entry("levels/doom2/c.zip", 3, "C"),
        ];

        let search = finish_search(local, Ok(from_api), &building()).unwrap();

        assert_eq!(
            paths(&search),
            [
                "levels/doom2/a.zip",
                "levels/doom2/b.zip",
                "levels/doom2/c.zip"
            ]
        );
        assert_eq!(search.entries[0].id, 1);
        assert_eq!(search.entries[1].description, "B");
        assert_eq!(search.note, None);
    }

    /// The issue in one test: Cloudflare has closed the door, and search carries on.
    #[test]
    fn test_a_blocked_api_is_no_failure_when_the_index_has_an_answer() {
        let local = vec![entry("levels/doom2/a.zip", 0, "")];

        let search = finish_search(local, Err(blocked()), &building()).unwrap();

        assert_eq!(paths(&search), ["levels/doom2/a.zip"]);
        let note = search.note.unwrap();
        assert!(note.contains("still being built"), "{note}");
        assert!(note.contains("blocked"), "{note}");
    }

    #[test]
    fn test_an_index_with_nothing_yet_cannot_stand_in_for_the_api() {
        let failed = finish_search(Vec::new(), Err(blocked()), &IndexStatus::default());
        assert!(matches!(failed, Err(SourceError::WafBlocked { .. })));
    }

    /// Empty would read as `No results`, from an index that could not say so.
    #[test]
    fn test_finding_nothing_in_a_half_built_index_says_it_might_be_there() {
        let search = finish_search(Vec::new(), Err(blocked()), &building()).unwrap();
        assert!(search.entries.is_empty());
        assert!(search.note.unwrap().contains("still being built"));
    }

    // -----------------------------------------------------------------------
    // Asking both, against a real index and a fake API
    // -----------------------------------------------------------------------

    use crate::idgames::index::ListingState;
    use crate::idgames::listing::{ListedUpload, TextStamp};
    use crate::idgames::test_archive::TestArchive;

    const SUNLUST: &str = "levels/doom2/Ports/megawads/sunlust.zip";

    struct Fixture {
        api: TestArchive,
        archive: Archive,
        index: Index,
        _dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("index.db");
            let api = TestArchive::start();
            let index = Index::open(&path).unwrap();
            index
                .apply_listing(
                    &[ListedUpload {
                        dir: "levels/doom2/Ports/megawads/".to_string(),
                        filename: "sunlust.zip".to_string(),
                        size: 1000,
                        date: "2015-08-09".to_string(),
                        text: Some(TextStamp {
                            name: "sunlust.txt".to_string(),
                            size: 10,
                            date: "2015-08-09".to_string(),
                        }),
                    }],
                    &ListingState::default(),
                )
                .unwrap();
            Self {
                archive: Archive::new(path, IdgamesClient::with_api_url(api.api_url())),
                api,
                index,
                _dir: dir,
            }
        }

        /// As the API would say, with the id that connects it to everything else.
        fn describe(&self) {
            self.index.record_entry(&self.api_says_sunlust()).unwrap();
        }

        fn api_says_sunlust(&self) -> FileEntry {
            serde_json::from_value(self.api_sunlust_json()).unwrap()
        }

        fn api_sunlust_json(&self) -> serde_json::Value {
            serde_json::json!({
                "id": 18168,
                "title": "Sunlust",
                "dir": "levels/doom2/Ports/megawads/",
                "filename": "sunlust.zip",
                "size": 1000,
                "author": "Ribbiks",
                "description": "32 maps.",
            })
        }
    }

    /// The point of the whole thing: a described index, and nothing asked of doomworld.com.
    #[test]
    fn test_a_complete_index_answers_without_asking_the_api() {
        let f = Fixture::new();
        f.describe();

        let search = f.archive.search("sunlust").unwrap();
        assert_eq!(paths(&search), [SUNLUST]);
        assert_eq!(search.note, None);

        assert_eq!(f.archive.by_id(18168).unwrap().title, "Sunlust");
        assert_eq!(f.archive.by_path(SUNLUST).unwrap().id, 18168);
        assert_eq!(f.api.log(), Vec::<String>::new());
    }

    #[test]
    fn test_a_search_in_a_half_built_index_asks_the_api_about_titles_and_names() {
        let f = Fixture::new();
        f.api.put_entry(f.api_sunlust_json());

        let search = f.archive.search("sunlust").unwrap();

        assert_eq!(paths(&search), [SUNLUST]);
        // Told apart by what the API knows, and the index had not read yet.
        assert_eq!(search.entries[0].id, 18168);
        assert_eq!(search.entries[0].author, "Ribbiks");
        assert_eq!(f.api.log(), ["api search sunlust", "api search sunlust"]);
    }

    #[test]
    fn test_a_blocked_api_does_not_stop_a_search_of_a_half_built_index() {
        let f = Fixture::new();
        f.api.block_api(true);

        let search = f.archive.search("sunlust").unwrap();

        assert_eq!(paths(&search), [SUNLUST]);
        assert!(search.note.unwrap().contains("blocked"));
    }

    #[test]
    fn test_an_id_the_index_does_not_know_is_asked_of_the_api() {
        let f = Fixture::new();
        f.api.put_entry(serde_json::json!({
            "id": 7,
            "title": "Something Else",
            "dir": "levels/doom/",
            "filename": "else.zip",
        }));

        assert_eq!(f.archive.by_id(7).unwrap().title, "Something Else");
    }

    #[test]
    fn test_an_upload_with_no_id_gets_one_if_the_api_can_be_asked_and_keeps_it() {
        let f = Fixture::new();
        f.api.put_entry(f.api_sunlust_json());

        assert_eq!(f.archive.by_path(SUNLUST).unwrap().id, 18168);

        // It only needed asking once.
        f.api.clear_log();
        assert_eq!(f.archive.by_path(SUNLUST).unwrap().id, 18168);
        assert_eq!(f.api.log(), Vec::<String>::new());
    }

    /// The other half of importing on a blocked day.
    #[test]
    fn test_an_upload_with_no_id_is_still_an_upload_if_the_api_cannot_be_asked() {
        let f = Fixture::new();
        f.api.block_api(true);

        let entry = f
            .archive
            .entry(&IdgamesRef::Path(SUNLUST.to_string()))
            .unwrap();

        assert_eq!(entry.id, 0);
        assert_eq!(entry.path(), SUNLUST);
    }

    /// The index knows the id from one run, and the text file changed since. In the few
    /// minutes before it is read, the API is better than a WAD called `sunlust`.
    #[test]
    fn test_a_row_with_an_id_and_nothing_else_is_worth_asking_about() {
        let f = Fixture::new();
        let older_file = serde_json::json!({
            "id": 18168,
            "dir": "levels/doom2/Ports/megawads/",
            "filename": "sunlust.zip",
            "size": 999,
            "title": "Sunlust",
            "author": "Ribbiks",
        });
        f.index
            .record_entry(&serde_json::from_value(older_file.clone()).unwrap())
            .unwrap();
        assert_eq!(f.index.by_id(18168).unwrap().unwrap().title, "sunlust");

        f.api.put_entry(older_file);
        assert_eq!(f.archive.by_id(18168).unwrap().title, "Sunlust");

        f.api.block_api(true);
        assert_eq!(f.archive.by_id(18168).unwrap().title, "sunlust");
    }

    #[test]
    fn test_nothing_known_anywhere_is_a_failure() {
        let f = Fixture::new();
        f.api.block_api(true);
        assert!(f.archive.by_id(7).is_err());
        assert!(f.archive.by_path("levels/doom/else.zip").is_err());
    }

    fn wad(id: Option<&str>, source_url: Option<&str>, filename: Option<&str>) -> WadRecord {
        let conn = caco_core::db::open_memory().unwrap();
        caco_core::db::init_db(&conn).unwrap();
        let mut new = caco_core::db::NewWad::new("W", caco_core::db::SourceType::Idgames);
        if let Some(id) = id {
            new = new.idgames_id(id);
        }
        if let Some(url) = source_url {
            new = new.source_url(url);
        }
        if let Some(name) = filename {
            new = new.filename(name);
        }
        let id = caco_core::db::add_wad(&conn, &new).unwrap();
        caco_core::db::get_wad(&conn, id, false).unwrap().unwrap()
    }

    #[test]
    fn test_a_wads_place_in_the_archive_is_what_the_index_says_it_is_now() {
        let f = Fixture::new();
        f.describe();
        // A row from before it moved. The API has moved on, and so has the index.
        let moved = wad(
            Some("18168"),
            Some("https://www.doomworld.com/idgames/levels/doom2/s-u/sunlust"),
            Some("sunlust.zip"),
        );
        assert_eq!(f.archive.path_of(&moved).as_deref(), Some(SUNLUST));
        assert_eq!(f.api.log(), Vec::<String>::new());
    }

    #[test]
    fn test_with_nobody_to_ask_a_wad_says_where_it_is_itself() {
        let f = Fixture::new();
        f.api.block_api(true);
        let url = "https://www.doomworld.com/idgames/levels/doom2/s-u/scythe";

        let by_id = wad(Some("11742"), Some(url), Some("scythe.zip"));
        assert_eq!(
            f.archive.path_of(&by_id).as_deref(),
            Some("levels/doom2/s-u/scythe.zip")
        );
        let no_id = wad(None, Some(url), Some("scythe.zip"));
        assert_eq!(
            f.archive.path_of(&no_id).as_deref(),
            Some("levels/doom2/s-u/scythe.zip")
        );
    }

    #[test]
    fn test_a_row_with_only_an_id_in_its_url_is_found_by_it() {
        let f = Fixture::new();
        f.describe();
        let by_url = wad(
            None,
            Some("https://www.doomworld.com/idgames/?id=18168"),
            None,
        );
        assert_eq!(f.archive.path_of(&by_url).as_deref(), Some(SUNLUST));
    }

    #[test]
    fn test_an_entry_is_named_by_its_id_if_it_has_one() {
        assert_eq!(
            IdgamesRef::of(&entry("levels/doom2/a.zip", 7, "")),
            IdgamesRef::Id(7)
        );
        assert_eq!(
            IdgamesRef::of(&entry("levels/doom2/a.zip", 0, "")),
            IdgamesRef::Path("levels/doom2/a.zip".to_string())
        );
    }
}
