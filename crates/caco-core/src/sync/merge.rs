//! Applying other machines' events, so that every machine ends up with the same library
//! whatever order they arrive in.
//!
//! Every field goes by last writer wins, per field, with the writer's clock kept in
//! `sync_clocks`. That is commutative and idempotent, so nothing here depends on seeing
//! events in order or only once. They are applied in clock order anyway, but only
//! because a parent usually comes before its children. An event whose parent has not
//! arrived, since sync tools deliver files in any order they like, waits in `sync_parked`.
//!
//! Three kinds of field have rules of their own, each because plain last writer wins
//! would get it wrong:
//!
//! - **What a file is** ([`Role::Identity`]) is only taken from another machine by a
//!   machine with no file of its own. With one, the row says what that file is, and
//!   another machine's copy being different is not a reason to think otherwise: comparing
//!   it would just have the two arguing at every startup. The sessions they each record
//!   are what say the files differ, and the launch asks about that.
//! - **Per-map stats** (`stats_snapshot`) are merged map by map, because beating map 10 on
//!   one machine does not undo map 20 on the other. Clearing one is starting a playthrough
//!   over, which is a moment in time: progress from before it stays gone. So the row is
//!   worked out from the latest each machine said, in `wad_stats_by_machine`: a clear that
//!   arrives late has to be able to take away what came before it, and leave what came
//!   after, and that could not be untangled from one merged value.
//! - **Which folder a WAD's data is in** (`data_key`) has to be the same everywhere, so a
//!   row that loses keeps its files by renaming the folder.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use super::capture::{self, Entity, Field, Key, Parent, Role, Shape, Ty};
use super::clock::Clock;
use super::event::{Event, Line, Op, parse_line};
use super::log::{self, LogFile};
use crate::Result;
use crate::config;
use crate::db;

/// How many events to apply per transaction. A first merge can be tens of thousands of
/// them, and the UI's writes should get a turn.
const BATCH: usize = 500;

/// Why an event has not been applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waiting {
    /// What it is about has not arrived yet.
    ForParent,
    /// From a version of caco newer than this one.
    ForUpgrade,
}

impl Waiting {
    fn as_str(self) -> &'static str {
        match self {
            Waiting::ForParent => "parent",
            Waiting::ForUpgrade => "upgrade",
        }
    }
}

/// What became of an event.
enum Outcome {
    /// It said something, whether or not all of it was news.
    Applied,
    /// It is about something deleted for good, or says nothing usable.
    Dropped,
    Wait(Waiting),
}

/// An event on its way in, with the line it came from in case it has to wait.
struct Incoming {
    id: String,
    clock: Clock,
    line: String,
    event: Option<Event>,
}

/// What a merge found, for the worker to pass on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// Events applied, by the machine they came from.
    pub applied: BTreeMap<String, usize>,
    /// Events waiting for something they refer to.
    pub waiting: usize,
    /// Events from a version of caco newer than this one.
    pub too_new: usize,
    /// Lines that could not be read, in the files read this time.
    pub unreadable: usize,
}

/// What a merge has to do outside the database once it is committed.
#[derive(Default)]
struct AfterCommit {
    /// A WAD's data key that changed, from what to what.
    renamed_keys: Vec<(String, String)>,
}

/// Where a merge may put things. `companions` names where a companion's managed file
/// would be, which goes in its row: nothing writes there.
pub(super) struct MergePaths<'a> {
    pub log_dir: &'a Path,
    pub machine: &'a str,
    pub wad_data_dir: &'a Path,
    pub companion_dir: &'a Path,
}

/// Read what is new in the logs and apply it.
pub(super) fn merge(conn: &Connection, paths: &MergePaths<'_>) -> Result<MergeReport> {
    let mut report = MergeReport::default();

    // Everything off the disk before anything takes the write lock: see `super::log`.
    let changed = changed_files(conn, log::list_files(paths.log_dir)?)?;
    let mut lines = Vec::new();
    let mut read = Vec::new();
    for file in changed {
        match log::read_lines(&file.path) {
            Ok(text) => {
                lines.extend(text);
                read.push(file);
            }
            // Unreadable right now, and marked as not read, so it is asked again.
            Err(e) => tracing::warn!("cannot read {}: {e}", file.path.display()),
        }
    }

    let mut incoming = parked_events(conn)?;
    collect_new_lines(conn, lines, &mut incoming, &mut report)?;

    let mut pending: Vec<Incoming> = incoming.into_values().collect();
    pending.sort_by(|a, b| a.clock.cmp(&b.clock));

    let mut after = AfterCommit::default();
    let waiting = apply_all(conn, pending, paths, &mut report, &mut after)?;
    store_waiting(conn, &waiting)?;
    for item in &waiting {
        match item.1 {
            Waiting::ForParent => report.waiting += 1,
            Waiting::ForUpgrade => report.too_new += 1,
        }
    }

    remember_read(conn, &read)?;
    rename_data_dirs(paths.wad_data_dir, &after.renamed_keys);
    Ok(report)
}

/// The files that are not as they were when last read. Size and modification time, as
/// [`crate::identity`] goes by, so a quiet folder costs a `stat` per file, and an upgrade
/// loses nothing, since what is waiting for one is in `sync_parked`.
fn changed_files(conn: &Connection, files: Vec<LogFile>) -> Result<Vec<LogFile>> {
    let mut statement = conn.prepare("SELECT size, mtime_ms FROM sync_files WHERE path = ?1")?;
    let mut changed = Vec::new();
    for file in files {
        let known: Option<(i64, Option<i64>)> = statement
            .query_row([file.path.to_string_lossy()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        let same = known.is_some_and(|(size, mtime_ms)| {
            size == i64::try_from(file.size).unwrap_or(i64::MAX)
                && mtime_ms.is_some()
                && mtime_ms == file.mtime_ms
        });
        if !same {
            changed.push(file);
        }
    }
    Ok(changed)
}

fn remember_read(conn: &Connection, files: &[LogFile]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    db::with_transaction(conn, |tx| {
        for file in files {
            tx.execute(
                "INSERT OR REPLACE INTO sync_files (path, size, mtime_ms) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    file.path.to_string_lossy(),
                    i64::try_from(file.size).unwrap_or(i64::MAX),
                    file.mtime_ms
                ],
            )?;
        }
        Ok(())
    })
}

/// What was waiting from last time, read again, because this build may be newer than the
/// one that parked it.
fn parked_events(conn: &Connection) -> Result<HashMap<String, Incoming>> {
    let mut statement = conn.prepare("SELECT line FROM sync_parked")?;
    let lines: Vec<String> = statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut incoming = HashMap::with_capacity(lines.len());
    for line in lines {
        add_line(line, &mut incoming);
    }
    Ok(incoming)
}

/// Sort a line into `incoming`, and say if it could not be read.
fn add_line(line: String, incoming: &mut HashMap<String, Incoming>) -> bool {
    let (id, clock, event) = match parse_line(&line) {
        Line::Event(event) => (event.id.clone(), event.clock.clone(), Some(*event)),
        Line::TooNew { id, clock } => (id, clock, None),
        Line::Unreadable => return false,
    };
    incoming.entry(id.clone()).or_insert(Incoming {
        id,
        clock,
        line,
        event,
    });
    true
}

fn collect_new_lines(
    conn: &Connection,
    lines: Vec<String>,
    incoming: &mut HashMap<String, Incoming>,
    report: &mut MergeReport,
) -> Result<()> {
    let seen: HashSet<String> = if lines.is_empty() {
        HashSet::new()
    } else {
        conn.prepare("SELECT id FROM sync_seen")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for line in lines {
        // Checked by id after parsing, since an unseen line is the unusual one here only
        // in a folder where nothing has changed, and then this is not reached at all.
        let mut one = HashMap::with_capacity(1);
        if !add_line(line, &mut one) {
            report.unreadable += 1;
            continue;
        }
        for (id, item) in one {
            if !seen.contains(&id) {
                incoming.entry(id).or_insert(item);
            }
        }
    }
    Ok(())
}

/// Apply everything, going round again while anything that waited could now go. Returns
/// what still could not, and why.
fn apply_all(
    conn: &Connection,
    mut pending: Vec<Incoming>,
    paths: &MergePaths<'_>,
    report: &mut MergeReport,
    after: &mut AfterCommit,
) -> Result<Vec<(Incoming, Waiting)>> {
    loop {
        let mut waiting: Vec<(Incoming, Waiting)> = Vec::new();
        let mut progressed = false;
        let mut remaining = pending.into_iter().peekable();
        while remaining.peek().is_some() {
            let batch: Vec<Incoming> = remaining.by_ref().take(BATCH).collect();
            db::with_transaction(conn, |tx| {
                tx.execute("INSERT INTO sync_applying VALUES (1)", [])?;
                let mut hlc = log::load_hlc(tx)?;
                for item in batch {
                    let Some(event) = item.event.as_ref() else {
                        waiting.push((item, Waiting::ForUpgrade));
                        continue;
                    };
                    let outcome = match apply_event(tx, event, paths, after) {
                        Ok(outcome) => outcome,
                        Err(e) => {
                            // One event a constraint refuses must not stop the rest, and
                            // applying it again later is safe, since nothing here minds
                            // seeing an event twice.
                            tracing::warn!("could not apply {} {}: {e}", event.kind, event.key);
                            Outcome::Wait(Waiting::ForParent)
                        }
                    };
                    match outcome {
                        Outcome::Applied => {
                            hlc.observe(&item.clock);
                            mark_seen(tx, &item.id)?;
                            note_machine(tx, event)?;
                            *report.applied.entry(event.machine.clone()).or_default() += 1;
                            progressed = true;
                        }
                        Outcome::Dropped => {
                            hlc.observe(&item.clock);
                            mark_seen(tx, &item.id)?;
                            progressed = true;
                        }
                        Outcome::Wait(why) => waiting.push((item, why)),
                    }
                }
                log::save_hlc(tx, hlc)?;
                tx.execute("DELETE FROM sync_applying", [])?;
                Ok(())
            })?;
        }

        // Only what is waiting for a parent can be helped by another round.
        let can_retry = waiting.iter().any(|(_, why)| *why == Waiting::ForParent);
        if !progressed || !can_retry {
            return Ok(waiting);
        }
        pending = waiting.into_iter().map(|(item, _)| item).collect();
        pending.sort_by(|a, b| a.clock.cmp(&b.clock));
    }
}

fn store_waiting(conn: &Connection, waiting: &[(Incoming, Waiting)]) -> Result<()> {
    db::with_transaction(conn, |tx| {
        // Anything no longer here was applied, or dropped, and is not waiting for anything.
        tx.execute("DELETE FROM sync_parked", [])?;
        let mut insert = tx.prepare(
            "INSERT OR REPLACE INTO sync_parked (id, clock, line, reason) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (item, why) in waiting {
            insert.execute(rusqlite::params![
                item.id,
                item.clock.to_string(),
                item.line,
                why.as_str()
            ])?;
        }
        Ok(())
    })
}

fn mark_seen(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO sync_seen (id) VALUES (?1)", [id])?;
    Ok(())
}

fn note_machine(conn: &Connection, event: &Event) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_machines (machine, last_at, events) VALUES (?1, ?2, 1) \
         ON CONFLICT(machine) DO UPDATE SET events = events + 1, \
             last_at = MAX(last_at, excluded.last_at)",
        rusqlite::params![event.machine, event.at],
    )?;
    Ok(())
}

/// An event this machine just wrote, which it has no need to read back, but which the
/// next event from elsewhere has to be compared with.
pub(super) fn note_own_event(conn: &Connection, event: &Event) -> Result<()> {
    mark_seen(conn, &event.id)?;
    note_machine(conn, event)?;
    let entity_id = entity_id(&event.kind, &event.key);
    for (name, value) in &event.fields {
        if name == STATS_FIELD && event.kind == "wad" {
            note_own_stats(conn, event, &entity_id, value.as_str())?;
            continue;
        }
        let stamp = stamp_of(event);
        if newer(conn, &entity_id, name, &stamp)? {
            store_clock(conn, &entity_id, name, &stamp)?;
        }
    }
    Ok(())
}

/// Keep what this machine just said about its stats, and work the row out again from it, as
/// a merge would. For a library on one machine that comes to what the row already says.
fn note_own_stats(
    conn: &Connection,
    event: &Event,
    entity_id: &str,
    stats: Option<&str>,
) -> Result<()> {
    let clock = event.clock.to_string();
    let reset = clock_of(conn, entity_id, STATS_RESET)?;
    let Some(stats) = stats else {
        if reset.is_none_or(|reset| clock > reset) {
            store_clock(conn, entity_id, STATS_RESET, &clock)?;
            // The clear's trigger only saw what had been written out by then. Stats set
            // just before it, and only being noted now, have gone too.
            conn.execute(
                "DELETE FROM wad_stats_by_machine WHERE clock <= ?1 \
                 AND wad_id = (SELECT id FROM wads WHERE uid = ?2)",
                rusqlite::params![clock, event.key],
            )?;
            rework_stats(conn, &event.key)?;
        }
        return Ok(());
    };
    if reset.is_some_and(|reset| clock <= reset) {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO wad_stats_by_machine (wad_id, machine, is_this_machine, clock, stats) \
         SELECT id, ?1, 1, ?2, ?3 FROM wads WHERE uid = ?4 \
         ON CONFLICT(wad_id, machine) DO UPDATE SET \
             is_this_machine = 1, clock = excluded.clock, stats = excluded.stats \
         WHERE excluded.clock > clock",
        rusqlite::params![event.machine, clock, stats, event.key],
    )?;
    rework_stats(conn, &event.key)
}

/// Make a WAD's row say what all the machines have said. Every event ends with this, so
/// several written out together end with what the last of them leaves, whatever happened
/// to the row between them being written and being noted.
fn rework_stats(conn: &Connection, uid: &str) -> Result<()> {
    conn.execute(
        "UPDATE wads SET stats_snapshot = ?1 WHERE uid = ?2 AND stats_snapshot IS NOT ?1",
        rusqlite::params![stats_for_row(conn, uid)?, uid],
    )?;
    Ok(())
}

/// [`stats_by_machine`], for a WAD by uid.
fn stats_for_row(conn: &Connection, uid: &str) -> Result<Option<String>> {
    let wad_id: Option<i64> = conn
        .query_row("SELECT id FROM wads WHERE uid = ?1", [uid], |row| {
            row.get(0)
        })
        .optional()?;
    match wad_id {
        Some(wad_id) => stats_by_machine(conn, wad_id, false),
        None => Ok(None),
    }
}

fn entity_id(kind: &str, key: &str) -> String {
    format!("{kind}/{key}")
}

/// What an event's fields are worth: see [`Op`].
fn stamp_of(event: &Event) -> String {
    format!("{}{}", event.op.rank(), event.clock)
}

/// Whether `stamp` beats whatever last wrote this field.
fn newer(conn: &Connection, entity_id: &str, field: &str, stamp: &str) -> Result<bool> {
    let current = clock_of(conn, entity_id, field)?;
    Ok(current.is_none_or(|current| stamp > current.as_str()))
}

fn clock_of(conn: &Connection, entity_id: &str, field: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT clock FROM sync_clocks WHERE entity = ?1 AND field = ?2",
            [entity_id, field],
            |row| row.get(0),
        )
        .optional()?)
}

fn store_clock(conn: &Connection, entity_id: &str, field: &str, stamp: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_clocks (entity, field, clock) VALUES (?1, ?2, ?3)",
        [entity_id, field, stamp],
    )?;
    Ok(())
}

fn apply_event(
    tx: &Connection,
    event: &Event,
    paths: &MergePaths<'_>,
    after: &mut AfterCommit,
) -> Result<Outcome> {
    // A kind this build has not heard of is from a newer one, whatever its `v` says.
    let Some(entity) = capture::entity(&event.kind) else {
        return Ok(Outcome::Wait(Waiting::ForUpgrade));
    };
    match entity.shape {
        Shape::Row => apply_row(tx, entity, event, paths.machine, after),
        Shape::Membership => apply_membership(tx, entity, event, paths),
        Shape::Annotation => apply_annotation(tx, entity, event),
    }
}

/// A value from a log as this field's column wants it, or `None` if it cannot be.
///
/// The gate between a file somebody else can write to and a column something will later
/// trust: an integer column holding text would make its row unreadable, and a file name
/// holding `../../.bashrc` is a path on this machine.
fn decode(field: &Field, value: &Value) -> Option<SqlValue> {
    if value.is_null() {
        return (!field.required).then_some(SqlValue::Null);
    }
    match field.ty {
        Ty::Int => value.as_i64().map(SqlValue::Integer),
        Ty::Text => {
            let text = value.as_str()?;
            if let Some(allowed) = field.one_of
                && !allowed.contains(&text)
            {
                return None;
            }
            if field.role == Role::Name && text.contains(['/', '\\']) {
                return None;
            }
            if field.name == "data_key" && !config::is_data_key(text) {
                return None;
            }
            Some(SqlValue::Text(text.to_string()))
        }
    }
}

/// What a name in the log comes to on this machine.
enum Resolved {
    Here(i64),
    /// It has been and gone. No row is ever going to answer to it.
    Gone,
    /// Nothing has said anything about it yet.
    NotYet,
}

fn resolve(tx: &Connection, parent: Parent, name: &str) -> Result<Resolved> {
    let found: Option<i64> = tx
        .query_row(
            &format!(
                "SELECT id FROM {} WHERE {} = ?1",
                parent.table, parent.global
            ),
            [name],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        return Ok(Resolved::Here(id));
    }
    let tombstone: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM sync_tombstones WHERE kind = ?1 AND key = ?2",
            [parent.kind, name],
            |row| row.get(0),
        )
        .optional()?;
    // A membership is gone if it was ever here, which leaves its clock behind.
    let was_here = clock_of(tx, &entity_id(parent.kind, name), PRESENT)?.is_some();
    Ok(if tombstone.is_some() || was_here {
        Resolved::Gone
    } else {
        Resolved::NotYet
    })
}

const PRESENT: &str = "present";
const STATS_FIELD: &str = "stats_snapshot";
/// When a WAD's stats were last cleared, kept beside its fields in `sync_clocks`.
const STATS_RESET: &str = "stats_reset";

fn apply_row(
    tx: &Connection,
    entity: &'static Entity,
    event: &Event,
    this_machine: &str,
    after: &mut AfterCommit,
) -> Result<Outcome> {
    let Key::Column(key_column) = entity.key else {
        return Ok(Outcome::Dropped);
    };
    if !config::is_data_key(&event.key) {
        tracing::warn!(
            "{} has a key that could not be one: {}",
            event.kind,
            event.key
        );
        return Ok(Outcome::Dropped);
    }
    let table = entity.table;
    let tombstoned: bool = tx
        .query_row(
            "SELECT 1 FROM sync_tombstones WHERE kind = ?1 AND key = ?2",
            [entity.kind, event.key.as_str()],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if tombstoned {
        return Ok(Outcome::Dropped);
    }

    if event.op == Op::Delete {
        tx.execute(
            &format!("DELETE FROM {table} WHERE {key_column} = ?1"),
            [&event.key],
        )?;
        // The row's trigger did this if there was a row. There may not have been yet.
        tx.execute(
            "INSERT OR IGNORE INTO sync_tombstones (kind, key) VALUES (?1, ?2)",
            [entity.kind, event.key.as_str()],
        )?;
        return Ok(Outcome::Applied);
    }

    let entity_id = entity_id(entity.kind, &event.key);
    let existing: Option<i64> = tx
        .query_row(
            &format!("SELECT id FROM {table} WHERE {key_column} = ?1"),
            [&event.key],
            |row| row.get(0),
        )
        .optional()?;
    let is_wad = entity.kind == "wad";

    let stamp = stamp_of(event);
    let mut columns: Vec<(&'static str, SqlValue)> = Vec::new();
    let mut clocks: Vec<&'static str> = Vec::new();
    let mut stats: Option<Option<String>> = None;
    for (name, value) in &event.fields {
        let Some(field) = entity.field(name) else {
            continue;
        };
        let Some(mut decoded) = decode(field, value) else {
            tracing::warn!("ignoring {name} of {}: {value}", event.key);
            continue;
        };
        if let (Some(parent), SqlValue::Text(global)) = (field.parent(), &decoded) {
            match resolve(tx, parent, global)? {
                Resolved::Here(id) => decoded = SqlValue::Integer(id),
                Resolved::NotYet => return Ok(Outcome::Wait(Waiting::ForParent)),
                Resolved::Gone if field.required => {
                    // A row can never be made under a parent that is not coming back,
                    // and nor can anything later about it.
                    tx.execute(
                        "INSERT OR IGNORE INTO sync_tombstones (kind, key) VALUES (?1, ?2)",
                        [entity.kind, event.key.as_str()],
                    )?;
                    return Ok(Outcome::Dropped);
                }
                Resolved::Gone => decoded = SqlValue::Null,
            }
        }
        if is_wad && name == STATS_FIELD {
            stats = Some(match decoded {
                SqlValue::Text(text) => Some(text),
                _ => None,
            });
            continue;
        }
        if newer(tx, &entity_id, name, &stamp)? {
            clocks.push(field.name);
            columns.push((field.column, decoded));
        }
    }

    let id = match existing {
        Some(id) => {
            let has_file = is_wad && wad_has_file(tx, id)?;
            let mut changes = columns;
            if has_file {
                changes.retain(|(column, _)| !is_identity_column(entity, column));
            }
            if is_wad {
                note_data_key_change(tx, id, &changes, after)?;
            }
            if !changes.is_empty() {
                update_row(tx, table, id, &changes)?;
            }
            id
        }
        None => {
            let missing = entity.fields.iter().any(|field| {
                field.required && !columns.iter().any(|(column, _)| *column == field.column)
            });
            if missing {
                // A `set` that got here before the `add`, which is on its way.
                return Ok(Outcome::Wait(Waiting::ForParent));
            }
            let mut values = columns;
            values.push((key_column, SqlValue::Text(event.key.clone())));
            insert_row(tx, table, &values)?;
            tx.last_insert_rowid()
        }
    };
    for field in clocks {
        store_clock(tx, &entity_id, field, &stamp)?;
    }

    if is_wad {
        if let Some(value) = stats {
            apply_stats(tx, id, &entity_id, value.as_deref(), event, this_machine)?;
        }
        tx.execute(
            "UPDATE wads SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![event.at, id],
        )?;
    }
    Ok(Outcome::Applied)
}

fn is_identity_column(entity: &Entity, column: &str) -> bool {
    entity
        .fields
        .iter()
        .any(|field| field.column == column && field.role == Role::Identity)
}

/// Whether this machine has a file for a WAD, by the row. `presence` keeps that honest.
fn wad_has_file(tx: &Connection, wad_id: i64) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT COALESCE(cached_path, '') <> '' FROM wads WHERE id = ?1",
        [wad_id],
        |row| row.get(0),
    )?)
}

fn note_data_key_change(
    tx: &Connection,
    wad_id: i64,
    changes: &[(&'static str, SqlValue)],
    after: &mut AfterCommit,
) -> Result<()> {
    let new_key = changes.iter().find_map(|(column, value)| match value {
        SqlValue::Text(key) if *column == "data_key" => Some(key.clone()),
        _ => None,
    });
    let Some(new_key) = new_key else {
        return Ok(());
    };
    let old_key: Option<String> =
        tx.query_row("SELECT data_key FROM wads WHERE id = ?1", [wad_id], |row| {
            row.get(0)
        })?;
    if let Some(old_key) = old_key
        && old_key != new_key
    {
        after.renamed_keys.push((old_key, new_key));
    }
    Ok(())
}

fn update_row(tx: &Connection, table: &str, id: i64, values: &[(&str, SqlValue)]) -> Result<()> {
    let set = values
        .iter()
        .map(|(column, _)| format!("{column} = ?"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("UPDATE {table} SET {set} WHERE id = ?");
    let mut params: Vec<&dyn rusqlite::ToSql> = values
        .iter()
        .map(|(_, value)| value as &dyn rusqlite::ToSql)
        .collect();
    params.push(&id);
    tx.execute(&sql, params.as_slice())?;
    Ok(())
}

fn insert_row(tx: &Connection, table: &str, values: &[(&str, SqlValue)]) -> Result<()> {
    let columns = values
        .iter()
        .map(|(column, _)| *column)
        .collect::<Vec<_>>()
        .join(", ");
    let holes = vec!["?"; values.len()].join(", ");
    let sql = format!("INSERT INTO {table} ({columns}) VALUES ({holes})");
    let params: Vec<&dyn rusqlite::ToSql> = values
        .iter()
        .map(|(_, value)| value as &dyn rusqlite::ToSql)
        .collect();
    tx.execute(&sql, params.as_slice())?;
    Ok(())
}

/// Take in what a machine says about a WAD's maps, or that it has started over, and work
/// out the row again from what is left.
///
/// Compared by plain clock, without the op's rank: an `add` holds no clear, since it leaves
/// out what is empty, and a clear has to be ordered against every snapshot of either kind.
fn apply_stats(
    tx: &Connection,
    wad_id: i64,
    entity_id: &str,
    value: Option<&str>,
    event: &Event,
    this_machine: &str,
) -> Result<()> {
    let clock = event.clock.to_string();
    let reset = clock_of(tx, entity_id, STATS_RESET)?;
    match value {
        None => {
            if reset.is_some_and(|reset| clock <= reset) {
                return Ok(());
            }
            store_clock(tx, entity_id, STATS_RESET, &clock)?;
            tx.execute(
                "DELETE FROM wad_stats_by_machine WHERE wad_id = ?1 AND clock <= ?2",
                rusqlite::params![wad_id, clock],
            )?;
        }
        Some(stats) => {
            if reset.is_some_and(|reset| clock <= reset) {
                return Ok(());
            }
            if crate::wad_stats::stats_from_json(stats).is_err() {
                tracing::warn!("ignoring stats for WAD {wad_id} that are not stats");
                return Ok(());
            }
            tx.execute(
                "INSERT INTO wad_stats_by_machine (wad_id, machine, is_this_machine, clock, stats) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(wad_id, machine) DO UPDATE SET \
                     clock = excluded.clock, stats = excluded.stats \
                 WHERE excluded.clock > clock",
                rusqlite::params![
                    wad_id,
                    event.machine,
                    event.machine == this_machine,
                    clock,
                    stats
                ],
            )?;
        }
    }

    let known = stats_by_machine(tx, wad_id, false)?;
    tx.execute(
        "UPDATE wads SET stats_snapshot = ?1 WHERE id = ?2 AND stats_snapshot IS NOT ?1",
        rusqlite::params![known, wad_id],
    )?;
    Ok(())
}

/// What is known about a WAD's maps from every machine, or from every other one. In an
/// order of their own, because what comes out has to be the same text everywhere.
fn stats_by_machine(conn: &Connection, wad_id: i64, others_only: bool) -> Result<Option<String>> {
    let filter = if others_only {
        "AND is_this_machine = 0"
    } else {
        ""
    };
    let mut statement = conn.prepare(&format!(
        "SELECT stats FROM wad_stats_by_machine WHERE wad_id = ?1 {filter} ORDER BY machine"
    ))?;
    let said: Vec<String> = statement
        .query_map([wad_id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(said.iter().fold(None, |merged, next| {
        crate::wad_stats::merge_json(merged.as_deref(), next).or(merged)
    }))
}

/// What this machine's own stats say, with whatever other machines have said, by map. As
/// it is if they have said nothing, so that a library on one machine goes on as it always
/// has: see [`crate::player`].
pub fn with_other_machines(conn: &Connection, wad_id: i64, disk: &str) -> Result<String> {
    Ok(stats_by_machine(conn, wad_id, true)?
        .and_then(|others| crate::wad_stats::merge_json(Some(disk), &others))
        .unwrap_or_else(|| disk.to_string()))
}

/// What makes a membership row this row, as `(column, value)`, or why that cannot be said.
fn membership_identity(
    tx: &Connection,
    entity: &Entity,
    event: &Event,
) -> Result<std::result::Result<Vec<(&'static str, SqlValue)>, Outcome>> {
    if let Key::Column(column) = entity.key {
        return Ok(Ok(vec![(column, SqlValue::Text(event.key.clone()))]));
    }
    let mut identity = Vec::new();
    for field in entity.fields.iter().filter(|field| field.is_key()) {
        let Some(value) = event
            .fields
            .get(field.name)
            .and_then(|value| decode(field, value))
        else {
            return Ok(Err(Outcome::Dropped));
        };
        let value = match (field.parent(), value) {
            (Some(parent), SqlValue::Text(global)) => match resolve(tx, parent, &global)? {
                Resolved::Here(id) => SqlValue::Integer(id),
                Resolved::Gone => return Ok(Err(Outcome::Dropped)),
                Resolved::NotYet => return Ok(Err(Outcome::Wait(Waiting::ForParent))),
            },
            (_, value) => value,
        };
        identity.push((field.column, value));
    }
    Ok(Ok(identity))
}

fn apply_membership(
    tx: &Connection,
    entity: &'static Entity,
    event: &Event,
    paths: &MergePaths<'_>,
) -> Result<Outcome> {
    let identity = match membership_identity(tx, entity, event)? {
        Ok(identity) => identity,
        Err(outcome) => return Ok(outcome),
    };
    let table = entity.table;
    let condition = identity
        .iter()
        .map(|(column, _)| format!("{column} = ?"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let params: Vec<&dyn rusqlite::ToSql> = identity
        .iter()
        .map(|(_, value)| value as &dyn rusqlite::ToSql)
        .collect();
    let exists: bool = tx
        .query_row(
            &format!("SELECT 1 FROM {table} WHERE {condition}"),
            params.as_slice(),
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);

    let entity_id = entity_id(entity.kind, &event.key);
    let stamp = stamp_of(event);
    let present = event.fields.get(PRESENT).and_then(Value::as_i64);
    let was_removed_or_known = clock_of(tx, &entity_id, PRESENT)?.is_some();
    let takes_presence = present.is_some() && newer(tx, &entity_id, PRESENT, &stamp)?;

    if takes_presence {
        store_clock(tx, &entity_id, PRESENT, &stamp)?;
    }
    if takes_presence && present == Some(0) {
        tx.execute(
            &format!("DELETE FROM {table} WHERE {condition}"),
            params.as_slice(),
        )?;
        return Ok(Outcome::Applied);
    }

    let attributes: Vec<(&'static Field, SqlValue)> = entity
        .fields
        .iter()
        .filter(|field| !field.is_key())
        .filter_map(|field| {
            let value = event.fields.get(field.name)?;
            match decode(field, value) {
                Some(decoded) => Some((field, decoded)),
                None => {
                    tracing::warn!("ignoring {} of {}: {value}", field.name, event.key);
                    None
                }
            }
        })
        .collect();

    if !exists {
        if !(takes_presence && present == Some(1)) {
            // An older add that lost to a removal, or a change to a row that has gone, is
            // finished with. A change to one nothing has said anything about yet is not.
            return Ok(if was_removed_or_known {
                Outcome::Dropped
            } else {
                Outcome::Wait(Waiting::ForParent)
            });
        }
        let missing = entity.fields.iter().any(|field| {
            field.required
                && !field.is_key()
                && !attributes.iter().any(|(given, _)| given.name == field.name)
        });
        if missing {
            return Ok(Outcome::Dropped);
        }
        let mut values: Vec<(&str, SqlValue)> = identity.clone();
        values.extend(
            attributes
                .iter()
                .map(|(field, value)| (field.column, value.clone())),
        );
        // The one column that is about this machine, worked out here rather than sent.
        let path;
        if entity.kind == "companion" {
            let Some(filename) = attributes.iter().find_map(|(field, value)| match value {
                SqlValue::Text(text) if field.name == "filename" => Some(text.clone()),
                _ => None,
            }) else {
                return Ok(Outcome::Dropped);
            };
            let Some(managed) = managed_companion_path(paths.companion_dir, &event.key, &filename)
            else {
                return Ok(Outcome::Dropped);
            };
            path = managed.to_string_lossy().into_owned();
            values.push(("path", SqlValue::Text(path)));
        }
        insert_row(tx, table, &values)?;
        for (field, _) in &attributes {
            if newer(tx, &entity_id, field.name, &stamp)? {
                store_clock(tx, &entity_id, field.name, &stamp)?;
            }
        }
        return Ok(Outcome::Applied);
    }

    let mut changes: Vec<(String, SqlValue)> = Vec::new();
    for (field, value) in &attributes {
        if newer(tx, &entity_id, field.name, &stamp)? {
            store_clock(tx, &entity_id, field.name, &stamp)?;
            changes.push((format!("{} = ?", field.column), value.clone()));
        }
    }
    if !changes.is_empty() {
        let set = changes
            .iter()
            .map(|(clause, _)| clause.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("UPDATE {table} SET {set} WHERE {condition}");
        let mut all: Vec<&dyn rusqlite::ToSql> = changes
            .iter()
            .map(|(_, value)| value as &dyn rusqlite::ToSql)
            .collect();
        all.extend(params.iter().copied());
        tx.execute(&sql, all.as_slice())?;
    }
    Ok(Outcome::Applied)
}

/// Where a companion lives, as `companion_service` files it: `{md5[:12]}_{filename}`. Only
/// for a key that could be an MD5, because a key is what comes from the file.
fn managed_companion_path(dir: &Path, md5: &str, filename: &str) -> Option<PathBuf> {
    if md5.len() != 32 || !md5.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(dir.join(format!("{}_{filename}", &md5[..12])))
}

fn apply_annotation(tx: &Connection, entity: &'static Entity, event: &Event) -> Result<Outcome> {
    let Key::Column(key_column) = entity.key else {
        return Ok(Outcome::Dropped);
    };
    let table = entity.table;
    let id: Option<i64> = tx
        .query_row(
            &format!("SELECT id FROM {table} WHERE {key_column} = ?1"),
            [&event.key],
            |row| row.get(0),
        )
        .optional()?;
    // Not in this build's dataset, which an upgrade may change.
    let Some(id) = id else {
        return Ok(Outcome::Wait(Waiting::ForParent));
    };

    let entity_id = entity_id(entity.kind, &event.key);
    let stamp = stamp_of(event);
    let mut changes = Vec::new();
    for (name, value) in &event.fields {
        let Some(field) = entity.field(name) else {
            continue;
        };
        let Some(mut decoded) = decode(field, value) else {
            continue;
        };
        if let (Some(parent), SqlValue::Text(global)) = (field.parent(), &decoded) {
            decoded = match resolve(tx, parent, global)? {
                Resolved::Here(id) => SqlValue::Integer(id),
                Resolved::Gone => SqlValue::Null,
                Resolved::NotYet => return Ok(Outcome::Wait(Waiting::ForParent)),
            };
        }
        if newer(tx, &entity_id, name, &stamp)? {
            store_clock(tx, &entity_id, name, &stamp)?;
            changes.push((field.column, decoded));
        }
    }
    if !changes.is_empty() {
        update_row(tx, table, id, &changes)?;
    }
    Ok(Outcome::Applied)
}

/// Bring a WAD's folder along when the key that names it changes, which happens when two
/// machines had each already made one for the same WAD. Only when that leaves one folder.
/// With two, nothing moves, and nothing is lost: the other just goes unused until someone
/// looks in it, and the Clean tab will list it.
fn rename_data_dirs(root: &Path, renames: &[(String, String)]) {
    for (old_key, new_key) in renames {
        let Some(old) = config::find_data_dir_in(root, old_key, "") else {
            continue;
        };
        if config::find_data_dir_in(root, new_key, "").is_some() {
            continue;
        }
        let Some(name) = old
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        let rest = name.split_once('_').map_or("", |(_, rest)| rest);
        let new = root.join(format!("{new_key}_{rest}"));
        if let Err(e) = std::fs::rename(&old, &new) {
            tracing::warn!(
                "could not rename {} to {}: {e}",
                old.display(),
                new.display()
            );
        }
    }
}
