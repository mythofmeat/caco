//! Recording a demo and playing it back, through the real `player::play` and
//! `player::play_demo`, with a script for a sourceport that says what it was given and
//! writes a demo where `-record` tells it to, adding `.lmp` as nyan-doom does. Like
//! nyan-doom, it refuses to record without a skill and a complevel, which is what v4.6.0
//! never gave it: a script that took anything passed this test while every real recording
//! failed.
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
use caco_core::player::{self, DemoOptions, PlayOptions, RecordStart};

/// A dsda-family sourceport: writes its arguments to `$PLAY_ARGS`, one a line, and a Doom
/// 1.9 demo of one second, at the skill and on the map it was told, to whatever follows
/// `-record`, with `.lmp` added, or `-00002.lmp` and on where that is taken. As `dsda_InitDemoRecording` does, it stops with an error and
/// writes nothing when it is told to record without a skill or a complevel. With `$EXITS` set
/// it says in its `stats.txt` that the map has been exited that many times, as one played to
/// the end would.
fn write_sourceport(path: &Path) {
    let script = r#"#!/bin/sh
printf '%s\n' "$@" > "$PLAY_ARGS"
prev=""
record=""
skill=""
complevel=""
warp="1"
for arg in "$@"; do
    case "$prev" in
        -record) record="$arg" ;;
        -skill) skill="$arg" ;;
        -complevel) complevel="$arg" ;;
        -warp) warp="$arg" ;;
        -data) data="$arg" ;;
    esac
    prev="$arg"
done
if [ -n "$record" ]; then
    if [ -z "$skill" ]; then
        echo "You must specify a skill level when recording a demo!" >&2
        exit 1
    fi
    if [ -z "$complevel" ]; then
        echo "You must specify a compatibility level when recording a demo!" >&2
        exit 1
    fi
    byte() { printf "\\$(printf '%03o' "$1")"; }
    # A name that is taken is numbered, as dsda_GenerateDemoName does.
    out="$record.lmp"
    n=2
    while [ -e "$out" ]; do
        out=$(printf '%s-%05d.lmp' "$record" "$n")
        n=$((n + 1))
    done
    {
        printf '\155'
        byte $((skill - 1))
        printf '\001'
        byte "$warp"
        printf '\000\000\000\000\000\001\000\000\000'
        head -c 140 /dev/zero
        printf '\200'
    } > "$out"
    if [ -n "$EXITS" ]; then
        printf '1\n0\nMAP%02d 1 %d 4 1234 -1 -1 %d 0 0 0 0 -1 -1 -1\n' \
            "$warp" "$warp" "$EXITS" > "$data/stats.txt"
    fi
fi
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

    // ---- Recording is a session with where it starts and -record last, under a name of
    // the WAD's. This one is played to the exit.
    set("EXITS", "1");
    let opts = PlayOptions {
        record: Some(RecordStart {
            map: Some("MAP07".to_string()),
            skill: 4,
        }),
        ..PlayOptions::default()
    };
    let played = player::play(&conn, wad_id, opts.clone()).unwrap();
    let recorded = played.recorded.clone().expect("asked to record");
    assert_eq!(recorded.demos.len(), 1, "{recorded:?}");
    assert_eq!(recorded.set_aside, 0);
    let name = &recorded.demos[0].name;
    assert_eq!(recorded.demos[0].exited, Some(vec!["MAP07".to_string()]));
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
        args[args.len() - 6..],
        [
            "-skill".to_string(),
            "4".to_string(),
            "-warp".to_string(),
            "7".to_string(),
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
    assert_eq!(record.exited, Some(vec!["MAP07".to_string()]));
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
    assert_eq!(header.skill_name().as_deref(), Some("UV"));
    assert_eq!(header.duration_secs(), Some(1));
    assert_eq!(listed[0].finished(), demos::Finished::Yes);

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

    // ---- Changed since: another complevel, and a -record, a -warp and a -skill of the
    // user's own, any of which would wreck a playback.
    let update = WadUpdate::new().set_int("complevel", Some(11)).set_text(
        "custom_args",
        Some(r#"["-record","junk","-fast","-warp","3","-skill","2"]"#.to_string()),
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
    for flag in ["-record", "-fast", "-warp", "-skill"] {
        assert!(!args.iter().any(|a| a == flag), "{flag} in {args:?}");
    }
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
    for flag in ["-record", "junk", "-warp", "-skill"] {
        assert!(!args.iter().any(|a| a == flag), "{flag} in {args:?}");
    }

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

    // ---- Another go at MAP07 that dies before the exit is a demo that didn't finish it, which
    // is not the same as not knowing.
    set("EXITS", "");
    let again = player::play(
        &conn,
        wad_id,
        PlayOptions {
            accept_changed_file: true,
            ..opts
        },
    )
    .unwrap();
    let recorded = again.recorded.expect("asked to record");
    assert_eq!(recorded.demos.len(), 1, "{recorded:?}");
    assert_eq!(recorded.demos[0].exited, Some(Vec::new()));
    let second = demos::find_demo_files(demos_dir.parent().unwrap())
        .into_iter()
        .find(|d| d.name == recorded.demos[0].name)
        .unwrap();
    assert_eq!(second.finished(), demos::Finished::No);
    // The one added from a file has nothing to say.
    let added = demos::find_demo_files(demos_dir.parent().unwrap())
        .into_iter()
        .find(|d| d.name == added_name)
        .unwrap();
    assert_eq!(added.finished(), demos::Finished::Unknown);
}
