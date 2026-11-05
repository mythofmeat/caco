//! The story #57 is about, told through the real thing: two machines with a library each
//! and a folder between them, playing through `player::play` with a script for a
//! sourceport, and everything they know about each other coming through the logs.
//!
//! A test binary of its own, and the only test in it, because `crate::player` reads global
//! config, and being a machine here means pointing that at its folders. So nothing here can
//! launch a real sourceport or touch a real library.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use caco_core::config::{self, MirrorConfig};
use caco_core::db::{self, NewWad, SourceType, Status};
use caco_core::identity;
use caco_core::player::{self, AutoCompleteResult, PlayOptions};
use caco_core::presence::{self, StorePaths};
use caco_core::sync::{self, SyncPaths};
use caco_sources::mirror::{sync_mirrors, upload_wad};
use caco_sources::wad_fetch::fetch_missing_file;

/// A little two-map WAD, each map with an exit and enough monsters to count as a level, so
/// that finishing it means exiting both.
fn two_map_wad() -> Vec<u8> {
    // Imps, by the ten bytes each THING takes.
    let things: Vec<u8> = (0..20)
        .flat_map(|_| [0i16, 0, 0, 3001, 7])
        .flat_map(|field| field.to_le_bytes())
        .collect();
    let lumps: [(&str, &[u8]); 4] = [
        ("MAP01", &[]),
        ("THINGS", &things),
        ("MAP02", &[]),
        ("THINGS", &things),
    ];
    let mut data = Vec::new();
    let mut directory = Vec::new();
    for (name, bytes) in lumps {
        let offset = 12 + data.len() as i32;
        data.extend_from_slice(bytes);
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(&(bytes.len() as i32).to_le_bytes());
        let mut padded = [0u8; 8];
        padded[..name.len()].copy_from_slice(name.as_bytes());
        directory.extend_from_slice(&padded);
    }
    let mut wad = b"PWAD".to_vec();
    wad.extend_from_slice(&(lumps.len() as i32).to_le_bytes());
    wad.extend_from_slice(&(12 + data.len() as i32).to_le_bytes());
    wad.extend_from_slice(&data);
    wad.extend_from_slice(&directory);
    wad
}

/// A sourceport that does what the story needs: writes `$PLAY_STATS` as its stats file, and
/// `$PLAY_SAVE` as a save, into the data dir it was told about.
fn write_fake_sourceport(path: &Path) {
    let script = r#"#!/bin/sh
while [ $# -gt 0 ]; do
    case "$1" in -data) data="$2"; shift ;; esac
    shift
done
[ -n "$data" ] || exit 3
mkdir -p "$data"
printf '%b' "$PLAY_STATS" > "$data/stats.txt"
[ -z "$PLAY_SAVE" ] || printf '%s' "$PLAY_SAVE" > "$data/slot1.dsg"
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Stats with one map exited: lump, episode, map, skill, times, exits, kills and the rest.
fn stats_for(exited: &str) -> String {
    let line = |lump: &str, map: u32| {
        if lump == exited {
            format!("{lump} 1 {map} 4 1000 -1 -1 1 0 0 0 0 0 0 0\\n")
        } else {
            format!("{lump} 1 {map} 0 -1 -1 -1 0 0 0 0 0 0 0 0\\n")
        }
    };
    format!("1\\n0\\n{}{}", line("MAP01", 1), line("MAP02", 2))
}

/// A machine: its own folders, and its own library.
struct Machine {
    home: PathBuf,
    world: PathBuf,
    conn: rusqlite::Connection,
}

impl Machine {
    fn new(world: &Path, name: &str) -> Self {
        let home = world.join(name);
        let machine = Self::become_it(world, &home, name);
        fs::create_dir_all(&home).unwrap();
        machine
    }

    fn become_it(world: &Path, home: &Path, name: &str) -> Self {
        // SAFETY: the only test in this binary, and it has no other threads.
        unsafe {
            std::env::set_var("CACO_HOME", home.join("data"));
            std::env::set_var("CACO_DB_PATH", home.join("data/library.db"));
            std::env::set_var("CACO_DATA_DIR", home.join("data/data"));
            std::env::set_var("CACO_CACHE_HOME", home.join("cache"));
            std::env::set_var("CACO_CACHE_DIR", home.join("cache/wads"));
            std::env::set_var("CACO_MACHINE_ID", home.join("machine-id"));
            std::env::set_var("CACO_MACHINE_CONFIG", home.join("machine.toml"));
            std::env::set_var("CACO_SYNC_DIR", world.join("sync"));
            std::env::set_var("CACO_CONFIG", world.join("config.toml"));
        }
        fs::create_dir_all(home.join("data")).unwrap();
        fs::write(home.join("machine-id"), format!("{name}-00000000\n")).unwrap();
        let conn = db::open_connection(&home.join("data/library.db")).unwrap();
        db::init_db(&conn).unwrap();
        Self {
            home: home.to_path_buf(),
            world: world.to_path_buf(),
            conn,
        }
    }

    /// Make this the machine everything global is talking about.
    fn be(&self) {
        let name = self
            .home
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let conn = Self::become_it(&self.world, &self.home, &name);
        drop(conn);
    }

    fn sync(&self) -> sync::SyncReport {
        self.be();
        let paths = SyncPaths::from_config(&config::get_db_path()).unwrap();
        let report = sync::run(&self.conn, &paths);
        assert_eq!(report.write_error, None);
        assert_eq!(report.read_error, None);
        report
    }

    fn wad(&self) -> db::WadRecord {
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM wads", [], |row| row.get(0))
            .unwrap();
        db::get_wad(&self.conn, id, true).unwrap().unwrap()
    }

    fn sessions(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap()
    }

    fn play(
        &self,
        exited: &str,
        save: &str,
        opts: PlayOptions,
    ) -> caco_core::Result<player::PlayResult> {
        self.be();
        // SAFETY: as in `become_it`.
        unsafe {
            std::env::set_var("PLAY_STATS", stats_for(exited));
            std::env::set_var("PLAY_SAVE", save);
        }
        player::play(&self.conn, self.wad().id, opts)
    }

    fn data_dir(&self) -> PathBuf {
        let wad = self.wad();
        config::get_wad_data_dir(&wad.data_key, &wad.title)
    }
}

fn exits(wad: &db::WadRecord) -> Vec<(String, i32)> {
    let snapshot = wad.stats_snapshot.as_deref().unwrap();
    let mut exits: Vec<(String, i32)> = caco_core::wad_stats::stats_from_json(snapshot)
        .unwrap()
        .maps
        .into_iter()
        .map(|map| (map.lump, map.total_exits))
        .collect();
    exits.sort();
    exits
}

#[test]
fn two_machines_play_one_wad_and_nothing_is_lost() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path();

    let sourceport = world.join("dsda-doom");
    write_fake_sourceport(&sourceport);
    // A sandbox's temp dir can be noexec, as in `mirror_play.rs`.
    if std::process::Command::new(&sourceport)
        .args(["-data", &world.join("probe").display().to_string()])
        .status()
        .is_err()
    {
        eprintln!("skipped: a script in the temp dir cannot be run here");
        return;
    }
    fs::write(
        world.join("config.toml"),
        format!("sourceport = \"{}\"\n", sourceport.display()),
    )
    .unwrap();

    let mirror_root = world.join("mirror");
    fs::create_dir_all(&mirror_root).unwrap();
    let mirror = MirrorConfig {
        name: "home".to_string(),
        path: Some(mirror_root.display().to_string()),
        write: true,
        ..Default::default()
    };
    let mirrors = std::slice::from_ref(&mirror);

    // ---- The desktop imports a WAD only it has, plays the first map, and saves.
    let desktop = Machine::new(world, "desktop");
    let keep = desktop.home.join("data/wads");
    fs::create_dir_all(&keep).unwrap();
    let file = keep.join("road.wad");
    fs::write(&file, two_map_wad()).unwrap();
    let id = db::add_wad(
        &desktop.conn,
        &NewWad::new("Lonesome Road", SourceType::Doomworld)
            .source_id("151858")
            .filename("road.wad")
            .cached_path(file.display().to_string()),
    )
    .unwrap();
    assert_eq!(desktop.wad().uid, "doomworld-151858");

    let played = desktop
        .play("MAP01", "desktop's first save", PlayOptions::default())
        .unwrap();
    assert_eq!(played.exit_code, Some(0));
    assert_eq!(
        played.auto_complete,
        AutoCompleteResult::Incomplete {
            exited: 1,
            required: 2
        }
    );
    assert_eq!(desktop.wad().status, Status::InProgress);
    upload_wad(&desktop.conn, id, &mirror, "2026-11-05T00:00:00Z").unwrap();
    desktop.sync();

    // ---- The laptop has never seen any of it, and gets it from the folder.
    let laptop = Machine::new(world, "laptop");
    let report = laptop.sync();
    assert!(report.changed());
    assert_eq!(report.merge.waiting, 0);

    let there = laptop.wad();
    assert_eq!(there.title, "Lonesome Road");
    assert_eq!(there.status, Status::InProgress);
    assert_eq!(
        there.cached_path, None,
        "where it is was only true over there"
    );
    assert_eq!(there.data_key, "doomworld-151858");
    assert_eq!(laptop.sessions(), 1);
    assert_eq!(exits(&there)[0], ("MAP01".to_string(), 1));
    let hash = there
        .identity
        .as_ref()
        .expect("what the file is did travel");
    assert_eq!(hash.file.sha256, identity::hash_file(&file).unwrap().sha256);

    // Files by hash: it was never here, and a mirror it has only just heard of has it.
    assert!(there.is_lost());
    sync_mirrors(&laptop.conn, mirrors).unwrap();
    let there = laptop.wad();
    assert!(!there.is_lost());
    assert!(there.retrievability().is_automatic());
    let paths = StorePaths::from_config();
    assert!(
        !presence::check_wad(&laptop.conn, &there, &paths)
            .unwrap()
            .is_here()
    );
    let fetched = fetch_missing_file(&laptop.conn, &there, mirrors, &paths, 0).unwrap();
    assert_eq!(fs::read(&fetched).unwrap(), two_map_wad());

    // ---- The desktop's save has not come across, and only the person playing knows if
    // that matters. Nothing has been launched or created by the time they are asked.
    let refused = laptop.play("MAP02", "laptop's save", PlayOptions::default());
    let Err(caco_core::Error::NewerSavesElsewhere { saves, .. }) = refused else {
        panic!("should have asked: {:?}", refused.map(|r| r.exit_code));
    };
    assert_eq!(saves.machine, "desktop-00000000");
    assert_eq!(saves.files, ["slot1.dsg"]);
    assert!(
        !laptop.data_dir().exists(),
        "asked before anything was made"
    );

    // ---- They go ahead, and finish the WAD on a map the desktop never played. Only by
    // counting the desktop's map does that make two out of two.
    let played = laptop
        .play(
            "MAP02",
            "laptop's save",
            PlayOptions {
                accept_newer_saves: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(played.auto_complete, AutoCompleteResult::Completed);
    let there = laptop.wad();
    assert_eq!(there.status, Status::Completed);
    assert_eq!(
        exits(&there),
        [("MAP01".to_string(), 1), ("MAP02".to_string(), 1)]
    );
    laptop.sync();

    // Which the desktop now hears about, sessions, stats and the completion and all.
    let report = desktop.sync();
    assert!(report.changed());
    let here = desktop.wad();
    assert_eq!(here.status, Status::Completed);
    assert_eq!(desktop.sessions(), 2);
    assert_eq!(db::get_wad_completions(&desktop.conn, id).unwrap().len(), 1);
    assert_eq!(
        exits(&here),
        [("MAP01".to_string(), 1), ("MAP02".to_string(), 1)]
    );
    assert_eq!(
        here.cached_path.as_deref(),
        Some(file.display().to_string().as_str()),
        "and it still has its own file where it was"
    );

    // ---- The desktop is told about the laptop's save in turn, and this time the sync tool
    // has done its job by the time they look, so there is nothing to ask.
    desktop.be();
    let refused = desktop.play("MAP01", "", PlayOptions::default());
    assert!(matches!(
        refused,
        Err(caco_core::Error::NewerSavesElsewhere { .. })
    ));
    laptop.be();
    let carried = fs::read(laptop.data_dir().join("slot1.dsg")).unwrap();
    desktop.be();
    fs::write(desktop.data_dir().join("slot1.dsg"), carried).unwrap();
    desktop.play("MAP01", "", PlayOptions::default()).unwrap();
    assert_eq!(desktop.sessions(), 3);
    desktop.sync();
    laptop.sync();

    // ---- A third machine, starting from nothing but the folder, with its own copy of the
    // keep dir. The file is found by its hash, and nothing has to be downloaded.
    let new = Machine::new(world, "new");
    let new_keep = new.home.join("data/wads");
    fs::create_dir_all(&new_keep).unwrap();
    fs::write(new_keep.join("road.wad"), two_map_wad()).unwrap();
    new.sync();
    let paths = StorePaths::from_config();
    assert_eq!(identity::adopt_files(&new.conn, &paths).unwrap(), 1);
    let rebuilt = new.wad();
    assert_eq!(
        rebuilt.cached_path.as_deref(),
        Some(new_keep.join("road.wad").display().to_string().as_str())
    );
    assert_eq!(rebuilt.status, Status::Completed);
    assert_eq!(new.sessions(), 3);
    assert_eq!(
        exits(&rebuilt),
        [("MAP01".to_string(), 1), ("MAP02".to_string(), 1)]
    );

    // And all three agree on what there is to agree on.
    for machine in [&desktop, &laptop] {
        let other = machine.wad();
        assert_eq!(other.status, rebuilt.status);
        assert_eq!(other.stats_snapshot, rebuilt.stats_snapshot);
        assert_eq!(machine.sessions(), new.sessions());
    }

    // Each only ever wrote to its own folder.
    let folders: Vec<String> = {
        let mut names: Vec<String> = fs::read_dir(world.join("sync"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(folders, ["desktop-00000000", "laptop-00000000"]);
}
