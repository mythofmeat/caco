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
/// Terminal maps need special care: reaching or exiting the preceding map is
/// not proof that a normal final map has been finished. If the terminal map's
/// only exit is a secret exit, the secret continuation is part of the required
/// path rather than optional completion content.
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
    let terminal_is_dead_end = analysis.maps.iter().any(|m| m.is_terminal && m.is_dead_end);

    if let Some(term) = &analysis.terminal_map
        && !terminal_is_dead_end
    {
        // Terminal map has exits, include it in required
        required_lumps.push(term.clone());
    }

    let terminal_secret_continuations = if let Some(term) = &analysis.terminal_map
        && !terminal_is_dead_end
        && terminal_has_secret_exit(analysis, term)
        && !terminal_has_normal_exit(analysis, term)
    {
        secret_continuation_maps(analysis, term)
    } else {
        Vec::new()
    };
    for lump in &terminal_secret_continuations {
        if !required_lumps.contains(lump) {
            required_lumps.push(lump.clone());
        }
    }

    // Build a lookup from map lump name to exit count
    let stats_map: std::collections::HashMap<&str, i32> = stats
        .maps
        .iter()
        .map(|m| (m.lump.as_str(), m.total_exits))
        .collect();

    // Bypassed-map heuristic: if the terminal map has been exited, any other
    // non-continuation required map with zero exits was bypassed by the WAD's
    // actual play flow.
    // Static analysis can't always reconstruct DEHACKED-patched progressions or
    // custom exit handling (e.g. Pina Colada 2's MAP27 is an orphan slot that
    // MAP26's patched exit skips over to reach MAP28). Treat reaching the
    // credits as the authoritative completion signal, except when the terminal's
    // only exit leads into a required secret continuation.
    if let Some(term) = &analysis.terminal_map
        && !terminal_is_dead_end
    {
        if map_exited(&stats_map, term) {
            required_lumps.retain(|lump| {
                lump == term
                    || terminal_secret_continuations.contains(lump)
                    || map_exited(&stats_map, lump)
            });
        }
    }

    // Count how many required maps have been exited
    let exited = required_lumps
        .iter()
        .filter(|lump| map_exited(&stats_map, lump))
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

fn map_exited(stats_map: &std::collections::HashMap<&str, i32>, lump: &str) -> bool {
    stats_map.get(lump).copied().unwrap_or(0) >= 1
}

fn terminal_has_normal_exit(analysis: &WadAnalysis, terminal: &str) -> bool {
    analysis
        .maps
        .iter()
        .find(|m| m.lump == terminal)
        .is_some_and(|m| m.has_normal_exit)
}

fn terminal_has_secret_exit(analysis: &WadAnalysis, terminal: &str) -> bool {
    analysis
        .maps
        .iter()
        .find(|m| m.lump == terminal)
        .is_some_and(|m| m.has_secret_exit)
}

fn secret_continuation_maps(analysis: &WadAnalysis, terminal: &str) -> Vec<String> {
    let Some(term_idx) = analysis.maps.iter().position(|m| m.lump == terminal) else {
        return Vec::new();
    };

    analysis
        .maps
        .iter()
        .skip(term_idx + 1)
        .filter(|m| m.reachable && m.is_secret && !m.is_dead_end)
        .map(|m| m.lump.clone())
        .collect()
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
                let is_term = terminal == Some(name);
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
            .filter(|m| !m.is_secret && !m.is_dead_end)
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
        make_stats_with_format("test", entries)
    }

    fn make_stats_with_format(format: &str, entries: &[(&str, i32)]) -> WadStats {
        WadStats {
            format: format.to_string(),
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
        let analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &[], Some("MAP03"));
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1), ("MAP03", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_incomplete_missing_maps() {
        let analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &[], Some("MAP03"));
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
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_complete_with_dead_end_excluded() {
        let analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &["MAP03"], None);
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1)]);
        // MAP03 is dead-end, not required
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
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
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::NoAnalysis
        );
    }

    // -----------------------------------------------------------------------
    // Terminal-map tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_normal_terminal_not_excluded_when_unexited() {
        // A normal terminal map with no exit stats is still required. Exiting
        // the preceding map only proves the player reached the finale, not
        // that the finale was finished.
        let analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &[], Some("MAP03"));
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1), ("MAP03", 0)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete {
                exited: 2,
                required: 3,
            }
        );
    }

    #[test]
    fn test_stats_txt_terminal_exit_completes() {
        let analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &[], Some("MAP03"));
        let stats =
            make_stats_with_format("stats_txt", &[("MAP01", 1), ("MAP02", 1), ("MAP03", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_levelstat_terminal_exit_completes() {
        let analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &[], Some("MAP03"));
        let stats =
            make_stats_with_format("levelstat_txt", &[("MAP01", 1), ("MAP02", 1), ("MAP03", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_frozen_heart_map29_snapshot_stays_incomplete() {
        let map_names: Vec<String> = (1..=32).map(|i| format!("MAP{i:02}")).collect();
        let map_refs: Vec<&str> = map_names.iter().map(|s| s.as_str()).collect();
        let analysis = make_analysis(&map_refs, &["MAP31", "MAP32"], &[], Some("MAP30"));

        let stat_owned: Vec<(String, i32)> = (1..=29)
            .map(|i| (format!("MAP{i:02}"), 1))
            .chain((30..=32).map(|i| (format!("MAP{i:02}"), 0)))
            .collect();
        let stat_entries: Vec<(&str, i32)> =
            stat_owned.iter().map(|(l, n)| (l.as_str(), *n)).collect();
        let stats = make_stats_with_format("stats_txt", &stat_entries);

        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete {
                exited: 29,
                required: 30,
            }
        );
    }

    #[test]
    fn test_frozen_heart_map30_stats_txt_completes() {
        let map_names: Vec<String> = (1..=32).map(|i| format!("MAP{i:02}")).collect();
        let map_refs: Vec<&str> = map_names.iter().map(|s| s.as_str()).collect();
        let analysis = make_analysis(&map_refs, &["MAP31", "MAP32"], &[], Some("MAP30"));

        let stat_owned: Vec<(String, i32)> = (1..=30)
            .map(|i| (format!("MAP{i:02}"), 1))
            .chain((31..=32).map(|i| (format!("MAP{i:02}"), 0)))
            .collect();
        let stat_entries: Vec<(&str, i32)> =
            stat_owned.iter().map(|(l, n)| (l.as_str(), *n)).collect();
        let stats = make_stats_with_format("stats_txt", &stat_entries);

        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_terminal_secret_continuation_not_complete_after_predecessor() {
        // Reproduces Formless Mother: MAP04 is a stopper terminal with no
        // normal exit, but it has a secret exit to MAP31. Because that is the
        // only continuation, MAP31 is part of the required completion path.
        let mut analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03", "MAP04", "MAP31"],
            &["MAP31"],
            &["MAP03"],
            Some("MAP04"),
        );
        for map in &mut analysis.maps {
            if map.lump == "MAP04" {
                map.has_normal_exit = false;
                map.has_secret_exit = true;
                map.is_dead_end = false;
                map.reachable = true;
            }
        }

        let stats = make_stats(&[
            ("MAP01", 1),
            ("MAP02", 1),
            ("MAP03", 1),
            ("MAP04", 0),
            ("MAP31", 0),
        ]);

        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete {
                exited: 2,
                required: 4,
            }
        );
    }

    #[test]
    fn test_terminal_secret_continuation_completes_after_secret_map_exit() {
        let mut analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03", "MAP04", "MAP31"],
            &["MAP31"],
            &["MAP03"],
            Some("MAP04"),
        );
        for map in &mut analysis.maps {
            if map.lump == "MAP04" {
                map.has_normal_exit = false;
                map.has_secret_exit = true;
                map.is_dead_end = false;
                map.reachable = true;
            }
        }

        let stats = make_stats(&[
            ("MAP01", 1),
            ("MAP02", 1),
            ("MAP03", 1),
            ("MAP04", 1),
            ("MAP31", 1),
        ]);

        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_terminal_secret_continuation_not_bypassed_after_terminal_exit() {
        let mut analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03", "MAP04", "MAP31"],
            &["MAP31"],
            &["MAP03"],
            Some("MAP04"),
        );
        for map in &mut analysis.maps {
            if map.lump == "MAP04" {
                map.has_normal_exit = false;
                map.has_secret_exit = true;
                map.is_dead_end = false;
                map.reachable = true;
            }
        }

        let stats = make_stats(&[
            ("MAP01", 1),
            ("MAP02", 1),
            ("MAP03", 1),
            ("MAP04", 1),
            ("MAP31", 0),
        ]);

        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete {
                exited: 3,
                required: 4,
            }
        );
    }

    #[test]
    fn test_terminal_incomplete_when_preceding_not_exited() {
        let analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &[], Some("MAP03"));
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
    fn test_terminal_incomplete_when_far_from_progress() {
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

        // MAP30 has 0 exits, MAP29 has 0 exits — preceding not exited.
        let result = check_completion(&analysis, &stats);
        assert!(matches!(result, CompletionVerdict::Incomplete { .. }));
    }

    #[test]
    fn test_terminal_dead_end_already_excluded() {
        // Terminal map is a dead-end (no exit linedefs) — already excluded from required
        let mut analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &[], Some("MAP03"));
        // Make MAP03 a dead-end terminal
        for m in &mut analysis.maps {
            if m.lump == "MAP03" {
                m.is_dead_end = true;
                m.has_normal_exit = false;
            }
        }
        analysis.dead_end_maps.clear(); // Dead-end list doesn't include terminal

        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_multiple_exits_count() {
        let analysis = make_analysis(&["MAP01", "MAP02"], &[], &[], Some("MAP02"));
        // Multiple exits should still count
        let stats = make_stats(&[("MAP01", 5), ("MAP02", 3)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
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
        let wads =
            crate::db::search_wads(&conn, Some("play:started"), None, false, false, 100).unwrap();
        for wad in &wads {
            let cached = match wad.cached_path.as_deref() {
                Some(p) if std::path::Path::new(p).exists() => p,
                _ => {
                    eprintln!("[{}] {} — no cached file", wad.id, wad.title);
                    continue;
                }
            };
            let path = PathBuf::from(cached);
            let is_pk3 = path
                .extension()
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
            let unreachable: Vec<&str> = analysis
                .maps
                .iter()
                .filter(|m| !m.reachable)
                .map(|m| m.lump.as_str())
                .collect();
            eprintln!(
                "  total={} required={} secret={:?} terminal={:?} unreachable={:?}",
                analysis.total_maps,
                analysis.required_maps,
                analysis.secret_maps,
                analysis.terminal_map,
                unreachable
            );
            for m in &analysis.maps {
                let r = if m.reachable { "" } else { " UNREACHABLE" };
                eprintln!(
                    "  {:8} exit={} secret_exit={} secret={} dead_end={} terminal={}{}",
                    m.lump,
                    m.has_normal_exit,
                    m.has_secret_exit,
                    m.is_secret,
                    m.is_dead_end,
                    m.is_terminal,
                    r
                );
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
    fn test_bypassed_intermediate_map_excluded_when_terminal_exited() {
        // Reproduces Pina Colada 2 (WAD id:61): 28 map slots exist in the WAD
        // but only 27 are in the real play flow. MAP27 is an orphan slot that
        // MAP26's patched exit skips over. Player exited MAP01-MAP26 + MAP28
        // (the credits map); MAP27 has zero exits because it's unreachable in
        // normal play. Auto-completion should fire because the terminal map
        // has been exited.
        let map_names: Vec<String> = (1..=28).map(|i| format!("MAP{i:02}")).collect();
        let map_refs: Vec<&str> = map_names.iter().map(|s| s.as_str()).collect();
        let analysis = make_analysis(&map_refs, &[], &[], Some("MAP28"));

        let stat_owned: Vec<(String, i32)> = (1..=26)
            .map(|i| (format!("MAP{i:02}"), 1))
            .chain(std::iter::once(("MAP27".to_string(), 0)))
            .chain(std::iter::once(("MAP28".to_string(), 1)))
            .collect();
        let stat_entries: Vec<(&str, i32)> =
            stat_owned.iter().map(|(l, n)| (l.as_str(), *n)).collect();
        let stats = make_stats(&stat_entries);

        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_bypass_excludes_multiple_zero_exit_maps() {
        // Two orphan map slots in the WAD (MAP27 and MAP29), terminal MAP30
        // reached by the player. Both orphans should be excluded.
        let map_names: Vec<String> = (1..=30).map(|i| format!("MAP{i:02}")).collect();
        let map_refs: Vec<&str> = map_names.iter().map(|s| s.as_str()).collect();
        let analysis = make_analysis(&map_refs, &[], &[], Some("MAP30"));

        // Exit everything except MAP27 and MAP29
        let stat_owned: Vec<(String, i32)> = (1..=30)
            .filter(|i| *i != 27 && *i != 29)
            .map(|i| (format!("MAP{i:02}"), 1))
            .chain(std::iter::once(("MAP27".to_string(), 0)))
            .chain(std::iter::once(("MAP29".to_string(), 0)))
            .collect();
        let stat_entries: Vec<(&str, i32)> =
            stat_owned.iter().map(|(l, n)| (l.as_str(), *n)).collect();
        let stats = make_stats(&stat_entries);

        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_bypass_preserves_secret_map_exclusion() {
        // Secret maps (MAP31/MAP32) are already filtered before the bypass
        // heuristic runs — whether they have exits or not shouldn't matter.
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03", "MAP31", "MAP32"],
            &["MAP31", "MAP32"],
            &[],
            Some("MAP03"),
        );
        // All main maps exited, secrets untouched
        let stats = make_stats(&[
            ("MAP01", 1),
            ("MAP02", 1),
            ("MAP03", 1),
            ("MAP31", 0),
            ("MAP32", 0),
        ]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_bypass_warp_to_terminal_only_completes() {
        // Known trade-off: if the player warps straight to the terminal and
        // exits it, the bypass heuristic excludes every other required map
        // and marks the WAD complete. This documents the current behavior so
        // any future tightening is a conscious choice, not an accident.
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03", "MAP04", "MAP05"],
            &[],
            &[],
            Some("MAP05"),
        );
        // Only terminal exited via warp
        let stats = make_stats(&[
            ("MAP01", 0),
            ("MAP02", 0),
            ("MAP03", 0),
            ("MAP04", 0),
            ("MAP05", 1),
        ]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_bypass_no_terminal_map_no_effect() {
        // Analysis with no terminal (rare but possible — e.g. single-map WAD
        // with no endgame markers). The bypass block is guarded by
        // `terminal_map`; a missing terminal means the heuristic never fires.
        let analysis = WadAnalysis {
            version: 0,
            total_maps: 3,
            required_maps: 3,
            secret_maps: vec![],
            dead_end_maps: vec![],
            terminal_map: None,
            has_umapinfo: false,
            maps: ["MAP01", "MAP02", "MAP03"]
                .iter()
                .map(|&n| MapInfo {
                    lump: n.to_string(),
                    has_normal_exit: true,
                    has_secret_exit: false,
                    is_secret: false,
                    is_dead_end: false,
                    is_terminal: false,
                    reachable: true,
                })
                .collect(),
        };
        // MAP03 unexited — without a terminal, the heuristic can't fire, so
        // this must still read as Incomplete.
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1), ("MAP03", 0)]);
        assert!(matches!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete { .. }
        ));
    }

    #[test]
    fn test_bypass_preserves_dead_end_exclusion() {
        // A dead-end map (e.g. MAP08 in PC2) is filtered before the bypass
        // heuristic runs. Even though its exit count is irrelevant to the
        // required set, this sanity-checks that the heuristic doesn't
        // re-introduce dead-ends by accident.
        let analysis = make_analysis(
            &["MAP01", "MAP02", "MAP03", "MAP04"],
            &[],
            &["MAP02"],
            Some("MAP04"),
        );
        // MAP02 is dead-end (excluded). MAP03 was never entered. Player
        // warped to MAP04 and exited.
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 0), ("MAP03", 0), ("MAP04", 1)]);
        // Required before heuristic: MAP01, MAP03, MAP04 (MAP02 is dead-end).
        // Bypass fires (terminal exited): MAP03 removed. MAP01 retained.
        // Required = {MAP01, MAP04}, Exited = 2 → Complete.
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn test_bypassed_heuristic_not_applied_when_terminal_not_exited() {
        // Same orphan-slot shape as the PC2 case, but the player hasn't
        // reached the credits map yet. The heuristic must stay conservative
        // mid-playthrough: MAP27 with zero exits still counts as missing.
        let map_names: Vec<String> = (1..=28).map(|i| format!("MAP{i:02}")).collect();
        let map_refs: Vec<&str> = map_names.iter().map(|s| s.as_str()).collect();
        let analysis = make_analysis(&map_refs, &[], &[], Some("MAP28"));

        let stat_owned: Vec<(String, i32)> = (1..=25).map(|i| (format!("MAP{i:02}"), 1)).collect();
        let stat_entries: Vec<(&str, i32)> =
            stat_owned.iter().map(|(l, n)| (l.as_str(), *n)).collect();
        let stats = make_stats(&stat_entries);

        assert!(matches!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete { .. }
        ));
    }

    #[test]
    fn test_no_stats_for_any_map() {
        let analysis = make_analysis(&["MAP01", "MAP02", "MAP03"], &[], &[], Some("MAP03"));
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
