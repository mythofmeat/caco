//! `caco stats` — library statistics; `caco sessions` — play session history.

use std::collections::HashMap;

use clap::Args;
use rusqlite::Connection;

use crate::output::{self, CacowardSummaryRow, CacowardView};
use crate::resolve;
use caco_core::db::{self, CORE_CATEGORIES, Status};
use caco_core::wad_stats;

#[derive(Args)]
pub struct StatsArgs {
    /// Group activity by month or year
    #[arg(long, default_value = "month")]
    period: String,

    /// Number of periods to show
    #[arg(long, default_value_t = 12)]
    limit: usize,

    /// Output format: plain | json | table
    #[arg(short = 'o', long = "output", default_value = "table")]
    output: String,

    /// Show the Cacowards completion breakdown instead of activity stats.
    /// Pass `--year YYYY` to drill into a single year's entries.
    #[arg(long)]
    cacowards: bool,

    /// Restrict cacowards output to a single year.
    #[arg(long, value_name = "YYYY")]
    year: Option<i64>,
}

pub fn run_stats(conn: &Connection, args: &StatsArgs) -> Result<(), String> {
    let format: output::OutputFormat = args.output.parse()?;

    if args.cacowards {
        return run_cacowards_stats(conn, args.year, format);
    }
    if args.year.is_some() {
        return Err("--year is only valid with --cacowards".to_string());
    }

    let snapshot = db::get_stats_snapshot(conn, &args.period).map_err(|e| e.to_string())?;
    output::render_stats(&snapshot, args.limit, format);
    Ok(())
}

/// Render Cacoward completion data. With `--year`, lists every entry for that
/// year with its linked-WAD status; without, shows a year × category summary.
fn run_cacowards_stats(
    conn: &Connection,
    year: Option<i64>,
    format: output::OutputFormat,
) -> Result<(), String> {
    let statuses = wad_statuses(conn)?;

    if let Some(year) = year {
        let records = db::get_cacowards_by_year(conn, year).map_err(|e| e.to_string())?;
        let views: Vec<CacowardView> = records
            .into_iter()
            .map(|r| {
                let status = r.wad_id.and_then(|id| statuses.get(&id).copied());
                (r, status)
            })
            .collect();
        output::render_cacowards_year(&views, year, format);
        return Ok(());
    }

    let all = db::get_all_cacowards(conn).map_err(|e| e.to_string())?;
    let rows = summarise_cacowards(&all, &statuses);
    output::render_cacowards_summary(&rows, format);
    Ok(())
}

/// Pull all wads' statuses into a map for cheap lookup. The library is small
/// enough (single-user) that we just slurp everything; this keeps the
/// rendering code free of N+1 lookups regardless of how many cacowards we
/// display.
fn wad_statuses(conn: &Connection) -> Result<HashMap<i64, Status>, String> {
    let wads = db::search_wads(conn, None, None, true, false, 0).map_err(|e| e.to_string())?;
    Ok(wads.into_iter().map(|w| (w.id, w.status)).collect())
}

/// Aggregate a flat list of cacoward records into (year, category) rows for
/// the summary view. Categories appear in the canonical order, and years are
/// emitted newest-first.
fn summarise_cacowards(
    records: &[db::CacowardRecord],
    statuses: &HashMap<i64, Status>,
) -> Vec<CacowardSummaryRow> {
    // Collect distinct years descending.
    let mut years: Vec<i64> = records.iter().map(|r| r.year).collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();

    let mut out = Vec::new();
    for year in years {
        for &cat in CORE_CATEGORIES {
            let mut total = 0;
            let mut linked = 0;
            let mut completed = 0;
            let mut in_progress = 0;
            for r in records
                .iter()
                .filter(|r| r.year == year && r.category == cat)
            {
                total += 1;
                if let Some(wad_id) = r.wad_id {
                    linked += 1;
                    match statuses.get(&wad_id) {
                        Some(Status::Completed) => completed += 1,
                        Some(Status::InProgress) => in_progress += 1,
                        _ => {}
                    }
                }
            }
            if total == 0 {
                continue;
            }
            out.push(CacowardSummaryRow {
                year,
                category: cat.to_string(),
                total,
                linked,
                completed,
                in_progress,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{
        CATEGORY_RUNNER_UP, CATEGORY_WINNER, CacowardRecord, NewCacoward, init_db, open_memory,
        upsert_cacoward,
    };

    fn record_for(
        conn: &rusqlite::Connection,
        year: i64,
        cat: &str,
        title: &str,
    ) -> CacowardRecord {
        let entry = NewCacoward {
            year,
            category: cat.to_string(),
            wad_title: title.to_string(),
            ..Default::default()
        };
        let id = upsert_cacoward(conn, &entry).unwrap();
        db::get_cacoward(conn, id).unwrap().unwrap()
    }

    #[test]
    fn summarise_orders_years_descending_and_categories_canonically() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        let r1 = record_for(&conn, 2022, CATEGORY_WINNER, "A");
        let r2 = record_for(&conn, 2023, CATEGORY_RUNNER_UP, "B");
        let r3 = record_for(&conn, 2023, CATEGORY_WINNER, "C");

        let rows = summarise_cacowards(&[r1, r2, r3], &HashMap::new());
        // 2023 should appear before 2022; winner before runner-up.
        let order: Vec<(i64, &str)> = rows.iter().map(|r| (r.year, r.category.as_str())).collect();
        assert_eq!(
            order,
            vec![(2023, "winner"), (2023, "runner-up"), (2022, "winner")]
        );
    }

    #[test]
    fn summarise_counts_linked_and_completed() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        // Three winners in 2023 — two linked, one completed, one in progress, one unlinked.
        conn.execute(
            "INSERT INTO wads (id, title, source_type, status) VALUES
             (1, 'foo', 'manual', 'completed'),
             (2, 'bar', 'manual', 'in-progress')",
            [],
        )
        .unwrap();

        let mut r1 = record_for(&conn, 2023, CATEGORY_WINNER, "A");
        let mut r2 = record_for(&conn, 2023, CATEGORY_WINNER, "B");
        let r3 = record_for(&conn, 2023, CATEGORY_WINNER, "C");
        r1.wad_id = Some(1);
        r2.wad_id = Some(2);

        let statuses: HashMap<i64, Status> = [(1, Status::Completed), (2, Status::InProgress)]
            .into_iter()
            .collect();

        let rows = summarise_cacowards(&[r1, r2, r3], &statuses);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.total, 3);
        assert_eq!(row.linked, 2);
        assert_eq!(row.completed, 1);
        assert_eq!(row.in_progress, 1);
    }
}

#[derive(Args)]
pub struct SessionsArgs {
    /// WAD query or ID
    query: Vec<String>,

    /// Plain TSV output
    #[arg(long)]
    plain: bool,

    /// Auto-select first match
    #[arg(short = 'y', long)]
    yes: bool,
}

pub fn run_sessions(conn: &Connection, args: &SessionsArgs) -> Result<(), String> {
    let wad = resolve::resolve_one_wad(conn, &args.query, args.yes)?;

    let sessions = db::get_sessions(conn, wad.id).map_err(|e| e.to_string())?;

    // Compute per-session map deltas
    let deltas: Vec<Option<Vec<String>>> = sessions
        .iter()
        .enumerate()
        .map(|(idx, session)| {
            let fallback_before = sessions
                .get(idx + 1)
                .and_then(|prev| prev.stats_after.as_deref());
            let before = session
                .stats_before
                .as_deref()
                .or(fallback_before)
                .and_then(|j| wad_stats::stats_from_json(j).ok());
            let after = session
                .stats_after
                .as_deref()
                .and_then(|j| wad_stats::stats_from_json(j).ok());

            match (before, after) {
                (Some(b), Some(a)) => {
                    let delta = wad_stats::compute_stats_delta(Some(&b), &a);
                    Some(delta.deltas.iter().map(|d| d.lump.clone()).collect())
                }
                (None, Some(a)) => {
                    // No before: all played maps from after
                    Some(a.played_maps().iter().map(|m| m.lump.clone()).collect())
                }
                _ => None,
            }
        })
        .collect();

    let format = if args.plain {
        output::OutputFormat::Plain
    } else {
        output::OutputFormat::Table
    };

    output::render_session_list(&sessions, &wad.title, &deltas, format);
    Ok(())
}
