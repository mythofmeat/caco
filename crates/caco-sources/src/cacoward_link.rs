//! Decides which library WAD a Cacoward row belongs to, and links them.
//!
//! One decision, reached from two moments, because either side can arrive first:
//!
//! - [`link_rows_with_keys`] runs after the dataset is written, for the rows it
//!   added or gave new evidence.
//! - [`link_imported_wad`] runs after every import, when the WAD is new.
//!
//! The decision ([`owner_of`]) is made per row rather than per WAD so its answer
//! cannot depend on which WAD is visited first. Walking the library and letting
//! each WAD claim what it matches would let a title match on one WAD take a row
//! that a later WAD owns by idgames id.
//!
//! A link is not revisited once made, which is why weaker evidence refuses a WAD
//! whose stronger identifiers differ from the row's: importing the right WAD
//! later would not correct it. A WAD with no identifiers has nothing to contradict
//! its title, so it can still take a row before the row's real owner arrives.
//! Unlink or a pin corrects that.
//!
//! Only rows [`db::get_linkable_cacowards`] returns are touched, so a pinned
//! link is never overwritten and nothing sets `manual_override`. Nothing here
//! sweeps every row either: the card menu's Unlink clears a row, and a sweep would
//! quietly undo that on the next launch. A cleared row is only reconsidered when a
//! WAD it matches is imported or a data update changes what it says.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use caco_core::Result;
use caco_core::db::{self, CacowardRecord};
use rusqlite::Connection;

use crate::doomwiki::{canonical_page, extract_doomwiki_title_from_url};
use crate::doomworld::extract_topic_id_from_url;
use crate::import_service::normalize_title;

/// Rows a linking run claimed, by the evidence that decided each one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LinkCounts {
    pub by_hash: usize,
    pub by_idgames: usize,
    pub by_doomworld: usize,
    pub by_doomwiki: usize,
    pub by_title: usize,
}

impl LinkCounts {
    pub(crate) fn total(&self) -> usize {
        self.by_hash + self.by_idgames + self.by_doomworld + self.by_doomwiki + self.by_title
    }
}

/// Link a WAD that just entered the library to the rows that belong to it.
pub(crate) fn link_imported_wad(conn: &Connection, wad_id: i64) -> Result<LinkCounts> {
    let rows = db::get_linkable_cacowards(conn)?;
    link_rows(conn, &rows, |owner| owner == wad_id)
}

/// Link each unclaimed row named in `keys` to the library WAD it belongs to.
pub(crate) fn link_rows_with_keys(conn: &Connection, keys: &HashSet<String>) -> Result<LinkCounts> {
    let mut rows = db::get_linkable_cacowards(conn)?;
    rows.retain(|row| keys.contains(&row.key));
    link_rows(conn, &rows, |_| true)
}

fn link_rows(
    conn: &Connection,
    rows: &[CacowardRecord],
    wanted: impl Fn(i64) -> bool,
) -> Result<LinkCounts> {
    let wads = WadIndex::build(conn)?;
    let mut counts = LinkCounts::default();

    for row in rows {
        let Some((wad_id, evidence)) = owner_of(row, &wads) else {
            continue;
        };
        if !wanted(wad_id) {
            continue;
        }
        db::link_wad(conn, row.id, wad_id, false)?;
        *match evidence {
            Evidence::Hash => &mut counts.by_hash,
            Evidence::IdgamesId => &mut counts.by_idgames,
            Evidence::DoomworldTopic => &mut counts.by_doomworld,
            Evidence::DoomwikiPage => &mut counts.by_doomwiki,
            Evidence::Title => &mut counts.by_title,
        } += 1;
    }

    Ok(counts)
}

enum Evidence {
    Hash,
    IdgamesId,
    DoomworldTopic,
    DoomwikiPage,
    Title,
}

/// The live WAD a row belongs to, strongest evidence first:
///
/// 1. **File hash**: the row's hash is one `identity` recorded for the WAD. The
///    same bytes are the same WAD, whatever else either side says.
/// 2. **idgames id**: `wads.idgames_id`.
/// 3. **Doomworld topic**: the number in a `doomworld.com` `source_url`.
/// 4. **Doom Wiki page**: a `doomwiki.org` `source_url`. A series can share one
///    page across several releases, so this ranks below the ids that tell them
///    apart.
/// 5. **Normalized title**: equal to exactly one WAD's, once both are case-folded,
///    diacritic-stripped and punctuation-collapsed. Two WADs sharing a name (e.g.
///    "Crusader" 1995 and 2023) are left for the user to pick between.
///
/// Each step refuses a WAD whose stronger identifiers differ from the row's. The
/// 2023 runner-up "Corruption" (idgames 20790) shares its name with two 2007 WADs,
/// and a title must not outweigh two ids that disagree. Identifiers of different
/// kinds do not contradict each other, since a release can have a forum topic and
/// an idgames entry both, and a hash mismatch contradicts nothing because a
/// release's file changes with every fix.
fn owner_of(row: &CacowardRecord, wads: &WadIndex) -> Option<(i64, Evidence)> {
    if let Some(wad) = row
        .sha256
        .as_deref()
        .and_then(|hash| first(wads, wads.by_hash.get(hash), |_| true))
    {
        return Some((wad.id, Evidence::Hash));
    }

    if let Some(wad) = row
        .idgames_id
        .and_then(|id| first(wads, wads.by_idgames.get(&id), |_| true))
    {
        return Some((wad.id, Evidence::IdgamesId));
    }

    if let Some(wad) = row.doomworld_id.and_then(|topic| {
        first(wads, wads.by_doomworld.get(&topic), |wad| {
            !wad.contradicts_idgames(row)
        })
    }) {
        return Some((wad.id, Evidence::DoomworldTopic));
    }

    let page = row.doomwiki_page.as_deref().map(canonical_page);
    if let Some(wad) = page.as_ref().and_then(|page| {
        first(wads, wads.by_doomwiki.get(page), |wad| {
            !wad.contradicts_idgames(row) && !wad.contradicts_doomworld(row)
        })
    }) {
        return Some((wad.id, Evidence::DoomwikiPage));
    }

    wads.unique_title_match(&row.wad_title)
        .filter(|wad| {
            !wad.contradicts_idgames(row)
                && !wad.contradicts_doomworld(row)
                && !wad.contradicts_doomwiki(page.as_deref())
        })
        .map(|wad| (wad.id, Evidence::Title))
}

/// The oldest of `candidates` that `allowed` accepts: two WADs with one identifier
/// are the same release imported twice, so the first keeps its rows.
fn first<'a>(
    wads: &'a WadIndex,
    candidates: Option<&Vec<usize>>,
    allowed: impl Fn(&IndexedWad) -> bool,
) -> Option<&'a IndexedWad> {
    candidates?
        .iter()
        .map(|&index| &wads.wads[index])
        .find(|wad| allowed(wad))
}

/// What the decision reads about a live WAD.
struct IndexedWad {
    id: i64,
    idgames: Option<i64>,
    doomworld: Option<i64>,
    doomwiki: Option<String>,
}

impl IndexedWad {
    fn contradicts_idgames(&self, row: &CacowardRecord) -> bool {
        differ(self.idgames, row.idgames_id)
    }

    fn contradicts_doomworld(&self, row: &CacowardRecord) -> bool {
        differ(self.doomworld, row.doomworld_id)
    }

    fn contradicts_doomwiki(&self, row_page: Option<&str>) -> bool {
        differ(self.doomwiki.as_deref(), row_page)
    }
}

/// Two identifiers of the same kind that are both there and not equal.
fn differ<T: PartialEq>(a: Option<T>, b: Option<T>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a != b)
}

/// The live WADs, findable by every kind of evidence. Each map holds indexes into
/// `wads` rather than one WAD each, so that "one" and "several" stay distinguishable.
struct WadIndex {
    wads: Vec<IndexedWad>,
    by_hash: HashMap<String, Vec<usize>>,
    by_idgames: HashMap<i64, Vec<usize>>,
    by_doomworld: HashMap<i64, Vec<usize>>,
    by_doomwiki: HashMap<String, Vec<usize>>,
    by_title: HashMap<String, Vec<usize>>,
}

impl WadIndex {
    fn build(conn: &Connection) -> Result<Self> {
        let mut index = Self {
            wads: Vec::new(),
            by_hash: HashMap::new(),
            by_idgames: HashMap::new(),
            by_doomworld: HashMap::new(),
            by_doomwiki: HashMap::new(),
            by_title: HashMap::new(),
        };

        // Oldest first, which is what `first` relies on. Pull only what the
        // decision reads; the rest of the row is dead weight.
        let mut stmt = conn.prepare(
            "SELECT id, title, idgames_id, source_url, file_sha256, content_sha256
             FROM wads WHERE deleted_at IS NULL ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;

        let mut position_of = HashMap::new();
        for row in rows {
            let (id, title, idgames_id, source_url, file_sha256, content_sha256) = row?;
            let source_url = source_url.as_deref().unwrap_or("");
            let wad = IndexedWad {
                id,
                idgames: idgames_id.and_then(|id| id.trim().parse().ok()),
                doomworld: extract_topic_id_from_url(source_url),
                doomwiki: extract_doomwiki_title_from_url(source_url)
                    .map(|page| canonical_page(&page)),
            };

            let position = index.wads.len();
            position_of.insert(id, position);
            if let Some(idgames) = wad.idgames {
                add(&mut index.by_idgames, idgames, position);
            }
            if let Some(topic) = wad.doomworld {
                add(&mut index.by_doomworld, topic, position);
            }
            if let Some(page) = &wad.doomwiki {
                add(&mut index.by_doomwiki, page.clone(), position);
            }
            for hash in [file_sha256, content_sha256].into_iter().flatten() {
                add(&mut index.by_hash, hash, position);
            }
            let title = normalize_title(&title);
            if !title.is_empty() {
                add(&mut index.by_title, title, position);
            }
            index.wads.push(wad);
        }

        let mut stmt = conn.prepare("SELECT wad_id, sha256 FROM wad_files ORDER BY wad_id")?;
        let inner_files = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for inner_file in inner_files {
            let (wad_id, hash) = inner_file?;
            // A trashed WAD's files are listed too, and it has no position.
            if let Some(&position) = position_of.get(&wad_id) {
                add(&mut index.by_hash, hash, position);
            }
        }

        Ok(index)
    }

    fn unique_title_match(&self, title: &str) -> Option<&IndexedWad> {
        match self.by_title.get(&normalize_title(title))?.as_slice() {
            [only] => Some(&self.wads[*only]),
            _ => None,
        }
    }
}

fn add<K: Eq + Hash>(map: &mut HashMap<K, Vec<usize>>, key: K, position: usize) {
    let positions = map.entry(key).or_default();
    // A WAD's file hash and content hash are the same value for a bare WAD.
    if !positions.contains(&position) {
        positions.push(position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::cacowards::entry_key;
    use caco_core::db::models::SourceType;
    use caco_core::db::wads::{NewWad, add_wad};
    use caco_core::db::{NewCacoward, WadUpdate, init_db, open_memory};

    const YEAR: i64 = 2025;

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_titled_wad(conn: &Connection, title: &str) -> i64 {
        add_wad(conn, &NewWad::new(title, SourceType::Doomworld)).unwrap()
    }

    fn add_idgames_wad(conn: &Connection, title: &str, id: &str) -> i64 {
        add_wad(
            conn,
            &NewWad::new(title, SourceType::Idgames)
                .source_id(id)
                .idgames_id(id),
        )
        .unwrap()
    }

    fn add_wad_at_url(conn: &Connection, title: &str, source_url: &str) -> i64 {
        add_wad(
            conn,
            &NewWad::new(title, SourceType::Doomworld).source_url(source_url),
        )
        .unwrap()
    }

    fn entry(year: i64, title: &str) -> NewCacoward {
        NewCacoward {
            key: entry_key(year, db::CATEGORY_WINNER, title),
            year,
            category: db::CATEGORY_WINNER.to_string(),
            page_order: 1,
            wad_title: title.to_string(),
            ..Default::default()
        }
    }

    fn add_row_in(conn: &Connection, year: i64, title: &str, idgames_id: Option<i64>) -> i64 {
        db::upsert_cacoward(
            conn,
            &NewCacoward {
                idgames_id,
                ..entry(year, title)
            },
        )
        .unwrap()
    }

    fn add_row(conn: &Connection, title: &str, idgames_id: Option<i64>) -> i64 {
        add_row_in(conn, YEAR, title, idgames_id)
    }

    /// A row that names only a Doomworld topic.
    fn add_row_at_topic(conn: &Connection, title: &str, topic: i64) -> i64 {
        db::upsert_cacoward(
            conn,
            &NewCacoward {
                doomworld_id: Some(topic),
                ..entry(YEAR, title)
            },
        )
        .unwrap()
    }

    /// A row that names only a Doom Wiki page.
    fn add_row_at_page(conn: &Connection, title: &str, page: &str) -> i64 {
        db::upsert_cacoward(
            conn,
            &NewCacoward {
                doomwiki_page: Some(page.to_string()),
                ..entry(YEAR, title)
            },
        )
        .unwrap()
    }

    fn linked_wad(conn: &Connection, row: i64) -> Option<i64> {
        db::get_cacoward(conn, row).unwrap().unwrap().wad_id
    }

    fn trash(conn: &Connection, wad_id: i64) {
        db::delete_wad(conn, wad_id, false).unwrap();
    }

    fn by_title(n: usize) -> LinkCounts {
        LinkCounts {
            by_title: n,
            ..Default::default()
        }
    }

    fn keys_of(conn: &Connection, rows: &[i64]) -> HashSet<String> {
        rows.iter()
            .map(|&row| db::get_cacoward(conn, row).unwrap().unwrap().key)
            .collect()
    }

    // -- idgames id --

    #[test]
    fn links_by_idgames_id_when_the_titles_differ() {
        let conn = setup();
        let row = add_row(&conn, "TNT: Overcharged", Some(19951));
        let wad = add_idgames_wad(&conn, "TNT:Overcharged", "19951");

        let counts = link_imported_wad(&conn, wad).unwrap();

        assert_eq!(
            counts,
            LinkCounts {
                by_idgames: 1,
                ..Default::default()
            }
        );
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    #[test]
    fn an_idgames_owner_beats_a_title_match_in_either_insertion_order() {
        // Which WAD was inserted first must not decide the outcome.
        for title_first in [true, false] {
            let conn = setup();
            let row = add_row(&conn, "Ray Mohawk", Some(12345));
            let (by_title, by_id) = if title_first {
                let by_title = add_titled_wad(&conn, "Ray Mohawk");
                (by_title, add_idgames_wad(&conn, "Other Name", "12345"))
            } else {
                let by_id = add_idgames_wad(&conn, "Other Name", "12345");
                (add_titled_wad(&conn, "Ray Mohawk"), by_id)
            };

            assert_eq!(
                link_imported_wad(&conn, by_title).unwrap(),
                LinkCounts::default(),
                "title_first = {title_first}"
            );
            assert_eq!(linked_wad(&conn, row), None);

            assert_eq!(link_imported_wad(&conn, by_id).unwrap().by_idgames, 1);
            assert_eq!(linked_wad(&conn, row), Some(by_id));
        }
    }

    #[test]
    fn a_data_update_gives_a_contested_row_to_its_idgames_owner() {
        for title_first in [true, false] {
            let conn = setup();
            let row = add_row(&conn, "Ray Mohawk", Some(12345));
            let by_id = if title_first {
                add_titled_wad(&conn, "Ray Mohawk");
                add_idgames_wad(&conn, "Other Name", "12345")
            } else {
                let by_id = add_idgames_wad(&conn, "Other Name", "12345");
                add_titled_wad(&conn, "Ray Mohawk");
                by_id
            };

            let counts = link_rows_with_keys(&conn, &keys_of(&conn, &[row])).unwrap();

            assert_eq!(counts.by_idgames, 1, "title_first = {title_first}");
            assert_eq!(counts.by_title, 0);
            assert_eq!(linked_wad(&conn, row), Some(by_id));
        }
    }

    #[test]
    fn a_title_does_not_outweigh_two_idgames_ids_that_disagree() {
        // Real collision: the 2023 runner-up is 20790, the 2007 WAD is 14848.
        let conn = setup();
        let row = add_row(&conn, "Corruption", Some(20790));
        let older = add_idgames_wad(&conn, "Corruption", "14848");

        assert_eq!(
            link_imported_wad(&conn, older).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(
            link_rows_with_keys(&conn, &keys_of(&conn, &[row])).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn the_right_wad_gets_the_row_whichever_is_imported_first() {
        for older_first in [true, false] {
            let conn = setup();
            let row = add_row(&conn, "Corruption", Some(20790));
            let import = |id: &str| {
                let wad = add_idgames_wad(&conn, "Corruption", id);
                link_imported_wad(&conn, wad).unwrap();
                wad
            };

            let awarded = if older_first {
                import("14848");
                import("20790")
            } else {
                let awarded = import("20790");
                import("14848");
                awarded
            };

            assert_eq!(
                linked_wad(&conn, row),
                Some(awarded),
                "older_first = {older_first}"
            );
        }
    }

    #[test]
    fn a_forced_second_copy_does_not_take_the_row_from_the_first() {
        let conn = setup();
        let row = add_row(&conn, "Eisberg", Some(100));
        let first = add_idgames_wad(&conn, "Eisberg", "100");
        link_imported_wad(&conn, first).unwrap();

        let second = add_idgames_wad(&conn, "Eisberg", "100");
        assert_eq!(
            link_imported_wad(&conn, second).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), Some(first));
    }

    #[test]
    fn with_two_copies_and_no_link_yet_the_older_one_takes_the_row() {
        let conn = setup();
        let older = add_idgames_wad(&conn, "Eisberg", "100");
        add_idgames_wad(&conn, "Eisberg", "100");
        let row = add_row(&conn, "Eisberg", Some(100));

        link_rows_with_keys(&conn, &keys_of(&conn, &[row])).unwrap();

        assert_eq!(linked_wad(&conn, row), Some(older));
    }

    #[test]
    fn reads_the_idgames_id_a_doom_wiki_import_stamped_after_insert() {
        // The row is matched on what the WAD says now, not on how it was inserted.
        let conn = setup();
        let row = add_row(&conn, "Another Name", Some(18184));
        let wad = add_titled_wad(&conn, "Scythe");
        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("idgames_id", Some("18184".to_string())),
        )
        .unwrap();

        assert_eq!(link_imported_wad(&conn, wad).unwrap().by_idgames, 1);
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    // -- Doomworld topic --

    #[test]
    fn links_a_doomworld_only_release_whose_title_differs() {
        // The real case: a title heuristic should not be trusted with either of
        // these, but the library row's `source_url` holds the number.
        for (title, wad_title, url, topic) in [
            (
                "Lonesome Road",
                "Lonesome Road | Chapter 1",
                "https://www.doomworld.com/vb/thread/151858",
                151858,
            ),
            (
                "The Evil Reborn: Resurrected",
                "The Evil Reborn",
                "https://www.doomworld.com/forum/topic/152129-the-evil-reborn-resurrected/",
                152129,
            ),
        ] {
            let conn = setup();
            let row = add_row_at_topic(&conn, title, topic);
            let wad = add_wad_at_url(&conn, wad_title, url);

            let counts = link_imported_wad(&conn, wad).unwrap();

            assert_eq!(counts.by_doomworld, 1, "{title}");
            assert_eq!(linked_wad(&conn, row), Some(wad), "{title}");
        }
    }

    #[test]
    fn a_doomworld_topic_beats_a_title_match() {
        let conn = setup();
        let row = add_row_at_topic(&conn, "Sepia", 132145);
        add_titled_wad(&conn, "Sepia");
        let by_topic = add_wad_at_url(
            &conn,
            "Sepia v2",
            "https://www.doomworld.com/vb/thread/132145",
        );

        link_rows_with_keys(&conn, &keys_of(&conn, &[row])).unwrap();

        assert_eq!(linked_wad(&conn, row), Some(by_topic));
    }

    #[test]
    fn an_idgames_id_beats_a_doomworld_topic() {
        let conn = setup();
        let row = db::upsert_cacoward(
            &conn,
            &NewCacoward {
                idgames_id: Some(100),
                doomworld_id: Some(200),
                ..entry(YEAR, "Both")
            },
        )
        .unwrap();
        add_wad_at_url(
            &conn,
            "Forum Copy",
            "https://www.doomworld.com/vb/thread/200",
        );
        let on_idgames = add_idgames_wad(&conn, "Archive Copy", "100");

        let counts = link_rows_with_keys(&conn, &keys_of(&conn, &[row])).unwrap();

        assert_eq!(counts.by_idgames, 1);
        assert_eq!(linked_wad(&conn, row), Some(on_idgames));
    }

    #[test]
    fn a_doomworld_topic_does_not_outweigh_two_idgames_ids_that_disagree() {
        let conn = setup();
        let row = db::upsert_cacoward(
            &conn,
            &NewCacoward {
                idgames_id: Some(100),
                doomworld_id: Some(200),
                ..entry(YEAR, "Episode 2")
            },
        )
        .unwrap();
        let wad = add_wad_at_url(
            &conn,
            "Episode 1",
            "https://www.doomworld.com/vb/thread/200",
        );
        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("idgames_id", Some("99".to_string())),
        )
        .unwrap();

        assert_eq!(
            link_imported_wad(&conn, wad).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn a_title_does_not_outweigh_two_doomworld_topics_that_disagree() {
        let conn = setup();
        let row = add_row_at_topic(&conn, "Crusader", 139104);
        let other = add_wad_at_url(&conn, "Crusader", "https://www.doomworld.com/vb/thread/1");

        assert_eq!(
            link_imported_wad(&conn, other).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn a_doomworld_topic_on_an_idgames_url_is_not_a_topic() {
        let conn = setup();
        let row = add_row_at_topic(&conn, "Different Title", 21460);
        let wad = add_wad_at_url(
            &conn,
            "Other Title",
            "https://www.doomworld.com/idgames/levels/doom2/Ports/21460",
        );

        link_imported_wad(&conn, wad).unwrap();

        assert_eq!(linked_wad(&conn, row), None);
    }

    // -- Doom Wiki page --

    #[test]
    fn links_by_doom_wiki_page_however_it_is_spelled() {
        for url in [
            "https://doomwiki.org/wiki/Realm_of_Chaos_2:_The_Revenge_of_'96!",
            "https://www.doomwiki.org/wiki/Realm_of_Chaos_2:_The_Revenge_of_%2796!",
            "https://doomwiki.org/wiki/Realm_of_Chaos_2:_The_Revenge_of_'96!#Reception",
        ] {
            let conn = setup();
            let row = add_row_at_page(&conn, "RoC2", "Realm_of_Chaos_2:_The_Revenge_of_'96!");
            let wad = add_wad_at_url(&conn, "Realm of Chaos II", url);

            let counts = link_imported_wad(&conn, wad).unwrap();

            assert_eq!(counts.by_doomwiki, 1, "{url}");
            assert_eq!(linked_wad(&conn, row), Some(wad), "{url}");
        }
    }

    #[test]
    fn a_shared_page_does_not_outweigh_two_idgames_ids_that_disagree() {
        // "Return to Hadron" has one page for three releases, and a WAD imported
        // from it learns the id of the first.
        let conn = setup();
        let row = db::upsert_cacoward(
            &conn,
            &NewCacoward {
                idgames_id: Some(18652),
                doomwiki_page: Some("Return_to_Hadron".to_string()),
                ..entry(YEAR, "Return to Hadron Episode 2")
            },
        )
        .unwrap();
        let first_release = add_wad_at_url(
            &conn,
            "Return to Hadron",
            "https://doomwiki.org/wiki/Return_to_Hadron",
        );
        db::update_wad(
            &conn,
            first_release,
            &WadUpdate::new().set_text("idgames_id", Some("18000".to_string())),
        )
        .unwrap();

        assert_eq!(
            link_imported_wad(&conn, first_release).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn a_title_does_not_outweigh_two_doom_wiki_pages_that_disagree() {
        let conn = setup();
        let row = add_row_at_page(&conn, "Crusader", "Crusader_(2023_WAD)");
        let other = add_wad_at_url(
            &conn,
            "Crusader",
            "https://doomwiki.org/wiki/Crusader_(1995)",
        );

        assert_eq!(
            link_imported_wad(&conn, other).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn identifiers_of_different_kinds_do_not_contradict_each_other() {
        // A forum import of a release the row knows by its idgames id and wiki page.
        let conn = setup();
        let row = db::upsert_cacoward(
            &conn,
            &NewCacoward {
                idgames_id: Some(100),
                doomwiki_page: Some("Eisberg".to_string()),
                ..entry(YEAR, "Eisberg")
            },
        )
        .unwrap();
        let wad = add_wad_at_url(&conn, "Eisberg", "https://www.doomworld.com/vb/thread/5");

        assert_eq!(link_imported_wad(&conn, wad).unwrap(), by_title(1));
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    // -- file hash --

    fn add_row_with_hash(conn: &Connection, title: &str, hash: &str) -> i64 {
        db::upsert_cacoward(
            conn,
            &NewCacoward {
                sha256: Some(hash.to_string()),
                ..entry(YEAR, title)
            },
        )
        .unwrap()
    }

    #[test]
    fn links_by_any_hash_identity_recorded_for_the_wad() {
        for column in ["file_sha256", "content_sha256"] {
            let conn = setup();
            let row = add_row_with_hash(&conn, "Awarded Title", "aaaa");
            let wad = add_titled_wad(&conn, "A Local Import");
            conn.execute(
                &format!("UPDATE wads SET {column} = 'aaaa' WHERE id = ?1"),
                [wad],
            )
            .unwrap();

            let counts = link_imported_wad(&conn, wad).unwrap();

            assert_eq!(counts.by_hash, 1, "{column}");
            assert_eq!(linked_wad(&conn, row), Some(wad), "{column}");
        }
    }

    #[test]
    fn links_by_the_hash_of_a_file_inside_the_archive() {
        let conn = setup();
        let row = add_row_with_hash(&conn, "Awarded Title", "bbbb");
        let wad = add_titled_wad(&conn, "A Local Import");
        conn.execute(
            "INSERT INTO wad_files (wad_id, inner_path, size, sha256, sha1)
             VALUES (?1, 'x.wad', 4, 'bbbb', 'cccc')",
            [wad],
        )
        .unwrap();

        assert_eq!(link_imported_wad(&conn, wad).unwrap().by_hash, 1);
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    #[test]
    fn the_same_bytes_are_the_same_wad_whatever_the_ids_say() {
        let conn = setup();
        let row = db::upsert_cacoward(
            &conn,
            &NewCacoward {
                sha256: Some("aaaa".to_string()),
                idgames_id: Some(100),
                ..entry(YEAR, "Awarded Title")
            },
        )
        .unwrap();
        let wad = add_idgames_wad(&conn, "A Reupload", "999");
        conn.execute(
            "UPDATE wads SET content_sha256 = 'aaaa' WHERE id = ?1",
            [wad],
        )
        .unwrap();

        assert_eq!(link_imported_wad(&conn, wad).unwrap().by_hash, 1);
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    #[test]
    fn a_trashed_wads_hashes_do_not_claim_a_row() {
        let conn = setup();
        let row = add_row_with_hash(&conn, "Awarded Title", "bbbb");
        let trashed = add_titled_wad(&conn, "Trashed");
        conn.execute(
            "INSERT INTO wad_files (wad_id, inner_path, size, sha256, sha1)
             VALUES (?1, 'x.wad', 4, 'bbbb', 'cccc')",
            [trashed],
        )
        .unwrap();
        trash(&conn, trashed);

        assert_eq!(
            link_rows_with_keys(&conn, &keys_of(&conn, &[row])).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn a_different_hash_says_nothing_against_the_rest_of_the_evidence() {
        // A release's file changes with every fix.
        let conn = setup();
        let row = db::upsert_cacoward(
            &conn,
            &NewCacoward {
                sha256: Some("aaaa".to_string()),
                ..entry(YEAR, "Eisberg")
            },
        )
        .unwrap();
        let wad = add_titled_wad(&conn, "Eisberg");
        conn.execute("UPDATE wads SET file_sha256 = 'ffff' WHERE id = ?1", [wad])
            .unwrap();

        assert_eq!(link_imported_wad(&conn, wad).unwrap(), by_title(1));
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    // -- normalized title --

    #[test]
    fn links_by_title_when_the_row_has_no_identifier() {
        let conn = setup();
        let row = add_row(&conn, "Eisberg", None);
        let wad = add_titled_wad(&conn, "EISBERG");

        assert_eq!(link_imported_wad(&conn, wad).unwrap(), by_title(1));
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    #[test]
    fn an_auto_link_is_not_a_pin() {
        let conn = setup();
        let row = add_row(&conn, "Eisberg", None);
        let wad = add_titled_wad(&conn, "Eisberg");

        link_imported_wad(&conn, wad).unwrap();

        assert!(
            !db::get_cacoward(&conn, row)
                .unwrap()
                .unwrap()
                .manual_override
        );
    }

    #[test]
    fn a_near_miss_title_does_not_link() {
        let conn = setup();
        let row = add_row(&conn, "Lonesome Road", None);
        let wad = add_titled_wad(&conn, "Lonesome Road | Chapter 1");

        assert_eq!(
            link_imported_wad(&conn, wad).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn an_ambiguous_title_links_nothing() {
        let conn = setup();
        let row = add_row(&conn, "Crusader", None);
        let first = add_titled_wad(&conn, "Crusader");
        let second = add_titled_wad(&conn, "crusader");

        for wad in [first, second] {
            assert_eq!(
                link_imported_wad(&conn, wad).unwrap(),
                LinkCounts::default()
            );
        }
        assert_eq!(
            link_rows_with_keys(&conn, &keys_of(&conn, &[row])).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn a_title_of_only_punctuation_matches_nothing() {
        let conn = setup();
        let row = add_row(&conn, "!!!", None);
        let wad = add_titled_wad(&conn, "???");

        link_imported_wad(&conn, wad).unwrap();

        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn a_wad_with_no_idgames_id_can_link_by_title_to_a_row_with_one() {
        // A forum or URL import knows nothing that contradicts the title.
        let conn = setup();
        let row = add_row(&conn, "Eisberg", Some(100));
        let wad = add_titled_wad(&conn, "Eisberg");

        assert_eq!(link_imported_wad(&conn, wad).unwrap(), by_title(1));
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    #[test]
    fn a_wad_with_an_idgames_id_can_link_by_title_to_a_row_with_none() {
        let conn = setup();
        let row = add_row(&conn, "The Road to Skull City", None);
        let wad = add_idgames_wad(&conn, "The Road to Skull City", "21867");

        assert_eq!(link_imported_wad(&conn, wad).unwrap(), by_title(1));
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    #[test]
    fn an_empty_idgames_id_is_no_idgames_id() {
        let conn = setup();
        let row = add_row(&conn, "Eisberg", Some(100));
        let wad = add_titled_wad(&conn, "Eisberg");
        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("idgames_id", Some(String::new())),
        )
        .unwrap();

        assert_eq!(link_imported_wad(&conn, wad).unwrap(), by_title(1));
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    // -- which rows are in play --

    #[test]
    fn links_every_row_the_wad_owns() {
        let conn = setup();
        let winner = add_row_in(&conn, 2024, "Eisberg", None);
        let later = add_row_in(&conn, 2025, "Eisberg", None);
        let wad = add_titled_wad(&conn, "Eisberg");

        assert_eq!(link_imported_wad(&conn, wad).unwrap(), by_title(2));
        assert_eq!(linked_wad(&conn, winner), Some(wad));
        assert_eq!(linked_wad(&conn, later), Some(wad));
    }

    #[test]
    fn a_pinned_row_is_left_alone() {
        let conn = setup();
        let pinned_to = add_titled_wad(&conn, "Hand Picked");
        let row = add_row(&conn, "Eisberg", None);
        db::link_wad(&conn, row, pinned_to, true).unwrap();

        let wad = add_titled_wad(&conn, "Eisberg");
        assert_eq!(
            link_imported_wad(&conn, wad).unwrap(),
            LinkCounts::default()
        );

        let record = db::get_cacoward(&conn, row).unwrap().unwrap();
        assert_eq!(record.wad_id, Some(pinned_to));
        assert!(record.manual_override);
    }

    #[test]
    fn a_pin_on_a_trashed_wad_waits_for_it_to_come_back() {
        let conn = setup();
        let pinned_to = add_titled_wad(&conn, "Hand Picked");
        let row = add_row(&conn, "Eisberg", None);
        db::link_wad(&conn, row, pinned_to, true).unwrap();
        trash(&conn, pinned_to);

        let wad = add_titled_wad(&conn, "Eisberg");
        link_imported_wad(&conn, wad).unwrap();

        assert_eq!(linked_wad(&conn, row), Some(pinned_to));
    }

    #[test]
    fn a_pin_left_behind_by_a_purged_wad_does_not_block_a_new_copy() {
        let conn = setup();
        let pinned_to = add_titled_wad(&conn, "Hand Picked");
        let row = add_row(&conn, "Eisberg", None);
        db::link_wad(&conn, row, pinned_to, true).unwrap();
        db::delete_wad(&conn, pinned_to, true).unwrap();

        let wad = add_titled_wad(&conn, "Eisberg");
        link_imported_wad(&conn, wad).unwrap();

        let record = db::get_cacoward(&conn, row).unwrap().unwrap();
        assert_eq!(record.wad_id, Some(wad));
        assert!(!record.manual_override);
    }

    #[test]
    fn a_row_linked_to_a_live_wad_is_left_alone() {
        let conn = setup();
        let other = add_titled_wad(&conn, "Something Else");
        let row = add_row(&conn, "Eisberg", None);
        db::link_wad(&conn, row, other, false).unwrap();

        let wad = add_titled_wad(&conn, "Eisberg");
        link_imported_wad(&conn, wad).unwrap();

        assert_eq!(linked_wad(&conn, row), Some(other));
    }

    #[test]
    fn a_row_linked_to_a_trashed_wad_can_be_claimed() {
        // The Cacowards view already shows this row as absent, and its Import
        // button is one way to end up here.
        let conn = setup();
        let trashed = add_titled_wad(&conn, "Old Copy");
        let row = add_row(&conn, "Eisberg", None);
        db::link_wad(&conn, row, trashed, false).unwrap();
        trash(&conn, trashed);

        let wad = add_titled_wad(&conn, "Eisberg");
        link_imported_wad(&conn, wad).unwrap();

        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    #[test]
    fn a_trashed_wad_neither_owns_a_row_nor_makes_a_title_ambiguous() {
        let conn = setup();
        let by_id = add_row(&conn, "Different Title", Some(100));
        let by_title_row = add_row(&conn, "Eisberg", None);
        trash(&conn, add_idgames_wad(&conn, "Eisberg", "100"));

        let wad = add_titled_wad(&conn, "Eisberg");
        assert_eq!(link_imported_wad(&conn, wad).unwrap(), by_title(1));

        assert_eq!(linked_wad(&conn, by_id), None);
        assert_eq!(linked_wad(&conn, by_title_row), Some(wad));
    }

    #[test]
    fn importing_one_wad_does_not_relink_a_row_the_user_cleared_from_another() {
        let conn = setup();
        let row = add_row(&conn, "Eisberg", None);
        let cleared = add_titled_wad(&conn, "Eisberg");
        db::link_wad(&conn, row, cleared, false).unwrap();
        db::unlink_wad(&conn, row).unwrap();

        let unrelated = add_titled_wad(&conn, "Something Else");
        assert_eq!(
            link_imported_wad(&conn, unrelated).unwrap(),
            LinkCounts::default()
        );
        assert_eq!(linked_wad(&conn, row), None);
    }

    #[test]
    fn only_the_named_rows_are_considered() {
        let conn = setup();
        let wad = add_titled_wad(&conn, "Eisberg");
        let named = add_row_in(&conn, 2025, "Eisberg", None);
        let cleared_earlier = add_row_in(&conn, 2024, "Eisberg", None);

        let counts = link_rows_with_keys(&conn, &keys_of(&conn, &[named])).unwrap();

        assert_eq!(counts, by_title(1));
        assert_eq!(linked_wad(&conn, named), Some(wad));
        assert_eq!(linked_wad(&conn, cleared_earlier), None);
    }

    #[test]
    fn the_counts_add_up() {
        let counts = LinkCounts {
            by_hash: 1,
            by_idgames: 2,
            by_doomworld: 3,
            by_doomwiki: 4,
            by_title: 5,
        };
        assert_eq!(counts.total(), 15);
    }
}
