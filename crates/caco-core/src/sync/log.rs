//! The files: what goes from the outbox into this machine's log, and reading them all.
//!
//! No file is touched inside a transaction. The sync folder can be on a network mount
//! that has stopped answering, and a write lock held across that would stall every
//! write the UI tries to make, for the reason [`crate::presence`] does its stats first.
//! So writing out the outbox takes three steps, and a crash between any two is harmless:
//!
//! 1. Give each waiting row its event id and clock, in a short transaction.
//! 2. Append them to the file.
//! 3. Delete what was written, in another short transaction.
//!
//! Dying after step 2 writes the same lines again next time, with the same ids, and
//! readers go by id. Dying in the middle of step 2 leaves half a line, which is skipped
//! as unreadable. The next append starts on a line of its own, so it costs nothing else.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Map, Value};

use super::capture::{self, Unset};
use super::clock::{Clock, Hlc, wall_ms_of};
use super::event::{Event, Op, VERSION};
use crate::Result;
use crate::db;

/// A row of `sync_outbox`.
struct Row {
    seq: i64,
    id: String,
    at: String,
    op: Op,
    kind: String,
    key: String,
    fields: Option<String>,
    as_of: bool,
    event: Option<String>,
    clock: Option<String>,
}

impl Row {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let op: String = row.get("op")?;
        Ok(Self {
            seq: row.get("seq")?,
            id: row.get("id")?,
            at: row.get("at")?,
            // Only ever written by this module's triggers.
            op: Op::parse(&op).unwrap_or(Op::Set),
            kind: row.get("kind")?,
            key: row.get("key")?,
            fields: row.get("fields")?,
            as_of: row.get::<_, i64>("as_of")? != 0,
            event: row.get("event")?,
            clock: row.get("clock")?,
        })
    }

    /// Whether this row can go in the same event as `other`: another write to the same
    /// thing, straight after it. A session's start, end and stats make one line that way.
    /// Never across ops, since an `add` and a `set` are worth different things.
    ///
    /// Nor after a clear of a WAD's stats, which is a moment and not a value: stats written
    /// after it would overwrite it in the event, and nobody would hear that it happened.
    /// A false positive from a note that quotes this only costs a line.
    fn continues(&self, other: &Row) -> bool {
        self.op == other.op
            && self.op != Op::Delete
            && self.kind == other.kind
            && self.key == other.key
            && !self.as_of
            && !other.as_of
            && !other
                .fields
                .as_deref()
                .is_some_and(|fields| fields.contains(r#""stats_snapshot":null"#))
    }
}

fn read_rows(conn: &Connection, condition: &str) -> Result<Vec<Row>> {
    let mut statement = conn.prepare(&format!(
        "SELECT seq, id, at, op, kind, key, fields, as_of, event, clock \
         FROM sync_outbox WHERE {condition} ORDER BY seq"
    ))?;
    let rows = statement
        .query_map([], Row::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// What the rows of one event say, later ones over earlier ones.
///
/// An `add` says what a new row holds, so it leaves out what a new row has anyway. That
/// is what stops the same WAD imported on a second machine having an opinion on whether
/// it has been played: see [`capture::Field::unset`].
fn event_fields(rows: &[Row]) -> Map<String, Value> {
    let mut fields = Map::new();
    for row in rows {
        let Some(text) = row.fields.as_deref() else {
            continue;
        };
        match serde_json::from_str::<Map<String, Value>>(text) {
            Ok(said) => fields.extend(said),
            // SQLite wrote it with json_object, so this would be a bug here, and nothing
            // to stop the rest of the log for.
            Err(e) => tracing::warn!("outbox row {} is not JSON: {e}", row.seq),
        }
    }
    if rows.first().is_some_and(|row| row.op == Op::Add)
        && let Some(entity) = capture::entity(&rows[0].kind)
    {
        fields.retain(|name, value| {
            !value.is_null()
                && !entity
                    .field(name)
                    .and_then(|field| field.unset)
                    .is_some_and(|unset| match unset {
                        Unset::Text(text) => value == text,
                        Unset::Int(n) => value == n,
                    })
        });
    }
    fields
}

/// Consecutive rows that make one event.
fn group(rows: &[Row]) -> Vec<&[Row]> {
    let mut groups = Vec::new();
    let mut start = 0;
    for i in 1..=rows.len() {
        if i == rows.len() || !rows[i].continues(&rows[i - 1]) {
            groups.push(&rows[start..i]);
            start = i;
        }
    }
    if rows.is_empty() {
        groups.clear();
    }
    groups
}

pub(super) fn load_hlc(conn: &Connection) -> Result<Hlc> {
    let state: Option<String> = conn
        .query_row(
            "SELECT value FROM sync_state WHERE key = 'hlc'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(state.as_deref().map(Hlc::from_state).unwrap_or_default())
}

pub(super) fn save_hlc(conn: &Connection, hlc: Hlc) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_state (key, value) VALUES ('hlc', ?1)",
        [hlc.to_state()],
    )?;
    Ok(())
}

/// Give everything new in the outbox its id and clock.
///
/// Clocks are worked out here rather than in the triggers, because they need to read and
/// advance a value, and an ordinary write should not have to. The wall time is from when
/// the write happened, which the trigger did record.
fn stamp(conn: &Connection, machine: &str) -> Result<()> {
    db::with_transaction(conn, |tx| {
        let rows = read_rows(tx, "clock IS NULL")?;
        if rows.is_empty() {
            return Ok(());
        }
        // Noting an event can rewrite a row, which is not a thing that happened.
        tx.execute("INSERT INTO sync_applying VALUES (1)", [])?;
        let mut hlc = load_hlc(tx)?;
        for rows in group(&rows) {
            let (first, last) = (&rows[0], &rows[rows.len() - 1]);
            let wall = wall_ms_of(&last.at).unwrap_or(0);
            // A row that predates the log goes by when it last changed, with nothing
            // ordered after it, so it cannot beat anything that happened since.
            let clock = if first.as_of {
                Clock::at(wall, machine)
            } else {
                hlc.tick(wall, machine)
            };
            hlc.observe(&clock);
            tx.execute(
                "UPDATE sync_outbox SET event = ?1, clock = ?2 WHERE seq BETWEEN ?3 AND ?4",
                rusqlite::params![first.id, clock.to_string(), first.seq, last.seq],
            )?;
            let event = Event {
                v: VERSION,
                id: first.id.clone(),
                machine: machine.to_string(),
                at: at_text(wall),
                clock,
                op: first.op,
                kind: first.kind.clone(),
                key: first.key.clone(),
                fields: event_fields(rows),
            };
            super::merge::note_own_event(tx, &event)?;
        }
        tx.execute("DELETE FROM sync_applying", [])?;
        save_hlc(tx, hlc)
    })
}

/// A time as a log line spells it, whatever spelling it was in.
fn at_text(wall_ms: u64) -> String {
    i64::try_from(wall_ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Write everything waiting in the outbox to this machine's log, and return how many
/// events that was.
pub(super) fn write_outbox(conn: &Connection, log_dir: &Path, machine: &str) -> Result<usize> {
    stamp(conn, machine)?;

    let rows = read_rows(conn, "clock IS NOT NULL")?;
    let Some(last_seq) = rows.last().map(|row| row.seq) else {
        return Ok(0);
    };
    let mut text = String::new();
    let mut events = 0;
    // By the id step 1 gave them, not by `group`, which could now see more rows than it
    // did then, and would make a different event with the same id out of a line that may
    // already have gone out.
    for rows in rows.chunk_by(|a, b| a.event == b.event) {
        let (first, last) = (&rows[0], &rows[rows.len() - 1]);
        let (Some(id), Some(clock)) = (first.event.clone(), first.clock.as_deref()) else {
            continue;
        };
        let event = Event {
            v: VERSION,
            id,
            machine: machine.to_string(),
            clock: clock
                .parse()
                .map_err(|e: String| crate::Error::Config(format!("outbox clock: {e}")))?,
            at: at_text(wall_ms_of(&last.at).unwrap_or(0)),
            op: first.op,
            kind: first.kind.clone(),
            key: first.key.clone(),
            fields: event_fields(rows),
        };
        text.push_str(&event.to_line()?);
        text.push('\n');
        events += 1;
    }

    append(&log_file_for_now(log_dir, machine), &text)?;

    conn.execute(
        "DELETE FROM sync_outbox WHERE seq <= ?1 AND clock IS NOT NULL",
        [last_seq],
    )?;
    Ok(events)
}

/// `<log>/<machine>/<yyyy-mm>.jsonl`, which only this machine ever writes to.
fn log_file_for_now(log_dir: &Path, machine: &str) -> PathBuf {
    let month = chrono::Utc::now().format("%Y-%m");
    log_dir.join(machine).join(format!("{month}.jsonl"))
}

/// Add lines to the end of a file, and get them onto the disk before saying so, because
/// the outbox rows they came from go next.
fn append(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    // A crash can leave half a line, and what comes next must not join it.
    let mut buffer = String::with_capacity(text.len() + 1);
    if ends_mid_line(&mut file)? {
        buffer.push('\n');
    }
    buffer.push_str(text);
    file.write_all(buffer.as_bytes())?;
    file.sync_data()
}

fn ends_mid_line(file: &mut fs::File) -> io::Result<bool> {
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

/// A log file, and what says whether it has changed since it was last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LogFile {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ms: Option<i64>,
}

/// Every log file of every machine. Anything ending in `.jsonl` one level down counts,
/// because a sync tool's conflict copy (`2026-11.sync-conflict-…jsonl`) holds events too,
/// and reading one costs nothing, since events go by id.
///
/// A folder that is not there is a machine with no log yet, and not an error, so a sync
/// folder on a drive that is not mounted looks the same. It is asked again next time.
pub(super) fn list_files(log_dir: &Path) -> io::Result<Vec<LogFile>> {
    let machines = match fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut files = Vec::new();
    for machine in machines.flatten() {
        let Ok(entries) = fs::read_dir(machine.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
                && let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                let mtime_ms = meta
                    .modified()
                    .ok()
                    .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                    .and_then(|since| i64::try_from(since.as_millis()).ok());
                files.push(LogFile {
                    path,
                    size: meta.len(),
                    mtime_ms,
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The complete lines of a log file. A last line with no newline is still being written,
/// by a machine or a sync tool, and will be there whole next time, when the file reads as
/// changed. A line that is not text is passed on as blank, which reads as unreadable.
pub(super) fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    let bytes = fs::read(path)?;
    let complete = match bytes.iter().rposition(|&b| b == b'\n') {
        Some(end) => &bytes[..end],
        None => return Ok(Vec::new()),
    };
    Ok(complete
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| std::str::from_utf8(line).unwrap_or_default().to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewWad, SourceType, WadUpdate};
    use crate::sync::event::{Line, parse_line};

    const MACHINE: &str = "here-00000000";

    struct Fixture {
        conn: Connection,
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let conn = db::open_memory().unwrap();
            db::init_db(&conn).unwrap();
            Self {
                conn,
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn write(&self) -> usize {
            write_outbox(&self.conn, self.dir.path(), MACHINE).unwrap()
        }

        fn events(&self) -> Vec<Event> {
            list_files(self.dir.path())
                .unwrap()
                .iter()
                .flat_map(|file| read_lines(&file.path).unwrap())
                .map(|line| match parse_line(&line) {
                    Line::Event(event) => *event,
                    other => panic!("{other:?}: {line}"),
                })
                .collect()
        }

        fn outbox_len(&self) -> i64 {
            self.conn
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
                .unwrap()
        }
    }

    #[test]
    fn test_a_write_ends_up_in_this_machines_file_for_this_month() {
        let f = Fixture::new();
        db::add_wad(&f.conn, &NewWad::new("Sunlust", SourceType::Local)).unwrap();

        assert_eq!(f.write(), 1);

        let month = chrono::Utc::now().format("%Y-%m").to_string();
        let file = f.dir.path().join(MACHINE).join(format!("{month}.jsonl"));
        assert!(file.is_file());
        assert_eq!(f.events()[0].fields["title"], "Sunlust");
        assert_eq!(f.outbox_len(), 0);
    }

    #[test]
    fn test_nothing_to_write_touches_nothing() {
        let f = Fixture::new();
        assert_eq!(f.write(), 0);
        assert!(!f.dir.path().join(MACHINE).exists());
    }

    /// A session's start and end are one line, and not two half-sentences.
    #[test]
    fn test_writes_to_one_thing_in_a_row_make_one_event() {
        let f = Fixture::new();
        let wad = db::add_wad(&f.conn, &NewWad::new("x", SourceType::Local)).unwrap();
        f.write();

        db::update_wad(
            &f.conn,
            wad,
            &WadUpdate::new().set_text("notes", Some("a".into())),
        )
        .unwrap();
        db::update_wad(&f.conn, wad, &WadUpdate::new().set_int("rating", Some(5))).unwrap();
        db::update_wad(
            &f.conn,
            wad,
            &WadUpdate::new().set_text("notes", Some("b".into())),
        )
        .unwrap();

        assert_eq!(f.write(), 1);
        let last = f.events().pop().unwrap();
        assert_eq!(last.fields["notes"], "b");
        assert_eq!(last.fields["rating"], 5);
    }

    #[test]
    fn test_writes_to_other_things_keep_their_order() {
        let f = Fixture::new();
        let a = db::add_wad(&f.conn, &NewWad::new("a", SourceType::Local)).unwrap();
        let b = db::add_wad(&f.conn, &NewWad::new("b", SourceType::Local)).unwrap();
        f.write();

        for (wad, note) in [(a, "1"), (b, "2"), (a, "3")] {
            db::update_wad(
                &f.conn,
                wad,
                &WadUpdate::new().set_text("notes", Some(note.into())),
            )
            .unwrap();
        }

        assert_eq!(f.write(), 3);
        let events = f.events();
        let notes: Vec<_> = events[2..]
            .iter()
            .map(|e| e.fields["notes"].clone())
            .collect();
        assert_eq!(notes, ["1", "2", "3"]);
        assert!(events[2].clock < events[3].clock && events[3].clock < events[4].clock);
    }

    /// An import's default status is not an opinion, or a second machine importing the
    /// same WAD would say it has not been played.
    #[test]
    fn test_an_add_leaves_out_what_any_new_row_would_have() {
        let f = Fixture::new();
        db::add_wad(&f.conn, &NewWad::new("x", SourceType::Local)).unwrap();
        f.write();
        let fields = &f.events()[0].fields;
        assert!(fields.get("status").is_none());
        assert!(fields.get("gc_ignore").is_none());
        assert!(fields.get("author").is_none());
        assert_eq!(fields["title"], "x");

        // A choice the row was given is one, whichever way it went.
        let f = Fixture::new();
        let wad = db::add_wad(&f.conn, &NewWad::new("x", SourceType::Local)).unwrap();
        f.write();
        db::update_wad(
            &f.conn,
            wad,
            &WadUpdate::new().set_status(db::Status::Completed),
        )
        .unwrap();
        db::update_wad(
            &f.conn,
            wad,
            &WadUpdate::new().set_status(db::Status::Unplayed),
        )
        .unwrap();
        f.write();
        let last = f.events().pop().unwrap();
        assert_eq!(last.fields["status"], "unplayed");
    }

    #[test]
    fn test_a_clear_of_stats_is_not_lost_to_what_comes_after_it() {
        let f = Fixture::new();
        let wad = db::add_wad(&f.conn, &NewWad::new("x", SourceType::Local)).unwrap();
        f.write();

        let set = |value: Option<&str>| {
            db::update_wad(
                &f.conn,
                wad,
                &WadUpdate::new().set_text("stats_snapshot", value.map(str::to_string)),
            )
            .unwrap();
        };
        set(Some("{}"));
        set(None);
        set(Some("{}"));

        assert_eq!(f.write(), 2, "the clear ends an event");
        let events = f.events();
        assert!(events[1].fields["stats_snapshot"].is_null());
        assert_eq!(events[2].fields["stats_snapshot"], "{}");
    }

    /// The crash this ordering is for: the file has the lines, and the rows are still
    /// there. Writing again must not change what those lines said, or a reader who took
    /// the first copy would miss what the second added.
    #[test]
    fn test_dying_after_the_write_repeats_the_lines_exactly() {
        let f = Fixture::new();
        let wad = db::add_wad(&f.conn, &NewWad::new("x", SourceType::Local)).unwrap();
        f.write();

        db::update_wad(
            &f.conn,
            wad,
            &WadUpdate::new().set_text("notes", Some("a".into())),
        )
        .unwrap();
        stamp(&f.conn, MACHINE).unwrap();
        let stamped: (String, String) = f
            .conn
            .query_row("SELECT event, clock FROM sync_outbox", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();

        // A later write to the same thing, which `group` would like to run together.
        db::update_wad(&f.conn, wad, &WadUpdate::new().set_int("rating", Some(5))).unwrap();
        f.write();

        let events = f.events();
        let notes = events.iter().find(|e| e.id == stamped.0).unwrap();
        assert_eq!(notes.clock.to_string(), stamped.1);
        assert!(notes.fields.get("rating").is_none());
        assert!(events.iter().any(|e| e.fields.get("rating").is_some()));
    }

    /// An unwritable folder stops nothing, and loses nothing: it is all still there for
    /// when the folder comes back.
    #[test]
    fn test_a_folder_that_cannot_be_written_keeps_the_outbox() {
        let f = Fixture::new();
        db::add_wad(&f.conn, &NewWad::new("x", SourceType::Local)).unwrap();
        let in_the_way = f.dir.path().join("file");
        fs::write(&in_the_way, "not a directory").unwrap();

        assert!(write_outbox(&f.conn, &in_the_way, MACHINE).is_err());
        assert_eq!(f.outbox_len(), 1);

        assert_eq!(f.write(), 1);
        assert_eq!(f.outbox_len(), 0);
    }

    #[test]
    fn test_half_a_line_left_by_a_crash_does_not_swallow_the_next() {
        let f = Fixture::new();
        db::add_wad(&f.conn, &NewWad::new("a", SourceType::Local)).unwrap();
        f.write();
        let file = list_files(f.dir.path()).unwrap().remove(0).path;
        let mut torn = OpenOptions::new().append(true).open(&file).unwrap();
        torn.write_all(br#"{"v":1,"id":"to"#).unwrap();
        drop(torn);

        db::add_wad(&f.conn, &NewWad::new("b", SourceType::Local)).unwrap();
        f.write();

        let lines = read_lines(&file).unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(parse_line(&lines[1]), Line::Unreadable);
        assert!(matches!(parse_line(&lines[2]), Line::Event(_)));
    }

    #[test]
    fn test_a_last_line_still_being_written_is_left_for_next_time() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("log.jsonl");

        fs::write(&file, "partial").unwrap();
        assert!(read_lines(&file).unwrap().is_empty());

        fs::write(&file, "one\ntwo\nthr").unwrap();
        assert_eq!(read_lines(&file).unwrap(), ["one", "two"]);

        fs::write(&file, "one\ntwo\nthree\n").unwrap();
        assert_eq!(read_lines(&file).unwrap(), ["one", "two", "three"]);
    }

    #[test]
    fn test_only_logs_one_level_down_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let machine = dir.path().join("laptop-1");
        fs::create_dir_all(&machine).unwrap();
        fs::write(machine.join("2026-11.jsonl"), "x\n").unwrap();
        fs::write(machine.join("2026-11.sync-conflict-1.jsonl"), "y\n").unwrap();
        fs::write(machine.join("notes.txt"), "z\n").unwrap();
        fs::write(dir.path().join("stray.jsonl"), "z\n").unwrap();

        let names: Vec<_> = list_files(dir.path())
            .unwrap()
            .into_iter()
            .map(|file| {
                file.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["2026-11.jsonl", "2026-11.sync-conflict-1.jsonl"]);
    }

    /// Not there yet is not a failure, on a laptop that has not seen this drive today.
    #[test]
    fn test_a_folder_that_is_not_there_has_no_logs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_files(&dir.path().join("nope")).unwrap().is_empty());
    }
}
