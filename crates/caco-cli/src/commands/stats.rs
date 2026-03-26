//! `caco stats` — library statistics; `caco sessions` — play session history.

use clap::Args;
use rusqlite::Connection;

use caco_core::db;
use caco_core::wad_stats;
use crate::output;
use crate::resolve;

#[derive(Args)]
pub struct StatsArgs {
    /// Group activity by month or year
    #[arg(long, default_value = "month")]
    period: String,

    /// Number of periods to show
    #[arg(long, default_value_t = 12)]
    limit: usize,

    /// Key=value output
    #[arg(long)]
    plain: bool,
}

pub fn run_stats(conn: &Connection, args: &StatsArgs) -> Result<(), String> {
    let snapshot = db::get_stats_snapshot(conn, &args.period).map_err(|e| e.to_string())?;
    output::render_stats(&snapshot, args.limit, args.plain);
    Ok(())
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
        .map(|session| {
            let before = session
                .stats_before
                .as_deref()
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
