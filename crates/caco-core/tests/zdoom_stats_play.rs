//! Map exits from uzdoom reaching the library, told through the real `player::play` with a
//! script for a sourceport that names its log the way each build of uzdoom does.
//!
//! uzdoom since August 2026 writes `+logfile NAME` as `log-NAME.txt` in its working
//! directory, with `.` and `:` made `-`, and caco used to give it a path, which it could not
//! open: every session's exits were lost, and nothing said so. So this plays a WAD under
//! both spellings, one session whose log never comes, and progress left behind in a save
//! from before any of it, and checks what each session is credited with.
//!
//! A test binary of its own, and the only test in it, because `crate::player` reads global
//! config, and this points that at a temp dir. caco's folders are given as relative paths
//! from there, since the sourceport is now run from the WAD's data folder, where one would
//! mean somewhere else.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use caco_core::config;
use caco_core::db::{self, NewWad, SourceType, Status, WadUpdate};
use caco_core::player::{self, PlayOptions, PlayResult, StatsReport};
use caco_core::stats_watcher;
use caco_core::wad_stats::{self, WadStats};

/// A PWAD of four maps, each with a few imps.
fn four_map_wad() -> Vec<u8> {
    let things: Vec<u8> = (0..20)
        .flat_map(|_| [0i16, 0, 0, 3001, 7])
        .flat_map(|field| field.to_le_bytes())
        .collect();
    let lumps: Vec<(&str, &[u8])> = ["MAP01", "MAP02", "MAP03", "MAP04"]
        .into_iter()
        .flat_map(|map| [(map, &[][..]), ("THINGS", &things[..])])
        .collect();
    let mut data = Vec::new();
    let mut directory = Vec::new();
    for (name, bytes) in &lumps {
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

/// A sourceport called `uzdoom`, so caco treats it as one. It says where it was run from and
/// with what, fails as uzdoom would if a file it was given is not there from where it runs,
/// writes `$PLAY_LOG` as its log, the way uzdoom since August does or, with
/// `$PLAY_OLD_LOGFILE`, the way every build before did, or no log at all with `$PLAY_NO_LOG`,
/// and copies `$PLAY_SAVE` in as a save.
fn write_fake_uzdoom(path: &Path) {
    let script = r#"#!/bin/sh
pwd > "$PLAY_CWD"
printf '%s\n' "$@" > "$PLAY_ARGS"
for arg in "$@"; do
    case "$arg" in
        *.wad|*.pk3|*.ini) [ -e "$arg" ] || { echo "Could not find $arg" >&2; exit 4; } ;;
    esac
done
while [ $# -gt 0 ]; do
    case "$1" in
        +logfile) log="$2"; shift ;;
        -savedir) savedir="$2"; shift ;;
    esac
    shift
done
if [ -n "$log" ] && [ -z "$PLAY_NO_LOG" ]; then
    if [ -n "$PLAY_OLD_LOGFILE" ]; then
        file="$log"
    else
        file="log-$(printf '%s' "$log" | tr '.:' '--').txt"
    fi
    { printf 'Log started\n'; printf '%b' "$PLAY_LOG"; } > "$file" || echo "Could not start log"
fi
[ -z "$PLAY_SAVE" ] || cp "$PLAY_SAVE" "$savedir/quick00.zds"
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A save as uzdoom writes one, cut down to what caco reads: made on `current`, with
/// `finished` in its statistics, at Ultra-Violence.
fn write_save(path: &Path, current: &str, finished: &[&str]) {
    let levels: Vec<serde_json::Value> = finished
        .iter()
        .map(|name| {
            serde_json::json!({
                "totalkills": 20, "killcount": 20, "totalitems": 0, "itemcount": 0,
                "totalsecrets": 0, "secretcount": 0, "leveltime": 3500, "levelname": name,
            })
        })
        .collect();
    let globals = serde_json::json!({
        "servercvars": {"skill": "3"},
        "statistics": {"startlevel": "map01", "levels": levels},
    });
    let info = serde_json::json!({"Engine": "UZDOOM", "Current Map": current});
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("info.json", options).unwrap();
    zip.write_all(info.to_string().as_bytes()).unwrap();
    zip.start_file("globals.json", options).unwrap();
    zip.write_all(globals.to_string().as_bytes()).unwrap();
    zip.finish().unwrap();
}

/// What a session is credited with: the maps its stats went from not exited to exited.
fn credited(conn: &rusqlite::Connection, wad_id: i64) -> Vec<String> {
    let session = &db::get_sessions(conn, wad_id).unwrap()[0];
    let parse = |json: &Option<String>| -> Option<WadStats> {
        json.as_deref()
            .map(|json| wad_stats::stats_from_json(json).unwrap())
    };
    let after = parse(&session.stats_after).expect("the session has stats");
    wad_stats::compute_stats_delta(parse(&session.stats_before).as_ref(), &after).maps_played
}

/// Every map this machine's stats say was finished, with how many times.
fn exits(conn: &rusqlite::Connection, wad_id: i64) -> Vec<(String, i32)> {
    let wad = db::get_wad(conn, wad_id, false).unwrap().unwrap();
    let stats = wad_stats::stats_from_json(&wad.stats_snapshot.unwrap()).unwrap();
    let mut exits: Vec<_> = stats
        .maps
        .into_iter()
        .filter(|m| m.total_exits > 0)
        .map(|m| (m.lump, m.total_exits))
        .collect();
    exits.sort();
    exits
}

fn set(var: &str, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: the only test in this binary, and it has no other threads.
    unsafe { std::env::set_var(var, value) };
}

fn unset(var: &str) {
    // SAFETY: as for `set`.
    unsafe { std::env::remove_var(var) };
}

fn play(conn: &rusqlite::Connection, wad_id: i64) -> PlayResult {
    let played = player::play(conn, wad_id, PlayOptions::default()).unwrap();
    assert_eq!(played.exit_code, Some(0), "the sourceport was not happy");
    played
}

#[test]
fn every_build_of_uzdoom_has_its_map_exits_counted() {
    let root = tempfile::tempdir().unwrap();
    // caco's folders are relative to where it was started, which is not where the
    // sourceport will be.
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
    assert!(config::default_data_dir().is_absolute());

    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let uzdoom = bin.join("uzdoom");
    write_fake_uzdoom(&uzdoom);
    // A sandbox's temp dir can be noexec, as in `caco-sources/tests/mirror_play.rs`, and
    // this also runs inside makepkg's and rpmbuild's check stages.
    let probe = root.path().join("probe");
    set("PLAY_CWD", &probe);
    set("PLAY_ARGS", &probe);
    if std::process::Command::new(&uzdoom).status().is_err() {
        eprintln!("skipped: a script in the temp dir cannot be run here");
        return;
    }
    let ran_in = root.path().join("ran-in");
    let ran_with = root.path().join("ran-with");
    set("PLAY_CWD", &ran_in);
    set("PLAY_ARGS", &ran_with);

    fs::create_dir_all("data").unwrap();
    fs::write("machine-id", "here-00000000\n").unwrap();
    let conn = db::open_connection(&config::get_db_path()).unwrap();
    db::init_db(&conn).unwrap();

    let keep = config::keep_dir();
    fs::create_dir_all(&keep).unwrap();
    let file = keep.join("quad.wad");
    fs::write(&file, four_map_wad()).unwrap();
    let wad_id = db::add_wad(
        &conn,
        &NewWad::new("Quad", SourceType::Local)
            .filename("quad.wad")
            .cached_path(file.display().to_string()),
    )
    .unwrap();
    let update = WadUpdate::new().set_text("custom_sourceport", Some(uzdoom.display().to_string()));
    db::update_wad(&conn, wad_id, &update).unwrap();
    let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
    let data_dir = config::get_wad_data_dir(&wad.data_key, &wad.title);
    fs::create_dir_all(&data_dir).unwrap();

    // ---- MAP01 was finished in a session whose log was lost, and only its save says so.
    let lost = data_dir.join("auto00.zds");
    write_save(&lost, "MAP02", &["MAP01"]);
    fs::File::options()
        .write(true)
        .open(&lost)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(86_400))
        .unwrap();
    // And a log from the days of `+logfile <path>` is still lying about.
    fs::write(
        data_dir.join("caco_stats.log"),
        "CACOSTATS|EXIT|MAP04|3|3500|20/20|0/0|0/0\n",
    )
    .unwrap();

    // ---- uzdoom as it is now finishes MAP02.
    set("PLAY_LOG", r"CACOSTATS|EXIT|MAP02|3|3500|20/20|0/0|0/0\n");
    let played = play(&conn, wad_id);

    assert_eq!(
        fs::canonicalize(fs::read_to_string(&ran_in).unwrap().trim()).unwrap(),
        fs::canonicalize(&data_dir).unwrap(),
        "run from the data folder, where its log lands"
    );
    let args = fs::read_to_string(&ran_with).unwrap();
    assert!(args.contains("+logfile\ncaco_stats\n"), "{args}");
    assert!(data_dir.join("log-caco_stats.txt").is_file());
    assert!(
        !data_dir.join("caco_stats.log").exists(),
        "cleared at launch"
    );
    assert_eq!(
        played.stats,
        StatsReport {
            log_missing_from: None,
            from_saves: vec!["MAP01".to_string()],
        }
    );
    // MAP01 was counted before the session began, so it is not this session's.
    assert_eq!(credited(&conn, wad_id), ["MAP02"]);
    assert_eq!(
        exits(&conn, wad_id),
        [("MAP01".into(), 1), ("MAP02".into(), 1)]
    );
    let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
    assert_eq!(wad.status, Status::InProgress);

    // ---- A session whose log never comes, saved on MAP04 after finishing MAP03.
    set("PLAY_NO_LOG", "1");
    let save = root.path().join("on-map04.zds");
    write_save(&save, "MAP04", &["MAP01", "MAP02", "MAP03"]);
    set("PLAY_SAVE", &save);
    let played = play(&conn, wad_id);
    unset("PLAY_NO_LOG");
    unset("PLAY_SAVE");

    assert_eq!(
        played.stats,
        StatsReport {
            log_missing_from: Some("uzdoom".to_string()),
            from_saves: vec!["MAP03".to_string()],
        }
    );
    assert_eq!(credited(&conn, wad_id), ["MAP03"]);

    // ---- A build from before August finishes MAP04, and its log is read too.
    set("PLAY_OLD_LOGFILE", "1");
    set("PLAY_LOG", r"CACOSTATS|EXIT|MAP04|3|3500|20/20|0/0|0/0\n");
    let played = play(&conn, wad_id);
    unset("PLAY_OLD_LOGFILE");

    assert!(data_dir.join(stats_watcher::LOG_NAME).is_file());
    assert!(
        !data_dir.join("log-caco_stats.txt").exists(),
        "the last session's log went at launch"
    );
    assert_eq!(played.stats, StatsReport::default());
    assert_eq!(credited(&conn, wad_id), ["MAP04"]);
    assert_eq!(
        exits(&conn, wad_id),
        [
            ("MAP01".into(), 1),
            ("MAP02".into(), 1),
            ("MAP03".into(), 1),
            ("MAP04".into(), 1),
        ]
    );
}
