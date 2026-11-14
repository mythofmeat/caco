//! A zip that only wraps what plays, launched through the real `player::play` with scripts
//! for sourceports that say what they were given and fail on a file that is not there.
//!
//! caco used to hand the sourceport the zip. uzdoom loads a `.wad` at a zip's root and
//! nothing else, so The Warrens: Burrow of the Were, a `.pk3` in a zip, played as Doom 2;
//! nyan-doom loads a zip's root and not a folder in it. So this plays a pk3 in a zip under
//! uzdoom and a WAD and its DEH in a folder in one under nyan-doom, checks that each was
//! handed the files and never the zip, and that what was unpacked goes once the zip does.
//!
//! A test binary of its own, and the only test in it, because `crate::player` reads global
//! config, and this points that at a temp dir.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use caco_core::config;
use caco_core::db::{self, NewWad, SourceType, WadUpdate};
use caco_core::player::{self, PlayOptions};
use sha2::{Digest, Sha256};

const PK3: &[u8] = b"PK3 of the Warrens";
const WAD: &[u8] = b"PWAD of the pack";
const DEH: &[u8] = b"Patch File for DeHackEd v3.0";

/// A sourceport called `name`, which writes its arguments to `$PLAY_ARGS`, one a line, and
/// fails as a sourceport would on a file it was given that is not there.
fn write_sourceport(path: &Path) {
    let script = r#"#!/bin/sh
printf '%s\n' "$@" > "$PLAY_ARGS"
for arg in "$@"; do
    case "$arg" in
        *.wad|*.pk3|*.deh|*.zip) [ -e "$arg" ] || { echo "Could not find $arg" >&2; exit 4; } ;;
    esac
done
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, data) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn set(var: &str, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: the only test in this binary, and it has no other threads.
    unsafe { std::env::set_var(var, value) };
}

/// A WAD whose file is `file`, launched with `sourceport`.
fn add(conn: &rusqlite::Connection, title: &str, file: &Path, sourceport: &Path) -> i64 {
    let id = db::add_wad(
        conn,
        &NewWad::new(title, SourceType::Local)
            .filename(file.file_name().unwrap().to_str().unwrap())
            .cached_path(file.display().to_string()),
    )
    .unwrap();
    let update =
        WadUpdate::new().set_text("custom_sourceport", Some(sourceport.display().to_string()));
    db::update_wad(conn, id, &update).unwrap();
    id
}

/// What the sourceport was given after each `flag`, in order.
fn after(args: &[String], flag: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut taking = false;
    for arg in args {
        if arg.starts_with('-') || arg.starts_with('+') {
            taking = arg == flag;
        } else if taking {
            found.push(PathBuf::from(arg));
            // `-deh` takes one file a time, as caco gives them; `-file` takes many.
            taking = flag == "-file";
        }
    }
    found
}

/// Launch a WAD, and what its sourceport was given.
fn play(conn: &rusqlite::Connection, wad_id: i64, ran_with: &Path) -> Vec<String> {
    let played = player::play(conn, wad_id, PlayOptions::default()).unwrap();
    assert_eq!(played.exit_code, Some(0), "the sourceport was not happy");
    let args = fs::read_to_string(ran_with).unwrap();
    assert!(!args.contains(".zip"), "handed a zip:\n{args}");
    args.lines().map(str::to_string).collect()
}

/// What the WAD's latest session recorded it played. Read from the table, as
/// `db::get_sessions` leaves out a launch as short as these.
fn played_content(conn: &rusqlite::Connection, wad_id: i64) -> Option<String> {
    conn.query_row(
        "SELECT content_sha256 FROM sessions WHERE wad_id = ?1 ORDER BY id DESC LIMIT 1",
        [wad_id],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn a_zip_that_wraps_what_plays_is_handed_over_unpacked() {
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
    let uzdoom = bin.join("uzdoom");
    let nyan = bin.join("nyan-doom");
    write_sourceport(&uzdoom);
    write_sourceport(&nyan);
    let ran_with = root.path().join("ran-with");
    set("PLAY_ARGS", &ran_with);
    // A sandbox's temp dir can be noexec, as in `zdoom_stats_play.rs`, and this also runs
    // inside makepkg's and rpmbuild's check stages.
    if std::process::Command::new(&uzdoom).status().is_err() {
        eprintln!("skipped: a script in the temp dir cannot be run here");
        return;
    }

    fs::create_dir_all("data").unwrap();
    fs::write("machine-id", "here-00000000\n").unwrap();
    let conn = db::open_connection(&config::get_db_path()).unwrap();
    db::init_db(&conn).unwrap();
    let keep = config::keep_dir();
    fs::create_dir_all(&keep).unwrap();
    let unpacked = config::unpacked_dir();

    // ---- A pk3 in a zip, under uzdoom: the pk3 is the WAD, and the readme nothing.
    let warrens_zip = keep.join("TheWarrensBOTW_1.4.3.zip");
    zip_of(
        &warrens_zip,
        &[
            ("README.txt", b"read me"),
            ("BurrowOfTheWere_1.4.3.pk3", PK3),
            ("CREDITS.txt", b"thanks"),
        ],
    );
    let warrens = add(&conn, "The Warrens", &warrens_zip, &uzdoom);

    let args = play(&conn, warrens, &ran_with);

    let warrens_pk3 = unpacked.join(sha256(PK3)).join("BurrowOfTheWere_1.4.3.pk3");
    assert_eq!(after(&args, "-file").last(), Some(&warrens_pk3), "{args:?}");
    assert_eq!(fs::read(&warrens_pk3).unwrap(), PK3);
    assert_eq!(played_content(&conn, warrens), Some(sha256(PK3)));

    // ---- A WAD and its DEH in a folder in a zip, under nyan-doom: the DEH goes to -deh.
    let pack_zip = keep.join("pack.zip");
    zip_of(
        &pack_zip,
        &[
            ("Pack/readme.txt", b"read me"),
            ("Pack/pack.deh", DEH),
            ("Pack/pack.wad", WAD),
        ],
    );
    let pack = add(&conn, "Pack", &pack_zip, &nyan);

    let args = play(&conn, pack, &ran_with);

    let pack_files = unpacked
        .join(played_content(&conn, pack).unwrap())
        .join("Pack");
    assert_eq!(after(&args, "-file"), [pack_files.join("pack.wad")]);
    assert_eq!(after(&args, "-deh"), [pack_files.join("pack.deh")]);

    // ---- Under uzdoom the DEH is a -file, after the WAD.
    let update = WadUpdate::new().set_text("custom_sourceport", Some(uzdoom.display().to_string()));
    db::update_wad(&conn, pack, &update).unwrap();

    let args = play(&conn, pack, &ran_with);

    let files = after(&args, "-file");
    assert_eq!(
        files[files.len() - 2..],
        [pack_files.join("pack.wad"), pack_files.join("pack.deh")]
    );
    assert!(after(&args, "-deh").is_empty());

    // ---- The Warrens' file goes, and what was unpacked from it with the next launch.
    conn.execute(
        "UPDATE wads SET cached_path = NULL WHERE id = ?1",
        [warrens],
    )
    .unwrap();
    fs::remove_file(&warrens_zip).unwrap();

    play(&conn, pack, &ran_with);

    assert!(!warrens_pk3.exists());
    assert!(pack_files.join("pack.wad").exists());
}
