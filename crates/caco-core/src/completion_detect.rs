//! Automated completion detection by comparing WAD analysis with play stats.
//!
//! Determines whether a player has completed all required maps in a WAD by
//! cross-referencing the structural analysis (which maps exist and which are
//! required) with per-map exit statistics from the sourceport.

use crate::wad_analysis::WadAnalysis;
use crate::wad_stats::WadStats;

/// Result of a completion check.
#[derive(Debug, Clone, PartialEq)]
pub enum CompletionVerdict {
    /// All required maps have been exited at least once.
    Complete,
    /// Some required maps have not been exited.
    Incomplete {
        /// Number of required maps the player has exited.
        exited: usize,
        /// Total number of required maps.
        required: usize,
    },
    /// No analysis data available to determine completion.
    NoAnalysis,
}

/// Check whether the player has completed a WAD.
///
/// Compares the WAD's structural analysis (required maps) against the player's
/// per-map statistics (exit counts) to determine completion status.
///
/// **Credits-map heuristic**: If the terminal map has zero exits in stats, the
/// preceding map has been exited, and the terminal is within 2 map slots of
/// the player's furthest progress, the terminal map is excluded from the
/// required set. This handles WADs where the final map is a credits/stopper
/// map that the player can't actually exit.
pub fn check_completion(analysis: &WadAnalysis, stats: &WadStats) -> CompletionVerdict {
    if analysis.maps.is_empty() {
        return CompletionVerdict::NoAnalysis;
    }

    // Build the set of required map lumps (reachable, non-secret, non-dead-end)
    let mut required_lumps: Vec<String> = analysis
        .maps
        .iter()
        .filter(|m| m.reachable && !m.is_secret && !m.is_dead_end && !m.is_terminal)
        .map(|m| m.lump.clone())
        .collect();

    // Include terminal map in required set initially (unless it's a dead-end)
    let terminal_is_dead_end = analysis
        .maps
        .iter()
        .any(|m| m.is_terminal && m.is_dead_end);

    if let Some(term) = &analysis.terminal_map
        && !terminal_is_dead_end
    {
        // Terminal map has exits, include it in required
        required_lumps.push(term.clone());
    }

    // Build a lookup from map lump name to exit count
    let stats_map: std::collections::HashMap<&str, i32> = stats
        .maps
        .iter()
        .map(|m| (m.lump.as_str(), m.total_exits))
        .collect();

    // Apply credits-map heuristic for terminal maps with exits
    // (dead-end terminals are already excluded from required)
    if let Some(term) = &analysis.terminal_map
        && !terminal_is_dead_end
    {
        let term_exits = stats_map.get(term.as_str()).copied().unwrap_or(0);
        if term_exits == 0 && should_exclude_terminal(analysis, &stats_map, term) {
            required_lumps.retain(|l| l != term);
        }
    }

    // Count how many required maps have been exited
    let exited = required_lumps
        .iter()
        .filter(|lump| stats_map.get(lump.as_str()).copied().unwrap_or(0) >= 1)
        .count();
    let required = required_lumps.len();

    // If no required maps were identified, the analysis is inconclusive.
    // (e.g. single-map WAD with ACS-only exit and no MAPINFO — the map is
    // classified as terminal+dead-end and filtered out, leaving required=0.
    // Returning Complete in that case would be a false positive.)
    if required == 0 {
        return CompletionVerdict::NoAnalysis;
    }

    if exited >= required {
        CompletionVerdict::Complete
    } else {
        CompletionVerdict::Incomplete { exited, required }
    }
}

/// Determine whether the terminal map should be excluded from required maps.
///
/// The terminal map is excluded when:
/// 1. Its exit count is zero in stats
/// 2. The preceding map has been exited at least once
/// 3. The terminal map is within 2 map slots of the player's furthest progress
fn should_exclude_terminal(
    analysis: &WadAnalysis,
    stats_map: &std::collections::HashMap<&str, i32>,
    terminal: &str,
) -> bool {
    // Find the map just before the terminal in the WAD's map ordering
    let map_lumps: Vec<&str> = analysis
        .maps
        .iter()
        .filter(|m| !m.is_secret)
        .map(|m| m.lump.as_str())
        .collect();

    let term_idx = match map_lumps.iter().position(|&m| m == terminal) {
        Some(idx) => idx,
        None => return false,
    };

    // Check condition 2: preceding map has been exited
    if term_idx == 0 {
        return false;
    }
    let preceding = map_lumps[term_idx - 1];
    let preceding_exits = stats_map.get(preceding).copied().unwrap_or(0);
    if preceding_exits < 1 {
        return false;
    }

    // Check condition 3: terminal is within 2 slots of furthest progress
    let furthest_idx = map_lumps
        .iter()
        .enumerate()
        .rev()
        .find(|(_, lump)| stats_map.get(*lump).copied().unwrap_or(0) >= 1)
        .map(|(idx, _)| idx);

    if let Some(furthest) = furthest_idx {
        // Terminal should be within 2 slots of the furthest exited map
        term_idx <= furthest + 2
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wad_analysis::MapInfo;
    use crate::wad_stats::MapStats;

    /// Create a simple WadAnalysis for testing.
    fn make_analysis(
        map_names: &[&str],
        secret: &[&str],
        dead_end: &[&str],
        terminal: Option<&str>,
    ) -> WadAnalysis {
        let secret_set: std::collections::HashSet<&str> = secret.iter().copied().collect();
        let dead_end_set: std::collections::HashSet<&str> = dead_end.iter().copied().collect();
        let terminal_name = terminal.map(|s| s.to_string());

        let maps: Vec<MapInfo> = map_names
            .iter()
            .map(|&name| {
                let is_secret = secret_set.contains(name);
                let is_dead = dead_end_set.contains(name);
                let is_term = terminal.map_or(false, |t| t == name);
                MapInfo {
                    lump: name.to_string(),
                    has_normal_exit: !is_dead,
                    has_secret_exit: false,
                    is_secret,
                    is_dead_end: is_dead,
                    is_terminal: is_term,
                    reachable: true,
                }
            })
            .collect();

        let required_maps = maps
            .iter()
            .filter(|m| {
                !m.is_secret && !m.is_dead_end && !(m.is_terminal && m.is_dead_end)
            })
            .count();

        WadAnalysis {
            version: 0,
            total_maps: maps.len(),
            required_maps,
            secret_maps: secret.iter().map(|s| s.to_string()).collect(),
            dead_end_maps: dead_end.iter().map(|s| s.to_string()).collect(),
            terminal_map: terminal_name,
            has_umapinfo: false,
            maps,
        }
    }

    /// Create a WadStats with given (lump, total_exits) pairs.
    fn make_stats(entries: &[(&str, i32)]) -> WadStats {
        WadStats {
            format: "stats.txt".to_string(),
            version: 1,
            header_total_kills: 0,
            maps: entries
                .iter()
                .map(|(lump, exits)| MapStats {
                    lump: lump.to_string(),
                    total_exits: *exits,
                    kills: 0,
                    total_kills: -1,
                    items: 0,
                    total_items: -1,
                    secrets: 0,
                    total_secrets: -1,
                    episode: 0,
                    map_num: 0,
                    best_skill: if *exits > 0 { 4 } else { 0 },
                    best_time: -1,
                    best_max_time: -1,
                    best_nm_time: -1,
                    cumulative_kills: 0,
                    time_secs: -1.0,
                    total_time_secs: -1.0,
                })
                .collect(),
        }
    }

    // -----------------------------------------------------------------------
    // Basic completion tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_complete_all_maps_exited() {
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03"],
            &[],
            &[],
            Some("MAP03"),
        );
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1), ("MAP03", 1)]);
        assert_eq!(check_completion(&analysis, &stats), CompletionVerdict::Complete);
    }

    #[test]
    fn test_incomplete_missing_maps() {
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03"],
            &[],
            &[],
            Some("MAP03"),
        );
        let stats = make_stats(&[("MAP01", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete {
                exited: 1,
                required: 3,
            }
        );
    }

    #[test]
    fn test_complete_with_secret_maps_skipped() {
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03", "MAP31", "MAP32"],
            &["MAP31", "MAP32"],
            &[],
            Some("MAP03"),
        );
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1), ("MAP03", 1)]);
        // Secret maps not required
        assert_eq!(check_completion(&analysis, &stats), CompletionVerdict::Complete);
    }

    #[test]
    fn test_complete_with_dead_end_excluded() {
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03"],
            &[],
            &["MAP03"],
            None,
        );
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1)]);
        // MAP03 is dead-end, not required
        assert_eq!(check_completion(&analysis, &stats), CompletionVerdict::Complete);
    }

    #[test]
    fn test_no_analysis_empty_maps() {
        let analysis = WadAnalysis {
            version: 0,
            maps: vec![],
            total_maps: 0,
            required_maps: 0,
            secret_maps: vec![],
            dead_end_maps: vec![],
            terminal_map: None,
            has_umapinfo: false,
        };
        let stats = make_stats(&[]);
        assert_eq!(check_completion(&analysis, &stats), CompletionVerdict::NoAnalysis);
    }

    // -----------------------------------------------------------------------
    // Credits-map heuristic tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_credits_map_heuristic_excludes_terminal() {
        // Terminal map has no exits in stats, preceding map exited,
        // terminal is within 2 slots of furthest progress.
        // Terminal map has exits in the linedef analysis (has_normal_exit=true)
        // but the player never managed to exit it (total_exits=0 in stats).
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03"],
            &[],
            &[],
            Some("MAP03"),
        );
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1), ("MAP03", 0)]);
        // MAP03 is terminal, player reached it (MAP02 exited), MAP03 within 2 slots
        // -> credits-map heuristic should exclude MAP03
        assert_eq!(check_completion(&analysis, &stats), CompletionVerdict::Complete);
    }

    #[test]
    fn test_credits_map_heuristic_not_applied_when_preceding_not_exited() {
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03"],
            &[],
            &[],
            Some("MAP03"),
        );
        // Player only exited MAP01, not MAP02 (the predecessor to terminal MAP03)
        let stats = make_stats(&[("MAP01", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete {
                exited: 1,
                required: 3,
            }
        );
    }

    #[test]
    fn test_credits_map_heuristic_not_applied_when_far_from_progress() {
        // Terminal is MAP30, player only got to MAP10 — too far away
        let map_names: Vec<String> = (1..=30).map(|i| format!("MAP{:02}", i)).collect();
        let map_refs: Vec<&str> = map_names.iter().map(|s| s.as_str()).collect();
        let analysis = make_analysis(&map_refs, &[], &[], Some("MAP30"));

        // Player exited MAP01-MAP10 only
        let stat_entries: Vec<(&str, i32)> = (1..=10)
            .map(|i| {
                let name: &str = map_refs[i - 1];
                (name, 1)
            })
            .collect();
        let stats = make_stats(&stat_entries);

        // MAP30 has 0 exits, MAP29 has 0 exits — preceding not exited
        // Credits heuristic should NOT apply
        let result = check_completion(&analysis, &stats);
        assert!(matches!(result, CompletionVerdict::Incomplete { .. }));
    }

    #[test]
    fn test_terminal_dead_end_already_excluded() {
        // Terminal map is a dead-end (no exit linedefs) — already excluded from required
        let mut analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03"],
            &[],
            &[],
            Some("MAP03"),
        );
        // Make MAP03 a dead-end terminal
        for m in &mut analysis.maps {
            if m.lump == "MAP03" {
                m.is_dead_end = true;
                m.has_normal_exit = false;
            }
        }
        analysis.dead_end_maps.clear(); // Dead-end list doesn't include terminal

        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1)]);
        assert_eq!(check_completion(&analysis, &stats), CompletionVerdict::Complete);
    }

    #[test]
    fn test_multiple_exits_count() {
        let analysis = make_analysis(
            &["MAP01", "MAP02"],
            &[],
            &[],
            Some("MAP02"),
        );
        // Multiple exits should still count
        let stats = make_stats(&[("MAP01", 5), ("MAP02", 3)]);
        assert_eq!(check_completion(&analysis, &stats), CompletionVerdict::Complete);
    }

    /// Integration test: check completion for a real WAD from the library.
    /// Run with: cargo test -p caco-core check_real_wad -- --ignored --nocapture
    #[test]
    #[ignore]
    fn check_real_wad() {
        use crate::utils::load_wad_data;
        use std::path::PathBuf;

        let db_path = dirs::data_dir().unwrap().join("caco/library.db");
        if !db_path.exists() {
            eprintln!("No library.db found, skipping");
            return;
        }
        let conn = crate::db::open_connection(&db_path).unwrap();

        // Check all playing WADs
        let wads = crate::db::search_wads(&conn, Some("play:started"), None, false, false, 100)
            .unwrap();
        for wad in &wads {
            let cached = match wad.cached_path.as_deref() {
                Some(p) if std::path::Path::new(p).exists() => p,
                _ => {
                    eprintln!("[{}] {} — no cached file", wad.id, wad.title);
                    continue;
                }
            };
            let path = PathBuf::from(cached);
            let is_pk3 = path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("pk3"));

            let analysis = if is_pk3 {
                crate::wad_analysis::analyze_pk3(&path)
            } else {
                let wad_data = match load_wad_data(&path) {
                    Some(d) => d,
                    None => {
                        eprintln!("[{}] {} — could not load WAD data", wad.id, wad.title);
                        continue;
                    }
                };
                crate::wad_analysis::analyze_wad(&wad_data)
            };
            let analysis = match analysis {
                Some(a) => a,
                None => {
                    eprintln!("[{}] {} — analysis returned None", wad.id, wad.title);
                    continue;
                }
            };

            eprintln!("\n[{}] {}", wad.id, wad.title);
            let unreachable: Vec<&str> = analysis.maps.iter()
                .filter(|m| !m.reachable).map(|m| m.lump.as_str()).collect();
            eprintln!("  total={} required={} secret={:?} terminal={:?} unreachable={:?}",
                analysis.total_maps, analysis.required_maps,
                analysis.secret_maps, analysis.terminal_map, unreachable);
            for m in &analysis.maps {
                let r = if m.reachable { "" } else { " UNREACHABLE" };
                eprintln!("  {:8} exit={} secret_exit={} secret={} dead_end={} terminal={}{}",
                    m.lump, m.has_normal_exit, m.has_secret_exit,
                    m.is_secret, m.is_dead_end, m.is_terminal, r);
            }

            if let Some(ref ss) = wad.stats_snapshot {
                let stats: crate::wad_stats::WadStats = serde_json::from_str(ss).unwrap();
                let verdict = check_completion(&analysis, &stats);
                eprintln!("  Verdict: {:?}", verdict);
            } else {
                eprintln!("  No stats snapshot");
            }
        }
    }

    #[test]
    fn test_no_stats_for_any_map() {
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03"],
            &[],
            &[],
            Some("MAP03"),
        );
        let stats = make_stats(&[]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete {
                exited: 0,
                required: 3,
            }
        );
    }
}
