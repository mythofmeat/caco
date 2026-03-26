//! `caco info` — show WAD details, optionally with per-map statistics.

use clap::Args;
use rusqlite::Connection;

use caco_core::db::{self, WadStats};
use caco_core::wad_stats;
use crate::output::{self, OutputFormat};
use crate::resolve;

#[derive(Args)]
pub struct InfoArgs {
    /// WAD query or ID
    query: Vec<String>,

    /// Output format
    #[arg(short, long, default_value = "table")]
    output: String,

    /// Show per-map statistics
    #[arg(long)]
    levelstats: bool,

    /// Show only live stats (with --levelstats)
    #[arg(long)]
    live: bool,

    /// Show stats for specific completion timestamp (with --levelstats)
    #[arg(short = 'b', long)]
    beaten: Option<String>,

    /// Plain TSV output (with --levelstats)
    #[arg(long)]
    plain: bool,
}

pub fn run(conn: &Connection, args: &InfoArgs) -> Result<(), String> {
    let format: OutputFormat = args.output.parse()?;

    let wads = resolve::resolve_wads(
        conn,
        &args.query,
        resolve::ResolveMode::Pick,
        false,
        false,
    )?;

    for wad in &wads {
        if args.levelstats {
            render_levelstats(conn, wad, &args.beaten, args.live, args.plain)?;
        } else {
            let stats = single_wad_stats(conn, wad.id)?;
            let completions = db::get_wad_completions(conn, wad.id).map_err(|e| e.to_string())?;
            let companions = db::get_companions_for_wad(conn, wad.id).map_err(|e| e.to_string())?;
            output::render_wad_info(wad, &stats, &completions, &companions, format);
        }

        if wads.len() > 1 {
            println!();
        }
    }

    Ok(())
}

fn single_wad_stats(conn: &Connection, wad_id: i64) -> Result<WadStats, String> {
    let batch = db::get_wad_stats_batch(conn, &[wad_id]).map_err(|e| e.to_string())?;
    Ok(batch.get(&wad_id).cloned().unwrap_or_default())
}

fn render_levelstats(
    conn: &Connection,
    wad: &db::WadRecord,
    beaten_ts: &Option<String>,
    live_only: bool,
    plain: bool,
) -> Result<(), String> {
    println!("Map stats for: {} (ID: {})", wad.title, wad.id);
    println!();

    let mut entries_shown = 0;

    // Live stats
    if (beaten_ts.is_none() || live_only)
        && let Some(ref snapshot_json) = wad.stats_snapshot
        && let Ok(stats) = wad_stats::stats_from_json(snapshot_json)
    {
        let label = format!("Current (live) - {} format", stats.format);
        if plain {
            render_stats_plain(&label, &stats);
        } else {
            render_stats_table(&label, &stats);
        }
        entries_shown += 1;
    }

    if live_only {
        if entries_shown == 0 {
            println!("No live stats available.");
        }
        return Ok(());
    }

    // Completion stats
    let completions = db::get_wad_completions(conn, wad.id).map_err(|e| e.to_string())?;
    for comp in &completions {
        // Filter by timestamp if specified
        if let Some(ts) = beaten_ts
            && !comp.completed_at.starts_with(ts)
        {
            continue;
        }

        if let Some(ref snapshot_json) = comp.stats_snapshot
            && let Ok(stats) = wad_stats::stats_from_json(snapshot_json)
        {
            let label = format!(
                "Completion {} - {} format",
                output::format_timestamp(&comp.completed_at),
                stats.format,
            );
            if plain {
                render_stats_plain(&label, &stats);
            } else {
                render_stats_table(&label, &stats);
            }
            entries_shown += 1;
        }
    }

    if entries_shown == 0 {
        println!("No stats available.");
    }

    Ok(())
}

fn render_stats_table(label: &str, stats: &wad_stats::WadStats) {
    use comfy_table::{presets, Table, Cell, CellAlignment};

    println!("  {label}");
    println!("  Maps: {}, Total time: {}",
        stats.played_maps().len(),
        stats.total_time_display(),
    );

    let mut table = Table::new();
    table
        .load_preset(presets::UTF8_FULL_CONDENSED)
        .set_header(vec!["Map", "Kills", "Items", "Secrets", "Time"]);

    for map in stats.played_maps() {
        let kills = format!("{}/{}", map.kills, map.total_kills);
        let items = format!("{}/{}", map.items, map.total_items);
        let secrets = format!("{}/{}", map.secrets, map.total_secrets);
        let time = if map.time_secs >= 0.0 {
            wad_stats::format_time_secs(map.time_secs)
        } else if map.best_time > 0 {
            wad_stats::format_time_tics(map.best_time)
        } else {
            "--".to_string()
        };

        table.add_row(vec![
            Cell::new(&map.lump),
            Cell::new(&kills).set_alignment(CellAlignment::Right),
            Cell::new(&items).set_alignment(CellAlignment::Right),
            Cell::new(&secrets).set_alignment(CellAlignment::Right),
            Cell::new(&time).set_alignment(CellAlignment::Right),
        ]);
    }
    println!("{table}");
    println!();
}

fn render_stats_plain(label: &str, stats: &wad_stats::WadStats) {
    println!("# {label}");
    println!("Map\tKills\tTotalKills\tItems\tTotalItems\tSecrets\tTotalSecrets\tTime");
    for map in stats.played_maps() {
        let time = if map.time_secs >= 0.0 {
            format!("{}", map.time_secs)
        } else if map.best_time > 0 {
            format!("{}", map.best_time)
        } else {
            String::new()
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            map.lump, map.kills, map.total_kills,
            map.items, map.total_items,
            map.secrets, map.total_secrets, time,
        );
    }
}
