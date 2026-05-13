//! `caco ls` — list WADs, tags, IWADs, or id24 WADs.

use std::collections::HashMap;

use clap::Args;
use rusqlite::Connection;

use crate::output::{self, OutputFormat};
use crate::parsing;
use caco_core::db::{
    self, CacowardFilters, EffectiveStatus, ParsedQuery, QueryTerm, Status, normalize_category,
    normalize_status, parse_query, search_cacowards,
};

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

    // If the query contains any `cacoward:` filter, route to the Cacoward
    // entry listing instead of the WAD list. That surfaces entries the user
    // doesn't own yet (status `absent`), which is the whole point of the
    // "what should I import next?" flow.
    let raw_query = parsing::join_query_args(&query_terms);
    let parsed = parse_query(&raw_query);
    if has_cacoward_term(&parsed) {
        return run_cacoward_listing(conn, &parsed, format);
    }

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

// ---------------------------------------------------------------------------
// Cacoward-entry listing mode
// ---------------------------------------------------------------------------

fn has_cacoward_term(parsed: &ParsedQuery) -> bool {
    parsed.or_groups.iter().any(|g| {
        g.terms
            .iter()
            .any(|t| matches!(t.field.as_deref(), Some("cacoward") | Some("cacowards")))
    })
}

/// Build [`CacowardFilters`] from the user's parsed query and dispatch to the
/// entry-list renderer.
///
/// The query is currently flattened to its first OR group — beets-style OR
/// composition with cacoward filters is a future enhancement. Any non-
/// cacoward/status/free-text terms are silently dropped (e.g. `tag:` doesn't
/// apply to entries that aren't yet in the library); the printed warning
/// surfaces unsupported fields so the user knows their filter wasn't honoured.
fn run_cacoward_listing(
    conn: &Connection,
    parsed: &ParsedQuery,
    format: OutputFormat,
) -> Result<(), String> {
    let terms = parsed
        .or_groups
        .first()
        .map(|g| g.terms.as_slice())
        .unwrap_or(&[]);
    if parsed.or_groups.len() > 1 {
        eprintln!("warning: cacoward listings ignore OR groups; using the first group only");
    }

    let (filters, dropped) = build_filters(terms)?;
    for field in dropped {
        eprintln!("warning: ignored unsupported filter in cacoward mode: {field}:");
    }

    let entries = search_cacowards(conn, &filters).map_err(|e| e.to_string())?;
    output::render_cacoward_entries(&entries, format);
    Ok(())
}

/// Translate query terms into [`CacowardFilters`] plus a list of dropped
/// fields the caller should warn about.
fn build_filters(terms: &[QueryTerm]) -> Result<(CacowardFilters, Vec<String>), String> {
    let mut filters = CacowardFilters::default();
    let mut dropped = Vec::new();
    let mut free_text = String::new();

    for term in terms {
        if term.negated {
            // Honouring negation properly across all fields needs more
            // thought; punt for now and warn so the user notices.
            dropped.push(format!("^{}", term.field.as_deref().unwrap_or("text")));
            continue;
        }

        match term.field.as_deref() {
            Some("cacoward") | Some("cacowards") => {
                apply_cacoward_value(&mut filters, &term.value)?;
            }
            Some("status") | Some("play") | Some("play_state") => {
                apply_status_value(&mut filters, &term.value)?;
            }
            Some("year") => {
                if let Ok(y) = term.value.parse::<i64>() {
                    filters.years.push(y);
                } else {
                    return Err(format!("invalid year: {}", term.value));
                }
            }
            Some("title") | Some("author") | None => {
                if !free_text.is_empty() {
                    free_text.push(' ');
                }
                free_text.push_str(&term.value);
            }
            Some(other) => dropped.push(other.to_string()),
        }
    }

    if !free_text.is_empty() {
        filters.free_text = Some(free_text);
    }
    Ok((filters, dropped))
}

fn apply_cacoward_value(filters: &mut CacowardFilters, value: &str) -> Result<(), String> {
    let v = value.trim();
    if v.is_empty() || v == "*" || v.eq_ignore_ascii_case("any") {
        return Ok(());
    }
    if let Some((y, c)) = v.split_once(':') {
        let year: i64 = y
            .parse()
            .map_err(|_| format!("cacoward filter has non-numeric year: {y}"))?;
        let category = normalize_category(c)
            .ok_or_else(|| format!("cacoward filter has unknown category: {c}"))?;
        filters.years.push(year);
        filters.categories.push(category.to_string());
        return Ok(());
    }
    if let Ok(year) = v.parse::<i64>() {
        filters.years.push(year);
        return Ok(());
    }
    let category =
        normalize_category(v).ok_or_else(|| format!("cacoward filter has unknown value: {v}"))?;
    filters.categories.push(category.to_string());
    Ok(())
}

fn apply_status_value(filters: &mut CacowardFilters, value: &str) -> Result<(), String> {
    let normalized = normalize_status(value);
    match normalized.as_str() {
        // `status:unplayed` is the headline "what to play next" filter, so
        // we include `absent` entries — both bucket as "haven't played".
        "unplayed" => {
            filters
                .statuses
                .push(EffectiveStatus::Library(Status::Unplayed));
            filters.statuses.push(EffectiveStatus::Absent);
        }
        "absent" => filters.statuses.push(EffectiveStatus::Absent),
        "in-progress" => filters
            .statuses
            .push(EffectiveStatus::Library(Status::InProgress)),
        "completed" => filters
            .statuses
            .push(EffectiveStatus::Library(Status::Completed)),
        "abandoned" => filters
            .statuses
            .push(EffectiveStatus::Library(Status::Abandoned)),
        other => return Err(format!("unknown status: {other}")),
    }
    Ok(())
}
