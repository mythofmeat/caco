//! Query resolution: parse WAD IDs, ranges, or queries and return matching WADs.

use std::io::{self, Write};
use std::path::PathBuf;

use rusqlite::Connection;

use caco_core::db::{self, WadRecord};

use crate::picker;

/// How to handle multiple matches.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum ResolveMode {
    /// Error if more than one match.
    Error,
    /// Interactive pick (fzf or numbered).
    Pick,
    /// Allow multiple results (confirm with user).
    Multiple,
}

/// Parse an ID range string like "3-6,9,11" into a list of IDs.
fn parse_id_range(value: &str) -> Option<Vec<i64>> {
    let mut ids = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            let start: i64 = start.trim().parse().ok()?;
            let end: i64 = end.trim().parse().ok()?;
            if start > end {
                return None;
            }
            for id in start..=end {
                ids.push(id);
            }
        } else {
            ids.push(part.parse().ok()?);
        }
    }
    if ids.is_empty() { None } else { Some(ids) }
}

/// Check if a string looks like an ID range (digits, commas, hyphens only).
fn looks_like_id_range(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == ',' || c == '-' || c == ' ')
}

/// Resolve a query (args) into WAD records.
///
/// Handles:
/// 1. Empty query (returns None — caller should handle)
/// 2. Single numeric ID
/// 3. ID range (e.g., "3-6,9")
/// 4. Beets-style query string
pub fn resolve_wads(
    conn: &Connection,
    query: &[String],
    mode: ResolveMode,
    yes: bool,
    first: bool,
) -> Result<Vec<WadRecord>, String> {
    if query.is_empty() {
        return Err("No query specified.".to_string());
    }

    let query_str = crate::parsing::join_query_args(query);

    // Check for single numeric ID
    if query.len() == 1
        && let Ok(id) = query[0].parse::<i64>()
    {
        return match db::get_wad(conn, id, false) {
            Ok(Some(wad)) => Ok(vec![wad]),
            Ok(None) => Err(format!("WAD with ID {id} not found.")),
            Err(e) => Err(format!("Database error: {e}")),
        };
    }

    // Check for ID range
    if query.len() == 1 && looks_like_id_range(&query[0])
        && let Some(ids) = parse_id_range(&query[0])
    {
        let mut wads = Vec::new();
        for id in ids {
            match db::get_wad(conn, id, false) {
                Ok(Some(wad)) => wads.push(wad),
                Ok(None) => return Err(format!("WAD with ID {id} not found.")),
                Err(e) => return Err(format!("Database error: {e}")),
            }
        }
        return Ok(wads);
    }

    // Beets-style query
    let results = db::search_wads(conn, Some(&query_str), None, true, false, 0)
        .map_err(|e| format!("Search error: {e}"))?;

    if results.is_empty() {
        return Err(format!("No WADs matching '{query_str}'."));
    }

    if first {
        return Ok(vec![results.into_iter().next().unwrap()]);
    }

    if results.len() == 1 {
        return Ok(results);
    }

    // Multiple results — handle based on mode
    match mode {
        ResolveMode::Error => {
            let preview_count = results.len().min(10);
            let mut msg = format!("Multiple WADs match ({} results):\n", results.len());
            for wad in results.iter().take(preview_count) {
                msg.push_str(&format!(
                    "  {}: {} - {}\n",
                    wad.id,
                    wad.title,
                    wad.author.as_deref().unwrap_or(""),
                ));
            }
            if results.len() > preview_count {
                msg.push_str(&format!("  ... and {} more\n", results.len() - preview_count));
            }
            msg.push_str("Use a more specific query or an ID.");
            Err(msg)
        }
        ResolveMode::Pick => {
            let indices = picker::pick_wads(&results, false);
            if indices.is_empty() {
                return Err("No WAD selected.".to_string());
            }
            Ok(indices.into_iter().map(|i| results[i].clone()).collect())
        }
        ResolveMode::Multiple => {
            if yes {
                return Ok(results);
            }
            let preview_count = results.len().min(10);
            eprintln!("Found {} WADs:", results.len());
            for wad in results.iter().take(preview_count) {
                eprintln!(
                    "  {}: {} - {}",
                    wad.id,
                    wad.title,
                    wad.author.as_deref().unwrap_or(""),
                );
            }
            if results.len() > preview_count {
                eprintln!("  ... and {} more", results.len() - preview_count);
            }
            if confirm("Apply to all?") {
                Ok(results)
            } else {
                Err("Aborted.".to_string())
            }
        }
    }
}

/// Resolve a single WAD from a query, using Pick mode by default.
pub fn resolve_one_wad(
    conn: &Connection,
    query: &[String],
    yes: bool,
) -> Result<WadRecord, String> {
    let wads = resolve_wads(conn, query, ResolveMode::Pick, yes, false)?;
    wads.into_iter().next().ok_or_else(|| "No WAD selected.".to_string())
}

/// Prompt for y/N confirmation on stderr. Returns true if user confirms.
pub fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [y/N] ");
    let _ = io::stderr().flush();
    let mut response = String::new();
    io::stdin().read_line(&mut response).is_ok()
        && response.trim().to_lowercase().starts_with('y')
}

/// Resolve a WAD and its data directory from a query.
pub fn resolve_data_dir(
    conn: &Connection,
    query: &[String],
    yes: bool,
) -> Result<(WadRecord, PathBuf), String> {
    let wad = resolve_one_wad(conn, query, yes)?;
    let data_dir = caco_core::config::find_wad_data_dir(wad.id)
        .unwrap_or_else(|| caco_core::config::get_wad_data_dir(wad.id, &wad.title));
    Ok((wad, data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_id_range_single() {
        assert_eq!(parse_id_range("5"), Some(vec![5]));
    }

    #[test]
    fn test_parse_id_range_range() {
        assert_eq!(parse_id_range("3-6"), Some(vec![3, 4, 5, 6]));
    }

    #[test]
    fn test_parse_id_range_mixed() {
        assert_eq!(parse_id_range("3-5,9,11"), Some(vec![3, 4, 5, 9, 11]));
    }

    #[test]
    fn test_parse_id_range_invalid() {
        assert!(parse_id_range("abc").is_none());
        assert!(parse_id_range("6-3").is_none()); // reversed
    }

    #[test]
    fn test_looks_like_id_range() {
        assert!(looks_like_id_range("3-6,9"));
        assert!(looks_like_id_range("42"));
        assert!(!looks_like_id_range("status:playing"));
        assert!(!looks_like_id_range("scythe"));
        assert!(!looks_like_id_range(""));
    }
}
