//! Linking what landed in the Downloads folder, off the UI thread, and what that tells
//! the user.

use std::path::{Path, PathBuf};

use caco_core::config::{self, LinkMode};
use caco_core::db;
use caco_core::downloads;
use caco_core::identity;
use caco_core::presence::StorePaths;
use caco_sources::import_service::{ImportResult, ImportService};
use rusqlite::Connection;

use crate::dialogs::get_prompt::{self, GetAnswer};
use crate::dialogs::link::link_picked_file_in;
use crate::get::{GetOutcome, GetTarget, LinkRequest};
use crate::message::{AppMessage, Notification};
use crate::state::AppState;
use crate::workers::BackgroundSender;

/// Ask about the file at the head of the queue, if there is one, and act on the answer.
pub(super) fn render_question(
    state: &mut AppState,
    conn: &Connection,
    ctx: &egui::Context,
    sender: &BackgroundSender,
) {
    let Some(asking) = state.gets.asking() else {
        return;
    };
    match get_prompt::render(ctx, &asking, config::get_link_mode()) {
        Some(GetAnswer::Link) => {
            if let Some(request) = state.gets.take_link(conn) {
                spawn_link(sender.clone(), state.db_path.clone(), request);
            }
        }
        Some(GetAnswer::NotThisOne) => state.gets.not_this_one(conn),
        None => {}
    }
}

/// Link a landed file, on a worker thread: it hashes the file and may copy it to another
/// drive, and either can take as long as a large file does.
fn spawn_link(sender: BackgroundSender, db_path: PathBuf, request: LinkRequest) {
    std::thread::spawn(move || {
        let LinkRequest {
            target,
            title,
            path,
        } = request;
        let outcome = caco_core::db::open_connection(&db_path)
            .map_err(|e| format!("DB open failed: {e}"))
            .and_then(|conn| {
                link_landed_file(
                    &conn,
                    target,
                    &path,
                    &StorePaths::from_config(),
                    config::get_link_mode(),
                    |record| ImportService::new().import_cacoward_file(&conn, record, &path),
                )
            });
        sender.send(AppMessage::GetLinked {
            target,
            title,
            filename: file_name(&path),
            outcome,
        });
    });
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Everything linking a landed file involves. The import is passed in so that tests stay
/// off the network, and [`StorePaths`] so that they stay out of the real library.
fn link_landed_file(
    conn: &Connection,
    target: GetTarget,
    path: &Path,
    paths: &StorePaths,
    mode: LinkMode,
    import: impl FnOnce(&db::CacowardRecord) -> ImportResult,
) -> Result<GetOutcome, String> {
    let (wad_id, for_entry) = match target {
        GetTarget::Wad(id) => (id, false),
        GetTarget::Cacoward(pk) => (wad_for_entry(conn, pk, import)?, true),
    };

    let wad = db::get_wad(conn, wad_id, true)
        .map_err(|e| format!("Failed to load WAD: {e}"))?
        .ok_or_else(|| "The WAD is no longer in the library.".to_string())?;
    if wad.deleted_at.is_some() {
        return Err(format!(
            "{} is in the Trash. Restore it from Storage, then get it again.",
            wad.title
        ));
    }
    if for_entry && !wad.cached_path.as_deref().unwrap_or("").is_empty() {
        return Ok(GetOutcome::AlreadyHad { title: wad.title });
    }

    let dest = link_picked_file_in(conn, wad_id, path, paths, mode)?;

    // Now rather than at the next launch, since this is on a worker anyway, and a file
    // with a hash can be found by it: `hash:`, `dup:`, or a mirror. The file is linked
    // whether or not this works, and the startup check will try again if it did not.
    if let Ok(Some(wad)) = db::get_wad(conn, wad_id, false) {
        let _ = identity::verify(conn, &wad, &dest);
    }
    Ok(GetOutcome::Linked { wad_id })
}

/// The WAD a Cacoward entry's file should go to, putting one in the library if it has
/// none. The clicked row is linked by hand, as an import from a card does, because an
/// import only links what it can prove, and the user has just vouched for this one.
fn wad_for_entry(
    conn: &Connection,
    pk: i64,
    import: impl FnOnce(&db::CacowardRecord) -> ImportResult,
) -> Result<i64, String> {
    let record = db::get_cacoward(conn, pk)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no Cacoward entry with pk {pk}"))?;
    // Linked since the Get began, by an Import from the card, or by evidence.
    if let Some(wad_id) = record.wad_id
        && matches!(db::get_wad(conn, wad_id, false), Ok(Some(_)))
    {
        return Ok(wad_id);
    }

    let imported = import(&record);
    if let Some(error) = imported.error {
        return Err(error);
    }
    let wad_id = imported
        .wad_id
        .or(imported.duplicate_id)
        .ok_or_else(|| "The import did not say what it made.".to_string())?;
    if record.wad_id != Some(wad_id) {
        db::link_wad(conn, record.id, wad_id, false).map_err(|e| e.to_string())?;
    }
    Ok(wad_id)
}

/// The status bar's word on a finished link.
pub(super) fn linked_notification(
    title: &str,
    filename: &str,
    outcome: Result<&GetOutcome, &String>,
) -> Notification {
    match outcome {
        Ok(GetOutcome::Linked { .. }) => {
            Notification::info(format!("Linked {filename} to {title}."))
        }
        Ok(GetOutcome::AlreadyHad { title }) => Notification::info(format!(
            "Already in your library as {title}, so {filename} was left where it is."
        )),
        Err(why) => Notification::error(format!("Could not link {filename}. {why}")),
    }
}

/// The status bar's word on a Get that ran out of time.
pub(super) fn expired_notification(titles: &[String]) -> Option<Notification> {
    let text = match titles {
        [] => return None,
        [title] => format!("Stopped waiting for {title}'s download."),
        many => format!("Stopped waiting for {} downloads.", many.len()),
    };
    Some(Notification::info(text))
}

/// The status bar's word on opening a page. Says where the watching is happening,
/// because a browser saving somewhere else is the one thing that would stop this working.
pub(super) fn opened_notification(page: &str) -> Notification {
    Notification::info(format!(
        "Opened {}. Watching {} for the file.",
        downloads::page_host(page),
        config::downloads_dir().display()
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use caco_core::db::{NewCacoward, NewWad, SourceType};
    use caco_core::mirrors::offer_targets;

    use super::*;
    use crate::message::Severity;

    struct Fixture {
        conn: Connection,
        paths: StorePaths,
        downloads: PathBuf,
        _root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let downloads = root.path().join("downloads");
            fs::create_dir_all(&downloads).unwrap();
            let conn = db::open_memory().unwrap();
            db::init_db(&conn).unwrap();
            Self {
                conn,
                paths: StorePaths {
                    cache_dir: root.path().join("cache"),
                    keep_dir: root.path().join("keep"),
                },
                downloads,
                _root: root,
            }
        }

        fn download(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.downloads.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }

        fn wad(&self, id: i64) -> db::WadRecord {
            db::get_wad(&self.conn, id, true).unwrap().unwrap()
        }

        fn link(
            &self,
            target: GetTarget,
            path: &Path,
            mode: LinkMode,
        ) -> Result<GetOutcome, String> {
            link_landed_file(&self.conn, target, path, &self.paths, mode, |record| {
                ImportService::new().import_cacoward_file_with(&self.conn, record, path, |_, _| {})
            })
        }

        fn entry(&self, new: NewCacoward) -> i64 {
            db::upsert_cacoward(
                &self.conn,
                &NewCacoward {
                    key: "2025-winner-lonesome-road".to_string(),
                    year: 2025,
                    category: "winner".to_string(),
                    page_order: 1,
                    wad_title: "Lonesome Road".to_string(),
                    wad_author: Some("tourniquet".to_string()),
                    ..new
                },
            )
            .unwrap()
        }

        fn thread_entry(&self) -> i64 {
            self.entry(NewCacoward {
                doomworld_id: Some(151858),
                ..Default::default()
            })
        }

        fn linked_wad_of(&self, entry: i64) -> Option<i64> {
            db::get_cacoward(&self.conn, entry).unwrap().unwrap().wad_id
        }
    }

    fn manual_wad(f: &Fixture) -> i64 {
        db::add_wad(&f.conn, &NewWad::new("Road", SourceType::Doomworld)).unwrap()
    }

    /// The issue's third step, whole: re-read the record, place the file for what the
    /// record is, hash it, and leave it ready to be offered to a mirror.
    #[test]
    fn test_linking_a_wad_places_and_hashes_the_file() {
        let f = Fixture::new();
        let id = manual_wad(&f);
        let file = f.download("road.zip", b"road");

        let outcome = f.link(GetTarget::Wad(id), &file, LinkMode::Move).unwrap();

        assert_eq!(outcome, GetOutcome::Linked { wad_id: id });
        let wad = f.wad(id);
        let kept = f.paths.keep_dir.join("road.zip");
        assert_eq!(wad.cached_path.as_deref(), kept.to_str());
        assert_eq!(fs::read(&kept).unwrap(), b"road");
        assert!(!file.exists(), "moved out of Downloads, as the mode says");

        let identity = wad.identity.as_ref().expect("hashed at once");
        assert_eq!(identity.file.size, 4);

        let mirror = caco_core::config::MirrorConfig {
            name: "home".to_string(),
            path: Some("/mnt/nas".to_string()),
            write: true,
            ..Default::default()
        };
        assert_eq!(offer_targets(&wad, &[mirror]), ["home"]);
    }

    #[test]
    fn test_the_link_mode_decides_whether_the_download_stays() {
        let f = Fixture::new();
        let id = manual_wad(&f);
        let file = f.download("road.zip", b"road");

        f.link(GetTarget::Wad(id), &file, LinkMode::Copy).unwrap();

        assert_eq!(fs::read(&file).unwrap(), b"road");
        assert_eq!(
            fs::read(f.paths.keep_dir.join("road.zip")).unwrap(),
            b"road"
        );
    }

    /// An idgames zip is stored as it is, and so is what a browser fetched.
    #[test]
    fn test_an_archive_is_linked_as_it_is() {
        let f = Fixture::new();
        let id = manual_wad(&f);
        let file = f.download("road.7z", b"7z-bytes");

        f.link(GetTarget::Wad(id), &file, LinkMode::Move).unwrap();

        assert_eq!(
            fs::read(f.paths.keep_dir.join("road.7z")).unwrap(),
            b"7z-bytes"
        );
    }

    #[test]
    fn test_a_file_that_has_gone_is_reported_and_changes_nothing() {
        let f = Fixture::new();
        let id = manual_wad(&f);

        let failure = f
            .link(
                GetTarget::Wad(id),
                &f.downloads.join("moved-away.zip"),
                LinkMode::Move,
            )
            .unwrap_err();

        assert!(failure.contains("moved-away.zip"), "{failure}");
        assert_eq!(f.wad(id).cached_path, None);
    }

    #[test]
    fn test_a_wad_trashed_since_is_not_linked_to() {
        let f = Fixture::new();
        let id = manual_wad(&f);
        db::delete_wad(&f.conn, id, false).unwrap();
        let file = f.download("road.zip", b"road");

        let failure = f
            .link(GetTarget::Wad(id), &file, LinkMode::Move)
            .unwrap_err();

        assert!(failure.contains("in the Trash"), "{failure}");
        assert!(file.exists(), "and the download is left alone");
    }

    /// An absent entry has no row to link to until now. The import makes one from the
    /// entry's own facts, and the card turns into a WAD.
    #[test]
    fn test_an_absent_entry_gets_a_row_and_the_file() {
        let f = Fixture::new();
        let entry = f.thread_entry();
        let file = f.download("LonesomeRoad.pk3", b"road");

        let outcome = f
            .link(GetTarget::Cacoward(entry), &file, LinkMode::Move)
            .unwrap();

        let GetOutcome::Linked { wad_id } = outcome else {
            panic!("expected the file to be linked, got {outcome:?}");
        };
        assert_eq!(f.linked_wad_of(entry), Some(wad_id));
        let wad = f.wad(wad_id);
        assert_eq!(wad.title, "Lonesome Road");
        let kept = f.paths.keep_dir.join("LonesomeRoad.pk3");
        assert_eq!(wad.cached_path.as_deref(), kept.to_str());
        assert!(!file.exists());
        assert!(wad.identity.is_some());
    }

    /// Identifiers lead an entry to its row, or the user gets a second copy of the WAD.
    /// A row with no file is exactly what a file is wanted for.
    #[test]
    fn test_an_entry_whose_wad_exists_without_a_file_fills_it_in() {
        let f = Fixture::new();
        let entry = f.thread_entry();
        let existing = db::add_wad(
            &f.conn,
            &NewWad::new("Lonesome Road | Chapter 1", SourceType::Doomworld)
                .source_id("151858")
                .source_url("https://www.doomworld.com/vb/thread/151858"),
        )
        .unwrap();
        let file = f.download("road.pk3", b"road");

        let outcome = f
            .link(GetTarget::Cacoward(entry), &file, LinkMode::Move)
            .unwrap();

        assert_eq!(outcome, GetOutcome::Linked { wad_id: existing });
        assert_eq!(f.linked_wad_of(entry), Some(existing));
        assert_eq!(
            f.wad(existing).cached_path.as_deref(),
            f.paths.keep_dir.join("road.pk3").to_str()
        );
    }

    /// The same bytes are the same WAD, and it already has them, so nothing is moved
    /// and the award just gets connected to what the user already has.
    #[test]
    fn test_an_entry_whose_wad_already_has_the_file_only_links_the_award() {
        let f = Fixture::new();
        let entry = f.thread_entry();
        let kept = f.paths.keep_dir.join("elsewhere.pk3");
        fs::create_dir_all(&f.paths.keep_dir).unwrap();
        fs::write(&kept, b"road").unwrap();
        let existing = db::add_wad(
            &f.conn,
            &NewWad::new("Road by Another Name", SourceType::Local)
                .cached_path(kept.display().to_string()),
        )
        .unwrap();
        identity::record(
            &f.conn,
            existing,
            &kept,
            &identity::identify(&kept).unwrap(),
        )
        .unwrap();
        let file = f.download("road.pk3", b"road");

        let outcome = f
            .link(GetTarget::Cacoward(entry), &file, LinkMode::Move)
            .unwrap();

        assert_eq!(
            outcome,
            GetOutcome::AlreadyHad {
                title: "Road by Another Name".to_string()
            }
        );
        assert_eq!(f.linked_wad_of(entry), Some(existing));
        assert!(file.exists(), "the download is the user's to deal with");
        assert_eq!(f.wad(existing).cached_path.as_deref(), kept.to_str());
    }

    #[test]
    fn test_an_entry_whose_wad_is_in_the_trash_says_so() {
        let f = Fixture::new();
        let entry = f.thread_entry();
        let existing = db::add_wad(
            &f.conn,
            &NewWad::new("Lonesome Road", SourceType::Doomworld).source_id("151858"),
        )
        .unwrap();
        db::delete_wad(&f.conn, existing, false).unwrap();
        let file = f.download("road.pk3", b"road");

        let failure = f
            .link(GetTarget::Cacoward(entry), &file, LinkMode::Move)
            .unwrap_err();

        assert_eq!(
            failure,
            "Lonesome Road is in the Trash. Restore it from Storage, then get it again."
        );
        assert!(file.exists());
    }

    /// Between pressing Get and the file landing, Import on the card may have worked.
    #[test]
    fn test_an_entry_linked_since_the_get_began_goes_to_that_wad() {
        let f = Fixture::new();
        let entry = f.thread_entry();
        let wad = manual_wad(&f);
        db::link_wad(&f.conn, entry, wad, false).unwrap();
        let file = f.download("road.pk3", b"road");

        let outcome = link_landed_file(
            &f.conn,
            GetTarget::Cacoward(entry),
            &file,
            &f.paths,
            LinkMode::Move,
            |_| panic!("nothing should have needed importing"),
        )
        .unwrap();

        assert_eq!(outcome, GetOutcome::Linked { wad_id: wad });
    }

    #[test]
    fn test_a_failed_import_is_what_gets_reported() {
        let f = Fixture::new();
        let entry = f.thread_entry();
        let file = f.download("road.pk3", b"road");

        let failure = link_landed_file(
            &f.conn,
            GetTarget::Cacoward(entry),
            &file,
            &f.paths,
            LinkMode::Move,
            |_| ImportResult {
                wad_id: None,
                is_duplicate: false,
                duplicate_id: None,
                duplicate_title: None,
                error: Some("the database is locked".to_string()),
            },
        )
        .unwrap_err();

        assert_eq!(failure, "the database is locked");
        assert!(file.exists());
    }

    #[test]
    fn test_what_a_finished_link_says() {
        let said = |outcome: Result<&GetOutcome, &String>| {
            let notification = linked_notification("Lonesome Road", "road.pk3", outcome);
            (notification.severity, notification.text)
        };
        assert_eq!(
            said(Ok(&GetOutcome::Linked { wad_id: 1 })),
            (
                Severity::Info,
                "Linked road.pk3 to Lonesome Road.".to_string()
            )
        );
        assert_eq!(
            said(Ok(&GetOutcome::AlreadyHad {
                title: "Road".to_string()
            })),
            (
                Severity::Info,
                "Already in your library as Road, so road.pk3 was left where it is.".to_string()
            )
        );
        assert_eq!(
            said(Err(&"it has gone".to_string())),
            (
                Severity::Error,
                "Could not link road.pk3. it has gone".to_string()
            )
        );
    }

    #[test]
    fn test_what_running_out_of_time_says() {
        assert!(expired_notification(&[]).is_none());
        assert_eq!(
            expired_notification(&["Road".to_string()]).unwrap().text,
            "Stopped waiting for Road's download."
        );
        assert_eq!(
            expired_notification(&["Road".to_string(), "Sunder".to_string()])
                .unwrap()
                .text,
            "Stopped waiting for 2 downloads."
        );
    }
}
