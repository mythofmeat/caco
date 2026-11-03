//! Whether the file a WAD row records is really on this machine.
//!
//! [`Availability`](crate::db::Availability) and [`WadRecord::is_lost`] are
//! derived from `cached_path` and never look at the filesystem, which makes
//! them exactly as honest as that column. A file can go without caco deleting
//! it: a data dir synced from another machine records paths that only exist
//! there, a drive gets unmounted, someone tidies a directory by hand. Until
//! something notices, the row keeps claiming a copy it does not have — which
//! is how a library came to list six manual-only WADs as cached, My House
//! among them, on a machine that had none of their files.
//!
//! This module is what notices. [`check_library`] runs over every row at
//! startup and [`check_wad`] over one before each launch, and both apply one
//! rule:
//!
//! - the recorded file is there: nothing to do;
//! - a file of the same name is in the cache or keep dir: point at it, because
//!   the directory moved (`cache_dir` is editable in Settings, and the data dir
//!   lives somewhere different on every platform);
//! - neither: clear `cached_path` and keep the path as `last_known_path`.
//!
//! A row with only a `last_known_path` goes through the same lookup, so a file
//! that comes back — the drive is mounted again, the sync caught up — is found
//! without anyone relinking it. The rule deliberately ignores retrievability:
//! an idgames WAD linked to a file on an unmounted drive is as worth finding
//! again as a manual one, and remembering where a cache file used to be costs
//! a column.
//!
//! `last_known_path` is history, not a copy of derivable state, so it does not
//! bring back the drift migration 38 removed. The two columns are never both
//! set: this module writes them together, and setting `cached_path` through
//! [`crate::db::WadUpdate`] clears the other.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::config;
use crate::db::{self, Retrievability, WadRecord};

/// The two directories caco files WADs into, which is where a file whose
/// recorded path has gone is looked for by name.
///
/// Passed explicitly for the reason [`crate::gc::GcPaths`] is: a test that
/// checked against the real roots could relink its rows to files in the user's
/// library.
#[derive(Debug, Clone)]
pub struct StorePaths {
    pub cache_dir: PathBuf,
    pub keep_dir: PathBuf,
}

impl StorePaths {
    /// The real, configured locations. Frontends use this; tests must not.
    pub fn from_config() -> Self {
        Self {
            cache_dir: config::get_cache_dir(),
            keep_dir: config::keep_dir(),
        }
    }

    /// Directories to look in for a file by name, the WAD's own store dir
    /// first, as [`config::wad_store_dir_in`] places it.
    ///
    /// Both rather than only that one: a manual WAD's file sat in the cache
    /// before there was a keep dir, and a WAD that learns its idgames id keeps
    /// the file it already had.
    fn search_order(&self, retrievability: Retrievability) -> [&Path; 2] {
        match retrievability {
            Retrievability::Automatic => [&self.cache_dir, &self.keep_dir],
            Retrievability::Manual => [&self.keep_dir, &self.cache_dir],
        }
    }
}

/// Whether a WAD's file is on this machine, as [`check_wad`] found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// The file is here, at this path.
    Here(PathBuf),
    /// It is not. `last_seen` is where it was the last time caco found it, if
    /// it ever had one.
    Missing { last_seen: Option<PathBuf> },
}

impl Presence {
    pub fn is_here(&self) -> bool {
        matches!(self, Presence::Here(_))
    }
}

/// What [`check_library`] changed, counted over live rows only: a trashed WAD
/// is checked, since it can be restored, but its file is not news.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// Rows pointed at their file again, found under its own name in the
    /// cache or keep dir or back where it was last seen.
    pub relinked: usize,
    /// Rows whose file has gone, now recorded as last seen there.
    pub missing: usize,
    /// Of `missing`, the ones caco cannot download again, which are now lost.
    pub lost: usize,
}

impl CheckReport {
    /// Whether anything a view of the library shows has changed.
    pub fn changed(&self) -> bool {
        self.relinked > 0 || self.missing > 0
    }
}

/// What a check writes back for one row.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// The file is at this path, so forget where it was last seen.
    Relink(PathBuf),
    /// The recorded file has gone: remember where it was.
    MarkMissing,
}

/// Check every row that records a file, and write back what was found.
///
/// Run it off the UI thread: it costs one `stat` per row on a local disk, and
/// however long a network mount that has stopped answering takes to fail.
pub fn check_library(conn: &Connection, paths: &StorePaths) -> crate::Result<CheckReport> {
    // Every stat happens before any write, so the transaction below lasts
    // microseconds instead of as long as a mount takes to answer, and the UI's
    // own writes never queue behind it.
    let changes: Vec<(WadRecord, Change)> = recorded_wads(conn)?
        .into_iter()
        .filter_map(|wad| decide(&wad, paths).map(|change| (wad, change)))
        .collect();

    db::with_transaction(conn, |tx| {
        let mut report = CheckReport::default();
        for (wad, change) in &changes {
            if !apply(tx, wad, change)? || wad.deleted_at.is_some() {
                continue;
            }
            match change {
                Change::Relink(_) => report.relinked += 1,
                Change::MarkMissing => {
                    report.missing += 1;
                    if !wad.retrievability().is_automatic() {
                        report.lost += 1;
                    }
                }
            }
        }
        Ok(report)
    })
}

/// Check one WAD's file before launching it, writing back what was found.
///
/// The rule [`check_library`] applies at startup, so a file deleted since then,
/// or on a drive unmounted since, is recorded as missing here instead of
/// failing inside the sourceport, and a file that has come back gets used.
pub fn check_wad(
    conn: &Connection,
    wad: &WadRecord,
    paths: &StorePaths,
) -> crate::Result<Presence> {
    let Some(change) = decide(wad, paths) else {
        return Ok(match nonempty(wad.cached_path.as_deref()) {
            Some(path) => Presence::Here(PathBuf::from(path)),
            None => Presence::Missing {
                last_seen: nonempty(wad.last_known_path.as_deref()).map(PathBuf::from),
            },
        });
    };
    // A miss means the row changed after the caller read it: a startup check
    // got there first with the same answer, or a file was linked in between,
    // which the next launch picks up. Neither makes this answer wrong.
    apply(conn, wad, &change)?;
    Ok(match change {
        Change::Relink(path) => Presence::Here(path),
        Change::MarkMissing => Presence::Missing {
            last_seen: nonempty(wad.cached_path.as_deref()).map(PathBuf::from),
        },
    })
}

/// Every row with a file to look for. Trashed rows too, because they can be
/// restored, and a restored row should not come back claiming a file.
fn recorded_wads(conn: &Connection) -> crate::Result<Vec<WadRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM wads \
         WHERE COALESCE(cached_path, '') <> '' OR COALESCE(last_known_path, '') <> ''",
    )?;
    let wads = stmt
        .query_map([], WadRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(wads)
}

/// Look for one row's file. Touches the filesystem and nothing else.
///
/// `None` when there is nothing to write: the file is where the row says, the
/// row records no file, or a file already known to be missing is still nowhere.
fn decide(wad: &WadRecord, paths: &StorePaths) -> Option<Change> {
    let (recorded, is_cached) = match (
        nonempty(wad.cached_path.as_deref()),
        nonempty(wad.last_known_path.as_deref()),
    ) {
        (Some(path), _) => (Path::new(path), true),
        (None, Some(path)) => (Path::new(path), false),
        (None, None) => return None,
    };
    // `exists`, not `is_file`: anything at the path is reason to leave the row
    // alone. `exists` also reads an error, a permission refused, say, as
    // absence, which is the right answer here: the sourceport could not open
    // the file either, and the first check that can will find it again.
    if recorded.exists() {
        return (!is_cached).then(|| Change::Relink(recorded.to_path_buf()));
    }
    if let Some(found) = find_by_name(recorded, wad.retrievability(), paths) {
        return Some(Change::Relink(found));
    }
    is_cached.then_some(Change::MarkMissing)
}

/// A file named like `recorded` directly under one of the store dirs, which
/// is how caco files every WAD it places — so a moved directory still has it
/// under the same name.
fn find_by_name(
    recorded: &Path,
    retrievability: Retrievability,
    paths: &StorePaths,
) -> Option<PathBuf> {
    let name = recorded.file_name()?;
    paths
        .search_order(retrievability)
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|candidate| candidate != recorded && candidate.is_file())
}

/// Write one row's change, unless the row has moved on since it was read.
///
/// A check reads, then stats, then writes, and a stat can take as long as a
/// mount does to answer. Comparing against what was read keeps a slow check
/// from undoing a link made in the meantime. `updated_at` is left alone:
/// nobody edited the WAD.
fn apply(conn: &Connection, wad: &WadRecord, change: &Change) -> crate::Result<bool> {
    let (cached_path, last_known_path) = match change {
        Change::Relink(path) => (Some(path.to_string_lossy().into_owned()), None),
        Change::MarkMissing => (None, wad.cached_path.clone()),
    };
    let updated = conn.execute(
        "UPDATE wads SET cached_path = ?1, last_known_path = ?2 \
         WHERE id = ?3 AND cached_path IS ?4 AND last_known_path IS ?5",
        rusqlite::params![
            cached_path,
            last_known_path,
            wad.id,
            wad.cached_path,
            wad.last_known_path
        ],
    )?;
    Ok(updated > 0)
}

/// Blank text fields arrive from the GUI as `Some("")`; they are not paths.
fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Availability, NewWad, SourceType, WadUpdate};

    struct Fixture {
        conn: Connection,
        paths: StorePaths,
        _root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let paths = StorePaths {
                cache_dir: root.path().join("cache"),
                keep_dir: root.path().join("keep"),
            };
            std::fs::create_dir_all(&paths.cache_dir).unwrap();
            std::fs::create_dir_all(&paths.keep_dir).unwrap();
            let conn = db::open_memory().unwrap();
            db::init_db(&conn).unwrap();
            Self {
                conn,
                paths,
                _root: root,
            }
        }

        /// A WAD recording `path` as its file, whether or not anything is there.
        fn wad(&self, source_type: SourceType, path: &Path) -> i64 {
            let id = db::add_wad(&self.conn, &NewWad::new("Test", source_type)).unwrap();
            let update =
                WadUpdate::new().set_text("cached_path", Some(path.to_string_lossy().into()));
            db::update_wad(&self.conn, id, &update).unwrap();
            id
        }

        fn row(&self, id: i64) -> WadRecord {
            db::get_wad(&self.conn, id, true).unwrap().unwrap()
        }

        fn check(&self) -> CheckReport {
            check_library(&self.conn, &self.paths).unwrap()
        }

        fn keep(&self, name: &str) -> PathBuf {
            self.paths.keep_dir.join(name)
        }

        fn cache(&self, name: &str) -> PathBuf {
            self.paths.cache_dir.join(name)
        }
    }

    fn touch(path: &Path) {
        std::fs::write(path, b"PWAD").unwrap();
    }

    fn text(path: &Path) -> Option<String> {
        Some(path.to_string_lossy().into_owned())
    }

    /// A path that resolves is right whatever the store dirs hold: a linked
    /// file in someone's collection is not swapped for a same-named one.
    #[test]
    fn test_file_where_the_row_says_is_left_alone() {
        let f = Fixture::new();
        let collection = tempfile::tempdir().unwrap();
        let linked = collection.path().join("here.wad");
        touch(&linked);
        touch(&f.cache("here.wad"));
        touch(&f.keep("here.wad"));
        let id = f.wad(SourceType::Doomworld, &linked);

        assert_eq!(f.check(), CheckReport::default());
        assert_eq!(f.row(id).cached_path, text(&linked));
        assert_eq!(f.row(id).last_known_path, None);
    }

    /// The issue's case: a manual WAD recorded in the keep dir of a library
    /// whose data dir arrived without the file. It read `Cached` and so was
    /// missing from `retrievable:lost`, while no launch could have worked.
    #[test]
    fn test_vanished_manual_file_is_lost_and_remembered() {
        let f = Fixture::new();
        let id = db::add_wad(
            &f.conn,
            &NewWad::new("My House", SourceType::Doomwiki)
                .source_url("https://doomwiki.org/wiki/My_House"),
        )
        .unwrap();
        let recorded = f.keep("myhouse.pk3");
        let update = WadUpdate::new().set_text("cached_path", text(&recorded));
        db::update_wad(&f.conn, id, &update).unwrap();
        assert_eq!(f.row(id).availability(), Availability::Cached);
        assert!(!f.row(id).is_lost());

        let report = f.check();

        assert_eq!(
            report,
            CheckReport {
                relinked: 0,
                missing: 1,
                lost: 1
            }
        );
        let wad = f.row(id);
        assert_eq!(wad.cached_path, None);
        assert_eq!(wad.last_known_path, text(&recorded));
        assert_eq!(wad.availability(), Availability::Downloadable);
        assert!(wad.is_lost());
    }

    /// An idgames file that has gone downloads again on Play, so it is not
    /// lost, but where it was is still worth knowing if it comes back.
    #[test]
    fn test_vanished_idgames_file_is_remembered_but_not_lost() {
        let f = Fixture::new();
        let id = f.wad(SourceType::Idgames, &f.cache("scythe.zip"));

        assert_eq!(
            f.check(),
            CheckReport {
                relinked: 0,
                missing: 1,
                lost: 0
            }
        );
        assert_eq!(f.row(id).cached_path, None);
        assert_eq!(f.row(id).last_known_path, text(&f.cache("scythe.zip")));
    }

    /// The drive is mounted again: the file is back where it was last seen,
    /// outside both store dirs, and is picked up without a relink by hand.
    #[test]
    fn test_file_back_where_it_was_last_seen_is_relinked() {
        let f = Fixture::new();
        let drive = tempfile::tempdir().unwrap();
        let file = drive.path().join("belot.wad");
        touch(&file);
        let id = f.wad(SourceType::Idgames, &file);

        std::fs::remove_file(&file).unwrap();
        assert_eq!(f.check().missing, 1);
        assert_eq!(f.row(id).cached_path, None);

        touch(&file);
        assert_eq!(
            f.check(),
            CheckReport {
                relinked: 1,
                missing: 0,
                lost: 0
            }
        );
        assert_eq!(f.row(id).cached_path, text(&file));
        assert_eq!(f.row(id).last_known_path, None);
    }

    /// `cache_dir` is editable in Settings, and moving it relocates the files
    /// but not the absolute paths recorded against them.
    #[test]
    fn test_relinks_by_name_after_the_cache_moves() {
        let f = Fixture::new();
        touch(&f.cache("map01.wad"));
        let id = f.wad(
            SourceType::Idgames,
            Path::new("/old/cache/caco/wads/map01.wad"),
        );

        assert_eq!(f.check().relinked, 1);
        assert_eq!(f.row(id).cached_path, text(&f.cache("map01.wad")));
        // Idempotent: the second pass has nothing left to do.
        assert_eq!(f.check(), CheckReport::default());
    }

    /// The data dir is somewhere else on every platform, so a keep-dir path
    /// recorded on Linux does not resolve on a Mac that has the same file in
    /// its own keep dir.
    #[test]
    fn test_relinks_by_name_after_the_data_dir_moves() {
        let f = Fixture::new();
        touch(&f.keep("RAMP2025.pk3"));
        let id = f.wad(
            SourceType::Doomwiki,
            Path::new("/home/someone/.local/share/caco/wads/RAMP2025.pk3"),
        );

        assert_eq!(f.check().relinked, 1);
        assert_eq!(f.row(id).cached_path, text(&f.keep("RAMP2025.pk3")));
    }

    /// A row already known to be missing goes through the same lookup, so a
    /// copy that lands in the keep dir later is found.
    #[test]
    fn test_missing_file_that_turns_up_by_name_is_relinked() {
        let f = Fixture::new();
        let id = f.wad(SourceType::Doomworld, Path::new("/elsewhere/purgatory.pk3"));
        assert_eq!(f.check().lost, 1);

        touch(&f.keep("purgatory.pk3"));
        assert_eq!(f.check().relinked, 1);
        assert_eq!(f.row(id).cached_path, text(&f.keep("purgatory.pk3")));
        assert_eq!(f.row(id).last_known_path, None);
    }

    #[test]
    fn test_name_lookup_prefers_the_wads_own_store_dir() {
        let f = Fixture::new();
        touch(&f.cache("dup.wad"));
        touch(&f.keep("dup.wad"));
        let manual = f.wad(SourceType::Doomworld, Path::new("/gone/dup.wad"));
        let automatic = f.wad(SourceType::Idgames, Path::new("/gone/dup.wad"));

        f.check();

        assert_eq!(f.row(manual).cached_path, text(&f.keep("dup.wad")));
        assert_eq!(f.row(automatic).cached_path, text(&f.cache("dup.wad")));
    }

    #[test]
    fn test_file_still_missing_writes_nothing() {
        let f = Fixture::new();
        let id = f.wad(SourceType::Doomworld, Path::new("/gone/aftercare.wad"));
        f.check();
        let before = f.row(id);

        assert_eq!(f.check(), CheckReport::default());

        let after = f.row(id);
        assert_eq!(after.cached_path, None);
        assert_eq!(after.last_known_path, before.last_known_path);
        assert_eq!(after.updated_at, before.updated_at);
    }

    #[test]
    fn test_trashed_rows_are_checked_but_not_reported() {
        let f = Fixture::new();
        let id = f.wad(SourceType::Doomworld, Path::new("/gone/trashed.wad"));
        db::delete_wad(&f.conn, id, false).unwrap();

        assert_eq!(f.check(), CheckReport::default());
        assert_eq!(f.row(id).cached_path, None);
        assert_eq!(
            f.row(id).last_known_path.as_deref(),
            Some("/gone/trashed.wad")
        );
    }

    /// The check reads, stats, then writes; a file linked while it was waiting
    /// on a slow mount must survive the write.
    #[test]
    fn test_row_changed_mid_check_is_not_overwritten() {
        let f = Fixture::new();
        let id = f.wad(SourceType::Doomworld, Path::new("/slow/mount/datw.wad"));
        let read = f.row(id);
        let change = decide(&read, &f.paths).unwrap();
        assert_eq!(change, Change::MarkMissing);

        touch(&f.keep("datw.wad"));
        let linked = WadUpdate::new().set_text("cached_path", text(&f.keep("datw.wad")));
        db::update_wad(&f.conn, id, &linked).unwrap();

        assert!(!apply(&f.conn, &read, &change).unwrap());
        assert_eq!(f.row(id).cached_path, text(&f.keep("datw.wad")));
        assert_eq!(f.row(id).last_known_path, None);
    }

    #[test]
    fn test_check_wad_answers_for_one_wad_and_records_it() {
        let f = Fixture::new();
        touch(&f.keep("present.wad"));
        let present = f.wad(SourceType::Doomworld, &f.keep("present.wad"));
        let gone = f.wad(SourceType::Doomworld, Path::new("/gone/wormwood4.zip"));
        let never = db::add_wad(&f.conn, &NewWad::new("Never", SourceType::Doomworld)).unwrap();

        let check = |id| check_wad(&f.conn, &f.row(id), &f.paths).unwrap();

        assert_eq!(check(present), Presence::Here(f.keep("present.wad")));
        let missing = Presence::Missing {
            last_seen: Some(PathBuf::from("/gone/wormwood4.zip")),
        };
        assert_eq!(check(gone), missing);
        assert_eq!(f.row(gone).cached_path, None, "Play records what it found");
        // Asking again answers from the row it wrote.
        assert_eq!(check(gone), missing);
        assert_eq!(check(never), Presence::Missing { last_seen: None });

        touch(&f.keep("wormwood4.zip"));
        assert_eq!(check(gone), Presence::Here(f.keep("wormwood4.zip")));
        assert_eq!(f.row(gone).cached_path, text(&f.keep("wormwood4.zip")));
    }

    #[test]
    fn test_blank_paths_are_not_paths() {
        let f = Fixture::new();
        let id = db::add_wad(&f.conn, &NewWad::new("Blank", SourceType::Doomworld)).unwrap();
        f.conn
            .execute(
                "UPDATE wads SET cached_path = '', last_known_path = '' WHERE id = ?1",
                [id],
            )
            .unwrap();

        assert_eq!(f.check(), CheckReport::default());
        assert_eq!(
            check_wad(&f.conn, &f.row(id), &f.paths).unwrap(),
            Presence::Missing { last_seen: None }
        );
    }
}
