//! Whole machines talking to each other through a folder, which is what all of this is
//! for. Everything goes through the real triggers, the real files and the real merge.
//! What each one ends up with is compared by [`Machine::library`], which is generated
//! from the same table that decides what travels, so nothing can be left out of the
//! comparison by being left out of the log.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{Value, json};

use super::capture::{self, ENTITIES, Shape};
use super::clock::Clock;
use super::event::{Event, Op, VERSION};
use super::saves::{SaveEntry, newer_saves_elsewhere, read_saves, record_saves};
use super::*;
use crate::db::{self, NewWad, SourceType, Status, WadUpdate};

/// A folder they all sync through, and a place for each one's files.
struct World {
    dir: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn log_dir(&self) -> PathBuf {
        self.dir.path().join("sync")
    }

    fn machine(&self, name: &str) -> Machine {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        self.machine_with(name, conn)
    }

    fn machine_with(&self, name: &str, conn: Connection) -> Machine {
        Machine {
            conn,
            paths: self.paths(name),
        }
    }

    fn paths(&self, name: &str) -> SyncPaths {
        SyncPaths {
            log_dir: self.log_dir(),
            machine: name.to_string(),
            wad_data_dir: self.dir.path().join(name).join("data"),
            companion_dir: self.dir.path().join(name).join("companions"),
        }
    }

    /// Put a line in a machine's log, as if it had been synced here.
    fn write_line(&self, machine: &str, line: &str) {
        let dir = self.log_dir().join(machine);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("2026-11.jsonl");
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        text.push_str(line);
        text.push('\n');
        fs::write(path, text).unwrap();
    }

    /// An event from a machine, as a line. It goes by the clock it is given.
    fn event(
        &self,
        machine: &str,
        wall_ms: u64,
        op: Op,
        kind: &str,
        key: &str,
        fields: Value,
    ) -> String {
        let event = Event {
            v: VERSION,
            id: format!("{machine}-{wall_ms}-{kind}-{key}-{}", fields),
            machine: machine.to_string(),
            clock: Clock::at(wall_ms, machine),
            at: "2026-11-05T00:00:00.000Z".to_string(),
            op,
            kind: kind.to_string(),
            key: key.to_string(),
            fields: fields.as_object().cloned().unwrap_or_default(),
        };
        event.to_line().unwrap()
    }
}

struct Machine {
    conn: Connection,
    paths: SyncPaths,
}

impl Machine {
    fn sync(&self) -> SyncReport {
        let report = run(&self.conn, &self.paths);
        assert_eq!(report.write_error, None);
        assert_eq!(report.read_error, None);
        report
    }

    fn add(&self, title: &str) -> i64 {
        db::add_wad(&self.conn, &NewWad::new(title, SourceType::Local)).unwrap()
    }

    /// The row for a WAD another machine made.
    fn wad_named(&self, title: &str) -> db::WadRecord {
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM wads WHERE title = ?1", [title], |row| {
                row.get(0)
            })
            .unwrap_or_else(|e| panic!("no WAD called {title}: {e}"));
        db::get_wad(&self.conn, id, true).unwrap().unwrap()
    }

    fn has_wad(&self, title: &str) -> bool {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM wads WHERE title = ?1",
                [title],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            > 0
    }

    fn count(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn set(&self, wad: i64, field: &'static str, value: &str) {
        db::update_wad(
            &self.conn,
            wad,
            &WadUpdate::new().set_text(field, Some(value.to_string())),
        )
        .unwrap();
    }

    fn play(&self, wad: i64) -> i64 {
        let session = db::start_session(&self.conn, wad, Some("dsda-doom"), None).unwrap();
        db::end_session(&self.conn, session, None, Some(0)).unwrap();
        session
    }

    /// Everything about the library that should be the same on every machine.
    fn library(&self) -> BTreeMap<String, Value> {
        let mut library = BTreeMap::new();
        for entity in ENTITIES {
            let key = capture_key_sql(entity);
            let fields = capture_fields_sql(entity);
            let condition = match entity.shape {
                // Only the part the log has an opinion on, and only rows it has one about.
                Shape::Annotation => {
                    "t.wad_id IS NOT NULL OR t.manual_override <> 0 \
                     OR t.supported_override IS NOT NULL"
                }
                _ => "1",
            };
            let mut statement = self
                .conn
                .prepare(&format!(
                    "SELECT {key}, {fields} FROM {} AS t WHERE {condition}",
                    entity.table
                ))
                .unwrap();
            let rows = statement
                .query_map([], |row| {
                    let key: String = row.get(0)?;
                    let fields: String = row.get(1)?;
                    Ok((key, fields))
                })
                .unwrap();
            for row in rows {
                let (key, fields) = row.unwrap();
                library.insert(
                    format!("{}/{key}", entity.kind),
                    serde_json::from_str(&fields).unwrap(),
                );
            }
        }
        library
    }
}

fn capture_key_sql(entity: &capture::Entity) -> String {
    capture::test_key_sql(entity)
}

fn capture_fields_sql(entity: &capture::Entity) -> String {
    capture::test_fields_sql(entity)
}

/// Sync everyone until nothing more happens. Twice round is enough for any order, and the
/// test fails if a third still has something to do.
fn settle(machines: &[&Machine]) {
    for _ in 0..2 {
        for machine in machines {
            machine.sync();
        }
    }
    for machine in machines {
        let report = machine.sync();
        assert_eq!(
            report.written, 0,
            "{} is still writing",
            machine.paths.machine
        );
        assert_eq!(
            report.applied(),
            0,
            "{} is still merging",
            machine.paths.machine
        );
    }
}

/// Which fields of a row differ, which is all anyone wants from a row this size.
fn difference(here: &Value, there: Option<&Value>) -> String {
    let Some(there) = there else {
        return "the second has no such row".to_string();
    };
    let (Some(here), Some(there)) = (here.as_object(), there.as_object()) else {
        return format!("{here} / {there}");
    };
    here.iter()
        .filter(|(name, value)| there.get(*name) != Some(value))
        .map(|(name, value)| format!("{name}: {value} / {}", there[name]))
        .collect::<Vec<_>>()
        .join(", ")
}

fn assert_same_library(machines: &[&Machine]) {
    let first = machines[0].library();
    for machine in &machines[1..] {
        let other = machine.library();
        for (key, value) in &first {
            let there = other.get(key);
            if Some(value) != there {
                panic!(
                    "{key} differs between {} and {}: {}",
                    machines[0].paths.machine,
                    machine.paths.machine,
                    difference(value, there)
                );
            }
        }
        for key in other.keys() {
            assert!(
                first.contains_key(key),
                "{key} is only on {}",
                machine.paths.machine
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The story in the issue
// ---------------------------------------------------------------------------

/// What #57 is for. Played on two machines between syncs, with a git conflict on one
/// binary file as the old result, and one machine's sessions thrown away.
#[test]
fn test_playing_on_two_machines_between_syncs_loses_nothing() {
    let world = World::new();
    let desktop = world.machine("desktop");
    let laptop = world.machine("laptop");

    let wad = desktop.add("Sunlust");
    desktop.play(wad);
    settle(&[&desktop, &laptop]);
    let there = laptop.wad_named("Sunlust").id;

    // Neither hears from the other while they both do things.
    desktop.play(wad);
    desktop.play(wad);
    db::add_wad_completion(&desktop.conn, wad, None, Some("UV"), None).unwrap();
    desktop.set(wad, "notes", "hard");
    db::add_tag(&desktop.conn, wad, "slaughter").unwrap();

    laptop.play(there);
    db::add_wad_completion(&laptop.conn, there, None, Some("HMP"), None).unwrap();
    db::update_wad(
        &laptop.conn,
        there,
        &WadUpdate::new().set_int("rating", Some(5)),
    )
    .unwrap();
    db::add_tag(&laptop.conn, there, "megawad").unwrap();

    settle(&[&desktop, &laptop]);

    for machine in [&desktop, &laptop] {
        let wad = machine.wad_named("Sunlust");
        assert_eq!(machine.count("sessions"), 4, "{}", machine.paths.machine);
        assert_eq!(machine.count("wad_completions"), 2);
        assert_eq!(wad.notes.as_deref(), Some("hard"));
        assert_eq!(wad.rating, Some(5));
        assert_eq!(wad.tags, ["megawad", "slaughter"]);
    }
    assert_same_library(&[&desktop, &laptop]);
}

#[test]
fn test_a_new_machine_gets_the_whole_library_from_the_logs() {
    let world = World::new();
    let desktop = world.machine("desktop");
    let laptop = world.machine("laptop");

    let a = desktop.add("A");
    let session = desktop.play(a);
    let playthrough = db::start_playthrough(&desktop.conn, a).unwrap();
    desktop
        .conn
        .execute(
            "UPDATE sessions SET playthrough_id = ?1 WHERE id = ?2",
            [playthrough, session],
        )
        .unwrap();
    db::complete_playthrough(&desktop.conn, playthrough, None, Some("done")).unwrap();
    db::add_tag(&desktop.conn, a, "x").unwrap();
    db::create_collection(&desktop.conn, "todo", "status:u", Some("title"), false).unwrap();
    desktop.sync();

    let b = laptop.add("B");
    db::delete_wad(&laptop.conn, b, false).unwrap();
    laptop.sync();

    let new = world.machine("new");
    let report = new.sync();
    assert_eq!(report.merge.waiting, 0);
    assert!(report.changed());
    assert!(
        new.wad_named("B").deleted_at.is_some(),
        "the trash travels too"
    );
    assert_eq!(new.wad_named("A").status, Status::Completed);

    settle(&[&desktop, &laptop, &new]);
    assert_same_library(&[&desktop, &laptop, &new]);
}

// ---------------------------------------------------------------------------
// library.db can go, and come back from elsewhere
// ---------------------------------------------------------------------------

/// The promise that makes the log the truth: nothing is lost with the database.
#[test]
fn test_a_deleted_database_comes_back_from_the_logs() {
    let world = World::new();
    let path = world.dir.path().join("library.db");
    let machine = {
        let conn = db::open_connection(&path).unwrap();
        db::init_db(&conn).unwrap();
        world.machine_with("here", conn)
    };
    let wad = machine.add("Sunlust");
    machine.play(wad);
    db::add_tag(&machine.conn, wad, "x").unwrap();
    machine.set(wad, "notes", "note");
    machine.sync();
    let before = machine.library();
    drop(machine);

    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", path.display()));
    }

    let conn = db::open_connection(&path).unwrap();
    db::init_db(&conn).unwrap();
    let again = world.machine_with("here", conn);
    assert_eq!(again.count("wads"), 0);
    again.sync();
    assert_eq!(again.library(), before);
    assert_eq!(again.wad_named("Sunlust").notes.as_deref(), Some("note"));
}

/// The old way of resolving a conflict: keep the other machine's file. It used to cost
/// everything this machine had done since. Now it costs nothing.
#[test]
fn test_going_back_to_another_machines_database_loses_nothing() {
    let world = World::new();
    let desktop_path = world.dir.path().join("desktop.db");
    let laptop_path = world.dir.path().join("laptop.db");
    let open = |path: &Path, name: &str| {
        let conn = db::open_connection(path).unwrap();
        db::init_db(&conn).unwrap();
        world.machine_with(name, conn)
    };

    let desktop = open(&desktop_path, "desktop");
    let wad = desktop.add("Sunlust");
    desktop.sync();

    let laptop = open(&laptop_path, "laptop");
    laptop.sync();
    let there = laptop.wad_named("Sunlust").id;
    laptop.play(there);
    laptop.set(there, "notes", "from the laptop");
    laptop.sync();
    drop(laptop);

    // The desktop hasn't heard about any of that, and git says to pick one.
    desktop.play(wad);
    desktop.sync();
    desktop
        .conn
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    fs::copy(&desktop_path, &laptop_path).unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", laptop_path.display()));
    }

    let laptop = open(&laptop_path, "laptop");
    laptop.sync();
    assert_eq!(laptop.count("sessions"), 2);
    assert_eq!(
        laptop.wad_named("Sunlust").notes.as_deref(),
        Some("from the laptop")
    );
}

// ---------------------------------------------------------------------------
// Who wins
// ---------------------------------------------------------------------------

#[test]
fn test_two_edits_to_different_fields_both_stick() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    settle(&[&a, &b]);
    let there = b.wad_named("X").id;

    a.set(wad, "notes", "from a");
    b.set(there, "author", "from b");
    settle(&[&a, &b]);

    assert_eq!(a.wad_named("X").notes.as_deref(), Some("from a"));
    assert_eq!(a.wad_named("X").author.as_deref(), Some("from b"));
    assert_same_library(&[&a, &b]);
}

#[test]
fn test_two_edits_to_one_field_end_the_same_on_both_machines() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    settle(&[&a, &b]);
    let there = b.wad_named("X").id;

    a.set(wad, "notes", "from a");
    b.set(there, "notes", "from b");
    settle(&[&a, &b]);

    assert_same_library(&[&a, &b]);
}

/// An hour's clock skew, and a person who read the other machine's note and changed it.
#[test]
fn test_an_edit_made_after_seeing_another_wins_whatever_the_clocks_say() {
    let world = World::new();
    let wad_add = world.event(
        "ahead",
        1_000,
        Op::Add,
        "wad",
        "idgames-1",
        json!({ "title": "X", "source_type": "idgames", "data_key": "idgames-1" }),
    );
    world.write_line("ahead", &wad_add);
    // Far ahead of any clock on this machine.
    let note = world.event(
        "ahead",
        9_000_000_000_000,
        Op::Set,
        "wad",
        "idgames-1",
        json!({ "notes": "from the clock that is ahead" }),
    );
    world.write_line("ahead", &note);

    let behind = world.machine("behind");
    behind.sync();
    let wad = behind.wad_named("X");
    assert_eq!(wad.notes.as_deref(), Some("from the clock that is ahead"));

    behind.set(wad.id, "notes", "read that, and changed it");
    behind.sync();

    let third = world.machine("third");
    third.sync();
    assert_eq!(
        third.wad_named("X").notes.as_deref(),
        Some("read that, and changed it")
    );
}

#[test]
fn test_clearing_a_field_beats_an_older_value() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    a.set(wad, "notes", "note");
    settle(&[&a, &b]);

    db::update_wad(&a.conn, wad, &WadUpdate::new().set_text("notes", None)).unwrap();
    settle(&[&a, &b]);
    assert_eq!(b.wad_named("X").notes, None);
}

#[test]
fn test_a_tag_taken_off_on_one_machine_and_left_on_another_is_taken_off() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    db::add_tag(&a.conn, wad, "x").unwrap();
    settle(&[&a, &b]);
    assert_eq!(b.wad_named("X").tags, ["x"]);

    db::remove_tag(&a.conn, wad, "x").unwrap();
    settle(&[&a, &b]);
    assert!(b.wad_named("X").tags.is_empty());
    assert_same_library(&[&a, &b]);

    // And back again, which a tag can do, as a WAD deleted for good cannot.
    let there = b.wad_named("X").id;
    db::add_tag(&b.conn, there, "x").unwrap();
    settle(&[&a, &b]);
    assert_eq!(a.wad_named("X").tags, ["x"]);
}

#[test]
fn test_the_trash_goes_by_last_writer_like_anything_else() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    settle(&[&a, &b]);

    db::delete_wad(&a.conn, wad, false).unwrap();
    settle(&[&a, &b]);
    assert!(b.wad_named("X").deleted_at.is_some());

    db::restore_wad(&b.conn, b.wad_named("X").id).unwrap();
    settle(&[&a, &b]);
    assert!(a.wad_named("X").deleted_at.is_none());
}

// ---------------------------------------------------------------------------
// The same WAD imported twice
// ---------------------------------------------------------------------------

fn import_sunlust(machine: &Machine) -> i64 {
    db::add_wad(
        &machine.conn,
        &NewWad::new("Sunlust", SourceType::Idgames)
            .source_id("18000")
            .idgames_id("18000"),
    )
    .unwrap()
}

#[test]
fn test_the_same_wad_imported_on_two_machines_is_one_wad() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");

    let on_a = import_sunlust(&a);
    let on_b = import_sunlust(&b);
    a.play(on_a);
    b.play(on_b);
    settle(&[&a, &b]);

    assert_eq!(a.count("wads"), 1);
    assert_eq!(b.count("wads"), 1);
    assert_eq!(a.count("sessions"), 2);
    assert_same_library(&[&a, &b]);
}

/// Importing it again later, on a machine that has not heard, is not someone saying they
/// have not played it, or that they think its notes should go.
#[test]
fn test_a_later_import_elsewhere_does_not_undo_anything() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");

    let on_a = import_sunlust(&a);
    db::update_wad(
        &a.conn,
        on_a,
        &WadUpdate::new()
            .set_status(Status::InProgress)
            .set_text("title", Some("Sunlust (retitled)".into()))
            .set_text("notes", Some("from a".into()))
            .set_int("rating", Some(5)),
    )
    .unwrap();
    a.sync();

    // Without having heard any of that, and with a later clock, for as much as that helps.
    std::thread::sleep(std::time::Duration::from_millis(5));
    import_sunlust(&b);
    settle(&[&a, &b]);

    let wad = b.wad_named("Sunlust (retitled)");
    assert_eq!(wad.status, Status::InProgress);
    assert_eq!(wad.notes.as_deref(), Some("from a"));
    assert_eq!(wad.rating, Some(5));
    assert_same_library(&[&a, &b]);
}

/// In the other order too, or two machines would end up disagreeing about it.
#[test]
fn test_an_edit_beats_an_import_whichever_arrives_first() {
    let world = World::new();
    let add = world.event(
        "later",
        5_000,
        Op::Add,
        "wad",
        "idgames-1",
        json!({ "title": "From the import", "source_type": "idgames" }),
    );
    let edit = world.event(
        "earlier",
        1_000,
        Op::Set,
        "wad",
        "idgames-1",
        json!({ "title": "From the edit" }),
    );
    let add_too = world.event(
        "earlier",
        900,
        Op::Add,
        "wad",
        "idgames-1",
        json!({ "title": "From an earlier import", "source_type": "idgames" }),
    );

    world.write_line("later", &add);
    let first = world.machine("first");
    first.sync();
    assert_eq!(first.count("wads"), 1);

    world.write_line("earlier", &add_too);
    world.write_line("earlier", &edit);
    first.sync();
    assert!(first.has_wad("From the edit"));

    let second = world.machine("second");
    second.sync();
    assert!(second.has_wad("From the edit"));
}

// ---------------------------------------------------------------------------
// Deleted for good
// ---------------------------------------------------------------------------

#[test]
fn test_a_purge_reaches_every_machine_and_stays() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    a.play(wad);
    db::add_tag(&a.conn, wad, "x").unwrap();
    settle(&[&a, &b]);
    assert!(b.has_wad("X"));

    db::delete_wad(&a.conn, wad, true).unwrap();
    settle(&[&a, &b]);
    assert!(!b.has_wad("X"));
    assert_eq!(b.count("sessions"), 0);
    assert_eq!(b.count("tags"), 0);

    let new = world.machine("new");
    new.sync();
    assert!(!new.has_wad("X"), "a replay comes to the same thing");
    assert_same_library(&[&a, &b, &new]);
}

/// Played on one machine, which had not heard it was deleted on the other.
#[test]
fn test_what_was_on_its_way_about_a_purged_wad_does_not_wait_for_ever() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    settle(&[&a, &b]);
    let there = b.wad_named("X").id;

    db::delete_wad(&a.conn, wad, true).unwrap();
    a.sync();

    let session = db::start_session(&b.conn, there, None, None).unwrap();
    b.sync();
    db::end_session(&b.conn, session, None, Some(0)).unwrap();
    b.set(there, "notes", "still playing");
    settle(&[&a, &b]);

    assert!(!a.has_wad("X"));
    assert!(!b.has_wad("X"));
    assert_eq!(a.count("sync_parked"), 0);
    assert_eq!(b.count("sync_parked"), 0);
    assert_same_library(&[&a, &b]);
}

/// A new row under a new name, as the uid's own trigger says, once the purge has been seen.
#[test]
fn test_a_wad_imported_again_after_a_purge_is_a_wad_again() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = import_sunlust(&a);
    settle(&[&a, &b]);
    db::delete_wad(&a.conn, wad, true).unwrap();
    settle(&[&a, &b]);

    let again = import_sunlust(&b);
    assert_ne!(
        db::get_wad(&b.conn, again, false).unwrap().unwrap().uid,
        "idgames-18000"
    );
    settle(&[&a, &b]);
    assert!(a.has_wad("Sunlust"));
    assert_same_library(&[&a, &b]);
}

#[test]
fn test_deleting_a_completion_is_final_everywhere() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    let keep = db::add_wad_completion(&a.conn, wad, None, Some("keep"), None).unwrap();
    let drop = db::add_wad_completion(&a.conn, wad, None, Some("drop"), None).unwrap();
    settle(&[&a, &b]);
    assert_eq!(b.count("wad_completions"), 2);

    db::delete_wad_completion(&a.conn, drop).unwrap();
    db::update_wad_completion(&a.conn, keep, None, Some(Some("kept")), None).unwrap();
    settle(&[&a, &b]);
    let there = db::get_wad_completions(&b.conn, b.wad_named("X").id).unwrap();
    assert_eq!(there.len(), 1);
    assert_eq!(there[0].notes.as_deref(), Some("kept"));
}

// ---------------------------------------------------------------------------
// In any order, any number of times
// ---------------------------------------------------------------------------

/// A sync tool delivers files in the order it pleases.
#[test]
fn test_a_session_that_arrives_before_its_wad_waits_for_it() {
    let world = World::new();
    let session = world.event(
        "laptop",
        2_000,
        Op::Add,
        "session",
        "0123456789abcdef0123456789abcdef",
        json!({ "wad": "idgames-1", "started_at": "2026-11-05T00:00:00Z" }),
    );
    world.write_line("laptop", &session);

    let here = world.machine("here");
    let report = here.sync();
    assert_eq!(report.merge.waiting, 1);
    assert!(!report.changed());
    assert_eq!(parked_counts(&here.conn).unwrap(), (1, 0));
    assert_eq!(here.count("sessions"), 0);

    let wad = world.event(
        "desktop",
        1_000,
        Op::Add,
        "wad",
        "idgames-1",
        json!({ "title": "X", "source_type": "idgames" }),
    );
    world.write_line("desktop", &wad);

    let report = here.sync();
    assert_eq!(report.merge.waiting, 0);
    assert_eq!(here.count("sessions"), 1);
    assert_eq!(parked_counts(&here.conn).unwrap(), (0, 0));
    assert_eq!(report.merge.applied.values().sum::<usize>(), 2);
}

#[test]
fn test_reading_a_log_again_changes_nothing() {
    let world = World::new();
    let a = world.machine("a");
    let wad = a.add("X");
    a.play(wad);
    a.sync();

    let b = world.machine("b");
    b.sync();
    let before = b.library();

    // As a sync tool's restore would: same lines, new time.
    b.conn.execute("DELETE FROM sync_files", []).unwrap();
    let report = b.sync();
    assert_eq!(report.applied(), 0);

    // And if everything is read as if for the first time, it comes to the same thing.
    b.conn.execute("DELETE FROM sync_files", []).unwrap();
    b.conn.execute("DELETE FROM sync_seen", []).unwrap();
    b.sync();
    assert_eq!(b.library(), before);
    assert_eq!(b.count("sessions"), 1);
}

/// What another machine says being applied here is not this machine's to say again.
#[test]
fn test_machines_do_not_echo_each_other() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    a.set(wad, "notes", "note");
    a.play(wad);
    a.sync();

    let report = b.sync();
    assert!(report.changed());
    assert_eq!(b.count("sync_outbox"), 0);
    assert!(!world.log_dir().join("b").exists());
}

/// The test that all the others are special cases of: anything may happen, on any machine,
/// and everyone may hear about it in any order, and they still come out the same.
#[test]
fn test_any_history_in_any_order_ends_the_same_everywhere() {
    for seed in 1u64..=stress_seeds() {
        let world = World::new();
        let machines = [world.machine("a"), world.machine("b"), world.machine("c")];
        let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));

        for _ in 0..200 {
            let machine = &machines[random.below(3)];
            let wads: Vec<i64> = machine
                .conn
                .prepare("SELECT id FROM wads")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            let pick = (!wads.is_empty()).then(|| wads[random.below(wads.len())]);
            random_step(&mut random, machine, pick);
            if random.below(4) == 0 {
                machines[random.below(3)].sync();
            }
        }

        let refs: Vec<&Machine> = machines.iter().collect();
        settle(&refs);
        assert_same_library(&refs);

        // And someone starting from nothing, who only has the files.
        let new = world.machine("new");
        new.sync();
        assert_same_library(&[&machines[0], &new]);
    }
}

fn random_step(random: &mut Random, machine: &Machine, wad: Option<i64>) {
    let conn = &machine.conn;
    let Some(wad) = wad else {
        machine.add(&format!("WAD {}", random.below(1000)));
        return;
    };
    let word = ["a", "b", "c"][random.below(3)];
    match random.below(23) {
        0 => {
            machine.add(&format!("WAD {}", random.below(1000)));
        }
        // The same WAD on several machines, if the dice say so.
        1 => {
            let id = random.below(4).to_string();
            let new = NewWad::new(format!("Imported {id}"), SourceType::Idgames).source_id(&id);
            if db::find_duplicate(conn, SourceType::Idgames, Some(&id), None, None, None)
                .unwrap()
                .is_none()
            {
                db::add_wad(conn, &new).unwrap();
            }
        }
        2 => machine.set(wad, "notes", word),
        3 => machine.set(wad, "title", &format!("Title {word}")),
        4 => {
            db::update_wad(
                conn,
                wad,
                &WadUpdate::new().set_int("rating", Some(random.below(6) as i64)),
            )
            .unwrap();
        }
        5 => {
            let status = Status::ALL[random.below(Status::ALL.len())];
            db::update_wad(conn, wad, &WadUpdate::new().set_status(status)).unwrap();
        }
        6 => {
            db::add_tag(conn, wad, word).unwrap();
        }
        7 => {
            db::remove_tag(conn, wad, word).unwrap();
        }
        8 | 9 => {
            machine.play(wad);
        }
        10 => {
            db::add_wad_completion(conn, wad, None, Some(word), None).unwrap();
        }
        11 => {
            let newest: Option<i64> = conn
                .query_row(
                    "SELECT MAX(id) FROM wad_completions WHERE wad_id = ?1",
                    [wad],
                    |row| row.get(0),
                )
                .unwrap();
            if let Some(completion) = newest {
                db::delete_wad_completion(conn, completion).unwrap();
            }
        }
        12 => {
            db::delete_wad(conn, wad, false).unwrap();
        }
        13 => {
            db::restore_wad(conn, wad).unwrap();
        }
        14 => {
            if random.below(4) == 0 {
                db::delete_wad(conn, wad, true).unwrap();
            }
        }
        // Progress by map, and starting over, which merge rather than overwrite.
        15 => {
            let map = format!("MAP{:02}", random.below(4));
            machine.set(
                wad,
                "stats_snapshot",
                &stats(&[(&map, 1 + random.below(3) as i32)]),
            );
        }
        16 => {
            if random.below(3) == 0 {
                db::update_wad(
                    conn,
                    wad,
                    &WadUpdate::new().set_text("stats_snapshot", None),
                )
                .unwrap();
            }
        }
        17 => {
            if db::get_active_playthrough(conn, wad).unwrap().is_none() {
                db::start_playthrough(conn, wad).unwrap();
            }
        }
        18 => {
            if let Some(active) = db::get_active_playthrough(conn, wad).unwrap() {
                db::complete_playthrough(conn, active.id, None, Some(word)).unwrap();
            }
        }
        19 => {
            let session: Option<i64> = conn
                .query_row(
                    "SELECT MAX(id) FROM sessions WHERE wad_id = ?1",
                    [wad],
                    |row| row.get(0),
                )
                .unwrap();
            if let Some(session) = session {
                db::update_session_stats(conn, session, Some(word), Some(word)).unwrap();
            }
        }
        // What a file is. Nobody here has one, so it goes by last writer.
        20 => {
            conn.execute(
                "UPDATE wads SET file_size = ?1, file_sha256 = ?2, file_sha1 = ?2, \
                 content_sha256 = ?2 WHERE id = ?3",
                rusqlite::params![random.below(9) as i64, word, wad],
            )
            .unwrap();
        }
        21 => {
            let md5 = format!("{:032x}", random.below(3));
            let existing = db::find_companion_by_md5(conn, &md5).unwrap();
            let id = match existing {
                Some(existing) => existing.id,
                None => db::add_companion(conn, &md5, "patch.deh", "/here/patch.deh", 9).unwrap(),
            };
            match random.below(3) {
                0 => {
                    db::link_companion_to_wad(conn, wad, id).unwrap();
                }
                1 => {
                    db::unlink_companion_from_wad(conn, wad, id).unwrap();
                }
                _ => {
                    db::set_companion_enabled(conn, wad, id, random.below(2) == 0).unwrap();
                }
            }
        }
        _ => {
            let name = format!("collection {word}");
            if db::get_collection(conn, &name).unwrap().is_some() {
                db::delete_collection(conn, &name).unwrap();
            } else {
                db::create_collection(conn, &name, "status:u", None, true).unwrap();
            }
        }
    }
}

/// xorshift, so that a failure comes back the same every run without a dependency.
struct Random(u64);

impl Random {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

// ---------------------------------------------------------------------------
// A log written by this version, read by later ones, and the other way about
// ---------------------------------------------------------------------------

/// Never regenerate this file. It is what a log written by this version looks like to
/// every later one, and when a change breaks this, the change has to read it, or bump
/// [`VERSION`] and read it.
const LOG_FROM_V1: &str = include_str!("testdata/v1.jsonl");

#[test]
fn test_a_log_from_version_1_replays_for_ever() {
    let world = World::new();
    for line in LOG_FROM_V1.lines() {
        world.write_line("old-machine", line);
    }
    let here = world.machine("here");
    here.conn
        .execute(
            "INSERT INTO cacowards (key, year, category, page_order, wad_title) \
             VALUES ('2025-winner-sunlust', 2025, 'winner', 1, 'Sunlust')",
            [],
        )
        .unwrap();
    let report = here.sync();
    assert_eq!(report.merge.unreadable, 0);
    assert_eq!(report.merge.waiting, 0);
    assert_eq!(report.merge.too_new, 0);
    assert_eq!(report.applied(), LOG_FROM_V1.lines().count());

    let wad = here.wad_named("Sunlust");
    assert_eq!(wad.uid, "idgames-18000");
    assert_eq!(wad.source_type, SourceType::Idgames);
    assert_eq!(wad.source_id.as_deref(), Some("18000"));
    assert_eq!(wad.data_key, "101");
    assert_eq!(wad.status, Status::Completed);
    assert_eq!(wad.rating, Some(5));
    assert_eq!(wad.notes.as_deref(), Some("hard"));
    assert_eq!(wad.custom_iwad.as_deref(), Some("doom2"));
    assert_eq!(wad.tags, ["megawad"]);
    let identity = wad.identity.unwrap();
    assert_eq!(identity.file.size, 4);
    assert_eq!(identity.file.sha256, "aa");

    let sessions = db::get_sessions(&here.conn, wad.id).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].duration_seconds, Some(1200));
    assert_eq!(sessions[0].sourceport.as_deref(), Some("dsda-doom"));

    let completions = db::get_wad_completions(&here.conn, wad.id).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].notes.as_deref(), Some("UV"));

    let playthroughs = db::get_playthroughs(&here.conn, wad.id).unwrap();
    assert_eq!(playthroughs.len(), 1);
    assert!(playthroughs[0].completed_at.is_some());

    let link: i64 = here
        .conn
        .query_row(
            "SELECT wad_id FROM cacowards WHERE key = '2025-winner-sunlust'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(link, wad.id);

    assert_eq!(
        db::get_collection(&here.conn, "todo")
            .unwrap()
            .unwrap()
            .query,
        "status:u"
    );
    let companions = db::get_companions_for_wad(&here.conn, wad.id).unwrap();
    assert_eq!(companions.len(), 1);
    assert_eq!(companions[0].filename, "patch.deh");
    assert_eq!(here.count("wad_saves"), 1);
}

/// Not understood is not the same as not interesting. Marked as seen, it would be gone for
/// good, so it waits for this build to be replaced by one that can read it.
#[test]
fn test_a_line_from_a_newer_version_waits_for_an_upgrade() {
    let world = World::new();
    let clock = Clock::at(5_000, "future");
    world.write_line(
        "future",
        &format!(
            r#"{{"v":{},"id":"from-the-future","clock":"{clock}","whatever":"it wants"}}"#,
            VERSION + 1
        ),
    );

    let here = world.machine("here");
    let report = here.sync();
    assert_eq!(report.merge.too_new, 1);
    assert_eq!(report.merge.unreadable, 0);
    assert_eq!(here.count("sync_seen"), 0);
    assert_eq!(parked_counts(&here.conn).unwrap(), (0, 1));

    // Nothing has changed, so it is not read again, and it is still known about.
    let report = here.sync();
    assert_eq!(report.merge.too_new, 1);
    assert_eq!(parked_counts(&here.conn).unwrap(), (0, 1));
}

/// As would be left by an upgrade: the file is unchanged, and this build can read the line.
#[test]
fn test_what_waited_for_an_upgrade_is_applied_after_one() {
    let world = World::new();
    let here = world.machine("here");
    let line = world.event(
        "other",
        1_000,
        Op::Add,
        "wad",
        "idgames-1",
        json!({ "title": "X", "source_type": "idgames" }),
    );
    here.conn
        .execute(
            "INSERT INTO sync_parked (id, clock, line, reason) \
             VALUES ('old-id', '0000000001000.000000.other', ?1, 'upgrade')",
            [&line],
        )
        .unwrap();

    let report = here.sync();
    assert_eq!(report.applied(), 1);
    assert!(here.has_wad("X"));
    assert_eq!(parked_counts(&here.conn).unwrap(), (0, 0));
}

#[test]
fn test_a_kind_this_build_has_not_heard_of_waits_for_an_upgrade() {
    let world = World::new();
    world.write_line(
        "future",
        &world.event("future", 1_000, Op::Set, "hologram", "x", json!({ "y": 1 })),
    );
    let here = world.machine("here");
    assert_eq!(here.sync().merge.too_new, 1);
}

/// What later versions add to a row, this one leaves alone, and applies the rest of.
#[test]
fn test_a_field_this_build_has_not_heard_of_is_ignored() {
    let world = World::new();
    world.write_line(
        "future",
        &world.event(
            "future",
            1_000,
            Op::Add,
            "wad",
            "idgames-1",
            json!({ "title": "X", "source_type": "idgames", "difficulty": "nightmare" }),
        ),
    );
    let here = world.machine("here");
    here.sync();
    assert!(here.has_wad("X"));
}

#[test]
fn test_lines_nobody_could_have_written_are_counted_and_passed_over() {
    let world = World::new();
    world.write_line("other", "<<<<<<< HEAD");
    world.write_line(
        "other",
        &world.event(
            "other",
            1_000,
            Op::Add,
            "wad",
            "idgames-1",
            json!({ "title": "X", "source_type": "idgames" }),
        ),
    );
    world.write_line("other", "=======");
    world.write_line("other", r#"{"half of a line"#);

    let here = world.machine("here");
    let report = here.sync();
    assert_eq!(report.merge.unreadable, 3);
    assert!(here.has_wad("X"));
}

// ---------------------------------------------------------------------------
// A log is a file somebody else can write to
// ---------------------------------------------------------------------------

/// Either of these names a path on this machine, or a value that would make the row
/// unreadable. A row with one of them must still come through, without it.
#[test]
fn test_what_a_log_says_is_checked_before_it_gets_anywhere() {
    let world = World::new();
    let line = world.event(
        "other",
        1_000,
        Op::Add,
        "wad",
        "idgames-1",
        json!({
            "title": "X",
            "source_type": "idgames",
            "filename": "../../.bashrc",
            "custom_iwad": "/etc/passwd",
            "custom_sourceport": "..\\evil.exe",
            "custom_config": "../../../somewhere",
            "data_key": "../../etc",
            "year": "not a year",
            "rating": { "nested": true },
            "status": "owned",
            "file_size": "big",
        }),
    );
    world.write_line("other", &line);

    let here = world.machine("here");
    here.sync();
    let wad = here.wad_named("X");
    assert_eq!(wad.filename, None);
    assert_eq!(wad.custom_iwad, None);
    assert_eq!(wad.custom_sourceport, None);
    assert_eq!(wad.custom_config, None);
    assert_eq!(
        wad.data_key, "idgames-1",
        "the key it would have had anyway"
    );
    assert_eq!(wad.year, None);
    assert_eq!(wad.rating, None);
    assert_eq!(wad.status, Status::Unplayed);
}

#[test]
fn test_a_new_row_with_an_impossible_type_does_not_get_in() {
    let world = World::new();
    world.write_line(
        "other",
        &world.event(
            "other",
            1_000,
            Op::Add,
            "wad",
            "idgames-1",
            json!({ "title": "X", "source_type": "ftp" }),
        ),
    );
    let here = world.machine("here");
    here.sync();
    assert_eq!(here.count("wads"), 0);
}

#[test]
fn test_a_key_that_could_not_be_one_is_not_applied() {
    let world = World::new();
    world.write_line(
        "other",
        &world.event(
            "other",
            1_000,
            Op::Add,
            "wad",
            "../../etc",
            json!({ "title": "X", "source_type": "idgames" }),
        ),
    );
    let here = world.machine("here");
    here.sync();
    assert_eq!(here.count("wads"), 0);
    assert_eq!(here.count("sync_parked"), 0);
}

#[test]
fn test_a_companion_with_a_path_for_a_name_goes_nowhere() {
    let world = World::new();
    let md5 = "0123456789abcdef0123456789abcdef";
    world.write_line(
        "other",
        &world.event(
            "other",
            1_000,
            Op::Set,
            "companion",
            md5,
            json!({ "filename": "../../.bashrc", "size": 4, "present": 1 }),
        ),
    );
    world.write_line(
        "other",
        &world.event(
            "other",
            1_001,
            Op::Set,
            "companion",
            "../../etc",
            json!({ "filename": "ok.deh", "size": 4, "present": 1 }),
        ),
    );
    let here = world.machine("here");
    here.sync();
    assert_eq!(here.count("companion_files_registry"), 0);
}

// ---------------------------------------------------------------------------
// What stays on its own machine
// ---------------------------------------------------------------------------

#[test]
fn test_where_a_file_is_stays_on_the_machine_that_has_it() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    a.set(wad, "cached_path", "/on/a/x.wad");
    a.sync();
    b.sync();
    let there = b.wad_named("X");
    assert_eq!(there.cached_path, None);

    b.set(there.id, "cached_path", "/on/b/x.wad");
    b.set(there.id, "notes", "note");
    settle(&[&a, &b]);
    assert_eq!(a.wad_named("X").cached_path.as_deref(), Some("/on/a/x.wad"));
    assert_eq!(b.wad_named("X").cached_path.as_deref(), Some("/on/b/x.wad"));
    assert_eq!(a.wad_named("X").notes.as_deref(), Some("note"));
}

fn set_identity(machine: &Machine, wad: i64, sha: &str) {
    machine
        .conn
        .execute(
            "UPDATE wads SET file_size = 4, file_sha256 = ?1, file_sha1 = 'x', \
             content_sha256 = ?1 WHERE id = ?2",
            rusqlite::params![sha, wad],
        )
        .unwrap();
}

/// What names the file to anyone asked for it, which a machine without one needs most.
#[test]
fn test_a_machine_with_no_file_learns_what_the_file_is() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    set_identity(&a, wad, "aaaa");
    settle(&[&a, &b]);

    let identity = b.wad_named("X").identity.unwrap();
    assert_eq!(identity.file.sha256, "aaaa");
    assert_eq!(identity.content_sha256, "aaaa");
}

/// With a file, a row says what that file is. If it took the other machine's word, the
/// stamp would still match, and nothing would look again: it would be wrong for good.
#[test]
fn test_a_machine_with_its_own_file_keeps_saying_what_that_file_is() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    settle(&[&a, &b]);
    let there = b.wad_named("X").id;
    b.set(there, "cached_path", "/on/b/x.wad");
    set_identity(&b, there, "bbbb");
    b.sync();

    set_identity(&a, wad, "aaaa");
    settle(&[&a, &b]);

    assert_eq!(b.wad_named("X").identity.unwrap().file.sha256, "bbbb");
}

/// #54 and #52 meeting: the file was never on this machine, and it can still be asked for.
#[test]
fn test_a_hash_from_another_machine_is_enough_to_find_the_file_on_a_mirror() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    set_identity(&a, wad, "aaaa");
    settle(&[&a, &b]);

    let there = b.wad_named("X");
    assert!(!there.mirrored);
    assert!(!there.retrievability().is_automatic());

    b.conn
        .execute(
            "INSERT INTO mirror_files (mirror, sha256, size, filename, content_sha256) \
             VALUES ('home', 'aaaa', 4, 'x.wad', 'aaaa')",
            [],
        )
        .unwrap();
    let there = b.wad_named("X");
    assert!(there.mirrored);
    assert!(there.retrievability().is_automatic());
}

// ---------------------------------------------------------------------------
// Stats go by map
// ---------------------------------------------------------------------------

fn stats(maps: &[(&str, i32)]) -> String {
    let maps: Vec<Value> = maps
        .iter()
        .map(|(lump, exits)| {
            json!({
                "lump": lump, "kills": 0, "total_kills": -1, "items": 0, "total_items": -1,
                "secrets": 0, "total_secrets": -1, "episode": 1, "map_num": 1, "best_skill": 4,
                "best_time": -1, "best_max_time": -1, "best_nm_time": -1, "total_exits": exits,
                "cumulative_kills": 0, "time_secs": -1.0, "total_time_secs": -1.0
            })
        })
        .collect();
    json!({ "format": "stats_txt", "maps": maps, "version": 1, "header_total_kills": 0 })
        .to_string()
}

fn exits(machine: &Machine, title: &str) -> BTreeMap<String, i32> {
    let wad = machine.wad_named(title);
    let Some(snapshot) = wad.stats_snapshot else {
        return BTreeMap::new();
    };
    crate::wad_stats::stats_from_json(&snapshot)
        .unwrap()
        .maps
        .into_iter()
        .map(|map| (map.lump, map.total_exits))
        .collect()
}

#[test]
fn test_progress_on_each_machine_adds_up() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    settle(&[&a, &b]);
    let there = b.wad_named("X").id;

    a.set(wad, "stats_snapshot", &stats(&[("MAP01", 1), ("MAP02", 1)]));
    b.set(there, "stats_snapshot", &stats(&[("MAP03", 2)]));
    settle(&[&a, &b]);

    let expected: BTreeMap<_, _> = [("MAP01", 1), ("MAP02", 1), ("MAP03", 2)]
        .map(|(lump, exits)| (lump.to_string(), exits))
        .into();
    assert_eq!(exits(&a, "X"), expected);
    assert_eq!(exits(&b, "X"), expected);
}

/// Starting a playthrough over is a moment in time. What came before it stays gone, even
/// from a machine that had not heard, and what came after counts.
#[test]
fn test_starting_over_stays_started_over() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    a.set(wad, "stats_snapshot", &stats(&[("MAP01", 1)]));
    settle(&[&a, &b]);
    assert_eq!(exits(&b, "X").len(), 1);

    db::update_wad(
        &a.conn,
        wad,
        &WadUpdate::new().set_text("stats_snapshot", None),
    )
    .unwrap();
    a.sync();
    b.sync();
    assert!(b.wad_named("X").stats_snapshot.is_none());
    assert_eq!(b.count("wad_stats_by_machine"), 0);

    // A replay of the old progress, from a machine with its clock in the past.
    world.write_line(
        "old",
        &world.event(
            "old",
            1_000,
            Op::Set,
            "wad",
            &a.wad_named("X").uid,
            json!({ "stats_snapshot": stats(&[("MAP31", 1)]) }),
        ),
    );
    b.sync();
    assert!(b.wad_named("X").stats_snapshot.is_none());

    // The new attempt, from whoever makes it.
    let there = b.wad_named("X").id;
    b.set(there, "stats_snapshot", &stats(&[("MAP05", 1)]));
    settle(&[&a, &b]);
    assert_eq!(exits(&a, "X").keys().collect::<Vec<_>>(), ["MAP05"]);
}

/// Nothing has changed for someone with one machine: the row follows the file, even
/// backwards, as it always has. Only what another machine said is kept against it.
#[test]
fn test_a_machine_on_its_own_follows_its_own_stats() {
    let world = World::new();
    let a = world.machine("a");
    let wad = a.add("X");
    a.set(wad, "stats_snapshot", &stats(&[("MAP01", 1)]));
    a.sync();
    let disk = stats(&[("MAP02", 1)]);
    assert_eq!(
        merge::with_other_machines(&a.conn, wad, &disk).unwrap(),
        disk
    );
    a.set(wad, "stats_snapshot", &disk);
    a.sync();
    assert_eq!(exits(&a, "X").keys().collect::<Vec<_>>(), ["MAP02"]);
}

/// A second machine's map counts towards finishing it, here as anywhere.
#[test]
fn test_what_this_machine_knows_includes_what_others_said() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    a.set(wad, "stats_snapshot", &stats(&[("MAP01", 1)]));
    settle(&[&a, &b]);
    let there = b.wad_named("X").id;

    let known = merge::with_other_machines(&b.conn, there, &stats(&[("MAP02", 1)])).unwrap();
    let maps: Vec<_> = crate::wad_stats::stats_from_json(&known)
        .unwrap()
        .maps
        .into_iter()
        .map(|map| map.lump)
        .collect();
    assert_eq!(maps, ["MAP01", "MAP02"]);
}

/// The order this exists for. A clear that arrives after newer progress has to leave that
/// progress there, and one that arrives before it has to end the same way.
#[test]
fn test_a_clear_and_newer_progress_end_the_same_in_either_order() {
    let wad = world_wad_add();
    let progress_before = |world: &World| {
        world.event(
            "a",
            2_000,
            Op::Set,
            "wad",
            "idgames-1",
            json!({ "stats_snapshot": stats(&[("MAP01", 1)]) }),
        )
    };
    let clear = |world: &World| {
        world.event(
            "a",
            3_000,
            Op::Set,
            "wad",
            "idgames-1",
            json!({ "stats_snapshot": null }),
        )
    };
    let progress_after = |world: &World| {
        world.event(
            "b",
            4_000,
            Op::Set,
            "wad",
            "idgames-1",
            json!({ "stats_snapshot": stats(&[("MAP05", 1)]) }),
        )
    };

    // Progress from after it first, which the clear must leave alone.
    let world = World::new();
    world.write_line(
        "o",
        &world.event("o", 1_000, Op::Add, "wad", "idgames-1", wad.clone()),
    );
    world.write_line("a", &progress_before(&world));
    let here = world.machine("here");
    here.sync();
    world.write_line("b", &progress_after(&world));
    here.sync();
    assert_eq!(exits(&here, "X").len(), 2);
    world.write_line("a", &clear(&world));
    here.sync();
    assert_eq!(exits(&here, "X").keys().collect::<Vec<_>>(), ["MAP05"]);

    // In order, for someone with all of it.
    let fresh = world.machine("fresh");
    fresh.sync();
    assert_eq!(exits(&fresh, "X").keys().collect::<Vec<_>>(), ["MAP05"]);
    assert_same_library(&[&here, &fresh]);
}

/// All written out together, after the clear's trigger has already been and gone.
#[test]
fn test_progress_cleared_before_it_was_written_out_is_gone() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    settle(&[&a, &b]);

    a.set(wad, "stats_snapshot", &stats(&[("MAP01", 1)]));
    db::update_wad(
        &a.conn,
        wad,
        &WadUpdate::new().set_text("stats_snapshot", None),
    )
    .unwrap();
    let there = b.wad_named("X").id;
    b.set(there, "stats_snapshot", &stats(&[("MAP02", 1)]));
    settle(&[&a, &b]);

    assert_same_library(&[&a, &b]);
    assert!(!exits(&a, "X").contains_key("MAP01"));
}

/// What a new WAD's event says, including what would otherwise default to the time of
/// whichever machine reads it, as the trigger's always does.
fn world_wad_add() -> Value {
    json!({ "title": "X", "source_type": "idgames", "created_at": "2026-11-05 00:00:00" })
}

// ---------------------------------------------------------------------------
// The folder a WAD's data is in
// ---------------------------------------------------------------------------

#[test]
fn test_a_wads_folder_is_the_same_on_every_machine() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    let on_a = db::get_wad(&a.conn, wad, false).unwrap().unwrap();
    assert_eq!(on_a.data_key, on_a.uid);
    settle(&[&a, &b]);
    assert_eq!(b.wad_named("X").data_key, on_a.data_key);

    // The rowid is free to be anything on the other machine, and in a library with more
    // history than this one, it would be.
    let b_extra = world.machine("c");
    b_extra.add("Something else first");
    b_extra.sync();
    assert_eq!(b_extra.wad_named("X").data_key, on_a.data_key);
    assert_ne!(b_extra.wad_named("X").id, on_a.id);
}

/// A WAD made independently on two machines before there were uids, so that each has its
/// own folder. Whichever folder wins, the other machine's saves must not be left behind.
#[test]
fn test_a_folder_named_for_a_key_that_lost_comes_along() {
    let world = World::new();
    let here = world.machine("here");
    let wad = db::add_wad(
        &here.conn,
        &NewWad::new("Sunlust", SourceType::Idgames).source_id("18000"),
    )
    .unwrap();
    here.conn
        .execute("UPDATE wads SET data_key = '117' WHERE id = ?1", [wad])
        .unwrap();
    here.sync();
    let saves = here.paths.wad_data_dir.join("117_sunlust/saves/slot1.dsg");
    fs::create_dir_all(saves.parent().unwrap()).unwrap();
    fs::write(&saves, b"save").unwrap();

    world.write_line(
        "other",
        &world.event(
            "other",
            u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap() + 60_000,
            Op::Set,
            "wad",
            "idgames-18000",
            json!({ "data_key": "120" }),
        ),
    );
    here.sync();

    assert_eq!(here.wad_named("Sunlust").data_key, "120");
    assert!(
        here.paths
            .wad_data_dir
            .join("120_sunlust/saves/slot1.dsg")
            .is_file()
    );
    assert!(!here.paths.wad_data_dir.join("117_sunlust").exists());
}

// ---------------------------------------------------------------------------
// What else hangs off a WAD
// ---------------------------------------------------------------------------

#[test]
fn test_a_cacoward_link_and_flag_travel_and_the_facts_do_not() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    for machine in [&a, &b] {
        machine
            .conn
            .execute(
                "INSERT INTO cacowards (key, year, category, page_order, wad_title) \
                 VALUES ('2025-winner-x', 2025, 'winner', 1, 'X')",
                [],
            )
            .unwrap();
    }
    let wad = a.add("X");
    let pk: i64 = a
        .conn
        .query_row("SELECT id FROM cacowards", [], |row| row.get(0))
        .unwrap();
    db::link_wad(&a.conn, pk, wad, true).unwrap();
    db::set_supported(&a.conn, pk, false).unwrap();
    settle(&[&a, &b]);

    let (wad_id, pinned, supported): (i64, i64, Option<i64>) = b
        .conn
        .query_row(
            "SELECT wad_id, manual_override, supported_override FROM cacowards",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(wad_id, b.wad_named("X").id);
    assert_eq!(pinned, 1);
    assert_eq!(supported, Some(0));

    let pk_there: i64 = b
        .conn
        .query_row("SELECT id FROM cacowards", [], |row| row.get(0))
        .unwrap();
    db::unlink_wad(&b.conn, pk_there).unwrap();
    settle(&[&a, &b]);
    let linked: Option<i64> = a
        .conn
        .query_row("SELECT wad_id FROM cacowards", [], |row| row.get(0))
        .unwrap();
    assert_eq!(linked, None);
}

/// An entry this build's dataset does not have yet may be in the next one.
#[test]
fn test_a_link_to_an_entry_not_in_this_dataset_waits_for_it() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    a.conn
        .execute(
            "INSERT INTO cacowards (key, year, category, page_order, wad_title) \
             VALUES ('2030-winner-x', 2030, 'winner', 1, 'X')",
            [],
        )
        .unwrap();
    let wad = a.add("X");
    let pk: i64 = a
        .conn
        .query_row("SELECT id FROM cacowards", [], |row| row.get(0))
        .unwrap();
    db::link_wad(&a.conn, pk, wad, false).unwrap();
    a.sync();

    assert_eq!(b.sync().merge.waiting, 1);

    b.conn
        .execute(
            "INSERT INTO cacowards (key, year, category, page_order, wad_title) \
             VALUES ('2030-winner-x', 2030, 'winner', 1, 'X')",
            [],
        )
        .unwrap();
    assert_eq!(b.sync().merge.waiting, 0);
    let linked: i64 = b
        .conn
        .query_row("SELECT wad_id FROM cacowards", [], |row| row.get(0))
        .unwrap();
    assert_eq!(linked, b.wad_named("X").id);
}

/// The registry row has a path, which is this machine's. The hash and the name are not.
#[test]
fn test_a_companion_is_filed_where_this_machine_keeps_them() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    let wad = a.add("X");
    let md5 = "0123456789abcdef0123456789abcdef";
    let id = db::add_companion(&a.conn, md5, "patch.deh", "/on/a/patch.deh", 9).unwrap();
    db::link_companion_to_wad(&a.conn, wad, id).unwrap();
    db::set_companion_enabled(&a.conn, wad, id, false).unwrap();
    settle(&[&a, &b]);

    let there = db::get_companions_for_wad(&b.conn, b.wad_named("X").id).unwrap();
    assert_eq!(there.len(), 1);
    assert!(!there[0].enabled);
    assert_eq!(
        Path::new(&there[0].path),
        b.paths.companion_dir.join("0123456789ab_patch.deh")
    );

    db::unlink_companion_from_wad(&a.conn, wad, id).unwrap();
    settle(&[&a, &b]);
    assert!(
        db::get_companions_for_wad(&b.conn, b.wad_named("X").id)
            .unwrap()
            .is_empty()
    );
    assert_same_library(&[&a, &b]);
}

#[test]
fn test_a_collection_can_go_and_come_back() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    db::create_collection(&a.conn, "todo", "status:u", Some("title"), false).unwrap();
    settle(&[&a, &b]);
    let there = db::get_collection(&b.conn, "todo").unwrap().unwrap();
    assert_eq!(there.query, "status:u");
    assert_eq!(there.sort_by.as_deref(), Some("title"));
    assert!(!there.sort_desc);

    db::update_collection(&b.conn, "todo", Some("status:p"), None, None).unwrap();
    settle(&[&a, &b]);
    assert_eq!(
        db::get_collection(&a.conn, "todo").unwrap().unwrap().query,
        "status:p"
    );

    db::delete_collection(&a.conn, "todo").unwrap();
    settle(&[&a, &b]);
    assert!(db::get_collection(&b.conn, "todo").unwrap().is_none());

    db::create_collection(&b.conn, "todo", "status:c", None, true).unwrap();
    settle(&[&a, &b]);
    assert_eq!(
        db::get_collection(&a.conn, "todo").unwrap().unwrap().query,
        "status:c"
    );
}

// ---------------------------------------------------------------------------
// A library from before the log
// ---------------------------------------------------------------------------

/// Take a library back to before this existed, as an old binary would have left it.
fn rewind_to_before_sync(conn: &Connection) {
    capture::uninstall(conn).unwrap();
    conn.execute_batch(
        "DROP INDEX idx_wads_uid; DROP INDEX idx_sessions_uid;
         DROP INDEX idx_wad_completions_uid; DROP INDEX idx_playthroughs_uid;
         ALTER TABLE wads DROP COLUMN uid; ALTER TABLE wads DROP COLUMN data_key;
         ALTER TABLE sessions DROP COLUMN uid; ALTER TABLE wad_completions DROP COLUMN uid;
         ALTER TABLE playthroughs DROP COLUMN uid;
         DROP TABLE sync_outbox; DROP TABLE sync_clocks; DROP TABLE sync_seen;
         DROP TABLE sync_parked; DROP TABLE sync_files; DROP TABLE sync_tombstones;
         DROP TABLE sync_machines; DROP TABLE sync_state; DROP TABLE sync_applying;
         DROP TABLE wad_saves; DROP TABLE wad_stats_by_machine;
         DELETE FROM schema_migrations WHERE version = 45;",
    )
    .unwrap();
}

/// A library as an old binary leaves it: a played WAD, and a local one nothing names.
fn old_library() -> Connection {
    let conn = db::open_memory().unwrap();
    db::init_db(&conn).unwrap();
    rewind_to_before_sync(&conn);
    conn.execute_batch(
        "INSERT INTO wads (id, title, source_type, source_id, status, created_at, updated_at)
             VALUES (101, 'My House', 'doomwiki', '500', 'in-progress',
                     '2026-01-01 00:00:00', '2026-02-01T00:00:00+00:00'),
                    (102, 'Local One', 'local', NULL, 'unplayed',
                     '2026-01-02 00:00:00', '2026-01-02 00:00:00');
         INSERT INTO sessions (wad_id, started_at, ended_at, duration_seconds)
             VALUES (101, '2026-02-01T00:00:00+00:00', '2026-02-01T01:00:00+00:00', 3600);
         INSERT INTO wad_completions (wad_id, completed_at)
             VALUES (101, '2026-02-02T00:00:00+00:00');
         INSERT INTO playthroughs (wad_id, started_at) VALUES (101, '2026-02-01 00:00:00');
         INSERT INTO tags (wad_id, tag) VALUES (101, 'x');",
    )
    .unwrap();
    conn
}

fn upgrade(conn: &Connection) {
    db::init_db(conn).unwrap();
}

#[test]
fn test_upgrading_gives_every_row_a_name_and_keeps_its_folder() {
    let conn = old_library();
    upgrade(&conn);

    let wad = db::get_wad(&conn, 101, false).unwrap().unwrap();
    assert_eq!(wad.uid, "doomwiki-500");
    assert_eq!(wad.data_key, "101", "the name of the folder it already has");

    let local = db::get_wad(&conn, 102, false).unwrap().unwrap();
    assert_eq!(local.uid.len(), 32);
    assert_eq!(local.data_key, "102");

    for table in ["sessions", "wad_completions", "playthroughs"] {
        let missing: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE uid IS NULL"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(missing, 0, "{table}");
    }
}

#[test]
fn test_upgrading_logs_everything_that_was_already_there() {
    let world = World::new();
    let conn = old_library();
    upgrade(&conn);
    let upgraded = world.machine_with("old", conn);
    let report = upgraded.sync();
    assert_eq!(report.written, 6);

    let new = world.machine("new");
    new.sync();
    assert_same_library(&[&upgraded, &new]);
    assert_eq!(new.wad_named("My House").status, Status::InProgress);
    assert_eq!(new.wad_named("My House").data_key, "101");
}

/// The two machines of #57, both with a copy of one library and both upgrading. Their rows
/// have to meet as the same rows, or every WAD would be two WADs.
#[test]
fn test_two_copies_of_one_library_upgrade_into_one_library() {
    let world = World::new();
    let a_conn = old_library();
    let b_conn = old_library();
    upgrade(&a_conn);
    upgrade(&b_conn);
    let a = world.machine_with("a", a_conn);
    let b = world.machine_with("b", b_conn);

    settle(&[&a, &b]);

    assert_eq!(a.count("wads"), 2);
    assert_eq!(a.count("sessions"), 1);
    assert_eq!(a.count("wad_completions"), 1);
    assert_eq!(a.count("playthroughs"), 1);
    assert_eq!(a.count("tags"), 1);
    assert_same_library(&[&a, &b]);
}

/// What one of them did after the copy was taken and before it upgraded. The edit is more
/// recent than the other's copy of that row, whichever of them upgrades first.
#[test]
fn test_what_a_machine_did_before_it_upgraded_is_not_lost() {
    for a_first in [true, false] {
        let world = World::new();
        let a_conn = old_library();
        let b_conn = old_library();
        b_conn
            .execute_batch(
                "UPDATE wads SET status = 'completed', notes = 'done on b',
                     updated_at = '2026-03-01T00:00:00+00:00' WHERE id = 101;
                 INSERT INTO sessions (wad_id, started_at, duration_seconds)
                     VALUES (101, '2026-02-20T00:00:00+00:00', 3600);",
            )
            .unwrap();
        upgrade(&a_conn);
        upgrade(&b_conn);
        let a = world.machine_with("a", a_conn);
        let b = world.machine_with("b", b_conn);

        if a_first {
            a.sync();
            b.sync();
        } else {
            b.sync();
            a.sync();
        }
        settle(&[&a, &b]);

        assert_eq!(
            a.wad_named("My House").status,
            Status::Completed,
            "a first: {a_first}"
        );
        assert_eq!(a.wad_named("My House").notes.as_deref(), Some("done on b"));
        assert_eq!(a.count("sessions"), 2);
        assert_same_library(&[&a, &b]);
    }
}

/// Anything after the upgrade beats anything from before it, however the clocks fall.
#[test]
fn test_what_happens_after_upgrading_beats_what_was_there() {
    let world = World::new();
    let a_conn = old_library();
    let b_conn = old_library();
    upgrade(&a_conn);
    upgrade(&b_conn);
    let a = world.machine_with("a", a_conn);
    let b = world.machine_with("b", b_conn);
    a.set(101, "notes", "after upgrading");
    a.sync();
    b.sync();
    settle(&[&a, &b]);
    assert_eq!(
        b.wad_named("My House").notes.as_deref(),
        Some("after upgrading")
    );
}

#[test]
fn test_a_migration_is_not_anyones_edit() {
    let conn = old_library();
    upgrade(&conn);
    let logged: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sync_outbox WHERE as_of = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(logged, 0);
}

// ---------------------------------------------------------------------------
// Saves
// ---------------------------------------------------------------------------

fn entry(path: &str, sha256: &str) -> SaveEntry {
    SaveEntry {
        path: path.to_string(),
        size: 4,
        sha256: sha256.to_string(),
    }
}

struct SavesFixture {
    world: World,
    a: Machine,
    b: Machine,
    a_id: i64,
    b_id: i64,
    uid: String,
}

impl SavesFixture {
    fn new() -> Self {
        let world = World::new();
        let a = world.machine("a");
        let b = world.machine("b");
        let a_id = a.add("X");
        settle(&[&a, &b]);
        let b_id = b.wad_named("X").id;
        let uid = b.wad_named("X").uid;
        Self {
            world,
            a,
            b,
            a_id,
            b_id,
            uid,
        }
    }

    fn a_saves(&self, entries: &[SaveEntry]) {
        record_saves(&self.a.conn, self.a_id, "a", entries, false).unwrap();
    }

    fn b_saves(&self, entries: &[SaveEntry]) {
        record_saves(&self.b.conn, self.b_id, "b", entries, false).unwrap();
    }

    fn on_b(&self, local: &[SaveEntry]) -> Option<NewerSaves> {
        newer_saves_elsewhere(&self.b.conn, self.b_id, &self.uid, "b", local).unwrap()
    }

    fn sync(&self) {
        settle(&[&self.a, &self.b]);
    }
}

#[test]
fn test_saves_made_elsewhere_that_have_not_arrived_are_reported() {
    let f = SavesFixture::new();
    f.a_saves(&[
        entry("saves/slot1.dsg", "aa"),
        entry("saves/slot2.dsg", "bb"),
    ]);
    f.sync();

    let newer = f.on_b(&[]).unwrap();
    assert_eq!(newer.machine, "a");
    assert_eq!(newer.files, ["saves/slot1.dsg", "saves/slot2.dsg"]);
}

/// The sync tool brought them, which is the whole point of having one.
#[test]
fn test_saves_that_have_arrived_are_not_news() {
    let f = SavesFixture::new();
    f.a_saves(&[entry("saves/slot1.dsg", "aa")]);
    f.sync();
    assert_eq!(f.on_b(&[entry("saves/slot1.dsg", "aa")]), None);
}

#[test]
fn test_only_the_saves_that_differ_are_named() {
    let f = SavesFixture::new();
    f.a_saves(&[
        entry("saves/slot1.dsg", "aa"),
        entry("saves/slot2.dsg", "bb"),
    ]);
    f.sync();
    let newer = f
        .on_b(&[
            entry("saves/slot1.dsg", "aa"),
            entry("saves/slot2.dsg", "old"),
        ])
        .unwrap();
    assert_eq!(newer.files, ["saves/slot2.dsg"]);
}

#[test]
fn test_having_played_since_means_these_are_the_newer_saves() {
    let f = SavesFixture::new();
    f.a_saves(&[entry("saves/slot1.dsg", "aa")]);
    f.sync();
    f.b_saves(&[entry("saves/slot1.dsg", "bb")]);
    f.sync();

    assert_eq!(f.on_b(&[entry("saves/slot1.dsg", "bb")]), None);
    // And the other machine is the one to ask now.
    let newer = newer_saves_elsewhere(
        &f.a.conn,
        f.a_id,
        &f.uid,
        "a",
        &[entry("saves/slot1.dsg", "aa")],
    )
    .unwrap()
    .unwrap();
    assert_eq!(newer.machine, "b");
}

/// Both played without hearing from each other, which is the conflict there is no
/// resolving. Whichever said so later is the one the other gets asked about.
#[test]
fn test_two_machines_writing_one_slot_is_asked_about_once_each_way() {
    let f = SavesFixture::new();
    f.a_saves(&[entry("saves/slot1.dsg", "aa")]);
    f.a.sync();
    std::thread::sleep(std::time::Duration::from_millis(5));
    f.b_saves(&[entry("saves/slot1.dsg", "bb")]);
    f.sync();

    assert_eq!(f.on_b(&[entry("saves/slot1.dsg", "bb")]), None);
    assert!(
        newer_saves_elsewhere(
            &f.a.conn,
            f.a_id,
            &f.uid,
            "a",
            &[entry("saves/slot1.dsg", "aa")],
        )
        .unwrap()
        .is_some()
    );
}

/// The answer to the question holds until something new happens. It would otherwise be
/// asked every launch for as long as they kept not having the other machine's saves.
#[test]
fn test_going_ahead_anyway_is_only_asked_once() {
    let f = SavesFixture::new();
    f.a_saves(&[entry("saves/slot1.dsg", "aa")]);
    f.sync();
    assert!(f.on_b(&[]).is_some());

    record_saves(&f.b.conn, f.b_id, "b", &[], true).unwrap();
    f.sync();
    assert_eq!(f.on_b(&[]), None);

    // Until there is something new to ask about.
    std::thread::sleep(std::time::Duration::from_millis(5));
    f.a_saves(&[entry("saves/slot1.dsg", "cc")]);
    f.sync();
    assert!(f.on_b(&[]).is_some());
}

/// Before it has gone out, it is still the newest thing this machine has to say.
#[test]
fn test_saves_not_written_out_yet_still_count_as_the_latest_word() {
    let f = SavesFixture::new();
    f.a_saves(&[entry("saves/slot1.dsg", "aa")]);
    f.sync();
    f.b_saves(&[entry("saves/slot1.dsg", "bb")]);
    assert_eq!(f.on_b(&[entry("saves/slot1.dsg", "bb")]), None);
}

/// Having none, and having said nothing, is nothing to tell anyone, or a launch-and-quit
/// of a WAD with no saves would add a line to the log every time.
#[test]
fn test_saying_the_same_thing_twice_is_not_news() {
    let f = SavesFixture::new();
    f.a_saves(&[]);
    assert_eq!(f.a.count("wad_saves"), 0);

    f.a_saves(&[entry("saves/slot1.dsg", "aa")]);
    let first = f.a.sync().written;
    assert_eq!(first, 1);
    f.a_saves(&[entry("saves/slot1.dsg", "aa")]);
    assert_eq!(f.a.sync().written, 0);
    let _ = &f.world;
}

#[test]
fn test_a_machines_saves_are_read_off_the_disk_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("doom2/sunlust");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("b.dsg"), b"two").unwrap();
    fs::write(nested.join("a.dsg"), b"one").unwrap();
    fs::write(dir.path().join("stats.txt"), b"not a save").unwrap();

    let saves = read_saves(dir.path());
    let paths: Vec<_> = saves.iter().map(|save| save.path.as_str()).collect();
    assert_eq!(paths, ["doom2/sunlust/a.dsg", "doom2/sunlust/b.dsg"]);
    assert_eq!(saves[0].size, 3);
    assert_eq!(saves[0].sha256.len(), 64);
}

#[test]
fn test_a_listing_of_saves_nobody_can_read_says_nothing() {
    let f = SavesFixture::new();
    f.a.conn
        .execute(
            "INSERT INTO wad_saves (wad_id, machine, files, recorded_at) \
             VALUES (?1, 'a', 'not json', 'now')",
            [f.a_id],
        )
        .unwrap();
    f.sync();
    assert_eq!(f.on_b(&[]), None);
}

// ---------------------------------------------------------------------------
// The status the tool shows
// ---------------------------------------------------------------------------

#[test]
fn test_every_machine_heard_from_is_listed() {
    let world = World::new();
    let a = world.machine("a");
    let b = world.machine("b");
    a.add("X");
    b.add("Y");
    settle(&[&a, &b]);

    let heard: Vec<_> = machines(&a.conn)
        .unwrap()
        .into_iter()
        .map(|status| (status.machine, status.events))
        .collect();
    assert!(heard.contains(&("a".to_string(), 1)));
    assert!(heard.contains(&("b".to_string(), 1)));
}

#[test]
fn test_a_folder_that_cannot_be_written_stops_neither_half_nor_loses_anything() {
    let world = World::new();
    let here = world.machine("here");
    here.add("X");

    let in_the_way = world.dir.path().join("a-file");
    fs::write(&in_the_way, "not a folder").unwrap();
    let mut paths = here.paths.clone();
    paths.log_dir = in_the_way;
    let report = run(&here.conn, &paths);
    assert!(report.write_error.is_some());
    assert_eq!(here.count("sync_outbox"), 1);

    assert_eq!(here.sync().written, 1);
}

/// How many histories to try. More than a test run wants to spend, when looking for trouble:
/// `CACO_SYNC_SEEDS=500 cargo test -p caco-core --lib test_any_history`
fn stress_seeds() -> u64 {
    std::env::var("CACO_SYNC_SEEDS")
        .ok()
        .and_then(|seeds| seeds.parse().ok())
        .unwrap_or(8)
}
