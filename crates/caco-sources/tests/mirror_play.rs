//! The story #54 is about, told the way the Play worker tells it: a WAD nothing could
//! fetch, a file on the user's own mirror, and a sourceport launched with that file.
//!
//! A test binary of its own, and the only test in it, because `crate::player` reads
//! global config and this points that somewhere disposable. The sourceport is a script
//! that writes down its arguments, set on the WAD's row, so nothing here can launch a
//! real one or touch a real library.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use caco_core::config::MirrorConfig;
use caco_core::db::{self, NewWad, SourceType, WadUpdate};
use caco_core::identity;
use caco_core::player::{self, PlayOptions};
use caco_core::presence::{self, StorePaths};
use caco_sources::mirror::{sync_mirrors, upload_wad};
use caco_sources::wad_fetch::fetch_missing_file;

/// A well-formed PWAD with an empty directory.
const WAD_BYTES: &[u8] = b"PWAD\0\0\0\0\x0c\0\0\0";
const TOPIC_URL: &str = "https://www.doomworld.com/forum/topic/151858-lonesome-road/";

fn write_fake_sourceport(path: &Path, args_file: &Path) {
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
        args_file.display()
    );
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_manual_wad_with_no_file_plays_from_a_mirror() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the only test in this binary, before anything has read these.
    unsafe {
        std::env::set_var("CACO_HOME", home.path().join("data"));
        std::env::set_var("CACO_CONFIG", home.path().join("data/config.toml"));
        std::env::set_var("CACO_CACHE_HOME", home.path().join("cache"));
        std::env::set_var("CACO_CACHE_DIR", home.path().join("cache/wads"));
        std::env::set_var("CACO_DATA_DIR", home.path().join("data/data"));
    }

    let mirror_root = home.path().join("mirror");
    fs::create_dir_all(&mirror_root).unwrap();
    let mirror = MirrorConfig {
        name: "home".to_string(),
        path: Some(mirror_root.display().to_string()),
        write: true,
        ..Default::default()
    };
    let mirrors = std::slice::from_ref(&mirror);

    // Another machine got the file by hand, and uploaded it when caco offered.
    let there = db::open_memory().unwrap();
    db::init_db(&there).unwrap();
    let collection = home.path().join("collection");
    fs::create_dir_all(&collection).unwrap();
    let original = collection.join("road.wad");
    fs::write(&original, WAD_BYTES).unwrap();
    let theirs = db::add_wad(
        &there,
        &NewWad::new("Lonesome Road", SourceType::Doomworld)
            .source_url(TOPIC_URL)
            .cached_path(original.display().to_string()),
    )
    .unwrap();
    upload_wad(&there, theirs, &mirror, "2026-11-03T00:00:00Z").unwrap();

    // This machine has only heard of it, from the forum thread.
    let conn = db::open_connection(&home.path().join("data/library.db")).unwrap();
    db::init_db(&conn).unwrap();
    let args_file = home.path().join("args");
    let sourceport = home.path().join("fake-sourceport");
    write_fake_sourceport(&sourceport, &args_file);
    // This also runs inside makepkg's and rpmbuild's check stages, and a sandbox can
    // mount its temp dir noexec. That says nothing about mirrors, and should not stop
    // a release.
    if std::process::Command::new(&sourceport).status().is_err() {
        eprintln!("skipped: a script in the temp dir cannot be run here");
        return;
    }
    let id = db::add_wad(
        &conn,
        &NewWad::new("Lonesome Road", SourceType::Doomworld).source_url(TOPIC_URL),
    )
    .unwrap();
    let update =
        WadUpdate::new().set_text("custom_sourceport", Some(sourceport.display().to_string()));
    db::update_wad(&conn, id, &update).unwrap();

    let row = |id| db::get_wad(&conn, id, false).unwrap().unwrap();
    assert!(row(id).is_lost(), "nothing can fetch it before the sync");

    sync_mirrors(&conn, mirrors).unwrap();
    let wad = row(id);
    assert!(!wad.is_lost());
    assert!(wad.retrievability().is_automatic());

    // What the Play worker does: not here, can be fetched, fetch it, play it.
    let paths = StorePaths::from_config();
    assert!(!presence::check_wad(&conn, &wad, &paths).unwrap().is_here());
    let fetched = fetch_missing_file(&conn, &wad, mirrors, &paths, 0).unwrap();
    assert_eq!(fetched, paths.cache_dir.join("road.wad"));

    let played = player::play(&conn, id, PlayOptions::default()).unwrap();
    assert_eq!(played.exit_code, Some(0));

    // The sourceport got the file from the mirror.
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.lines().any(|arg| arg == fetched.to_str().unwrap()),
        "{args}"
    );

    // The launch hashed it, which from now on is how the row knows its file.
    let wad = row(id);
    let identity = wad.identity.as_ref().expect("the launch hashes the file");
    assert_eq!(
        identity.file.sha256,
        identity::hash_file(&fetched).unwrap().sha256
    );
    assert!(wad.mirrored, "by its hash, now that it has one");

    // The cache gets cleaned, as it may: this reads as fetchable, so it comes back.
    fs::remove_file(&fetched).unwrap();
    let paths = StorePaths::from_config();
    assert!(!presence::check_wad(&conn, &wad, &paths).unwrap().is_here());
    let wad = row(id);
    assert!(!wad.is_lost());
    let again = fetch_missing_file(&conn, &wad, mirrors, &paths, 0).unwrap();
    assert_eq!(fs::read(&again).unwrap(), WAD_BYTES);
    assert_eq!(row(id).last_known_path, None);

    // And it plays again without asking about a changed file, because it is not one.
    player::play(&conn, id, PlayOptions::default()).unwrap();
}
