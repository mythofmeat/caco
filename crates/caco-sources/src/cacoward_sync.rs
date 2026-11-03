//! Keeps the `cacowards` table in step with the curated dataset.
//!
//! This runs on every launch, so what it does on an ordinary one matters most:
//! nothing. The dataset only differs from the table after a release or an edit to a
//! user file, and until it does [`sync_cacowards`] reads and returns, without so much
//! as waiting for the write lock.
//!
//! When something has changed, rows are updated in place and matched on their key,
//! so a corrected title keeps its row, link and pin. A row the dataset no longer has
//! is removed, unless it holds something of the user's. Only rows that are new or
//! whose evidence changed are considered for linking: anything wider would undo the
//! card menu's Unlink on the next launch.

use std::collections::{HashMap, HashSet};

use caco_core::Result;
use caco_core::cacowards::Dataset;
use caco_core::db::{self, CacowardRecord, NewCacoward};
use rusqlite::Connection;

use crate::cacoward_link;

/// What a sync wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub linked: usize,
}

/// Write the dataset to the table and link what it changed.
///
/// One transaction, so a failure partway through leaves the table as it was.
pub fn sync_cacowards(conn: &Connection, dataset: &Dataset) -> Result<SyncReport> {
    if Plan::new(conn, dataset)?.is_empty() {
        return Ok(SyncReport::default());
    }

    // Planned again now that the lock is held: another instance may have got here
    // first, and the first plan was only there to find out if the lock was needed.
    db::with_transaction(conn, |tx| Plan::new(tx, dataset)?.apply(tx))
}

/// What it takes to make the table say what the dataset says.
struct Plan<'a> {
    added: Vec<&'a NewCacoward>,
    updated: Vec<&'a NewCacoward>,
    /// Rows the dataset no longer has and that hold nothing of the user's.
    stale: Vec<i64>,
    /// The rows to consider for linking: new, or with new evidence.
    to_link: HashSet<String>,
}

impl<'a> Plan<'a> {
    fn new(conn: &Connection, dataset: &'a Dataset) -> Result<Self> {
        let rows: HashMap<String, CacowardRecord> = db::search_cacowards(conn)?
            .into_iter()
            .map(|(row, _)| (row.key.clone(), row))
            .collect();

        let mut plan = Plan {
            added: Vec::new(),
            updated: Vec::new(),
            stale: Vec::new(),
            to_link: HashSet::new(),
        };

        for entry in &dataset.entries {
            match rows.get(&entry.key) {
                None => {
                    plan.added.push(entry);
                    plan.to_link.insert(entry.key.clone());
                }
                Some(row) if !row.matches(entry) => {
                    plan.updated.push(entry);
                    if !row.same_evidence(entry) {
                        plan.to_link.insert(entry.key.clone());
                    }
                }
                Some(_) => {}
            }
        }

        let in_dataset: HashSet<&str> = dataset
            .entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect();
        plan.stale = rows
            .values()
            .filter(|row| !in_dataset.contains(row.key.as_str()) && !holds_user_state(row))
            .map(|row| row.id)
            .collect();

        Ok(plan)
    }

    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.stale.is_empty()
    }

    fn apply(&self, conn: &Connection) -> Result<SyncReport> {
        for id in &self.stale {
            db::delete_cacoward(conn, *id)?;
        }
        for entry in self.added.iter().chain(&self.updated) {
            db::upsert_cacoward(conn, entry)?;
        }
        let links = cacoward_link::link_rows_with_keys(conn, &self.to_link)?;

        Ok(SyncReport {
            added: self.added.len(),
            updated: self.updated.len(),
            removed: self.stale.len(),
            linked: links.total(),
        })
    }
}

/// Whether removing a row would lose something only the user knows: a link they
/// chose, or their word on whether it is playable. An auto-link can be worked out
/// again, and a pin whose WAD was purged is a pin on nothing.
fn holds_user_state(row: &CacowardRecord) -> bool {
    (row.manual_override && row.wad_id.is_some()) || row.supported_override.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::cacowards::entry_key;
    use caco_core::db::models::SourceType;
    use caco_core::db::wads::{NewWad, add_wad};
    use caco_core::db::{WadUpdate, init_db, open_connection, open_memory};

    const YEAR: i64 = 2025;

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection, title: &str) -> i64 {
        add_wad(conn, &NewWad::new(title, SourceType::Local)).unwrap()
    }

    fn winner(title: &str) -> NewCacoward {
        NewCacoward {
            key: entry_key(YEAR, db::CATEGORY_WINNER, title),
            year: YEAR,
            category: db::CATEGORY_WINNER.to_string(),
            page_order: 1,
            wad_title: title.to_string(),
            ..Default::default()
        }
    }

    fn dataset(entries: Vec<NewCacoward>) -> Dataset {
        Dataset {
            entries,
            ..Default::default()
        }
    }

    fn rows(conn: &Connection) -> Vec<CacowardRecord> {
        db::search_cacowards(conn)
            .unwrap()
            .into_iter()
            .map(|(row, _)| row)
            .collect()
    }

    fn row_called(conn: &Connection, title: &str) -> CacowardRecord {
        rows(conn)
            .into_iter()
            .find(|row| row.wad_title == title)
            .unwrap_or_else(|| panic!("no row called {title}"))
    }

    /// The table cannot be written to from here on, so anything that tries fails.
    fn refuse_writes(conn: &Connection) {
        conn.execute_batch(
            "CREATE TRIGGER refuse_insert BEFORE INSERT ON cacowards
             BEGIN SELECT RAISE(ABORT, 'wrote to cacowards'); END;
             CREATE TRIGGER refuse_update BEFORE UPDATE ON cacowards
             BEGIN SELECT RAISE(ABORT, 'wrote to cacowards'); END;
             CREATE TRIGGER refuse_delete BEFORE DELETE ON cacowards
             BEGIN SELECT RAISE(ABORT, 'wrote to cacowards'); END;",
        )
        .unwrap();
    }

    // -- writing the dataset --

    #[test]
    fn a_new_library_gets_every_entry() {
        let conn = setup();
        let report = sync_cacowards(&conn, &dataset(vec![winner("One"), winner("Two")])).unwrap();

        assert_eq!(
            report,
            SyncReport {
                added: 2,
                ..Default::default()
            }
        );
        assert_eq!(rows(&conn).len(), 2);
    }

    #[test]
    fn the_builtin_dataset_syncs_into_an_empty_library() {
        let conn = setup();
        let builtin = caco_core::cacowards::builtin();

        let report = sync_cacowards(&conn, &builtin).unwrap();

        assert_eq!(report.added, builtin.entries.len());
        assert_eq!(rows(&conn).len(), builtin.entries.len());
        assert_eq!(
            sync_cacowards(&conn, &builtin).unwrap(),
            SyncReport::default()
        );
    }

    #[test]
    fn a_second_sync_of_the_same_data_writes_nothing() {
        let conn = setup();
        let data = dataset(vec![winner("One"), winner("Two")]);
        sync_cacowards(&conn, &data).unwrap();
        refuse_writes(&conn);

        assert_eq!(sync_cacowards(&conn, &data).unwrap(), SyncReport::default());
    }

    /// The ordinary launch must not wait on another connection's write lock, which
    /// would hold the window back for the 5 s busy timeout and then fail.
    #[test]
    fn an_unchanged_dataset_does_not_need_the_write_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = open_connection(&path).unwrap();
        init_db(&conn).unwrap();
        let data = dataset(vec![winner("One")]);
        sync_cacowards(&conn, &data).unwrap();

        let other = open_connection(&path).unwrap();
        other.execute_batch("BEGIN IMMEDIATE").unwrap();
        conn.busy_timeout(std::time::Duration::ZERO).unwrap();

        assert_eq!(sync_cacowards(&conn, &data).unwrap(), SyncReport::default());
        other.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn a_corrected_title_updates_the_row_in_place() {
        let conn = setup();
        let wad = add_test_wad(&conn, "Hand Picked");
        let original = winner("Lonesome Road");
        sync_cacowards(&conn, &dataset(vec![original.clone()])).unwrap();
        let id = rows(&conn)[0].id;
        db::link_wad(&conn, id, wad, true).unwrap();

        let corrected = NewCacoward {
            wad_title: "Lonesome Road: Chapter 1".to_string(),
            ..original
        };
        let report = sync_cacowards(&conn, &dataset(vec![corrected])).unwrap();

        assert_eq!(report.updated, 1);
        let rows = rows(&conn);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].wad_title, "Lonesome Road: Chapter 1");
        assert_eq!(rows[0].wad_id, Some(wad));
        assert!(rows[0].manual_override);
    }

    #[test]
    fn a_failed_sync_keeps_the_rows_it_started_with() {
        let conn = setup();
        let wad = add_test_wad(&conn, "Survivor");
        sync_cacowards(&conn, &dataset(vec![winner("Survivor")])).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER refuse_row BEFORE INSERT ON cacowards
             WHEN NEW.wad_title = 'Refused'
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();

        let data = dataset(vec![winner("New Entry"), winner("Refused")]);
        assert!(sync_cacowards(&conn, &data).is_err());

        let rows = rows(&conn);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].wad_title, "Survivor");
        assert_eq!(rows[0].wad_id, Some(wad));
    }

    // -- rows the dataset no longer has --

    #[test]
    fn a_row_the_dataset_dropped_is_removed() {
        let conn = setup();
        sync_cacowards(&conn, &dataset(vec![winner("Kept"), winner("Dropped")])).unwrap();

        let report = sync_cacowards(&conn, &dataset(vec![winner("Kept")])).unwrap();

        assert_eq!(report.removed, 1);
        assert_eq!(rows(&conn).len(), 1);
        assert_eq!(rows(&conn)[0].wad_title, "Kept");
    }

    #[test]
    fn an_auto_link_does_not_keep_a_dropped_row() {
        let conn = setup();
        add_test_wad(&conn, "Dropped");
        sync_cacowards(&conn, &dataset(vec![winner("Dropped")])).unwrap();
        assert!(rows(&conn)[0].wad_id.is_some());

        sync_cacowards(&conn, &dataset(vec![])).unwrap();

        assert!(rows(&conn).is_empty());
    }

    #[test]
    fn a_pin_keeps_a_dropped_row() {
        let conn = setup();
        let wad = add_test_wad(&conn, "Hand Picked");
        sync_cacowards(&conn, &dataset(vec![winner("Dropped")])).unwrap();
        db::link_wad(&conn, rows(&conn)[0].id, wad, true).unwrap();

        sync_cacowards(&conn, &dataset(vec![])).unwrap();

        assert_eq!(rows(&conn).len(), 1);
    }

    #[test]
    fn a_pin_on_nothing_does_not_keep_a_dropped_row() {
        let conn = setup();
        let wad = add_test_wad(&conn, "Hand Picked");
        sync_cacowards(&conn, &dataset(vec![winner("Dropped")])).unwrap();
        db::link_wad(&conn, rows(&conn)[0].id, wad, true).unwrap();
        db::delete_wad(&conn, wad, true).unwrap();

        sync_cacowards(&conn, &dataset(vec![])).unwrap();

        assert!(rows(&conn).is_empty());
    }

    #[test]
    fn the_users_word_on_supported_keeps_a_dropped_row() {
        let conn = setup();
        sync_cacowards(&conn, &dataset(vec![winner("Dropped")])).unwrap();
        db::set_supported(&conn, rows(&conn)[0].id, false).unwrap();

        sync_cacowards(&conn, &dataset(vec![])).unwrap();

        assert_eq!(rows(&conn).len(), 1);
    }

    // -- what links, and when --

    #[test]
    fn a_new_row_is_linked_to_the_wad_it_names() {
        let conn = setup();
        let wad = add_test_wad(&conn, "Piña Colada");
        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("idgames_id", Some("20917".to_string())),
        )
        .unwrap();

        let entry = NewCacoward {
            idgames_id: Some(20917),
            ..winner("Pina Colada (the awarded version)")
        };
        let report = sync_cacowards(&conn, &dataset(vec![entry, winner("Dreamblood")])).unwrap();

        assert_eq!(report.added, 2);
        assert_eq!(report.linked, 1);
        let linked = row_called(&conn, "Pina Colada (the awarded version)");
        assert_eq!(linked.wad_id, Some(wad));
        assert!(!linked.manual_override);
    }

    #[test]
    fn an_idgames_import_followed_by_a_sync_links_on_the_id() {
        // The titles differ on purpose: only the id can connect these two.
        let conn = setup();
        let imported: crate::idgames::FileEntry = serde_json::from_value(serde_json::json!({
            "id": 21233,
            "title": "Liminal Doom 2",
            "filename": "ld2.zip",
        }))
        .unwrap();
        let wad_id = crate::import_service::ImportService::new()
            .import_idgames(&conn, &imported, None, false)
            .wad_id
            .unwrap();

        let entry = NewCacoward {
            idgames_id: Some(21233),
            ..winner("Liminal Doom 2: Crossing the Threshold")
        };
        let report = sync_cacowards(&conn, &dataset(vec![entry])).unwrap();

        assert_eq!(report.linked, 1);
        assert_eq!(rows(&conn)[0].wad_id, Some(wad_id));
    }

    #[test]
    fn unlink_sticks_across_launches() {
        // The sync runs at every launch, so reconsidering every row each time would
        // quietly undo this the next time caco opened.
        let conn = setup();
        add_test_wad(&conn, "Eisberg");
        let data = dataset(vec![winner("Eisberg")]);
        sync_cacowards(&conn, &data).unwrap();
        let row = rows(&conn)[0].clone();
        assert!(row.wad_id.is_some());

        db::unlink_wad(&conn, row.id).unwrap();

        assert_eq!(sync_cacowards(&conn, &data).unwrap(), SyncReport::default());
        assert_eq!(rows(&conn)[0].wad_id, None);
    }

    #[test]
    fn a_change_that_is_not_evidence_does_not_reconsider_a_row() {
        let conn = setup();
        add_test_wad(&conn, "Eisberg");
        let original = winner("Eisberg");
        sync_cacowards(&conn, &dataset(vec![original.clone()])).unwrap();
        db::unlink_wad(&conn, rows(&conn)[0].id).unwrap();

        let moved = NewCacoward {
            page_order: 7,
            wad_author: Some("Galactic".to_string()),
            ..original
        };
        let report = sync_cacowards(&conn, &dataset(vec![moved])).unwrap();

        assert_eq!(report.updated, 1);
        assert_eq!(report.linked, 0);
        assert_eq!(rows(&conn)[0].wad_id, None);
        assert_eq!(rows(&conn)[0].page_order, 7);
    }

    #[test]
    fn new_evidence_gets_a_row_considered_again() {
        // The wiki had no id for this one, then a curator found it.
        let conn = setup();
        let wad = add_test_wad(&conn, "A Different Title");
        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("idgames_id", Some("12345".to_string())),
        )
        .unwrap();
        let original = winner("Ray Mohawk");
        sync_cacowards(&conn, &dataset(vec![original.clone()])).unwrap();
        assert_eq!(rows(&conn)[0].wad_id, None);

        let curated = NewCacoward {
            idgames_id: Some(12345),
            ..original
        };
        let report = sync_cacowards(&conn, &dataset(vec![curated])).unwrap();

        assert_eq!(report.linked, 1);
        assert_eq!(rows(&conn)[0].wad_id, Some(wad));
    }

    #[test]
    fn a_data_update_does_not_overwrite_a_pinned_link() {
        let conn = setup();
        let pinned_to = add_test_wad(&conn, "Hand Picked");
        add_test_wad(&conn, "Eisberg");
        let original = winner("Eisberg");
        sync_cacowards(&conn, &dataset(vec![original.clone()])).unwrap();
        let id = rows(&conn)[0].id;
        db::link_wad(&conn, id, pinned_to, true).unwrap();

        let curated = NewCacoward {
            doomworld_id: Some(152701),
            ..original
        };
        let report = sync_cacowards(&conn, &dataset(vec![curated])).unwrap();

        assert_eq!(report.linked, 0);
        let record = db::get_cacoward(&conn, id).unwrap().unwrap();
        assert_eq!(record.wad_id, Some(pinned_to));
        assert!(record.manual_override);
    }

    #[test]
    fn a_title_never_links_to_one_of_two_wads_that_share_it() {
        let conn = setup();
        add_test_wad(&conn, "Crusader");
        add_test_wad(&conn, "crusader");

        let report = sync_cacowards(&conn, &dataset(vec![winner("Crusader")])).unwrap();

        assert_eq!(report.linked, 0);
    }

    // -- supported --

    #[test]
    fn the_datas_supported_flag_is_the_default() {
        let conn = setup();
        let entry = NewCacoward {
            supported: false,
            ..winner("Hedon")
        };

        sync_cacowards(&conn, &dataset(vec![entry])).unwrap();

        assert!(!rows(&conn)[0].supported);
    }

    #[test]
    fn a_data_update_cannot_undo_the_users_supported_choice() {
        let conn = setup();
        let original = winner("Hedon");
        sync_cacowards(&conn, &dataset(vec![original.clone()])).unwrap();
        db::set_supported(&conn, rows(&conn)[0].id, true).unwrap();

        let curated = NewCacoward {
            supported: false,
            ..original
        };
        let report = sync_cacowards(&conn, &dataset(vec![curated])).unwrap();

        assert_eq!(report.updated, 1);
        assert!(rows(&conn)[0].supported);
        assert!(!rows(&conn)[0].supported_default);
    }

    // -- a library from before the dataset --

    /// A row as migration 42 leaves it: keyed from its title, its pin and flag kept,
    /// and no identifiers.
    fn migrated_row(conn: &Connection, title: &str, wad_id: Option<i64>, pinned: bool) -> i64 {
        conn.execute(
            "INSERT INTO cacowards (key, year, category, page_order, wad_title, wad_id, manual_override)
             VALUES (?1, ?2, 'winner', 4, ?3, ?4, ?5)",
            rusqlite::params![
                entry_key(YEAR, db::CATEGORY_WINNER, title),
                YEAR,
                title,
                wad_id,
                pinned,
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn a_migrated_row_meets_its_entry_with_its_pin_intact() {
        let conn = setup();
        let wad = add_test_wad(&conn, "The Evil Reborn");
        let id = migrated_row(&conn, "The Evil Reborn: Resurrected", Some(wad), true);

        let entry = NewCacoward {
            doomworld_id: Some(152129),
            page_order: 19,
            ..winner("The Evil Reborn: Resurrected")
        };
        let report = sync_cacowards(&conn, &dataset(vec![entry])).unwrap();

        assert_eq!(report.added, 0);
        assert_eq!(report.updated, 1);
        let rows = rows(&conn);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].doomworld_id, Some(152129));
        assert_eq!(rows[0].page_order, 19);
        assert_eq!(rows[0].wad_id, Some(wad));
        assert!(rows[0].manual_override);
    }

    #[test]
    fn the_first_sync_links_a_row_that_only_an_identifier_could_reach() {
        // Lonesome Road, which had to be pinned by hand before there were identifiers.
        let conn = setup();
        let wad = add_wad(
            &conn,
            &NewWad::new("Lonesome Road | Chapter 1", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/vb/thread/151858"),
        )
        .unwrap();
        let id = migrated_row(&conn, "Lonesome Road", None, false);

        let entry = NewCacoward {
            doomworld_id: Some(151858),
            ..winner("Lonesome Road")
        };
        let report = sync_cacowards(&conn, &dataset(vec![entry])).unwrap();

        assert_eq!(report.linked, 1);
        assert_eq!(
            db::get_cacoward(&conn, id).unwrap().unwrap().wad_id,
            Some(wad)
        );
    }

    #[test]
    fn a_migrated_row_with_no_entry_goes_unless_it_holds_something_of_the_users() {
        let conn = setup();
        let wad = add_test_wad(&conn, "Hand Picked");
        migrated_row(&conn, "Forgotten", None, false);
        let pinned = migrated_row(&conn, "Pinned", Some(wad), true);

        sync_cacowards(&conn, &dataset(vec![])).unwrap();

        let rows = rows(&conn);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, pinned);
    }
}
