//! The mirror client against a real store: what the fake one in the unit tests was
//! modelled on, and what it could be wrong about. `#[ignore]`d, because
//! `cargo test --workspace` must not need a server. Point it at a throwaway one:
//!
//! ```bash
//! # A Gitea or Forgejo generic package registry, the store with the most opinions:
//! docker run -d --rm --name caco-mirror-test -p 127.0.0.1:38317:3000 \
//!     -e GITEA__security__INSTALL_LOCK=true docker.gitea.com/gitea:latest
//! docker exec -u git caco-mirror-test gitea admin user create --admin \
//!     --username tester --password 'Test-Pass-1!' --email t@example.invalid
//! export CACO_TEST_MIRROR_TOKEN=$(docker exec -u git caco-mirror-test gitea admin user \
//!     generate-access-token --username tester --scopes write:package,read:package --raw)
//! CACO_TEST_MIRROR_URL=http://127.0.0.1:38317/api/packages/tester/generic/doom-mirror \
//!     cargo test -p caco-sources --test mirror_live -- --ignored --nocapture
//!
//! # A WebDAV server, whose 409 means something else entirely:
//! rclone serve webdav --addr 127.0.0.1:38318 /tmp/some-empty-dir &
//! CACO_TEST_MIRROR_URL=http://127.0.0.1:38318 \
//!     cargo test -p caco-sources --test mirror_live -- --ignored --nocapture
//! ```
//!
//! Each run uses new bytes, so it can be repeated against the same store.

use std::fs;
use std::path::Path;

use caco_core::config::MirrorConfig;
use caco_core::db::{self, NewWad, SourceType, WadRecord, WadUpdate};
use caco_core::identity;
use caco_core::presence::StorePaths;
use caco_sources::mirror::{fetch_from_mirrors, sync_mirrors, upload_wad};
use rusqlite::Connection;

const NOW: &str = "2026-11-03T00:00:00Z";

struct Machine {
    conn: Connection,
    paths: StorePaths,
    dir: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = StorePaths {
            cache_dir: dir.path().join("cache"),
            keep_dir: dir.path().join("keep"),
        };
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        Self { conn, paths, dir }
    }

    fn wad_with_file(&self, filename: &str, bytes: &[u8], topic: u32) -> WadRecord {
        let path = self.dir.path().join("collection").join(filename);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        let new = NewWad::new(filename, SourceType::Doomworld)
            .source_url(format!("https://www.doomworld.com/vb/thread/{topic}"));
        let id = db::add_wad(&self.conn, &new).unwrap();
        let update = WadUpdate::new().set_text("cached_path", Some(path.display().to_string()));
        db::update_wad(&self.conn, id, &update).unwrap();
        db::get_wad(&self.conn, id, false).unwrap().unwrap()
    }

    fn wad_at_topic(&self, topic: u32) -> WadRecord {
        let new = NewWad::new("Heard of it", SourceType::Doomworld)
            .source_url(format!("https://www.doomworld.com/vb/thread/{topic}"));
        let id = db::add_wad(&self.conn, &new).unwrap();
        db::get_wad(&self.conn, id, false).unwrap().unwrap()
    }
}

/// Something no earlier run has uploaded, as long as clocks go forwards.
fn unique() -> u32 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    (since_epoch.as_nanos() % 1_000_000_000) as u32
}

#[test]
#[ignore = "needs a real store: see the module docs"]
fn a_real_store_makes_the_whole_trip() {
    let url = std::env::var("CACO_TEST_MIRROR_URL").expect("set CACO_TEST_MIRROR_URL");
    let mirror = MirrorConfig {
        name: "live".to_string(),
        url: Some(url),
        token_env: Some("CACO_TEST_MIRROR_TOKEN".to_string()),
        write: true,
        ..Default::default()
    };
    let mirrors = std::slice::from_ref(&mirror);
    let run = unique();

    // One upload to create the index, and another to replace it, which is where the
    // stores differ. The names are what Gitea was seen to treat differently: this one
    // it takes as it is, and this one it refuses.
    let here = Machine::new();
    let first_bytes = format!("PWAD first {run}").into_bytes();
    let second_bytes = format!("PWAD second {run}").into_bytes();
    let plain_name = format!("DATW Ver {run}.wad");
    let awkward_name = format!("Santa's Bender, Piña {run}.wad");

    let first = here.wad_with_file(&plain_name, &first_bytes, run);
    let uploaded = upload_wad(&here.conn, first.id, &mirror, NOW).unwrap();
    assert!(!uploaded.already_there);

    let second = here.wad_with_file(&awkward_name, &second_bytes, run + 1);
    upload_wad(&here.conn, second.id, &mirror, NOW).unwrap();

    // Uploading the same bytes again adds nothing new, and is no failure.
    let again = upload_wad(&here.conn, first.id, &mirror, NOW).unwrap();
    assert!(again.already_there, "the store should still have the file");

    for wad in [&first, &second] {
        let wad = db::get_wad(&here.conn, wad.id, false).unwrap().unwrap();
        assert!(wad.mirrored, "{}", wad.title);
    }

    // Another machine has only heard of these WADs, from their forum threads.
    let there = Machine::new();
    let plain = there.wad_at_topic(run);
    let awkward = there.wad_at_topic(run + 1);
    assert!(plain.is_lost());

    let report = sync_mirrors(&there.conn, mirrors).unwrap();
    assert_eq!(report.failed, [], "{report:?}");
    assert!(report.changed);

    let fetched = |wad: &WadRecord| {
        let wad = db::get_wad(&there.conn, wad.id, false).unwrap().unwrap();
        assert!(!wad.is_lost(), "{}", wad.title);
        fetch_from_mirrors(&there.conn, &wad, mirrors, &there.paths)
            .unwrap()
            .expect("a mirror should have it")
    };
    let name_of = |path: &Path| path.file_name().unwrap().to_str().unwrap().to_string();

    let plain_file = fetched(&plain);
    assert_eq!(name_of(&plain_file), plain_name);
    assert_eq!(fs::read(&plain_file).unwrap(), first_bytes);

    // Under its real name, whatever the store called it.
    let awkward_file = fetched(&awkward);
    assert_eq!(name_of(&awkward_file), awkward_name);
    assert_eq!(fs::read(&awkward_file).unwrap(), second_bytes);

    // And it is the same content the first machine recorded.
    let hash = identity::hash_file(&plain_file).unwrap().sha256;
    let recorded = db::get_wad(&here.conn, first.id, false)
        .unwrap()
        .unwrap()
        .identity
        .unwrap();
    assert_eq!(hash, recorded.file.sha256);
}
