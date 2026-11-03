//! The mirror client against each kind of store. The local ones use real files, and
//! the HTTP ones [`TestServer`], which behaves as the real stores were seen to.

use std::fs;

use caco_core::config::SavedToken;
use caco_core::db::{NewWad, SourceType, WadUpdate};

use super::test_server::{Flavor, TestServer};
use super::*;

const NOW: &str = "2026-11-03T00:00:00Z";
const TOPIC_URL: &str = "https://www.doomworld.com/vb/thread/151858";
const WAD_BYTES: &[u8] = b"PWAD for the mirror";

fn sha256_of(bytes: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bytes");
    fs::write(&path, bytes).unwrap();
    identity::hash_file(&path).unwrap().sha256
}

/// A library of its own, the "machine" in these tests.
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

    /// A manual-only WAD with this file on this machine.
    fn wad_with_file(&self, filename: &str, bytes: &[u8]) -> i64 {
        let path = self.dir.path().join("collection").join(filename);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        let new = NewWad::new("Lonesome Road", SourceType::Doomworld)
            .author("tourniquet")
            .source_url(TOPIC_URL);
        let id = db::add_wad(&self.conn, &new).unwrap();
        let update = WadUpdate::new().set_text("cached_path", Some(path.display().to_string()));
        db::update_wad(&self.conn, id, &update).unwrap();
        id
    }

    /// A manual-only WAD this machine has never had a file for.
    fn wad_without_file(&self) -> WadRecord {
        let new = NewWad::new("Lonesome Road", SourceType::Doomworld).source_url(TOPIC_URL);
        let id = db::add_wad(&self.conn, &new).unwrap();
        self.row(id)
    }

    /// A WAD whose file this machine hashed once and has since lost.
    fn wad_that_lost(&self, bytes: &[u8]) -> WadRecord {
        let id = db::add_wad(&self.conn, &NewWad::new("Lost", SourceType::Doomworld)).unwrap();
        let hash = sha256_of(bytes);
        self.conn
            .execute(
                "UPDATE wads SET file_size = ?1, file_sha256 = ?2, file_sha1 = 'x', \
                 content_sha256 = ?2 WHERE id = ?3",
                rusqlite::params![bytes.len() as i64, hash, id],
            )
            .unwrap();
        self.row(id)
    }

    fn row(&self, id: i64) -> WadRecord {
        db::get_wad(&self.conn, id, true).unwrap().unwrap()
    }
}

fn local_mirror(root: &Path) -> MirrorConfig {
    MirrorConfig {
        name: "home".to_string(),
        path: Some(root.to_string_lossy().into_owned()),
        write: true,
        ..Default::default()
    }
}

fn http_mirror(server: &TestServer) -> MirrorConfig {
    MirrorConfig {
        name: "home".to_string(),
        url: Some(server.url.clone()),
        write: true,
        ..Default::default()
    }
}

/// An index listing one file, as a mirror's index would say.
fn index_listing(entries: &[MirrorEntry]) -> String {
    let mut index = MirrorIndex::default();
    for entry in entries {
        index.upsert(entry.clone());
    }
    index.to_json()
}

fn entry_for(bytes: &[u8], filename: &str) -> MirrorEntry {
    MirrorEntry {
        sha256: sha256_of(bytes),
        size: bytes.len() as u64,
        filename: filename.to_string(),
        ..Default::default()
    }
}

/// A local store holding `entries`' files, each containing `bytes`.
fn local_store_with(root: &Path, entries: &[MirrorEntry], bytes: &[u8]) {
    for entry in entries {
        let path = root.join(entry.stored_path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fs::create_dir_all(root.join("index")).unwrap();
    fs::write(root.join(INDEX_PATH), index_listing(entries)).unwrap();
}

fn rows_held(conn: &Connection, mirror: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM mirror_files WHERE mirror = ?1",
        [mirror],
        |row| row.get(0),
    )
    .unwrap()
}

// -- Syncing --

#[test]
fn test_sync_reads_a_local_index_into_the_library() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    local_store_with(root.path(), &[entry_for(WAD_BYTES, "a.wad")], WAD_BYTES);
    let config = local_mirror(root.path());

    let report = sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    assert!(report.changed);
    assert_eq!(report.failed, []);
    assert_eq!(report.read, [("home".to_string(), 1)]);
    assert_eq!(rows_held(&m.conn, "home"), 1);
    // The ordinary launch: nothing new, and nothing written.
    let again = sync_mirrors(&m.conn, &[config]).unwrap();
    assert!(!again.changed);
    assert_eq!(again.read, [("home".to_string(), 1)]);
}

/// What lets the Mirrors dialog say "empty" rather than "not read".
#[test]
fn test_an_empty_mirror_is_read_and_says_so() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();

    let report = sync_mirrors(&m.conn, &[local_mirror(root.path())]).unwrap();

    assert_eq!(report.read, [("home".to_string(), 0)]);
    assert_eq!(report.failed, []);
}

#[test]
fn test_sync_reads_an_http_index_into_the_library() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    server.put(
        INDEX_PATH,
        index_listing(&[entry_for(WAD_BYTES, "a.wad")]).as_bytes(),
    );

    let report = sync_mirrors(&m.conn, &[http_mirror(&server)]).unwrap();

    assert!(report.changed);
    assert_eq!(report.failed, []);
    assert_eq!(rows_held(&m.conn, "home"), 1);
    assert_eq!(server.log(), [format!("GET {INDEX_PATH}")]);
}

/// The whole point of keeping rows: a laptop away from home does not see half its
/// library go manual because the mirror did not answer.
#[test]
fn test_a_mirror_that_does_not_answer_keeps_what_it_was_known_to_hold() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    local_store_with(root.path(), &[entry_for(WAD_BYTES, "a.wad")], WAD_BYTES);
    let config = local_mirror(root.path());
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    // The drive is unmounted.
    let parked = root.path().with_extension("parked");
    fs::rename(root.path(), &parked).unwrap();
    let report = sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();
    fs::rename(&parked, root.path()).unwrap();

    assert!(!report.changed);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, "home");
    assert!(report.failed[0].1.contains("is not there"), "{report:?}");
    assert_eq!(rows_held(&m.conn, "home"), 1);
}

#[test]
fn test_an_http_mirror_that_errors_keeps_what_it_was_known_to_hold() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    server.put(
        INDEX_PATH,
        index_listing(&[entry_for(WAD_BYTES, "a.wad")]).as_bytes(),
    );
    let config = http_mirror(&server);
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    server.fail_once("GET", INDEX_PATH, 503);
    let report = sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    assert_eq!(report.failed[0].1, "HTTP 503 Service Unavailable");
    assert_eq!(rows_held(&m.conn, "home"), 1);
}

#[test]
fn test_a_mirror_that_refuses_to_connect_says_so() {
    let m = Machine::new();
    // A port nothing is listening on: bind one to find it, then let it go.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let config = MirrorConfig {
        name: "home".to_string(),
        url: Some(format!("http://127.0.0.1:{port}/doom")),
        ..Default::default()
    };

    let report = sync_mirrors(&m.conn, &[config]).unwrap();

    // With the reason, since "could not connect" alone is little use setting one up.
    let why = &report.failed[0].1;
    assert!(why.starts_with("could not connect: "), "{why}");
    assert!(why.to_lowercase().contains("refused"), "{why}");
}

/// The other way round from a failure to answer, and on purpose: the mirror has
/// answered, and the safe mistake is to understate what can be fetched.
#[test]
fn test_a_mirror_with_no_index_holds_nothing() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    local_store_with(root.path(), &[entry_for(WAD_BYTES, "a.wad")], WAD_BYTES);
    let config = local_mirror(root.path());
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    fs::remove_file(root.path().join(INDEX_PATH)).unwrap();
    let report = sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    assert!(report.changed);
    assert_eq!(report.failed, []);
    assert_eq!(rows_held(&m.conn, "home"), 0);
}

#[test]
fn test_a_page_that_is_not_an_index_keeps_what_it_was_known_to_hold() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    server.put(
        INDEX_PATH,
        index_listing(&[entry_for(WAD_BYTES, "a.wad")]).as_bytes(),
    );
    let config = http_mirror(&server);
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    // A login page with a 200, the classic.
    server.put(INDEX_PATH, b"<html>Please sign in</html>");
    let report = sync_mirrors(&m.conn, &[config]).unwrap();

    assert!(
        report.failed[0].1.contains("could not read its index"),
        "{report:?}"
    );
    assert_eq!(rows_held(&m.conn, "home"), 1);
}

#[test]
fn test_a_waf_challenge_is_named_as_one() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    server.put(INDEX_PATH, b"<html>Just a moment...</html>");
    server.add_header("cf-mitigated", "challenge");

    let report = sync_mirrors(&m.conn, &[http_mirror(&server)]).unwrap();

    assert_eq!(report.failed[0].1, "Cloudflare answered with a challenge");
}

#[test]
fn test_a_mirror_taken_out_of_the_config_is_forgotten() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    local_store_with(root.path(), &[entry_for(WAD_BYTES, "a.wad")], WAD_BYTES);
    sync_mirrors(&m.conn, &[local_mirror(root.path())]).unwrap();

    let report = sync_mirrors(&m.conn, &[]).unwrap();

    assert!(report.changed);
    assert_eq!(rows_held(&m.conn, "home"), 0);
}

#[test]
fn test_malformed_entries_are_counted_and_the_rest_still_used() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("index")).unwrap();
    fs::write(
        root.path().join(INDEX_PATH),
        serde_json::json!({
            "version": 1,
            "files": [
                { "sha256": "abc", "size": 1, "filename": "bad.wad" },
                { "sha256": sha256_of(WAD_BYTES), "size": 1, "filename": "../../escape.wad" },
                { "sha256": sha256_of(WAD_BYTES), "size": 1, "filename": "good.wad" },
            ],
        })
        .to_string(),
    )
    .unwrap();

    let report = sync_mirrors(&m.conn, &[local_mirror(root.path())]).unwrap();

    assert_eq!(report.skipped, 2);
    assert_eq!(rows_held(&m.conn, "home"), 1);
}

#[test]
fn test_two_mirrors_with_one_name_read_only_the_first() {
    let m = Machine::new();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    local_store_with(first.path(), &[entry_for(b"first", "a.wad")], b"first");
    local_store_with(second.path(), &[entry_for(b"second", "b.wad")], b"second");

    let report = sync_mirrors(
        &m.conn,
        &[local_mirror(first.path()), local_mirror(second.path())],
    )
    .unwrap();

    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].1.contains("same name"), "{report:?}");
    assert_eq!(rows_held(&m.conn, "home"), 1);
}

#[test]
fn test_a_mirror_with_no_name_is_reported_rather_than_read() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    local_store_with(root.path(), &[entry_for(WAD_BYTES, "a.wad")], WAD_BYTES);
    let config = MirrorConfig {
        name: " ".to_string(),
        ..local_mirror(root.path())
    };

    let report = sync_mirrors(&m.conn, &[config]).unwrap();

    assert_eq!(report.failed[0].1, "it needs a name");
    assert!(!report.changed);
}

#[test]
fn test_a_mirror_with_nowhere_to_be_is_reported_rather_than_read() {
    let m = Machine::new();
    let config = MirrorConfig {
        name: "home".to_string(),
        ..Default::default()
    };
    let report = sync_mirrors(&m.conn, &[config]).unwrap();
    assert_eq!(report.failed[0].1, "set a URL or a path");
}

// -- Fetching --

fn fetch(m: &Machine, wad: &WadRecord, mirrors: &[MirrorConfig]) -> Result<Option<PathBuf>> {
    fetch_from_mirrors(&m.conn, wad, mirrors, &m.paths)
}

#[test]
fn test_a_file_is_fetched_over_http_and_checked() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    let entry = MirrorEntry {
        doomworld: Some(151858),
        ..entry_for(WAD_BYTES, "road.zip")
    };
    server.put(&entry.stored_path(), WAD_BYTES);
    server.put(INDEX_PATH, index_listing(&[entry]).as_bytes());
    let config = http_mirror(&server);
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    let dest = fetch(&m, &m.wad_without_file(), &[config])
        .unwrap()
        .unwrap();

    assert_eq!(fs::read(&dest).unwrap(), WAD_BYTES);
    assert_eq!(dest.file_name().unwrap(), "road.zip");
}

#[test]
fn test_a_name_with_spaces_survives_the_round_trip() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    let entry = entry_for(WAD_BYTES, "DATW Ver 1.8 & more.wad");
    server.put(&entry.stored_path(), WAD_BYTES);
    server.put(INDEX_PATH, index_listing(&[entry]).as_bytes());
    let config = http_mirror(&server);
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    let dest = fetch(&m, &m.wad_that_lost(WAD_BYTES), &[config])
        .unwrap()
        .unwrap();

    assert_eq!(dest.file_name().unwrap(), "DATW Ver 1.8 & more.wad");
}

/// "Download, verify, and place": a server's word for what a file is counts for
/// nothing, and nothing unverified may land where a launch could pick it up.
#[test]
fn test_a_file_that_is_not_what_the_index_says_is_not_placed() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let entry = entry_for(WAD_BYTES, "road.zip");
    local_store_with(root.path(), std::slice::from_ref(&entry), WAD_BYTES);
    let config = local_mirror(root.path());
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();
    // Same size, different bytes.
    let mut tampered = WAD_BYTES.to_vec();
    tampered[0] = b'I';
    fs::write(root.path().join(entry.stored_path()), &tampered).unwrap();

    let failure = fetch(&m, &m.wad_that_lost(WAD_BYTES), &[config])
        .unwrap_err()
        .to_string();

    assert!(
        failure.contains("not the file its index lists"),
        "{failure}"
    );
    assert_eq!(fs::read_dir(&m.paths.cache_dir).unwrap().count(), 0);
}

#[test]
fn test_a_file_of_the_wrong_size_is_not_placed() {
    for bytes in [
        &b"PWAD for the"[..],
        &b"PWAD for the mirror, and a bit more besides"[..],
    ] {
        let m = Machine::new();
        let root = tempfile::tempdir().unwrap();
        let entry = entry_for(WAD_BYTES, "road.zip");
        local_store_with(root.path(), std::slice::from_ref(&entry), WAD_BYTES);
        let config = local_mirror(root.path());
        sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();
        fs::write(root.path().join(entry.stored_path()), bytes).unwrap();

        let failure = fetch(&m, &m.wad_that_lost(WAD_BYTES), &[config])
            .unwrap_err()
            .to_string();

        assert!(failure.contains("not the 19 bytes"), "{failure}");
        assert_eq!(fs::read_dir(&m.paths.cache_dir).unwrap().count(), 0);
    }
}

/// Said in the user's words, with no hash and no OS error number, since this ends up
/// in the link dialog.
#[test]
fn test_a_file_the_index_lists_and_the_mirror_lost_is_explained() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let server = TestServer::start(Flavor::Static, None);
    let entry = entry_for(WAD_BYTES, "road.zip");
    local_store_with(root.path(), std::slice::from_ref(&entry), WAD_BYTES);
    fs::remove_dir_all(root.path().join(&entry.sha256)).unwrap();
    server.put(
        INDEX_PATH,
        index_listing(std::slice::from_ref(&entry)).as_bytes(),
    );

    for config in [local_mirror(root.path()), http_mirror(&server)] {
        sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();
        let failure = fetch(&m, &m.wad_that_lost(WAD_BYTES), &[config])
            .unwrap_err()
            .to_string();
        assert_eq!(
            failure,
            "Mirror home: its index lists road.zip, but the file is not there"
        );
    }
}

#[test]
fn test_no_mirror_listing_it_is_not_a_failure() {
    let m = Machine::new();
    let wad = m.wad_that_lost(WAD_BYTES);
    assert_eq!(fetch(&m, &wad, &[]).unwrap(), None);
}

#[test]
fn test_a_failed_mirror_falls_through_to_the_next() {
    let m = Machine::new();
    let broken = tempfile::tempdir().unwrap();
    let working = tempfile::tempdir().unwrap();
    let entry = entry_for(WAD_BYTES, "road.zip");
    local_store_with(broken.path(), std::slice::from_ref(&entry), WAD_BYTES);
    local_store_with(working.path(), std::slice::from_ref(&entry), WAD_BYTES);
    let mirrors = [
        MirrorConfig {
            name: "broken".to_string(),
            ..local_mirror(broken.path())
        },
        MirrorConfig {
            name: "working".to_string(),
            ..local_mirror(working.path())
        },
    ];
    sync_mirrors(&m.conn, &mirrors).unwrap();
    fs::remove_dir_all(broken.path().join(&entry.sha256)).unwrap();

    let dest = fetch(&m, &m.wad_that_lost(WAD_BYTES), &mirrors)
        .unwrap()
        .unwrap();

    assert_eq!(fs::read(dest).unwrap(), WAD_BYTES);
}

#[test]
fn test_mirrors_are_tried_in_the_order_listed() {
    let m = Machine::new();
    let first = TestServer::start(Flavor::Static, None);
    let second = TestServer::start(Flavor::Static, None);
    let entry = entry_for(WAD_BYTES, "road.zip");
    for server in [&first, &second] {
        server.put(&entry.stored_path(), WAD_BYTES);
        server.put(
            INDEX_PATH,
            index_listing(std::slice::from_ref(&entry)).as_bytes(),
        );
    }
    // Named so that the alphabet says the opposite of the config.
    let mirrors = [
        MirrorConfig {
            name: "z-first".to_string(),
            ..http_mirror(&first)
        },
        MirrorConfig {
            name: "a-second".to_string(),
            ..http_mirror(&second)
        },
    ];
    sync_mirrors(&m.conn, &mirrors).unwrap();
    first.clear_log();
    second.clear_log();

    fetch(&m, &m.wad_that_lost(WAD_BYTES), &mirrors)
        .unwrap()
        .unwrap();

    assert_eq!(first.log(), [format!("GET {}", entry.stored_path())]);
    assert_eq!(second.log(), Vec::<String>::new());
}

/// A file named in the index under one name and stored under another, because the
/// store would not have the real one.
#[test]
fn test_a_file_stored_under_another_name_is_found_and_gets_its_own_back() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    let entry = MirrorEntry {
        stored_as: Some("Santa_s.wad".to_string()),
        ..entry_for(WAD_BYTES, "Santa's.wad")
    };
    server.put(&entry.stored_path(), WAD_BYTES);
    server.put(INDEX_PATH, index_listing(&[entry]).as_bytes());
    let config = http_mirror(&server);
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();

    let dest = fetch(&m, &m.wad_that_lost(WAD_BYTES), &[config])
        .unwrap()
        .unwrap();

    assert_eq!(dest.file_name().unwrap(), "Santa's.wad");
}

/// `map01.wad` is not one WAD. Overwriting another WAD's file of the same name would
/// cost it its file, without a word.
#[test]
fn test_a_file_of_the_same_name_is_not_overwritten() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let entry = entry_for(WAD_BYTES, "map01.wad");
    local_store_with(root.path(), std::slice::from_ref(&entry), WAD_BYTES);
    let config = local_mirror(root.path());
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();
    fs::create_dir_all(&m.paths.cache_dir).unwrap();
    let theirs = m.paths.cache_dir.join("map01.wad");
    fs::write(&theirs, b"another WAD's map01").unwrap();

    let dest = fetch(&m, &m.wad_that_lost(WAD_BYTES), &[config])
        .unwrap()
        .unwrap();

    assert_eq!(fs::read(&theirs).unwrap(), b"another WAD's map01");
    assert_ne!(dest, theirs);
    assert_eq!(dest.parent(), Some(m.paths.cache_dir.as_path()));
    let name = dest.file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with("map01-") && name.ends_with(".wad"),
        "{name}"
    );
    assert_eq!(fs::read(&dest).unwrap(), WAD_BYTES);
}

#[test]
fn test_the_same_file_already_there_is_used_without_a_download() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    let entry = entry_for(WAD_BYTES, "road.zip");
    server.put(&entry.stored_path(), WAD_BYTES);
    server.put(INDEX_PATH, index_listing(&[entry]).as_bytes());
    let config = http_mirror(&server);
    sync_mirrors(&m.conn, std::slice::from_ref(&config)).unwrap();
    fs::create_dir_all(&m.paths.cache_dir).unwrap();
    fs::write(m.paths.cache_dir.join("road.zip"), WAD_BYTES).unwrap();
    server.clear_log();

    let dest = fetch(&m, &m.wad_that_lost(WAD_BYTES), &[config])
        .unwrap()
        .unwrap();

    assert_eq!(dest, m.paths.cache_dir.join("road.zip"));
    assert_eq!(server.log(), Vec::<String>::new());
}

#[test]
fn test_names_with_a_hash_keep_their_extension() {
    let hash = "1a2b3c4d5e";
    assert_eq!(name_with_hash("road.zip", hash), "road-1a2b3c4d.zip");
    assert_eq!(name_with_hash("road.v2.pk3", hash), "road.v2-1a2b3c4d.pk3");
    assert_eq!(name_with_hash("road", hash), "road-1a2b3c4d");
    assert_eq!(name_with_hash(".road", hash), ".road-1a2b3c4d");
}

// -- Uploading --

fn upload(m: &Machine, wad_id: i64, mirror: &MirrorConfig) -> Result<Uploaded> {
    upload_wad(&m.conn, wad_id, mirror, NOW)
}

fn read_back(config: &MirrorConfig) -> Vec<MirrorEntry> {
    Store::open(config)
        .unwrap()
        .read_index()
        .unwrap()
        .unwrap_or_default()
        .entries()
        .0
}

#[test]
fn test_uploading_to_a_local_mirror_makes_the_wad_fetchable() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());
    let id = m.wad_with_file("road.zip", WAD_BYTES);
    assert!(!m.row(id).retrievability().is_automatic());

    let uploaded = upload(&m, id, &config).unwrap();

    assert_eq!(
        uploaded,
        Uploaded {
            mirror: "home".to_string(),
            filename: "road.zip".to_string(),
            already_there: false,
        }
    );
    let sha256 = sha256_of(WAD_BYTES);
    let stored = root.path().join(&sha256).join("road.zip");
    assert_eq!(fs::read(stored).unwrap(), WAD_BYTES);
    let row = m.row(id);
    assert!(row.mirrored, "the library knows without waiting for a sync");
    assert!(row.retrievability().is_automatic());

    let entries = read_back(&config);
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.sha256, sha256);
    assert_eq!(entry.size, WAD_BYTES.len() as u64);
    assert_eq!(entry.title.as_deref(), Some("Lonesome Road"));
    assert_eq!(entry.author.as_deref(), Some("tourniquet"));
    assert_eq!(entry.doomworld, Some(151858));
    assert_eq!(entry.added.as_deref(), Some(NOW));
}

#[test]
fn test_uploading_leaves_nothing_half_written_behind() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    upload(
        &m,
        m.wad_with_file("road.zip", WAD_BYTES),
        &local_mirror(root.path()),
    )
    .unwrap();

    let sha256 = sha256_of(WAD_BYTES);
    let names = |dir: &Path| -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(names(&root.path().join(&sha256)), ["road.zip"]);
    assert_eq!(names(&root.path().join("index")), ["index.json"]);
}

/// A drive that is not mounted must not get a mirror built on its mount point.
#[test]
fn test_uploading_to_a_mirror_that_is_not_there_creates_nothing() {
    let m = Machine::new();
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("not-mounted");
    let id = m.wad_with_file("road.zip", WAD_BYTES);

    let failure = upload(&m, id, &local_mirror(&root))
        .unwrap_err()
        .to_string();

    assert!(failure.starts_with("Mirror home:"), "{failure}");
    assert!(failure.contains("is not there"), "{failure}");
    assert!(!root.exists());
}

#[test]
fn test_two_wads_share_a_mirror() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());

    upload(&m, m.wad_with_file("a.wad", b"PWAD a"), &config).unwrap();
    upload(&m, m.wad_with_file("b.wad", b"PWAD b"), &config).unwrap();

    assert_eq!(read_back(&config).len(), 2);
    assert_eq!(rows_held(&m.conn, "home"), 2);
}

#[test]
fn test_uploading_a_file_again_only_adds_what_is_known_about_it() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());
    let first = m.wad_with_file("road.zip", WAD_BYTES);
    upload(&m, first, &config).unwrap();

    // The same bytes under another name, on a WAD that knows something else.
    let second = m.wad_with_file("renamed.zip", WAD_BYTES);
    m.conn
        .execute(
            "UPDATE wads SET idgames_id = '42', source_url = NULL WHERE id = ?1",
            [second],
        )
        .unwrap();
    let uploaded = upload(&m, second, &config).unwrap();

    assert!(uploaded.already_there);
    let entries = read_back(&config);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].filename, "road.zip",
        "the file on the mirror keeps its name"
    );
    assert_eq!(entries[0].doomworld, Some(151858));
    assert_eq!(entries[0].idgames, Some(42));
    let stored: Vec<_> = fs::read_dir(root.path().join(sha256_of(WAD_BYTES)))
        .unwrap()
        .collect();
    assert_eq!(stored.len(), 1);
}

/// The index said it was there, and someone tidied the mirror by hand.
#[test]
fn test_a_file_the_index_lists_but_the_mirror_lost_is_put_back() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());
    let id = m.wad_with_file("road.zip", WAD_BYTES);
    upload(&m, id, &config).unwrap();
    let stored = root.path().join(sha256_of(WAD_BYTES)).join("road.zip");
    fs::remove_file(&stored).unwrap();

    let uploaded = upload(&m, id, &config).unwrap();

    assert!(!uploaded.already_there);
    assert_eq!(fs::read(stored).unwrap(), WAD_BYTES);
}

/// Put back where the index says, whatever this copy happens to be called, or the
/// index goes on pointing at nothing.
#[test]
fn test_a_lost_file_put_back_from_a_copy_with_another_name_can_be_fetched() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());
    upload(&m, m.wad_with_file("road.zip", WAD_BYTES), &config).unwrap();
    fs::remove_dir_all(root.path().join(sha256_of(WAD_BYTES))).unwrap();

    let renamed = m.wad_with_file("renamed.zip", WAD_BYTES);
    let uploaded = upload(&m, renamed, &config).unwrap();

    assert!(!uploaded.already_there);
    let elsewhere = Machine::new();
    sync_mirrors(&elsewhere.conn, std::slice::from_ref(&config)).unwrap();
    let dest = fetch(&elsewhere, &elsewhere.wad_that_lost(WAD_BYTES), &[config])
        .unwrap()
        .unwrap();
    assert_eq!(dest.file_name().unwrap(), "road.zip");
    assert_eq!(fs::read(dest).unwrap(), WAD_BYTES);
}

#[test]
fn test_a_wad_with_no_file_here_cannot_be_uploaded() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let wad = m.wad_without_file();

    let failure = upload(&m, wad.id, &local_mirror(root.path()))
        .unwrap_err()
        .to_string();

    assert!(failure.contains("no file on this machine"), "{failure}");
}

#[test]
fn test_a_file_gone_since_the_row_was_written_cannot_be_uploaded() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let id = m.wad_with_file("road.zip", WAD_BYTES);
    fs::remove_file(m.row(id).cached_path.unwrap()).unwrap();

    let failure = upload(&m, id, &local_mirror(root.path()))
        .unwrap_err()
        .to_string();

    assert!(failure.contains("could not read"), "{failure}");
}

/// An index this build cannot read may hold what it cannot see. Writing it back would
/// lose that, and the file would be on the mirror with nothing pointing at it.
#[test]
fn test_an_index_that_cannot_be_read_stops_the_upload_before_anything_is_written() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("index")).unwrap();
    let from_the_future = r#"{"version": 99, "files": []}"#;
    fs::write(root.path().join(INDEX_PATH), from_the_future).unwrap();
    let id = m.wad_with_file("road.zip", WAD_BYTES);

    let failure = upload(&m, id, &local_mirror(root.path()))
        .unwrap_err()
        .to_string();

    assert!(failure.contains("format 99"), "{failure}");
    assert_eq!(
        fs::read_to_string(root.path().join(INDEX_PATH)).unwrap(),
        from_the_future
    );
    assert!(!root.path().join(sha256_of(WAD_BYTES)).exists());
}

// -- Against a registry --

#[test]
fn test_uploading_to_a_registry() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, None);
    let config = http_mirror(&server);
    let id = m.wad_with_file("road.zip", WAD_BYTES);

    upload(&m, id, &config).unwrap();

    let sha256 = sha256_of(WAD_BYTES);
    assert_eq!(
        server.file(&format!("{sha256}/road.zip")).unwrap(),
        WAD_BYTES
    );
    assert_eq!(read_back(&config).len(), 1);
    assert!(m.row(id).mirrored);
}

/// A registry will not overwrite the index. A whole copy goes up first, and is cleared
/// away afterwards, so that there is an index to read at every moment.
#[test]
fn test_a_registry_gets_its_index_replaced_with_a_spare_in_place() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, None);
    let config = http_mirror(&server);
    upload(&m, m.wad_with_file("a.wad", b"PWAD a"), &config).unwrap();
    server.clear_log();

    upload(&m, m.wad_with_file("b.wad", b"PWAD b"), &config).unwrap();

    let writes: Vec<String> = server
        .log()
        .into_iter()
        .filter(|line| line.contains("index/") && !line.starts_with("GET"))
        .collect();
    assert_eq!(
        writes,
        [
            format!("PUT {INDEX_PATH}"),
            format!("HEAD {INDEX_PATH}"),
            format!("DELETE {SPARE_INDEX_PATH}"),
            format!("PUT {SPARE_INDEX_PATH}"),
            format!("DELETE {INDEX_PATH}"),
            format!("PUT {INDEX_PATH}"),
            format!("DELETE {SPARE_INDEX_PATH}"),
        ]
    );
    assert_eq!(read_back(&config).len(), 2);
    assert_eq!(server.file(SPARE_INDEX_PATH), None);
}

/// The failure the spare exists for: the index has been deleted, and the `PUT` of the
/// new one fails. Every entry would otherwise be gone, and no store can be asked what
/// it holds.
#[test]
fn test_a_failure_while_replacing_the_index_loses_nothing() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, None);
    let config = http_mirror(&server);
    upload(&m, m.wad_with_file("a.wad", b"PWAD a"), &config).unwrap();
    upload(&m, m.wad_with_file("b.wad", b"PWAD b"), &config).unwrap();

    // Fail the second PUT of the index: the first is the one the registry refuses.
    server.fail_once("PUT", INDEX_PATH, 409);
    server.fail_once("PUT", INDEX_PATH, 503);
    // The delete in between happens only if the file is there, so make sure.
    assert!(server.file(INDEX_PATH).is_some());
    let failure = upload(&m, m.wad_with_file("c.wad", b"PWAD c"), &config)
        .unwrap_err()
        .to_string();
    assert!(failure.contains("HTTP 503"), "{failure}");
    assert_eq!(
        server.file(INDEX_PATH),
        None,
        "the failure this test is about"
    );

    // Another machine, reading the mirror in that state, sees all of it.
    let elsewhere = Machine::new();
    let report = sync_mirrors(&elsewhere.conn, std::slice::from_ref(&config)).unwrap();
    assert_eq!(report.failed, []);
    assert_eq!(rows_held(&elsewhere.conn, "home"), 3);

    // And the next upload puts the index back where it belongs.
    upload(&m, m.wad_with_file("d.wad", b"PWAD d"), &config).unwrap();
    assert!(server.file(INDEX_PATH).is_some());
    assert_eq!(read_back(&config).len(), 4);
}

#[test]
fn test_a_file_already_on_a_registry_is_not_a_failure() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, None);
    let config = http_mirror(&server);
    // An upload cut short after the file went up, before the index did.
    let sha256 = sha256_of(WAD_BYTES);
    server.put(&format!("{sha256}/road.zip"), WAD_BYTES);

    let uploaded = upload(&m, m.wad_with_file("road.zip", WAD_BYTES), &config).unwrap();

    assert_eq!(uploaded.filename, "road.zip");
    assert_eq!(read_back(&config).len(), 1);
}

/// What Gitea 28.3 does to real file names: "Santa's Bender.wad" gets a 400.
#[test]
fn test_a_name_a_registry_refuses_is_stored_under_a_safe_one() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, None);
    let config = http_mirror(&server);
    let id = m.wad_with_file("Santa's Bender.wad", WAD_BYTES);

    upload(&m, id, &config).unwrap();

    let sha256 = sha256_of(WAD_BYTES);
    assert_eq!(
        server
            .file(&format!("{sha256}/Santa_s_Bender.wad"))
            .unwrap(),
        WAD_BYTES
    );
    let entry = &read_back(&config)[0];
    assert_eq!(
        entry.filename, "Santa's Bender.wad",
        "the file keeps its real name"
    );
    assert_eq!(entry.stored_as.as_deref(), Some("Santa_s_Bender.wad"));

    // And it comes back on another machine under its real name.
    let elsewhere = Machine::new();
    sync_mirrors(&elsewhere.conn, std::slice::from_ref(&config)).unwrap();
    let dest = fetch(&elsewhere, &elsewhere.wad_that_lost(WAD_BYTES), &[config])
        .unwrap()
        .unwrap();
    assert_eq!(dest.file_name().unwrap(), "Santa's Bender.wad");
}

/// A name that passes a registry's check stays as it is. The safe name is only for
/// when the real one has been refused.
#[test]
fn test_a_name_a_registry_accepts_is_left_alone() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, None);
    let config = http_mirror(&server);

    upload(&m, m.wad_with_file("DATW Ver 1.8.wad", WAD_BYTES), &config).unwrap();

    let entry = &read_back(&config)[0];
    assert_eq!(entry.stored_as, None);
    let sha256 = sha256_of(WAD_BYTES);
    assert!(server.file(&format!("{sha256}/DATW Ver 1.8.wad")).is_some());
}

#[test]
fn test_a_token_is_sent_when_one_is_configured() {
    const VAR: &str = "CACO_TEST_MIRROR_TOKEN_SENT";
    // SAFETY: test-only env mutation, on a variable only this test reads.
    unsafe { std::env::set_var(VAR, "sekrit") };
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, Some("Bearer sekrit"));
    let config = MirrorConfig {
        token_env: Some(VAR.to_string()),
        ..http_mirror(&server)
    };

    upload(&m, m.wad_with_file("road.zip", WAD_BYTES), &config).unwrap();

    assert_eq!(read_back(&config).len(), 1);
}

/// The usual first-time failure: a GUI app often does not inherit what a shell's rc
/// file exports.
#[test]
fn test_a_refusal_says_what_is_missing() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, Some("Bearer sekrit"));

    let unset = MirrorConfig {
        token_env: Some("CACO_TEST_MIRROR_TOKEN_NEVER_SET".to_string()),
        ..http_mirror(&server)
    };
    let failure = upload(&m, m.wad_with_file("a.wad", b"PWAD a"), &unset)
        .unwrap_err()
        .to_string();
    assert!(
        failure.contains("CACO_TEST_MIRROR_TOKEN_NEVER_SET is not set"),
        "{failure}"
    );

    let none = http_mirror(&server);
    let failure = upload(&m, m.wad_with_file("b.wad", b"PWAD b"), &none)
        .unwrap_err()
        .to_string();
    assert!(failure.contains("needs a token or password"), "{failure}");

    let wrong = MirrorConfig {
        saved_token: Some(SavedToken("wrong".to_string())),
        ..http_mirror(&server)
    };
    let failure = upload(&m, m.wad_with_file("c.wad", b"PWAD c"), &wrong)
        .unwrap_err()
        .to_string();
    assert!(failure.contains("it refused the token"), "{failure}");
}

#[test]
fn test_a_store_that_takes_no_uploads_says_so_and_changes_nothing() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Static, None);
    let id = m.wad_with_file("road.zip", WAD_BYTES);

    let failure = upload(&m, id, &http_mirror(&server))
        .unwrap_err()
        .to_string();

    assert!(failure.contains("HTTP 405"), "{failure}");
    assert!(!m.row(id).mirrored);
    assert_eq!(server.file(INDEX_PATH), None);
}

// -- Against WebDAV --

/// WebDAV's 409 means something else entirely: no directory for the file to go in.
#[test]
fn test_uploading_to_webdav_makes_the_directories_it_needs() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Webdav, None);
    let config = http_mirror(&server);

    upload(&m, m.wad_with_file("a.wad", b"PWAD a"), &config).unwrap();
    upload(&m, m.wad_with_file("b.wad", b"PWAD b"), &config).unwrap();

    let sha256 = sha256_of(b"PWAD a");
    assert_eq!(server.file(&format!("{sha256}/a.wad")).unwrap(), b"PWAD a");
    assert_eq!(read_back(&config).len(), 2);
    assert!(server.log().contains(&format!("MKCOL {sha256}")));
    // Overwriting needs no spare: a store that can overwrite just does.
    assert_eq!(server.file(SPARE_INDEX_PATH), None);
    assert!(
        !server
            .log()
            .iter()
            .any(|line| line.contains(SPARE_INDEX_PATH) && line.starts_with("PUT"))
    );
}

/// The 409 has to be asked about, not assumed: taking it for "already there" would
/// report success having stored nothing.
#[test]
fn test_a_conflict_that_is_not_the_file_being_there_is_not_success() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Webdav, None);
    let sha256 = sha256_of(WAD_BYTES);
    server.fail_once("MKCOL", &sha256, 403);

    let failure = upload(
        &m,
        m.wad_with_file("road.zip", WAD_BYTES),
        &http_mirror(&server),
    )
    .unwrap_err()
    .to_string();

    assert!(failure.contains("refused"), "{failure}");
    assert_eq!(server.file(&format!("{sha256}/road.zip")), None);
}

// -- Logging in --

/// `doom:secret`, as a WebDAV server wants it.
const BASIC_LOGIN: &str = "Basic ZG9vbTpzZWNyZXQ=";

fn with_login(server: &TestServer) -> MirrorConfig {
    MirrorConfig {
        username: Some("doom".to_string()),
        saved_token: Some(SavedToken("secret".to_string())),
        ..http_mirror(server)
    }
}

/// The WebDAV servers in common use answer a bearer token with a 401, so without this
/// a mirror on one with a password could not be used at all.
#[test]
fn test_a_username_and_password_log_in_to_webdav() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Webdav, Some(BASIC_LOGIN));
    let config = with_login(&server);

    upload(&m, m.wad_with_file("road.zip", WAD_BYTES), &config).unwrap();

    let elsewhere = Machine::new();
    let report = sync_mirrors(&elsewhere.conn, std::slice::from_ref(&config)).unwrap();
    assert_eq!(report.failed, []);
    assert_eq!(rows_held(&elsewhere.conn, "home"), 1);
}

#[test]
fn test_a_saved_token_goes_as_a_bearer_token_without_a_username() {
    let m = Machine::new();
    let server = TestServer::start(Flavor::Registry, Some("Bearer sekrit"));
    let config = MirrorConfig {
        saved_token: Some(SavedToken("sekrit".to_string())),
        ..http_mirror(&server)
    };

    upload(&m, m.wad_with_file("road.zip", WAD_BYTES), &config).unwrap();

    assert_eq!(read_back(&config).len(), 1);
}

// -- Setting up --

#[test]
fn test_a_new_mirror_probes_as_empty_and_writable() {
    let root = tempfile::tempdir().unwrap();
    let server = TestServer::start(Flavor::Registry, None);

    for config in [local_mirror(root.path()), http_mirror(&server)] {
        assert_eq!(
            probe_mirror(&config),
            Ok(MirrorProbe {
                files: None,
                folder_missing: false,
                can_write: Some(Ok(())),
            })
        );
    }
}

/// Trying a mirror must not leave anything behind on it.
#[test]
fn test_a_probe_leaves_nothing_on_the_mirror() {
    let root = tempfile::tempdir().unwrap();
    let local = local_mirror(root.path());
    probe_mirror(&local).unwrap();
    assert!(!root.path().join(WRITE_TEST_PATH).exists());
    assert!(!root.path().join(INDEX_PATH).exists());

    let server = TestServer::start(Flavor::Registry, None);
    probe_mirror(&http_mirror(&server)).unwrap();
    assert_eq!(server.file(WRITE_TEST_PATH), None);
    assert_eq!(server.file(WRITE_TEST_SPARE_PATH), None);
    assert_eq!(server.file(INDEX_PATH), None);
}

#[test]
fn test_a_probe_reports_what_a_mirror_holds() {
    let root = tempfile::tempdir().unwrap();
    local_store_with(root.path(), &[entry_for(WAD_BYTES, "a.wad")], WAD_BYTES);

    let probe = probe_mirror(&local_mirror(root.path())).unwrap();

    assert_eq!(probe.files, Some(1));
}

#[test]
fn test_a_folder_that_is_not_there_is_not_a_failure_until_it_cannot_be_made() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("new-mirror");

    let probe = probe_mirror(&local_mirror(&root)).unwrap();

    assert!(probe.folder_missing);
    assert!(probe.is_ok());
    assert!(!root.exists(), "trying it creates nothing");
}

#[test]
fn test_a_file_is_not_a_folder() {
    let parent = tempfile::tempdir().unwrap();
    let file = parent.path().join("a-file");
    fs::write(&file, b"x").unwrap();

    let failure = probe_mirror(&local_mirror(&file)).unwrap_err();

    assert!(failure.contains("is a file"), "{failure}");
    assert!(set_up_mirror(&local_mirror(&file)).is_err());
}

/// The check that matters: a server can answer every read and take no writes, and
/// finding that out at the first upload is too late.
#[test]
fn test_a_store_that_takes_no_writes_fails_a_probe_for_a_writable_mirror() {
    let server = TestServer::start(Flavor::Static, None);
    server.put(INDEX_PATH, index_listing(&[]).as_bytes());

    let writable = probe_mirror(&http_mirror(&server)).unwrap();
    assert!(!writable.is_ok());
    let failure = writable.can_write.unwrap().unwrap_err();
    assert!(failure.contains("HTTP 405"), "{failure}");

    // Which is fine if it was never going to be asked to.
    let read_only = MirrorConfig {
        write: false,
        ..http_mirror(&server)
    };
    let probe = probe_mirror(&read_only).unwrap();
    assert!(probe.is_ok());
    assert_eq!(probe.can_write, None);
    assert_eq!(probe.files, Some(0));
}

#[test]
fn test_a_wrong_password_fails_a_probe() {
    let server = TestServer::start(Flavor::Webdav, Some(BASIC_LOGIN));
    let config = MirrorConfig {
        saved_token: Some(SavedToken("wrong".to_string())),
        ..with_login(&server)
    };

    let failure = probe_mirror(&config).unwrap_err();

    assert!(
        failure.contains("it refused the token or password"),
        "{failure}"
    );
}

/// Everything the wizard's Finish does to a folder.
#[test]
fn test_setting_up_a_folder_creates_it_and_gives_it_an_index() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("deep/new-mirror");
    let config = local_mirror(&root);

    set_up_mirror(&config).unwrap();

    assert!(root.join(INDEX_PATH).is_file());
    assert_eq!(probe_mirror(&config).unwrap().files, Some(0));
    let m = Machine::new();
    let report = sync_mirrors(&m.conn, &[config]).unwrap();
    assert_eq!(report.read, [("home".to_string(), 0)]);
    assert_eq!(report.failed, []);
}

/// A registry only makes a package when something is put in it, so this is what makes
/// a new mirror on one exist. And a store that will not overwrite is fine with it.
#[test]
fn test_setting_up_a_registry_mirror_writes_an_index() {
    let server = TestServer::start(Flavor::Registry, None);
    let config = http_mirror(&server);

    set_up_mirror(&config).unwrap();
    set_up_mirror(&config).unwrap();

    assert!(server.file(INDEX_PATH).is_some());
    assert_eq!(probe_mirror(&config).unwrap().files, Some(0));
}

/// A new mirror on Nextcloud has an address naming a folder that is not there yet, and
/// its parent directory is not there for the index either.
#[test]
fn test_setting_up_webdav_makes_the_folder_its_address_names() {
    let server = TestServer::start_without_root(Some(BASIC_LOGIN));
    let config = with_login(&server);

    let probe = probe_mirror(&config).unwrap();
    assert_eq!(
        probe.can_write,
        Some(Ok(())),
        "trying it works on a new folder"
    );

    set_up_mirror(&config).unwrap();

    assert!(server.file(INDEX_PATH).is_some());
    assert!(server.log().iter().any(|line| line == "MKCOL "));
}

/// Finishing the wizard on a mirror that is already in use must not touch what is
/// there: a second machine set up the same way, for one.
#[test]
fn test_setting_up_a_mirror_that_has_an_index_leaves_it_alone() {
    let root = tempfile::tempdir().unwrap();
    let entries = [entry_for(WAD_BYTES, "a.wad")];
    local_store_with(root.path(), &entries, WAD_BYTES);
    let before = fs::read_to_string(root.path().join(INDEX_PATH)).unwrap();

    set_up_mirror(&local_mirror(root.path())).unwrap();

    assert_eq!(
        fs::read_to_string(root.path().join(INDEX_PATH)).unwrap(),
        before
    );
}

/// There is nothing to write to a read-only mirror, and it must not be tried.
#[test]
fn test_setting_up_a_read_only_mirror_writes_nothing() {
    let server = TestServer::start(Flavor::Static, None);
    let config = MirrorConfig {
        write: false,
        ..http_mirror(&server)
    };

    set_up_mirror(&config).unwrap();

    assert!(!server.log().iter().any(|line| line.starts_with("PUT")));
}

// -- Bulk uploads --

fn no_progress(_: UploadProgress<'_>) {}

#[test]
fn test_a_bulk_upload_sends_each_wad_and_says_how_far_it_got() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());
    m.wad_with_file("a.wad", b"PWAD a");
    m.wad_with_file("b.wad", b"PWAD b");
    let wads = db::mirrors::wads_to_upload(&m.conn, "home").unwrap();
    assert_eq!(wads.len(), 2);

    let mut seen = Vec::new();
    let report = upload_wads(
        &m.conn,
        &wads,
        &config,
        NOW,
        &mut |progress| seen.push((progress.done, progress.total)),
        &|| false,
    );

    assert_eq!(report.uploaded, 2);
    assert_eq!(report.failed, []);
    assert!(!report.cancelled);
    assert_eq!(seen, [(0, 2), (1, 2)]);
    assert_eq!(read_back(&config).len(), 2);
    assert_eq!(
        db::mirrors::wads_to_upload(&m.conn, "home").unwrap().len(),
        0
    );
}

/// Each WAD is on the mirror or not by itself, so one bad file costs that one.
#[test]
fn test_a_wad_that_fails_does_not_stop_the_rest() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());
    let gone = m.wad_with_file("a.wad", b"PWAD a");
    m.wad_with_file("b.wad", b"PWAD b");
    fs::remove_file(m.row(gone).cached_path.unwrap()).unwrap();
    let wads = db::mirrors::wads_to_upload(&m.conn, "home").unwrap();

    let report = upload_wads(&m.conn, &wads, &config, NOW, &mut no_progress, &|| false);

    assert_eq!(report.uploaded, 1);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, "Lonesome Road");
}

#[test]
fn test_a_cancelled_bulk_upload_keeps_what_it_did() {
    let m = Machine::new();
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());
    m.wad_with_file("a.wad", b"PWAD a");
    m.wad_with_file("b.wad", b"PWAD b");
    let wads = db::mirrors::wads_to_upload(&m.conn, "home").unwrap();

    let asked = std::cell::Cell::new(0);
    let report = upload_wads(&m.conn, &wads, &config, NOW, &mut no_progress, &|| {
        asked.set(asked.get() + 1);
        asked.get() > 1
    });

    assert!(report.cancelled);
    assert_eq!(report.uploaded, 1);
    assert_eq!(read_back(&config).len(), 1);
}

// -- The whole trip --

/// What the issue is for. One machine links a file it got from somewhere that may not
/// last and uploads it, and another, which has only ever heard of the WAD from a forum
/// thread, plays it.
#[test]
fn test_a_file_uploaded_on_one_machine_is_fetched_on_another() {
    for flavor in [Flavor::Registry, Flavor::Webdav] {
        let server = TestServer::start(flavor, None);
        let config = http_mirror(&server);

        let here = Machine::new();
        upload(&here, here.wad_with_file("road.zip", WAD_BYTES), &config).unwrap();

        let there = Machine::new();
        let wad = there.wad_without_file();
        assert!(
            wad.is_lost(),
            "before the sync it is a WAD nothing can fetch"
        );

        sync_mirrors(&there.conn, std::slice::from_ref(&config)).unwrap();
        let wad = there.row(wad.id);
        assert!(!wad.is_lost(), "{flavor:?}");

        let dest = fetch(&there, &wad, &[config]).unwrap().unwrap();
        assert_eq!(fs::read(dest).unwrap(), WAD_BYTES, "{flavor:?}");
    }
}

#[test]
fn test_a_local_mirror_makes_the_same_trip() {
    let root = tempfile::tempdir().unwrap();
    let config = local_mirror(root.path());
    let here = Machine::new();
    upload(&here, here.wad_with_file("road.zip", WAD_BYTES), &config).unwrap();

    let there = Machine::new();
    let wad = there.wad_without_file();
    sync_mirrors(&there.conn, std::slice::from_ref(&config)).unwrap();

    let dest = fetch(&there, &there.row(wad.id), &[config])
        .unwrap()
        .unwrap();

    assert_eq!(fs::read(dest).unwrap(), WAD_BYTES);
}
