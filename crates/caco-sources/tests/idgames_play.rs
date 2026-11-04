//! The other half of #56, told the way the app tells it: a copy of idgames on a disk, no
//! network to speak of, and a WAD found, imported and played anyway.
//!
//! A test binary of its own, and the only test in it, because the index, the importer and
//! the player all read global config, and this points that somewhere disposable. The API
//! is at a port nothing listens on. The sourceport is a script that writes down its
//! arguments, so nothing here can launch a real one or touch a real library.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use caco_core::config::{self, Config, MirrorConfig};
use caco_core::db::{self, WadUpdate};
use caco_core::identity;
use caco_core::player::{self, PlayOptions};
use caco_core::presence::{self, StorePaths};
use caco_sources::idgames::Index;
use caco_sources::idgames::sync::{ListingOutcome, SyncOptions, sync_index};
use caco_sources::import_service::{ImportService, backfill_idgames_ids};
use caco_sources::wad_fetch::fetch_missing_file;

const WAD_BYTES: &[u8] = b"PWAD\0\0\0\0\x0c\0\0\0";
const SUNLUST: &str = "levels/doom2/Ports/megawads/sunlust";

/// An idgames-shaped zip: one WAD in it, as the archive has them.
fn zip_with_a_wad() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut bytes);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("sunlust.wad", options).unwrap();
    zip.write_all(WAD_BYTES).unwrap();
    zip.finish().unwrap();
    bytes.into_inner()
}

fn gzip(text: &str) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn a_wad_on_idgames_is_found_imported_and_played_with_nothing_but_a_copy_of_the_archive() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the only test in this binary, before anything has read these.
    unsafe {
        std::env::set_var("CACO_HOME", home.path().join("data"));
        std::env::set_var("CACO_CONFIG", home.path().join("data/config.toml"));
        std::env::set_var("CACO_CACHE_HOME", home.path().join("cache"));
        std::env::set_var("CACO_CACHE_DIR", home.path().join("cache/wads"));
        std::env::set_var("CACO_DATA_DIR", home.path().join("data/data"));
        std::env::set_var("CACO_IDGAMES_API", "http://127.0.0.1:9/api.php");
    }

    // What `rsync` left on the disk.
    let root = home.path().join("idgames");
    let zip_bytes = zip_with_a_wad();
    let text = "Title : Sunlust\r\nAuthor : Ribbiks & Dannebubinga\r\n";
    fs::create_dir_all(root.join(SUNLUST).parent().unwrap()).unwrap();
    fs::write(root.join(format!("{SUNLUST}.zip")), &zip_bytes).unwrap();
    fs::write(root.join(format!("{SUNLUST}.txt")), text).unwrap();
    fs::write(
        root.join("ls-laR.gz"),
        gzip(&format!(
            "./levels/doom2/Ports/megawads:\ntotal 1\n\
             -rw-rw-r-- 1 a b {} Aug  9  2015 sunlust.txt\n\
             -rw-rw-r-- 1 a b {} Aug  9  2015 sunlust.zip\n",
            text.len(),
            zip_bytes.len(),
        )),
    )
    .unwrap();
    let copy = MirrorConfig {
        name: "disk".to_string(),
        path: Some(root.display().to_string()),
        layout: Some("idgames".to_string()),
        ..Default::default()
    };
    let saved = Config {
        mirrors: vec![copy],
        // The wiki is no part of this, and a test has no business asking it things.
        auto_doomwiki_enrich: false,
        ..Default::default()
    };
    config::save_config(&saved).unwrap();
    config::reload_config();
    let settings = config::load_config();

    // The app's first run, with the API refusing. Everything comes off the disk.
    let index = Index::open_default().unwrap();
    let report = sync_index(
        &index,
        &SyncOptions::for_config(&settings, false, chrono::Utc::now()),
        &mut |_| {},
        &|| false,
    )
    .unwrap();
    assert!(
        matches!(report.listing, ListingOutcome::Updated(_)),
        "{report:?}"
    );
    assert!(report.api_failure.is_some(), "{report:?}");
    assert_eq!(report.text_read, 1);
    assert_eq!(report.text_failure, None);
    assert_eq!(index.status().unwrap().listing.unwrap().source, "disk");

    // Searching for it, by something only the text file says.
    let found = index.search("ribbiks", 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "Sunlust");
    assert_eq!(found[0].id, 0);

    // Importing it, with no id and nothing to ask for one.
    let conn = db::open_connection(&home.path().join("data/library.db")).unwrap();
    db::init_db(&conn).unwrap();
    let imported = ImportService::new().import_idgames(&conn, &found[0], None, false);
    let id = imported.wad_id.unwrap_or_else(|| panic!("{imported:?}"));

    let args_file = home.path().join("args");
    let sourceport = home.path().join("fake-sourceport");
    fs::write(
        &sourceport,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            args_file.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&sourceport, fs::Permissions::from_mode(0o755)).unwrap();
    // This also runs inside makepkg's and rpmbuild's check stages, and a sandbox can
    // mount its temp dir noexec. That says nothing about idgames.
    if std::process::Command::new(&sourceport).status().is_err() {
        eprintln!("skipped: a script in the temp dir cannot be run here");
        return;
    }
    let update =
        WadUpdate::new().set_text("custom_sourceport", Some(sourceport.display().to_string()));
    db::update_wad(&conn, id, &update).unwrap();

    let row = |id| db::get_wad(&conn, id, false).unwrap().unwrap();
    let wad = row(id);
    assert!(!wad.is_lost());
    assert!(wad.retrievability().is_automatic());

    // What the Play worker does: not here, can be fetched, fetch it, play it.
    let paths = StorePaths::from_config();
    assert!(!presence::check_wad(&conn, &wad, &paths).unwrap().is_here());
    let fetched = fetch_missing_file(&conn, &wad, &settings.mirrors, &paths, 0).unwrap();
    assert_eq!(fetched, paths.cache_dir.join("sunlust.zip"));

    let played = player::play(&conn, id, PlayOptions::default()).unwrap();
    assert_eq!(played.exit_code, Some(0));
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.lines()
            .any(|arg| arg.ends_with("sunlust.wad") || arg.ends_with("sunlust.zip")),
        "{args}"
    );

    // The launch hashed it, and went by the WAD inside the zip.
    let wad = row(id);
    let identity = wad.identity.as_ref().expect("the launch hashes the file");
    assert_eq!(
        identity.file.sha256,
        identity::hash_file(&fetched).unwrap().sha256
    );

    // The cache gets cleaned, as it may. The copy on the disk is not the cache, so that
    // costs nothing, and the copy is still there.
    fs::remove_file(&fetched).unwrap();
    assert!(root.join(format!("{SUNLUST}.zip")).exists());
    let paths = StorePaths::from_config();
    assert!(!presence::check_wad(&conn, &wad, &paths).unwrap().is_here());
    let wad = row(id);
    let again = fetch_missing_file(&conn, &wad, &settings.mirrors, &paths, 0).unwrap();
    assert_eq!(fs::read(&again).unwrap(), zip_bytes);
    player::play(&conn, id, PlayOptions::default()).unwrap();

    // The API comes back one day, and the index learns the id. The row catches up, which
    // is what makes it the same WAD as anything else that goes by id.
    index
        .record_entry(
            &serde_json::from_value(serde_json::json!({
                "id": 18168,
                "dir": "levels/doom2/Ports/megawads/",
                "filename": "sunlust.zip",
                "size": zip_bytes.len(),
            }))
            .unwrap(),
        )
        .unwrap();
    assert_eq!(backfill_idgames_ids(&conn, &index).unwrap(), 1);
    assert_eq!(row(id).idgames_id.as_deref(), Some("18168"));
}
