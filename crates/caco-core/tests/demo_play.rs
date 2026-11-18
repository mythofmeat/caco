//! Recording a demo and playing it back, through the real `player::play` and
//! `player::play_demo`, with a script for a sourceport that says what it was given and
//! writes a demo where `-record` tells it to, adding `.lmp` as nyan-doom does.
//!
//! Playback used to launch under whatever the WAD was set to that day, so a demo recorded
//! at one complevel played at another and desynced with no word of why. So this records at
//! complevel 9 with a companion loaded, changes the WAD to complevel 11 with a `-record` in
//! its own arguments, and checks that the demo still plays as it was recorded and that the
//! stray `-record` is not handed over. Then it takes away the companion and changes the WAD's
//! file, and checks that playback stops to say so, and plays anyway once told to.
//!
//! A test binary of its own, and the only test in it, because `crate::player` reads global
//! config, and this points that at a temp dir.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use caco_core::config;
use caco_core::db::{self, NewWad, SourceType, WadUpdate};
use caco_core::demos::{self, DemoDifference, FileRole};
use caco_core::player::{self, DemoOptions, PlayOptions};

/// A dsda-family sourceport: writes its arguments to `$PLAY_ARGS`, one a line, and a Doom
/// 1.9 demo of one second to whatever follows `-record`, with `.lmp` added.
fn write_sourceport(path: &Path) {
    let script = r#"#!/bin/sh
printf '%s\n' "$@" > "$PLAY_ARGS"
prev=""
for arg in "$@"; do
    if [ "$prev" = "-record" ]; then
        printf '\155\003\001\007\000\000\000\000\000\001\000\000\000' > "$arg.lmp"
        head -c 140 /dev/zero >> "$arg.lmp"
        printf '\200' >> "$arg.lmp"
    fi
    prev="$arg"
done
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn set(var: &str, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: the only test in this binary, and it has no other threads.
    unsafe { std::env::set_var(var, value) };
}

fn ran_with(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// What follows `flag`.
fn value_of(args: &[String], flag: &str) -> Option<String> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1).cloned()
}

#[test]
fn a_demo_plays_back_as_it_was_recorded() {
    let root = tempfile::tempdir().unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    for (var, value) in [
        ("CACO_HOME", "data"),
        ("CACO_CONFIG", "data/config.toml"),
        ("CACO_DB_PATH", "data/library.db"),
        ("CACO_DATA_DIR", "data/data"),
        ("CACO_CACHE_HOME", "cache"),
        ("CACO_CACHE_DIR", "cache/wads"),
        ("CACO_SYNC_DIR", "sync"),
        ("CACO_MACHINE_ID", "machine-id"),
        ("CACO_MACHINE_CONFIG", "machine.toml"),
        ("CACO_TOKEN_FILE", "tokens.toml"),
    ] {
        set(var, value);
    }

    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let nyan = bin.join("nyan-doom");
    write_sourceport(&nyan);
    // Found on PATH when a record names it, as a sourceport caco did not build would be.
    let path = std::env::var("PATH").unwrap_or_default();
    set("PATH", format!("{}:{path}", bin.display()));
    let args_file = root.path().join("ran-with");
    set("PLAY_ARGS", &args_file);
    // A sandbox's temp dir can be noexec, and this also runs inside makepkg's and rpmbuild's
    // check stages.
    if std::process::Command::new(&nyan).status().is_err() {
        eprintln!("skipped: a script in the temp dir cannot be run here");
        return;
    }

    fs::create_dir_all("data").unwrap();
    fs::write("machine-id", "here-00000000\n").unwrap();
    let conn = db::open_connection(&config::get_db_path()).unwrap();
    db::init_db(&conn).unwrap();
    let keep = config::keep_dir();
    fs::create_dir_all(&keep).unwrap();

    let wad_file = keep.join("scythe.wad");
    fs::write(&wad_file, b"PWAD the first").unwrap();
    let wad_id = db::add_wad(
        &conn,
        &NewWad::new("Scythe", SourceType::Local)
            .filename("scythe.wad")
            .cached_path(wad_file.display().to_string()),
    )
    .unwrap();
    let update = WadUpdate::new()
        .set_text("custom_sourceport", Some(nyan.display().to_string()))
        .set_int("complevel", Some(9));
    db::update_wad(&conn, wad_id, &update).unwrap();

    // A companion, linked the way the Companions tab links one.
    let deh = keep.join("fix.deh");
    fs::write(&deh, b"Patch File for DeHackEd v3.0").unwrap();
    let companion =
        db::add_companion(&conn, "f1x", "fix.deh", &deh.display().to_string(), 28).unwrap();
    db::link_companion_to_wad(&conn, wad_id, companion).unwrap();

    // ---- Recording is a session with -record last, under a name of the WAD's.
    let opts = PlayOptions {
        record: true,
        ..PlayOptions::default()
    };
    let played = player::play(&conn, wad_id, opts).unwrap();
    let recorded = played.recorded.clone().expect("asked to record");
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    let name = &recorded[0];
    assert!(
        name.starts_with("scythe_") && name.ends_with(".lmp"),
        "{name}"
    );

    let args = ran_with(&args_file);
    let demos_dir = config::find_wad_data_dir(
        &db::get_wad(&conn, wad_id, false).unwrap().unwrap().data_key,
        "Scythe",
    )
    .map(|dir| demos::demos_dir(&dir))
    .unwrap();
    assert_eq!(
        args[args.len() - 2..],
        [
            "-record".to_string(),
            demos_dir
                .join(name.trim_end_matches(".lmp"))
                .display()
                .to_string()
        ]
    );
    let sessions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE wad_id = ?1",
            [wad_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sessions, 1, "recording is a session");

    // What it was recorded with is beside it, naming the session.
    let demo = demos_dir.join(name);
    let record = demos::read_record(&demo).expect("a record beside the demo");
    let session_uid: String = conn
        .query_row(
            "SELECT uid FROM sessions WHERE wad_id = ?1",
            [wad_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(record.session.as_deref(), Some(session_uid.as_str()));
    assert_eq!(record.sourceport, "nyan-doom");
    assert_eq!(record.complevel, Some(9));
    assert_eq!(
        record
            .files
            .iter()
            .map(|f| (f.role, f.name.as_str()))
            .collect::<Vec<_>>(),
        [
            (FileRole::Companion, "fix.deh"),
            (FileRole::Wad, "scythe.wad")
        ]
    );
    let listed = demos::find_demo_files(demos_dir.parent().unwrap());
    let header = listed[0].header.as_ref().expect("its header reads");
    assert_eq!(header.map_name(false).as_deref(), Some("MAP07"));
    assert_eq!(header.duration_secs(), Some(1));

    // ---- Exported to share, with the WAD and the companion, and a note of how to play it.
    let export = demos::export::plan_export(&conn, wad_id, &listed, true, false).unwrap();
    let zipped = root.path().join("scythe-demos.zip");
    demos::export::write_export(&export, &zipped).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(&zipped).unwrap()).unwrap();
    let mut in_zip: Vec<String> = zip
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    in_zip.sort();
    let stem = name.trim_end_matches(".lmp");
    let mut expected = vec![
        name.clone(),
        format!("{name}.toml"),
        format!("{stem}.txt"),
        "fix.deh".to_string(),
        "scythe.wad".to_string(),
    ];
    expected.sort();
    assert_eq!(in_zip, expected);
    let mut note = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(&format!("{stem}.txt")).unwrap(), &mut note)
        .unwrap();
    assert!(
        note.contains(&format!(
            "nyan-doom -complevel 9 -deh fix.deh -file scythe.wad -playdemo {name}"
        )),
        "{note}"
    );

    // ---- Changed since: another complevel, and a -record of the user's own.
    let update = WadUpdate::new().set_int("complevel", Some(11)).set_text(
        "custom_args",
        Some(r#"["-record","junk","-fast"]"#.to_string()),
    );
    db::update_wad(&conn, wad_id, &update).unwrap();

    let watched = player::play_demo(&conn, wad_id, name, DemoOptions::default()).unwrap();
    assert_eq!(&watched.demo, name);
    assert!(!watched.crashed());
    let args = ran_with(&args_file);
    assert_eq!(
        value_of(&args, "-complevel").as_deref(),
        Some("9"),
        "{args:?}"
    );
    assert_eq!(value_of(&args, "-deh"), Some(deh.display().to_string()));
    assert!(!args.iter().any(|a| a == "-record"), "{args:?}");
    assert!(!args.iter().any(|a| a == "-fast"), "{args:?}");
    assert_eq!(
        args[args.len() - 2..],
        ["-playdemo".to_string(), demo.display().to_string()]
    );
    let sessions_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE wad_id = ?1",
            [wad_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sessions_after, 1, "watching a demo is not a session");

    // A demo caco didn't record goes by what the WAD is set to now, without its -record.
    let elsewhere = root.path().join("from-a-speedrun-archive.lmp");
    fs::copy(&demo, &elsewhere).unwrap();
    let added = demos::add_demo_from_file(demos_dir.parent().unwrap(), &elsewhere).unwrap();
    let added_name = added.file_name().unwrap().to_string_lossy().into_owned();
    player::play_demo(&conn, wad_id, &added_name, DemoOptions::default()).unwrap();
    let args = ran_with(&args_file);
    assert_eq!(
        value_of(&args, "-complevel").as_deref(),
        Some("11"),
        "{args:?}"
    );
    assert!(args.iter().any(|a| a == "-fast"));
    assert!(
        !args.iter().any(|a| a == "-record" || a == "junk"),
        "{args:?}"
    );

    // ---- What can't be had any more is said, and nothing launches until it is accepted.
    fs::remove_file(&deh).unwrap();
    fs::write(&wad_file, b"PWAD the second, fixed upstream").unwrap();
    fs::remove_file(&args_file).unwrap();
    match player::play_demo(&conn, wad_id, name, DemoOptions::default()) {
        Err(caco_core::Error::DemoDiffers { title, differences }) => {
            assert_eq!(title, "Scythe");
            assert_eq!(
                differences,
                [
                    DemoDifference::FileMissing {
                        role: FileRole::Companion,
                        name: "fix.deh".to_string()
                    },
                    DemoDifference::WadChanged,
                ]
            );
        }
        other => panic!("expected DemoDiffers, got {other:?}"),
    }
    assert!(!args_file.exists(), "launched before asking");

    let anyway = DemoOptions {
        accept_differences: true,
    };
    player::play_demo(&conn, wad_id, name, anyway).unwrap();
    let args = ran_with(&args_file);
    assert_eq!(value_of(&args, "-complevel").as_deref(), Some("9"));
    assert_eq!(value_of(&args, "-deh"), None);
}
