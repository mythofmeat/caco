//! `caco enrich` — re-run detection and enrichment on existing WADs.
//!
//! Output only. The detection, wiki fallback and cacoward ingest all live in
//! `caco_sources::enrich_service`, shared with the GUI.

use std::io::IsTerminal;

use clap::Args;
use rusqlite::Connection;

use caco_core::complevel::complevel_name;
use caco_core::db;
use caco_sources::enrich_service::{self, EnrichProgress, EnrichSummary};

#[derive(Args, Default)]
pub struct EnrichArgs {
    /// WAD query (all WADs if omitted)
    query: Vec<String>,

    /// Only enrich WADs with missing complevel
    #[arg(long)]
    complevel: bool,

    /// Fetch the Cacowards page for `--year` from the Doom Wiki and upsert
    /// entries into the cacowards table, auto-linking to library WADs by
    /// idgames id.
    #[arg(long)]
    cacowards: bool,

    /// Year to scrape (required with `--cacowards`).
    #[arg(long, value_name = "YYYY")]
    year: Option<i64>,

    /// Preview changes without applying them
    #[arg(long)]
    dry_run: bool,
}

pub fn run(conn: &Connection, args: &EnrichArgs) -> Result<(), String> {
    if args.cacowards {
        let Some(year) = args.year else {
            return Err("--cacowards requires --year YYYY".to_string());
        };
        if !args.query.is_empty() {
            return Err("--cacowards does not accept a WAD query".to_string());
        }
        return run_cacowards(conn, year, args.dry_run);
    }
    if args.year.is_some() {
        return Err("--year is only valid with --cacowards".to_string());
    }

    let query = if args.query.is_empty() {
        None
    } else {
        Some(crate::parsing::join_query_args(&args.query))
    };

    let mut wads = db::search_wads(conn, query.as_deref(), None, true, false, 0)
        .map_err(|e| format!("Search error: {e}"))?;

    if wads.is_empty() {
        return Err("No WADs found.".to_string());
    }

    if args.complevel {
        wads.retain(|w| w.complevel.is_none());
        if wads.is_empty() {
            println!("All matching WADs already have complevel set.");
            return Ok(());
        }
    }

    eprintln!("Enriching {} WAD(s)...", wads.len());

    // The in-place counter is only meaningful on a terminal; piping stderr to
    // a file would otherwise collect a stream of control characters.
    let interactive = std::io::stderr().is_terminal();

    // The CLI runs to completion; only the GUI needs to cancel mid-run.
    let summary = enrich_service::enrich_wads(
        conn,
        &wads,
        args.dry_run,
        &mut |progress| {
            if let EnrichProgress::Started {
                index,
                total,
                title,
            } = progress
                && interactive
            {
                eprint!("\r[{}/{}] {title:.40}\x1b[K", index + 1, total);
            }
        },
        &|| false,
    );
    if interactive {
        eprintln!();
    }

    print_summary(&summary, wads.len(), args.dry_run);
    Ok(())
}

fn print_summary(summary: &EnrichSummary, total: usize, dry_run: bool) {
    if summary.changed.is_empty() {
        println!("No new metadata detected.");
    } else {
        let verb = if dry_run { "Would set" } else { "Set" };
        for outcome in &summary.changed {
            let mut parts = Vec::new();
            if let Some(cl) = outcome.complevel {
                parts.push(format!("complevel {cl} ({})", complevel_name(Some(cl))));
            }
            if let Some(ref iwad) = outcome.iwad {
                parts.push(format!("IWAD {iwad}"));
            }
            if outcome.zdoom_required == Some(true) {
                parts.push("zdoom_required".to_string());
            }
            println!("{verb} {} -> {}", outcome.title, parts.join(", "));
        }
        println!();
        let suffix = if dry_run { " (dry run)" } else { "" };
        println!("{}/{total} WAD(s) enriched{suffix}", summary.changed.len());
    }

    if summary.wiki_lookups > 0 {
        println!("({} Doom Wiki lookup(s) performed)", summary.wiki_lookups);
    }
}

fn run_cacowards(conn: &Connection, year: i64, dry_run: bool) -> Result<(), String> {
    eprintln!("Fetching Cacowards {year} from Doom Wiki\u{2026}");
    let summary = enrich_service::enrich_cacowards(conn, year, dry_run)
        .map_err(|e| format!("Cacowards enrich failed: {e}"))?;

    for preview in &summary.previews {
        println!(
            "Would upsert {year} {} — {} ({})",
            preview.category,
            preview.wad_title,
            preview.idgames_url.as_deref().unwrap_or("no idgames link"),
        );
    }

    let suffix = if dry_run { " (dry run)" } else { "" };
    println!(
        "Cacowards {year}: scraped {}, upserted {}, auto-linked {} ({} by idgames, {} by title){suffix}",
        summary.scraped,
        summary.upserted,
        summary.linked_total(),
        summary.linked_by_idgames,
        summary.linked_by_title,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::models::SourceType;
    use caco_core::db::wads::{NewWad, add_wad};
    use caco_core::db::{init_db, open_memory};

    // Detection, wiki fallback and cacoward ingest are tested in
    // `caco_sources::enrich_service`. What is left here is argument handling
    // and the WAD selection this command does before delegating.

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection, title: &str) -> i64 {
        add_wad(conn, &NewWad::new(title, SourceType::Local)).unwrap()
    }

    fn args(query: &[&str], complevel: bool) -> EnrichArgs {
        EnrichArgs {
            query: query.iter().map(|s| s.to_string()).collect(),
            complevel,
            dry_run: false,
            cacowards: false,
            year: None,
        }
    }

    #[test]
    fn test_run_no_wads() {
        let conn = setup();
        let err = run(&conn, &args(&[], false)).unwrap_err();
        assert!(err.contains("No WADs found"));
    }

    #[test]
    fn test_run_with_query_no_match() {
        let conn = setup();
        add_test_wad(&conn, "Alpha WAD");
        assert!(run(&conn, &args(&["Nonexistent"], false)).is_err());
    }

    #[test]
    fn test_run_with_query_filter() {
        let conn = setup();
        add_test_wad(&conn, "Alpha WAD");
        add_test_wad(&conn, "Beta WAD");
        assert!(run(&conn, &args(&["Alpha"], false)).is_ok());
    }

    #[test]
    fn test_run_complevel_filter_all_set() {
        let conn = setup();
        let wad_id = add_test_wad(&conn, "Has Complevel");
        db::update_wad(
            &conn,
            wad_id,
            &db::WadUpdate::new().set_int("complevel", Some(9)),
        )
        .unwrap();

        // Every match already has one — reports that rather than erroring.
        assert!(run(&conn, &args(&[], true)).is_ok());
    }

    #[test]
    fn test_cacowards_requires_year() {
        let conn = setup();
        let cmd = EnrichArgs {
            cacowards: true,
            ..Default::default()
        };
        let err = run(&conn, &cmd).unwrap_err();
        assert!(err.contains("requires --year"));
    }

    #[test]
    fn test_cacowards_rejects_wad_query() {
        let conn = setup();
        let cmd = EnrichArgs {
            cacowards: true,
            year: Some(2023),
            query: vec!["sunlust".to_string()],
            ..Default::default()
        };
        let err = run(&conn, &cmd).unwrap_err();
        assert!(err.contains("does not accept a WAD query"));
    }

    #[test]
    fn test_year_without_cacowards_is_rejected() {
        let conn = setup();
        let cmd = EnrichArgs {
            year: Some(2023),
            ..Default::default()
        };
        let err = run(&conn, &cmd).unwrap_err();
        assert!(err.contains("only valid with --cacowards"));
    }
}
