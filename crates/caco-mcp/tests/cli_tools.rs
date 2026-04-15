//! Integration tests that shell out to the real `caco` CLI through the
//! MCP server's `CliRunner`, against a sandboxed tempdir seeded by the
//! test harness in `common/`.

mod common;

use caco_mcp::cli_runner::CliRunner;
use caco_mcp::reset::{reset_sandbox, ResetOptions};
use common::with_test_server;
use tokio::runtime::Runtime;

fn rt() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn caco_ls_lists_fixture_wad() {
    with_test_server(|server, _sb, _src| {
        reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();

        let runner = CliRunner {
            bin: &server.caco_bin,
            paths: &server.paths,
        };
        let result = rt()
            .block_on(runner.run(vec!["ls".into(), "--output".into(), "json".into()]))
            .unwrap();
        assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
        let json = result.parsed_json.expect("json output");
        let arr = json.as_array().expect("array");
        assert!(
            arr.iter()
                .any(|w| w.get("title").and_then(|v| v.as_str()) == Some("Fixture WAD")),
            "fixture WAD not in ls output: {arr:?}"
        );
    });
}

#[test]
fn caco_modify_adds_tag_visible_in_db() {
    with_test_server(|server, _sb, _src| {
        reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();

        let runner = CliRunner {
            bin: &server.caco_bin,
            paths: &server.paths,
        };
        let rt = rt();

        let ls = rt
            .block_on(runner.run(vec!["ls".into(), "--output".into(), "json".into()]))
            .unwrap();
        assert_eq!(ls.exit_code, 0, "ls failed: {}", ls.stderr);
        let id = ls.parsed_json.unwrap()[0]["id"].as_i64().expect("id");

        let m = rt
            .block_on(runner.run(vec![
                "modify".into(),
                format!("id:{id}"),
                "tag=hard".into(),
            ]))
            .unwrap();
        assert_eq!(m.exit_code, 0, "modify failed: {}", m.stderr);

        let conn = rusqlite::Connection::open_with_flags(
            server.paths.db_path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let tags: Vec<String> = conn
            .prepare("SELECT tag FROM tags WHERE wad_id = ?1 ORDER BY tag")
            .unwrap()
            .query_map([id], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tags.contains(&"hard".to_string()), "tags were {tags:?}");
    });
}

#[test]
fn caco_ls_without_reset_auto_initializes_db() {
    // caco auto-creates its DB at CACO_HOME, so `ls` against an unreset
    // sandbox should succeed with an empty library rather than erroring.
    with_test_server(|server, _sb, _src| {
        let runner = CliRunner {
            bin: &server.caco_bin,
            paths: &server.paths,
        };
        let result = rt()
            .block_on(runner.run(vec!["ls".into(), "--output".into(), "json".into()]))
            .unwrap();
        assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
        let arr = result
            .parsed_json
            .expect("json output")
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(arr.is_empty(), "expected empty library, got {arr:?}");
    });
}
