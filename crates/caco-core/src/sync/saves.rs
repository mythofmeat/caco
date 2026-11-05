//! Noticing that another machine has saves this one does not.
//!
//! A save is an opaque file a sourceport rewrites, with nothing inside it caco could merge.
//! Two machines writing the same slot is a real conflict, and only the person playing can
//! say which save they meant to keep. So this detects and does not resolve: each machine
//! records what its saves are after a session, and a launch compares what is on the disk
//! with what the other machines last said. Whether saves get from one machine to another
//! is the sync tool's business, and if they have, the hashes agree and nothing is asked.
//!
//! By clock and not by file time. A checkout or a sync gives a file a time of its own, so
//! a save pulled just now looks newer than anything, and nothing here can tell how a file
//! came to be on this machine. What can be told is whether this machine has said anything
//! since the other one last did.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::identity;
use crate::saves;

/// A save file, as a machine reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveEntry {
    /// Under the WAD's data dir, with `/` as the separator on every machine.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// What a launch was stopped for. Which machine, when, and which saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerSaves {
    pub machine: String,
    /// When that machine said so, as RFC 3339.
    pub recorded_at: String,
    /// The saves it has and this machine does not, or has different.
    pub files: Vec<String>,
}

/// What this machine's saves for a WAD are right now, sorted by path so that the same
/// saves always read as the same. A file that cannot be read is left out, since it cannot
/// be compared with anything, and the next look will try again.
pub fn read_saves(data_dir: &Path) -> Vec<SaveEntry> {
    let mut entries: Vec<SaveEntry> = saves::find_save_files(data_dir)
        .into_iter()
        .filter_map(|save| {
            let hashes = identity::hash_file(&save.path).ok()?;
            Some(SaveEntry {
                path: save.rel_path.replace('\\', "/"),
                size: hashes.size,
                sha256: hashes.sha256,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

/// Say what this machine's saves are, if that is news.
///
/// `said_anyway` is for a launch that went ahead knowing another machine had newer saves:
/// saying so, even with nothing changed, is what makes that the latest word from here, so
/// that the same saves are not asked about again.
pub fn record_saves(
    conn: &Connection,
    wad_id: i64,
    machine: &str,
    entries: &[SaveEntry],
    said_anyway: bool,
) -> Result<()> {
    let files = serde_json::to_string(entries)?;
    let previous: Option<String> = conn
        .query_row(
            "SELECT files FROM wad_saves WHERE wad_id = ?1 AND machine = ?2",
            rusqlite::params![wad_id, machine],
            |row| row.get(0),
        )
        .optional()?;
    // Never having said anything and having no saves is not news either.
    let unchanged = match &previous {
        Some(previous) => *previous == files,
        None => entries.is_empty(),
    };
    if unchanged && !said_anyway {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO wad_saves (wad_id, machine, files, recorded_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(wad_id, machine) DO UPDATE SET \
             files = excluded.files, recorded_at = excluded.recorded_at",
        rusqlite::params![wad_id, machine, files, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// The latest word on a machine's saves, by the field that last changed. `None` before
/// the first write has gone out, which only this machine's own row can be.
fn clock_of_saves(conn: &Connection, uid: &str, machine: &str) -> Result<Option<String>> {
    let clock: Option<String> = conn.query_row(
        "SELECT MAX(SUBSTR(clock, 2)) FROM sync_clocks WHERE entity = ?1",
        [format!("saves/{uid}/{machine}")],
        |row| row.get(0),
    )?;
    Ok(clock)
}

/// Whether another machine has said something about its saves since this one last did,
/// and what it said is not what is on this disk.
///
/// A save this machine has that the other does not is no conflict, since nothing here is
/// about to overwrite anything of theirs: it is only the other way around that a person
/// would lose their place.
pub fn newer_saves_elsewhere(
    conn: &Connection,
    wad_id: i64,
    wad_uid: &str,
    machine: &str,
    local: &[SaveEntry],
) -> Result<Option<NewerSaves>> {
    let has_said = conn
        .query_row(
            "SELECT 1 FROM wad_saves WHERE wad_id = ?1 AND machine = ?2",
            rusqlite::params![wad_id, machine],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let ours = clock_of_saves(conn, wad_uid, machine)?;
    // Saying something this machine has not written out yet is still the newest thing it
    // knows of, since whatever else is here it has already merged.
    if has_said && ours.is_none() {
        return Ok(None);
    }

    let mut statement = conn.prepare(
        "SELECT machine, files, recorded_at FROM wad_saves WHERE wad_id = ?1 AND machine <> ?2",
    )?;
    let others: Vec<(String, String, String)> = statement
        .query_map(rusqlite::params![wad_id, machine], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut newest: Option<(String, NewerSaves)> = None;
    for (other, files, recorded_at) in others {
        let Some(theirs) = clock_of_saves(conn, wad_uid, &other)? else {
            continue;
        };
        if ours.as_ref().is_some_and(|ours| *ours >= theirs) {
            continue;
        }
        let differing = differing_saves(local, &files);
        if differing.is_empty() {
            continue;
        }
        if newest.as_ref().is_none_or(|(clock, _)| *clock < theirs) {
            newest = Some((
                theirs,
                NewerSaves {
                    machine: other,
                    recorded_at,
                    files: differing,
                },
            ));
        }
    }
    Ok(newest.map(|(_, saves)| saves))
}

/// The saves in `theirs` that are not on this disk, or are different here. Nothing at all,
/// if `theirs` cannot be read: there is nothing to warn about in a line nobody can parse.
fn differing_saves(local: &[SaveEntry], theirs: &str) -> Vec<String> {
    let Ok(theirs) = serde_json::from_str::<Vec<SaveEntry>>(theirs) else {
        return Vec::new();
    };
    theirs
        .into_iter()
        .filter(|entry| {
            !local
                .iter()
                .any(|mine| mine.path == entry.path && mine.sha256 == entry.sha256)
        })
        .map(|entry| entry.path)
        .collect()
}
