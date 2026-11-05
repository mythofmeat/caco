//! Keeping a library on several machines without syncing `library.db`.
//!
//! The data dir gets synced, by git, Syncthing or a shared drive, and `library.db` cannot
//! be: it is one binary file, so playing on two machines between syncs means keeping one
//! machine's database and throwing away the other's sessions (#57). So each machine keeps
//! an append-only log of what happened on it, under `<sync>/<machine>/<yyyy-mm>.jsonl`.
//! Each one only ever writes its own files, so files from different machines cannot
//! conflict and any transport works.
//!
//! # Which is the source of truth
//!
//! Between machines, the logs. On a machine, `library.db` is written to first, as ever,
//! and read as ever, and [`capture`]'s triggers write down what changed in the same
//! transaction, so nothing that writes to it has to know any of this. What makes the log
//! the truth is that everything worth keeping can be rebuilt from it: delete `library.db`,
//! start caco, and the library comes back, since what it would be missing is either in a
//! log or can be worked out again from a file or a dataset.
//!
//! So going back to a copy of `library.db` from another machine, which is what resolving a
//! git conflict on it used to mean, now loses nothing. Whatever this machine had done that
//! the copy has not seen is still in this machine's log, and is applied again.
//!
//! # A pass
//!
//! [`run`] writes out the outbox, and then merges. In that order, because a local write
//! has to be compared against what arrives by a clock it does not have until it is
//! written out. It runs on a worker at startup, and every so often after that: see
//! `caco::app::sync`.
//!
//! Moving the logs is left to whatever moves the rest of the data dir, so caco talks to
//! no remote itself.

pub mod capture;
pub mod clock;
pub mod event;
pub mod log;
pub mod machine;
pub mod merge;
pub mod saves;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use rusqlite::Connection;

pub use merge::MergeReport;
pub use saves::{NewerSaves, SaveEntry};

use crate::Result;
use crate::config;

/// Everything a pass reads and writes. Explicit, for the reason `GcPaths` is: a test that
/// could reach the real folders could write a log into the user's library, and a merge
/// from the real one into a test's.
#[derive(Debug, Clone)]
pub struct SyncPaths {
    /// Holds a folder for each machine.
    pub log_dir: PathBuf,
    /// This machine's id, and so its folder under `log_dir`.
    pub machine: String,
    /// Holds a folder for each WAD.
    pub wad_data_dir: PathBuf,
    /// Where a companion file would be.
    pub companion_dir: PathBuf,
}

impl SyncPaths {
    /// The real, configured locations for the library at `db_path`. Frontends use this;
    /// tests must not.
    pub fn from_config(db_path: &Path) -> Result<Self> {
        Ok(Self {
            log_dir: config::sync_dir_for(db_path),
            machine: machine::this_machine()?,
            wad_data_dir: config::get_data_dir(),
            companion_dir: config::get_companion_dir(),
        })
    }
}

/// What a pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Events this machine added to its log.
    pub written: usize,
    /// What was merged. Empty if the logs could not be read at all.
    pub merge: MergeReport,
    /// Why this machine's events could not be written out, in which case they are still
    /// in the outbox and will be tried again.
    pub write_error: Option<String>,
    /// Why the logs could not be read.
    pub read_error: Option<String>,
}

impl SyncReport {
    /// How many events were applied from anywhere.
    pub fn applied(&self) -> usize {
        self.merge.applied.values().sum()
    }

    /// Whether anything a view of the library shows has changed.
    pub fn changed(&self) -> bool {
        self.applied() > 0
    }
}

/// Write out what happened here, and take in what happened elsewhere.
///
/// Run it off the UI thread. A folder that cannot be reached fails on its own, and the
/// other half still goes: a laptop with its sync folder on a drive at home has plenty to
/// write down and nothing to read, and with a read-only folder it is the other way about.
pub fn run(conn: &Connection, paths: &SyncPaths) -> SyncReport {
    let mut report = SyncReport::default();
    match log::write_outbox(conn, &paths.log_dir, &paths.machine) {
        Ok(written) => report.written = written,
        Err(e) => report.write_error = Some(e.to_string()),
    }
    let merge_paths = merge::MergePaths {
        log_dir: &paths.log_dir,
        machine: &paths.machine,
        wad_data_dir: &paths.wad_data_dir,
        companion_dir: &paths.companion_dir,
    };
    match merge::merge(conn, &merge_paths) {
        Ok(merge) => report.merge = merge,
        Err(e) => report.read_error = Some(e.to_string()),
    }
    report
}

/// A machine this library has heard from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineStatus {
    pub machine: String,
    /// When its latest event happened, as RFC 3339.
    pub last_at: String,
    /// How many of its events this library has.
    pub events: i64,
}

/// Every machine this library has heard from, the most recently active first.
pub fn machines(conn: &Connection) -> Result<Vec<MachineStatus>> {
    let mut statement =
        conn.prepare("SELECT machine, last_at, events FROM sync_machines ORDER BY last_at DESC")?;
    let rows = statement
        .query_map([], |row| {
            Ok(MachineStatus {
                machine: row.get(0)?,
                last_at: row.get(1)?,
                events: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// How many events are waiting, for something to arrive and for a newer version of caco.
pub fn parked_counts(conn: &Connection) -> Result<(i64, i64)> {
    let count = |reason: &str| -> Result<i64> {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM sync_parked WHERE reason = ?1",
            [reason],
            |row| row.get(0),
        )?)
    };
    Ok((count("parent")?, count("upgrade")?))
}
