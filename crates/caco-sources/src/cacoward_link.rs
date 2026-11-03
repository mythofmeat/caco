//! Decides which library WAD a Cacoward row belongs to, and links them.
//!
//! One decision, reached from two moments, because either side can arrive first:
//!
//! - [`link_year`] runs after a year's page is scraped, when the rows are new.
//! - [`link_imported_wad`] runs after every import, when the WAD is new.
//!
//! The decision ([`owner_of`]) is made per row rather than per WAD so its answer
//! cannot depend on order. Walking the library and letting each WAD claim what
//! it matches would let a title match on one WAD take a row that a later WAD
//! owns by idgames id.
//!
//! Only rows [`db::get_linkable_cacowards`] returns are touched, so a pinned
//! link is never overwritten and nothing sets `manual_override`. Nothing here
//! sweeps every year either: the card menu's Unlink clears a row, and a sweep
//! would quietly undo that. A cleared row is only reconsidered when a WAD it
//! matches is imported or its own year is scraped again.

use std::collections::HashMap;

use caco_core::Result;
use caco_core::db::{self, CacowardRecord};
use rusqlite::Connection;

use crate::idgames::extract_idgames_id_from_url;
use crate::import_service::normalize_title;

/// Rows a linking run claimed, by the evidence that decided each one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LinkCounts {
    pub by_idgames: usize,
    pub by_title: usize,
}

/// Link a WAD that just entered the library to the rows that belong to it.
pub(crate) fn link_imported_wad(conn: &Connection, wad_id: i64) -> Result<LinkCounts> {
    let rows = db::get_linkable_cacowards(conn)?;
    link_rows(conn, &rows, |owner| owner == wad_id)
}

/// Link each unclaimed row of `year` to the library WAD it belongs to.
pub(crate) fn link_year(conn: &Connection, year: i64) -> Result<LinkCounts> {
    let mut rows = db::get_linkable_cacowards(conn)?;
    rows.retain(|row| row.year == year);
    link_rows(conn, &rows, |_| true)
}

fn link_rows(
    conn: &Connection,
    rows: &[CacowardRecord],
    wanted: impl Fn(i64) -> bool,
) -> Result<LinkCounts> {
    let titles = TitleIndex::build(conn)?;
    let mut counts = LinkCounts::default();

    for row in rows {
        let Some((wad_id, evidence)) = owner_of(conn, row, &titles)? else {
            continue;
        };
        if !wanted(wad_id) {
            continue;
        }
        db::link_wad(conn, row.id, wad_id, false)?;
        match evidence {
            Evidence::IdgamesId => counts.by_idgames += 1,
            Evidence::Title => counts.by_title += 1,
        }
    }

    Ok(counts)
}

enum Evidence {
    IdgamesId,
    Title,
}

/// The live WAD a row belongs to, strongest evidence first:
///
/// 1. **idgames id**: the numeric id from `{{ig|id=N}}` equals `wads.idgames_id`.
/// 2. **Normalized title**: the row's title equals exactly one WAD's, once both
///    are case-folded, diacritic-stripped and punctuation-collapsed. Two WADs
///    sharing a name (e.g. "Crusader" 1995 and 2023) are left for the user to
///    pick between.
fn owner_of(
    conn: &Connection,
    row: &CacowardRecord,
    titles: &TitleIndex,
) -> Result<Option<(i64, Evidence)>> {
    if let Some(id) = row
        .idgames_url
        .as_deref()
        .and_then(extract_idgames_id_from_url)
        && let Some(wad_id) = db::find_wad_by_idgames_id(conn, &id.to_string())?
    {
        return Ok(Some((wad_id, Evidence::IdgamesId)));
    }

    Ok(titles
        .unique_match(&row.wad_title)
        .map(|wad_id| (wad_id, Evidence::Title)))
}

/// Normalized title → the live WADs that have it. A vec rather than one id so
/// that [`unique_match`](Self::unique_match) can tell "one" from "several".
struct TitleIndex {
    by_title: HashMap<String, Vec<i64>>,
}

impl TitleIndex {
    fn build(conn: &Connection) -> Result<Self> {
        // Pull only id+title; the rest of the wads row is dead weight here.
        let mut stmt = conn.prepare("SELECT id, title FROM wads WHERE deleted_at IS NULL")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut by_title: HashMap<String, Vec<i64>> = HashMap::new();
        for row in rows {
            let (id, title) = row?;
            let key = normalize_title(&title);
            if !key.is_empty() {
                by_title.entry(key).or_default().push(id);
            }
        }
        Ok(Self { by_title })
    }

    fn unique_match(&self, title: &str) -> Option<i64> {
        match self.by_title.get(&normalize_title(title))?.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn add_row_in(conn: &Connection, year: i64, title: &str, idgames_id: Option<i64>) -> i64 {
        db::upsert_cacoward(
            conn,
            &NewCacoward {
                year,
                category: db::CATEGORY_WINNER.to_string(),
                wad_title: title.to_string(),
                idgames_url: idgames_id
                    .map(|id| format!("https://www.doomworld.com/idgames/?id={id}")),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn add_row(conn: &Connection, title: &str, idgames_id: Option<i64>) -> i64 {
        add_row_in(conn, YEAR, title, idgames_id)
    }

    fn linked_wad(conn: &Connection, row: i64) -> Option<i64> {
        db::get_cacoward(conn, row).unwrap().unwrap().wad_id
    }

    fn trash(conn: &Connection, wad_id: i64) {
        db::delete_wad(conn, wad_id, false).unwrap();
    }

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
                by_title: 0
            }
        );
        assert_eq!(linked_wad(&conn, row), Some(wad));
    }

    #[test]
    fn links_by_title_when_the_row_has_no_idgames_id() {
        let conn = setup();
        let row = add_row(&conn, "Eisberg", None);
        let wad = add_titled_wad(&conn, "EISBERG");

        let counts = link_imported_wad(&conn, wad).unwrap();

        assert_eq!(
            counts,
            LinkCounts {
                by_idgames: 0,
                by_title: 1
            }
        );
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
        assert_eq!(link_year(&conn, YEAR).unwrap(), LinkCounts::default());
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
    fn links_every_row_the_wad_owns() {
        let conn = setup();
        let winner = add_row_in(&conn, 2024, "Eisberg", None);
        let later = add_row_in(&conn, 2025, "Eisberg", None);
        let wad = add_titled_wad(&conn, "Eisberg");

        assert_eq!(link_imported_wad(&conn, wad).unwrap().by_title, 2);
        assert_eq!(linked_wad(&conn, winner), Some(wad));
        assert_eq!(linked_wad(&conn, later), Some(wad));
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
    fn link_year_gives_a_contested_row_to_its_idgames_owner() {
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

            let counts = link_year(&conn, YEAR).unwrap();

            assert_eq!(counts.by_idgames, 1, "title_first = {title_first}");
            assert_eq!(counts.by_title, 0);
            assert_eq!(linked_wad(&conn, row), Some(by_id));
        }
    }

    #[test]
    fn a_title_match_still_links_when_no_wad_has_the_rows_idgames_id() {
        // An updated upload can carry a new id while the wiki still names the
        // old one, so a differing id does not prove the title match wrong.
        let conn = setup();
        let row = add_row(&conn, "Eisberg", Some(100));
        let wad = add_idgames_wad(&conn, "Eisberg", "200");

        assert_eq!(link_imported_wad(&conn, wad).unwrap().by_title, 1);
        assert_eq!(linked_wad(&conn, row), Some(wad));
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
        let by_title = add_row(&conn, "Eisberg", None);
        trash(&conn, add_idgames_wad(&conn, "Eisberg", "100"));

        let wad = add_titled_wad(&conn, "Eisberg");
        assert_eq!(link_imported_wad(&conn, wad).unwrap().by_title, 1);

        assert_eq!(linked_wad(&conn, by_id), None);
        assert_eq!(linked_wad(&conn, by_title), Some(wad));
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
    fn link_year_only_touches_that_year() {
        let conn = setup();
        let wad = add_titled_wad(&conn, "Eisberg");
        let this_year = add_row_in(&conn, 2025, "Eisberg", None);
        let cleared_last_year = add_row_in(&conn, 2024, "Eisberg", None);

        let counts = link_year(&conn, 2025).unwrap();

        assert_eq!(counts.by_title, 1);
        assert_eq!(linked_wad(&conn, this_year), Some(wad));
        assert_eq!(linked_wad(&conn, cleared_last_year), None);
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
}
