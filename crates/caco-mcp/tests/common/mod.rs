//! Shared integration-test harness for `caco-mcp`.
//!
//! Exposes `with_test_server`, which sets up a sandboxed MCP server on top of
//! two tempdirs (sandbox + seeded source-home) inside a `temp_env::with_vars`
//! scope so the sandbox-safety guard accepts our paths and so parallel tests
//! don't race on `XDG_DATA_HOME` / `CACO_HOME`.

use std::path::Path;

use caco_mcp::sandbox::SandboxPaths;
use caco_mcp::server::CacoMcpServer;
use rusqlite::Connection;
use tempfile::TempDir;

/// Run `f` with a fully-wired `CacoMcpServer`. The sandbox tempdir and the
/// seeded source-home tempdir are owned by this function and dropped after
/// `f` returns, so tests don't need to manage cleanup.
pub fn with_test_server<F>(f: F)
where
    F: FnOnce(&CacoMcpServer, &Path, &Path),
{
    // Point XDG_DATA_HOME at a nonexistent path so dirs::data_dir() can't
    // resolve to anything our tempdirs could conceivably live under, and unset
    // CACO_HOME so the safety guard's forbidden list is fully synthetic.
    temp_env::with_vars(
        [
            ("XDG_DATA_HOME", Some("/nonexistent/caco-mcp-tests")),
            ("CACO_HOME", None),
        ],
        || {
            let sandbox = TempDir::new().expect("sandbox tempdir");
            let source = TempDir::new().expect("source tempdir");
            seed_source_home(source.path());

            let paths = SandboxPaths::new(
                sandbox.path().to_path_buf(),
                source.path().to_path_buf(),
            )
            .expect("sandbox path valid");

            // Use the cargo-run fallback — integration tests don't need a
            // prebuilt caco binary, and resolve() handles that case.
            let caco_bin = caco_mcp::bin_resolve::resolve(None).expect("caco bin");
            let server = CacoMcpServer::new(paths, caco_bin);

            f(&server, sandbox.path(), source.path());
        },
    );
}

/// Create a seeded `source_home` with a valid `library.db` plus the minimal
/// directory layout caco expects. Inserts one synthetic WAD tagged `fixture`
/// so tests have something to query.
pub fn seed_source_home(root: &Path) {
    for sub in &["wads", "data", "iwads", "id24", "companions", "backups"] {
        std::fs::create_dir_all(root.join(sub)).expect("mkdir fixture subdir");
    }

    let db_path = root.join("library.db");
    let conn = Connection::open(&db_path).expect("open fixture db");
    caco_core::db::init_db(&conn).expect("init_db");

    let new_wad = caco_core::db::NewWad::new("Fixture WAD", caco_core::db::SourceType::Local)
        .author("fixture")
        .year(1994)
        .description("seeded fixture WAD for caco-mcp integration tests");
    let id = caco_core::db::add_wad(&conn, &new_wad).expect("add_wad");
    caco_core::db::add_tag(&conn, id, "fixture").ok();
}
