//! Getting the file of a WAD that is not on this machine, from anywhere caco can.

use std::path::{Path, PathBuf};

use caco_core::config::MirrorConfig;
use caco_core::db::{self, WadRecord, WadUpdate};
use caco_core::presence::StorePaths;
use rusqlite::Connection;

use crate::error::{Result, SourceError};
use crate::idgames::IdgamesClient;
use crate::mirror;

/// Fetch a WAD's file and record where it landed.
///
/// Mirrors come first, because a mirror is the user's own copy, and it is the only
/// place a manual-only WAD can come from. A failure there falls through to idgames
/// for a WAD that is on it, since the two do not go down together. If both fail, the
/// mirror's failure is the one reported.
///
/// `download_urls` are not tried: they are share pages behind a browser. Those are for
/// the user to open, which is what Get does.
///
/// Only the path and the name are written: the launch that follows hashes the new
/// file before it plays it, and asks if it is not the file the WAD was last played
/// with.
pub fn fetch_missing_file(
    conn: &Connection,
    wad: &WadRecord,
    mirrors: &[MirrorConfig],
    paths: &StorePaths,
    idgames_mirror: usize,
) -> Result<PathBuf> {
    fetch_missing_file_with(conn, wad, mirrors, paths, |cache_dir| {
        download_from_idgames(wad, cache_dir, idgames_mirror)
    })
}

/// [`fetch_missing_file`], with the idgames download passed in so that tests can run
/// with no network.
fn fetch_missing_file_with(
    conn: &Connection,
    wad: &WadRecord,
    mirrors: &[MirrorConfig],
    paths: &StorePaths,
    idgames: impl FnOnce(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    let dest = match mirror::fetch_from_mirrors(conn, wad, mirrors, paths) {
        Ok(Some(dest)) => dest,
        Ok(None) if wad.on_idgames() => idgames(&paths.cache_dir)?,
        Err(failure) if wad.on_idgames() => idgames(&paths.cache_dir).map_err(|_| failure)?,
        Err(failure) => return Err(failure),
        // A row left by a mirror that has since been taken out of the config, which
        // the next sync drops. Nothing else makes a WAD read as fetchable.
        Ok(None) => {
            return Err(SourceError::NoMirrorHas {
                title: wad.title.clone(),
            });
        }
    };

    let mut update =
        WadUpdate::new().set_text("cached_path", Some(dest.to_string_lossy().into_owned()));
    if wad.filename.as_deref().unwrap_or("").is_empty()
        && let Some(name) = dest.file_name().and_then(|name| name.to_str())
    {
        update = update.set_text("filename", Some(name.to_string()));
    }
    db::update_wad(conn, wad.id, &update)?;
    Ok(dest)
}

/// Download a WAD's file from idgames into `cache_dir`.
fn download_from_idgames(wad: &WadRecord, cache_dir: &Path, mirror: usize) -> Result<PathBuf> {
    std::fs::create_dir_all(cache_dir)?;
    let client = IdgamesClient::new();

    // Ask the archive where the file is now, but a failed lookup is not a missing
    // file: source_url + filename still name it on idgames' own mirrors, and those do
    // not sit behind the API.
    let entry = wad
        .idgames_id
        .as_deref()
        .and_then(|id| id.parse::<i64>().ok())
        .and_then(|id| client.get(Some(id), None).ok());

    match entry {
        Some(entry) => client.download(&entry, Some(cache_dir), mirror, None),
        None => client.download_direct(
            wad.source_url.as_deref().unwrap_or(""),
            wad.filename.as_deref().unwrap_or(""),
            cache_dir,
            mirror,
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;

    use caco_core::db::{NewWad, SourceType};
    use caco_core::identity;
    use caco_core::mirrors::{MirrorEntry, MirrorIndex};

    use super::*;

    const WAD_BYTES: &[u8] = b"PWAD on a mirror";

    struct Fixture {
        conn: Connection,
        paths: StorePaths,
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let paths = StorePaths {
                cache_dir: root.path().join("cache"),
                keep_dir: root.path().join("keep"),
            };
            let conn = db::open_memory().unwrap();
            db::init_db(&conn).unwrap();
            Self { conn, paths, root }
        }

        /// A local mirror holding [`WAD_BYTES`], synced into the library. Returns its
        /// config and the file's hash.
        fn mirror_with_the_file(&self, topic: i64) -> (MirrorConfig, String) {
            let dir = self.root.path().join("mirror");
            fs::create_dir_all(&dir).unwrap();
            let sha256 = identity::hash_file(&write_temp(&self.root, WAD_BYTES))
                .unwrap()
                .sha256;
            let stored = dir.join(&sha256);
            fs::create_dir_all(&stored).unwrap();
            fs::write(stored.join("road.zip"), WAD_BYTES).unwrap();

            let mut index = MirrorIndex::default();
            index.upsert(MirrorEntry {
                sha256: sha256.clone(),
                size: WAD_BYTES.len() as u64,
                filename: "road.zip".to_string(),
                doomworld: Some(topic),
                ..Default::default()
            });
            fs::create_dir_all(dir.join("index")).unwrap();
            fs::write(dir.join("index/index.json"), index.to_json()).unwrap();

            let config = MirrorConfig {
                name: "home".to_string(),
                path: Some(dir.to_string_lossy().into_owned()),
                ..Default::default()
            };
            mirror::sync_mirrors(&self.conn, std::slice::from_ref(&config)).unwrap();
            (config, sha256)
        }

        fn wad(&self, new: NewWad) -> WadRecord {
            let id = db::add_wad(&self.conn, &new).unwrap();
            self.row(id)
        }

        fn row(&self, id: i64) -> WadRecord {
            db::get_wad(&self.conn, id, true).unwrap().unwrap()
        }
    }

    fn write_temp(root: &tempfile::TempDir, bytes: &[u8]) -> PathBuf {
        let path = root.path().join("scratch-file");
        fs::write(&path, bytes).unwrap();
        path
    }

    fn manual(topic: i64) -> NewWad {
        NewWad::new("Road", SourceType::Doomworld)
            .source_url(format!("https://www.doomworld.com/vb/thread/{topic}"))
    }

    fn untouched_idgames(_: &Path) -> Result<PathBuf> {
        panic!("idgames should not have been asked");
    }

    /// The issue in one test: a manual-only WAD with no file, fetched from the
    /// user's own store and placed where its retrievability says.
    #[test]
    fn test_a_manual_wad_comes_back_from_a_mirror() {
        let f = Fixture::new();
        let (config, _) = f.mirror_with_the_file(151858);
        let wad = f.wad(manual(151858));
        assert!(wad.mirrored);

        let dest = fetch_missing_file_with(
            &f.conn,
            &wad,
            std::slice::from_ref(&config),
            &f.paths,
            untouched_idgames,
        )
        .unwrap();

        // On a mirror, so it reads as automatic, so it goes where those go.
        assert_eq!(dest, f.paths.cache_dir.join("road.zip"));
        assert_eq!(fs::read(&dest).unwrap(), WAD_BYTES);
        let row = f.row(wad.id);
        assert_eq!(row.cached_path.as_deref(), dest.to_str());
        assert_eq!(row.filename.as_deref(), Some("road.zip"));
    }

    #[test]
    fn test_a_fetch_clears_where_the_file_was_last_seen_and_keeps_a_name_it_has() {
        let f = Fixture::new();
        let (config, _) = f.mirror_with_the_file(1);
        let wad = f.wad(manual(1).filename("own-name.zip"));
        f.conn
            .execute(
                "UPDATE wads SET last_known_path = '/gone/own-name.zip' WHERE id = ?1",
                [wad.id],
            )
            .unwrap();

        fetch_missing_file_with(
            &f.conn,
            &f.row(wad.id),
            &[config],
            &f.paths,
            untouched_idgames,
        )
        .unwrap();

        let row = f.row(wad.id);
        assert_eq!(row.last_known_path, None);
        assert_eq!(row.filename.as_deref(), Some("own-name.zip"));
    }

    /// Mirrors are tried first even for a WAD idgames could provide.
    #[test]
    fn test_a_mirror_comes_before_idgames() {
        let f = Fixture::new();
        let (config, _) = f.mirror_with_the_file(1);
        let wad = f.wad(manual(1).source_id("1"));
        // A mirror entry naming no idgames id does not contradict this one.
        f.conn
            .execute("UPDATE wads SET idgames_id = '42' WHERE id = ?1", [wad.id])
            .unwrap();
        let wad = f.row(wad.id);
        assert!(wad.on_idgames());

        fetch_missing_file_with(&f.conn, &wad, &[config], &f.paths, untouched_idgames).unwrap();
    }

    #[test]
    fn test_idgames_is_used_when_no_mirror_has_it() {
        let f = Fixture::new();
        let wad = f.wad(NewWad::new("Scythe", SourceType::Idgames).idgames_id("42"));
        let expected = f.paths.cache_dir.join("scythe.zip");
        let asked = Cell::new(false);

        let dest = fetch_missing_file_with(&f.conn, &wad, &[], &f.paths, |cache_dir| {
            assert_eq!(cache_dir, f.paths.cache_dir);
            asked.set(true);
            Ok(cache_dir.join("scythe.zip"))
        })
        .unwrap();

        assert!(asked.get());
        assert_eq!(dest, expected);
        assert_eq!(f.row(wad.id).cached_path.as_deref(), expected.to_str());
    }

    #[test]
    fn test_idgames_is_the_fallback_for_a_mirror_that_fails() {
        let f = Fixture::new();
        let (config, sha256) = f.mirror_with_the_file(1);
        let wad = f.wad(manual(1));
        f.conn
            .execute("UPDATE wads SET idgames_id = '42' WHERE id = ?1", [wad.id])
            .unwrap();
        let wad = f.row(wad.id);
        // The mirror's copy has gone.
        fs::remove_dir_all(f.root.path().join("mirror").join(&sha256)).unwrap();

        let dest = fetch_missing_file_with(&f.conn, &wad, &[config], &f.paths, |dir| {
            Ok(dir.join("from-idgames.zip"))
        })
        .unwrap();

        assert_eq!(dest, f.paths.cache_dir.join("from-idgames.zip"));
    }

    /// There is nothing behind it to try, so the user needs to know which mirror let
    /// them down, and not get a generic message.
    #[test]
    fn test_a_failed_mirror_is_named_when_there_is_nothing_else() {
        let f = Fixture::new();
        let (config, sha256) = f.mirror_with_the_file(1);
        let wad = f.wad(manual(1));
        fs::remove_dir_all(f.root.path().join("mirror").join(&sha256)).unwrap();

        let failure =
            fetch_missing_file_with(&f.conn, &wad, &[config], &f.paths, untouched_idgames)
                .unwrap_err()
                .to_string();

        assert!(failure.starts_with("Mirror home:"), "{failure}");
        assert_eq!(f.row(wad.id).cached_path, None);
    }

    #[test]
    fn test_when_both_fail_the_mirror_is_what_gets_reported() {
        let f = Fixture::new();
        let (config, sha256) = f.mirror_with_the_file(1);
        let wad = f.wad(manual(1));
        f.conn
            .execute("UPDATE wads SET idgames_id = '42' WHERE id = ?1", [wad.id])
            .unwrap();
        let wad = f.row(wad.id);
        fs::remove_dir_all(f.root.path().join("mirror").join(&sha256)).unwrap();

        let failure = fetch_missing_file_with(&f.conn, &wad, &[config], &f.paths, |_| {
            Err(SourceError::Api("idgames is down".to_string()))
        })
        .unwrap_err()
        .to_string();

        assert!(failure.starts_with("Mirror home:"), "{failure}");
    }

    /// A row left behind by a mirror the config no longer has: nothing to fetch from,
    /// and the next sync will drop it.
    #[test]
    fn test_a_mirror_no_longer_configured_is_not_tried() {
        let f = Fixture::new();
        let (_, _) = f.mirror_with_the_file(1);
        let wad = f.wad(manual(1));
        assert!(wad.mirrored);

        let failure = fetch_missing_file_with(&f.conn, &wad, &[], &f.paths, untouched_idgames)
            .unwrap_err()
            .to_string();

        assert_eq!(failure, "No configured mirror has Road");
    }
}
