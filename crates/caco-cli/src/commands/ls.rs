//! `caco ls` — list WADs, tags, IWADs, or id24 WADs.

use std::collections::HashMap;

use clap::Args;
use rusqlite::Connection;

use caco_core::db;
use crate::output::{self, OutputFormat};
use crate::parsing;

#[derive(Args)]
pub struct LsArgs {
    /// Query terms + optional inline sort (e.g., "status:playing playtime-")
    query: Vec<String>,

    /// Output format
    #[arg(short, long, default_value = "table")]
    output: String,

    /// Show deleted WADs
    #[arg(long, hide = true)]
    deleted: bool,

    /// List tags with counts
    #[arg(long)]
    tags: bool,

    /// List registered IWADs
    #[arg(long)]
    iwad: bool,

    /// List registered id24 WADs
    #[arg(long)]
    id24: bool,
}

pub fn run(conn: &Connection, args: &LsArgs) -> Result<(), String> {
    let format: OutputFormat = args.output.parse()?;

    if args.tags {
        let tags = db::get_tag_counts(conn).map_err(|e| e.to_string())?;
        output::render_tag_list(&tags, format);
        return Ok(());
    }

    if args.iwad {
        let iwads = db::get_all_iwads(conn).map_err(|e| e.to_string())?;
        // Build preferred variant map
        let mut preferred: HashMap<String, String> = HashMap::new();
        for iwad in &iwads {
            if !preferred.contains_key(&iwad.family) {
                // get_iwad resolves priority
                if let Ok(Some(pref)) = db::get_iwad(conn, &iwad.family, None) {
                    preferred.insert(iwad.family.clone(), pref.variant);
                }
            }
        }
        output::render_iwad_list(&iwads, &preferred, format);
        return Ok(());
    }

    if args.id24 {
        let id24s = db::get_all_id24(conn).map_err(|e| e.to_string())?;
        output::render_id24_list(&id24s, format);
        return Ok(());
    }

    // Normal WAD listing
    let (query_terms, sort_info) = parsing::extract_sort_from_args(&args.query);

    // Apply config defaults
    let config = caco_core::config::load_config();
    let (sort_by, sort_desc) = sort_info.unwrap_or_else(|| {
        // Parse "field+" / "field-" from config, or default to "id+"
        if let Some(ref sort_str) = config.list.sort {
            if sort_str.ends_with('-') {
                (sort_str[..sort_str.len() - 1].to_string(), true)
            } else if sort_str.ends_with('+') {
                (sort_str[..sort_str.len() - 1].to_string(), false)
            } else {
                (sort_str.clone(), true)
            }
        } else {
            ("id".to_string(), true)
        }
    });

    let query_str = if query_terms.is_empty() {
        None
    } else {
        Some(parsing::join_query_args(&query_terms))
    };

    let wads = db::search_wads(
        conn,
        query_str.as_deref(),
        Some(&sort_by),
        sort_desc,
        args.deleted,
        0,
    )
    .map_err(|e| e.to_string())?;

    let wad_ids: Vec<i64> = wads.iter().map(|w| w.id).collect();
    let stats = db::get_wad_stats_batch(conn, &wad_ids).map_err(|e| e.to_string())?;

    output::render_wad_list(&wads, &stats, format);
    Ok(())
}
