//! Integration tests for the introspect module, running against the
//! seeded fixture DB produced by `common::with_test_server`.

mod common;

use caco_mcp::introspect::execute_run_sql;
use caco_mcp::reset::{reset_sandbox, ResetOptions};
use common::with_test_server;

#[test]
fn run_sql_selects_fixture_wad() {
    with_test_server(|server, _sb, _src| {
        reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();
        let res =
            execute_run_sql(&server.paths, "SELECT id, title FROM wads ORDER BY id").unwrap();
        assert_eq!(res.columns, vec!["id", "title"]);
        assert!(!res.rows.is_empty(), "fixture DB should contain rows");
        assert!(res.rows.iter().any(|r| r[1] == serde_json::json!("Fixture WAD")));
        assert!(!res.truncated);
    });
}

#[test]
fn run_sql_rejects_writes() {
    with_test_server(|server, _sb, _src| {
        reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();
        let err = execute_run_sql(
            &server.paths,
            "INSERT INTO wads (title, source_type) VALUES ('hack', 'local')",
        )
        .unwrap_err();
        let msg = format!("{err:#}").to_lowercase();
        assert!(
            msg.contains("read") || msg.contains("reject"),
            "unexpected error: {err:#}"
        );
    });
}

#[test]
fn run_sql_rejects_multiple_statements() {
    with_test_server(|server, _sb, _src| {
        reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();
        let err = execute_run_sql(&server.paths, "SELECT 1; SELECT 2").unwrap_err();
        let msg = format!("{err:#}").to_lowercase();
        assert!(msg.contains("multiple"), "unexpected error: {err:#}");
    });
}

#[test]
fn run_sql_returns_empty_rows_for_matchless_select() {
    with_test_server(|server, _sb, _src| {
        reset_sandbox(&server.paths, &ResetOptions { skip_wads: true }).unwrap();
        let res = execute_run_sql(
            &server.paths,
            "SELECT id FROM wads WHERE title = '__nothing__'",
        )
        .unwrap();
        assert_eq!(res.columns, vec!["id"]);
        assert!(res.rows.is_empty());
        assert!(!res.truncated);
    });
}
