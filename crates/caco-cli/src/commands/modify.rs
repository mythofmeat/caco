//! `caco modify` — modify WAD metadata, tags, and completions.

use std::path::Path;

use clap::Args;
use rusqlite::Connection;

use crate::parsing::{self, ModifyAction};
use crate::resolve::{self, ResolveMode};
use caco_core::complevel::parse_complevel;
use caco_core::db::{self, Status, WadRecord, WadUpdate};
use caco_core::wad_stats;

#[derive(Args)]
#[command(after_long_help = "\
ACTIONS:
  field=value                      Set a field (title, author, year, status, iwad, ...)
  tag=value                        Add a tag
  !tag                             Remove all tags
  !tag:pattern                     Remove matching tags (glob supported)
  !field                           Clear a field
  beaten+[N]                       Add N completions (default 1)
  beaten-N                         Remove last N completions
  beaten-TIMESTAMP                 Remove completion by date
  beaten=N                         Set completion count
  completion.<id>.notes=<value>    Edit completion notes (empty clears)
  completion.<id>.date=<value>     Edit completion date
  completion.<id>.stats=<path>     Attach stats file to completion (empty clears)

EXAMPLES:
  caco modify id:5 status=in-progress
  caco modify id:5 tag=slaughter tag=hard
  caco modify id:5 !tag:slaughter
  caco modify id:5 !tag
  caco modify id:5 beaten+
  caco modify id:5 completion.42.notes=\"pacifist run\"
  caco modify id:5 completion.42.stats=stats.txt")]
pub struct ModifyArgs {
    /// Query + field=value actions
    args: Vec<String>,

    /// Completion notes
    #[arg(long)]
    notes: Option<String>,

    /// Completion date (ISO)
    #[arg(long)]
    date: Option<String>,

    /// Stats file to attach
    #[arg(short = 's', long = "stats-file")]
    stats_file: Option<String>,

    /// Target completion by timestamp prefix (for --stats-file)
    #[arg(short = 'b', long)]
    beaten: Option<String>,

    /// Target completion by id (for --stats-file; overrides --beaten)
    #[arg(long)]
    completion: Option<i64>,

    /// Link local file to cache
    #[arg(long)]
    link: Option<String>,

    /// Add companion file (repeatable)
    #[arg(long = "add-file")]
    add_files: Vec<String>,

    /// Remove companion file (repeatable)
    #[arg(long = "remove-file")]
    remove_files: Vec<String>,

    /// Preview changes
    #[arg(long)]
    dry_run: bool,

    /// Skip confirmation
    #[arg(short = 'y', long)]
    yes: bool,
}

pub fn run(conn: &Connection, args: &ModifyArgs) -> Result<(), String> {
    let (query_terms, actions, _sort) = parsing::parse_modify_args(&args.args)?;

    // If no query was given but tag-removal actions were, implicitly query by those tags.
    // e.g. `modify !tag:foo` → match WADs with tag:foo, then remove it.
    let query_terms = if query_terms.is_empty() {
        let implicit: Vec<String> = actions
            .iter()
            .filter_map(|a| match a {
                ModifyAction::RemoveTag { pattern } => Some(format!("tag:{pattern}")),
                ModifyAction::RemoveAllTags => None,
                _ => None,
            })
            .collect();
        if implicit.is_empty() {
            return Err("No query specified.".to_string());
        }
        implicit
    } else {
        query_terms
    };

    if actions.is_empty()
        && args.link.is_none()
        && args.add_files.is_empty()
        && args.remove_files.is_empty()
        && args.stats_file.is_none()
    {
        return Err("No modifications specified.".to_string());
    }

    let wads = resolve::resolve_wads(conn, &query_terms, ResolveMode::Multiple, args.yes, false)?;

    if args.dry_run {
        println!("Would modify {} WAD(s):", wads.len());
        for wad in &wads {
            println!("  {}: {}", wad.id, wad.title);
        }
        return Ok(());
    }

    // Read stats file if specified
    let stats_json = if let Some(ref path) = args.stats_file {
        let stats = wad_stats::parse_stats_file(Path::new(path))
            .map_err(|e| format!("Failed to parse stats file: {e}"))?;
        Some(
            wad_stats::stats_to_json(&stats)
                .map_err(|e| format!("Failed to serialize stats: {e}"))?,
        )
    } else {
        None
    };

    let has_beaten_action = actions.iter().any(|a| {
        matches!(
            a,
            ModifyAction::BeatenAdd { .. }
                | ModifyAction::BeatenRemove { .. }
                | ModifyAction::BeatenRemoveTimestamp { .. }
                | ModifyAction::BeatenSet { .. }
        )
    });

    let mut modified = 0;

    for wad in &wads {
        if apply_modifications(conn, wad, &actions, args, &stats_json, has_beaten_action)? {
            modified += 1;
        }
    }

    println!("Modified {modified} WAD(s).");
    Ok(())
}

fn apply_modifications(
    conn: &Connection,
    wad: &WadRecord,
    actions: &[ModifyAction],
    args: &ModifyArgs,
    stats_json: &Option<String>,
    has_beaten_action: bool,
) -> Result<bool, String> {
    let mut update = WadUpdate::new();
    let mut any_change = false;

    // Suppress auto-completion if explicit beaten actions are present
    if has_beaten_action {
        update = update.no_completion();
    }

    for action in actions {
        match action {
            ModifyAction::SetField { field, value } => {
                update = apply_field_update(update, field, value, wad)?;
                any_change = true;
            }
            ModifyAction::ClearField { field } => {
                let col: &'static str = match field.as_str() {
                    "iwad" => "custom_iwad",
                    "sourceport" => "custom_sourceport",
                    "args" => "custom_args",
                    "idgames-id" => "idgames_id",
                    "config" => "custom_config",
                    "complevel" => "complevel",
                    "title" => "title",
                    "author" => "author",
                    "year" => "year",
                    "description" => "description",
                    "status" => "status",
                    "rating" => "rating",
                    "notes" => "notes",
                    "version" => "version",
                    other => return Err(format!("Unknown field: {other}")),
                };
                if col == "complevel" {
                    update = update.set_int("complevel", None);
                } else {
                    update = update.set_text(col, None);
                }
                any_change = true;
            }
            ModifyAction::AddTag { tag } => {
                db::add_tag(conn, wad.id, tag).map_err(|e| e.to_string())?;
                any_change = true;
            }
            ModifyAction::RemoveAllTags => {
                db::remove_all_tags(conn, wad.id).map_err(|e| e.to_string())?;
                any_change = true;
            }
            ModifyAction::RemoveTag { pattern } => {
                remove_matching_tags(conn, wad.id, pattern)?;
                any_change = true;
            }
            ModifyAction::BeatenAdd { count } => {
                for _ in 0..*count {
                    // Use live stats snapshot for first completion if available
                    let snapshot = if stats_json.is_some() {
                        stats_json.as_deref()
                    } else {
                        wad.stats_snapshot.as_deref()
                    };
                    db::add_wad_completion(
                        conn,
                        wad.id,
                        snapshot,
                        args.notes.as_deref(),
                        args.date.as_deref(),
                    )
                    .map_err(|e| e.to_string())?;
                }
                any_change = true;
            }
            ModifyAction::BeatenRemove { count } => {
                let completions =
                    db::get_wad_completions(conn, wad.id).map_err(|e| e.to_string())?;
                for comp in completions.iter().take(*count as usize) {
                    db::delete_wad_completion(conn, comp.id).map_err(|e| e.to_string())?;
                }
                any_change = true;
            }
            ModifyAction::BeatenRemoveTimestamp { timestamp } => {
                let deleted = db::delete_wad_completion_by_timestamp(conn, wad.id, timestamp)
                    .map_err(|e| e.to_string())?;
                if !deleted {
                    eprintln!(
                        "Warning: no completion found with timestamp '{timestamp}' for WAD {}",
                        wad.id
                    );
                }
                any_change = true;
            }
            ModifyAction::BeatenSet { count } => {
                db::set_wad_completion_count(conn, wad.id, *count).map_err(|e| e.to_string())?;
                any_change = true;
            }
            ModifyAction::CompletionEditNotes { id, value } => {
                ensure_completion_belongs(conn, *id, wad)?;
                let updated =
                    db::update_wad_completion(conn, *id, None, Some(value.as_deref()), None)
                        .map_err(|e| e.to_string())?;
                if updated {
                    any_change = true;
                }
            }
            ModifyAction::CompletionEditDate { id, value } => {
                ensure_completion_belongs(conn, *id, wad)?;
                let updated = db::update_wad_completion(conn, *id, None, None, Some(value))
                    .map_err(|e| e.to_string())?;
                if updated {
                    any_change = true;
                }
            }
            ModifyAction::CompletionEditStats { id, path } => {
                ensure_completion_belongs(conn, *id, wad)?;
                let snapshot_owned = match path {
                    Some(p) => {
                        let stats = wad_stats::parse_stats_file(Path::new(p))
                            .map_err(|e| format!("Failed to parse stats file '{p}': {e}"))?;
                        Some(
                            wad_stats::stats_to_json(&stats)
                                .map_err(|e| format!("Failed to serialize stats: {e}"))?,
                        )
                    }
                    None => None,
                };
                let updated = db::update_wad_completion(
                    conn,
                    *id,
                    Some(snapshot_owned.as_deref()),
                    None,
                    None,
                )
                .map_err(|e| e.to_string())?;
                if updated {
                    any_change = true;
                }
            }
        }
    }

    // Handle --link
    if let Some(ref link_path) = args.link {
        let path = Path::new(link_path);
        if !path.exists() {
            return Err(format!("File not found: {link_path}"));
        }
        let resolved = path
            .canonicalize()
            .map_err(|e| format!("Cannot resolve path: {e}"))?;

        // Copy to cache
        let cache_dir = caco_core::config::get_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        let filename = resolved
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("linked.wad");
        let dest = cache_dir.join(filename);

        let config = caco_core::config::load_config();
        if config.link_mode == "move" {
            std::fs::rename(&resolved, &dest).map_err(|e| format!("Failed to move file: {e}"))?;
        } else {
            std::fs::copy(&resolved, &dest).map_err(|e| format!("Failed to copy file: {e}"))?;
        }

        update = update
            .set_text("cached_path", Some(dest.to_string_lossy().to_string()))
            .set_text("filename", Some(filename.to_string()));
        any_change = true;
    }

    // Handle --add-file (via companion service)
    if !args.add_files.is_empty() {
        for f in &args.add_files {
            let file_path = Path::new(f);
            let (_id, filename) =
                caco_core::companion_service::register_companion(conn, wad.id, file_path)
                    .map_err(|e| format!("Failed to add companion '{f}': {e}"))?;
            eprintln!("  Added companion '{filename}'.");
        }
        any_change = true;
    }

    // Handle --remove-file (via companion service)
    if !args.remove_files.is_empty() {
        let companions = db::get_companions_for_wad(conn, wad.id).map_err(|e| e.to_string())?;

        for name in &args.remove_files {
            if let Some(comp) = companions.iter().find(|c| c.filename == *name) {
                caco_core::companion_service::unregister_companion(
                    conn,
                    wad.id,
                    comp.companion_id,
                    Some("delete"),
                )
                .map_err(|e| format!("Failed to remove companion '{name}': {e}"))?;
                eprintln!("  Removed companion '{name}'.");
            } else {
                eprintln!(
                    "  Warning: companion '{name}' not found for '{}'.",
                    wad.title
                );
            }
        }
        any_change = true;
    }

    // Handle standalone --stats-file (no beaten action)
    if stats_json.is_some() && !has_beaten_action {
        let snapshot_arg = Some(stats_json.as_deref());
        if let Some(comp_id) = args.completion {
            // Attach to specific completion by ID
            let updated = db::update_wad_completion(conn, comp_id, snapshot_arg, None, None)
                .map_err(|e| e.to_string())?;
            if !updated {
                return Err(format!(
                    "No completion with id {comp_id} found for WAD {}",
                    wad.id
                ));
            }
            any_change = true;
        } else if let Some(ref ts) = args.beaten {
            // Attach to specific completion by timestamp prefix
            if let Ok(Some(comp)) = db::find_completion_by_timestamp(conn, wad.id, ts) {
                db::update_wad_completion(conn, comp.id, snapshot_arg, None, None)
                    .map_err(|e| e.to_string())?;
                any_change = true;
            } else {
                return Err(format!(
                    "No completion found with timestamp '{ts}' for WAD {}",
                    wad.id
                ));
            }
        } else {
            // Attach to most recent completion
            let completions = db::get_wad_completions(conn, wad.id).map_err(|e| e.to_string())?;
            if let Some(comp) = completions.first() {
                db::update_wad_completion(conn, comp.id, snapshot_arg, None, None)
                    .map_err(|e| e.to_string())?;
                any_change = true;
            } else {
                return Err(format!(
                    "No completions to attach stats to for WAD {}",
                    wad.id
                ));
            }
        }
    }

    if any_change && !update.is_empty() {
        db::update_wad(conn, wad.id, &update).map_err(|e| e.to_string())?;
    }

    Ok(any_change)
}

fn apply_field_update(
    update: WadUpdate,
    field: &str,
    value: &str,
    _wad: &WadRecord,
) -> Result<WadUpdate, String> {
    // Map CLI field names to DB column names (using 'static str to avoid lifetime issues)
    let col: &'static str = match field {
        "iwad" => "custom_iwad",
        "sourceport" => "custom_sourceport",
        "args" => "custom_args",
        "idgames-id" => "idgames_id",
        "config" => "custom_config",
        "title" => "title",
        "author" => "author",
        "year" => "year",
        "description" => "description",
        "status" => "status",
        "rating" => "rating",
        "notes" => "notes",
        "complevel" => "complevel",
        "version" => "version",
        other => return Err(format!("Unknown field: {other}")),
    };

    match field {
        "status" => {
            let status = Status::parse(value).ok_or_else(|| format!("Invalid status: {value}"))?;
            Ok(update.set_status(status))
        }
        "rating" => {
            let rating: i32 = value
                .parse()
                .map_err(|_| format!("Invalid rating: {value}"))?;
            if !(1..=5).contains(&rating) {
                return Err(format!("Rating must be 1-5, got {rating}"));
            }
            Ok(update.set_int(col, Some(rating as i64)))
        }
        "year" => {
            let year: i32 = value
                .parse()
                .map_err(|_| format!("Invalid year: {value}"))?;
            Ok(update.set_int(col, Some(year as i64)))
        }
        "complevel" => {
            let cl = parse_complevel(value).ok_or_else(|| format!("Invalid complevel: {value}"))?;
            Ok(update.set_int("complevel", Some(cl as i64)))
        }
        "args" => {
            let json = caco_core::player::normalize_custom_args(value)?;
            Ok(update.set_text(col, Some(json)))
        }
        _ => Ok(update.set_text(col, Some(value.to_string()))),
    }
}

/// Ensure completion `id` belongs to `wad`; error out otherwise.
fn ensure_completion_belongs(
    conn: &Connection,
    completion_id: i64,
    wad: &WadRecord,
) -> Result<(), String> {
    let completions = db::get_wad_completions(conn, wad.id).map_err(|e| e.to_string())?;
    if completions.iter().any(|c| c.id == completion_id) {
        Ok(())
    } else {
        Err(format!(
            "completion id {completion_id} does not belong to WAD {}",
            wad.id
        ))
    }
}

fn remove_matching_tags(conn: &Connection, wad_id: i64, pattern: &str) -> Result<(), String> {
    let tags = caco_core::db::fetch_tags(conn, wad_id).map_err(|e| e.to_string())?;

    for tag in &tags {
        if glob_matches(pattern, tag) {
            db::remove_tag(conn, wad_id, tag).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Simple glob matching (supports `*` wildcard).
fn glob_matches(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == value;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 2 {
        let (prefix, suffix) = (parts[0], parts[1]);
        return value.starts_with(prefix) && value.ends_with(suffix);
    }

    // Fallback: exact match
    pattern == value
}
