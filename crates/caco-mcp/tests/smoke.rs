mod common;

use common::with_test_server;

#[test]
fn harness_builds() {
    with_test_server(|server, sandbox, source| {
        let info = server.compute_sandbox_info();
        assert_eq!(info.sandbox_path, sandbox);
        assert_eq!(info.source_home, source);
        // No reset has run yet, so the sandbox DB doesn't exist.
        assert!(!info.exists);
    });
}

#[test]
fn fixture_db_has_seeded_wad() {
    with_test_server(|_server, _sandbox, source| {
        let conn = rusqlite::Connection::open(source.join("library.db")).unwrap();
        let (id, title): (i64, String) = conn
            .query_row("SELECT id, title FROM wads", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(title, "Fixture WAD");
        let tag: String = conn
            .query_row("SELECT tag FROM tags WHERE wad_id = ?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(tag, "fixture");
    });
}
