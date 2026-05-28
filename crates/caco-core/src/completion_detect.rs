//! Verdict layer: intersect the classifier's Required set with the player's
//! exit stats. The classifier is the single source of truth for what a WAD
//! requires; this layer only checks whether those maps were exited.

use std::collections::HashSet;

use crate::wad_analysis::{MapClassification, WadAnalysis};
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
/// `Complete` iff every map with `MapClassification::Required` has at least
/// one exit recorded in the stats. The classifier's classification is
/// authoritative — this function never inspects exit counts to decide what
/// is required.
pub fn check_completion(analysis: &WadAnalysis, stats: &WadStats) -> CompletionVerdict {
    if analysis.maps.is_empty() {
        return CompletionVerdict::NoAnalysis;
    }

    let required: Vec<&str> = analysis
        .maps
        .iter()
        .filter(|m| m.classification == MapClassification::Required)
        .map(|m| m.lump.as_str())
        .collect();

    if required.is_empty() {
        // No structural Required set — classifier was inconclusive (e.g. a
        // single-map WAD with no detectable exit and no MAPINFO data).
        return CompletionVerdict::NoAnalysis;
    }

    let exited: HashSet<&str> = stats
        .maps
        .iter()
        .filter(|m| m.total_exits >= 1)
        .map(|m| m.lump.as_str())
        .collect();

    let satisfied = required.iter().filter(|l| exited.contains(*l)).count();

    if satisfied == required.len() {
        CompletionVerdict::Complete
    } else {
        CompletionVerdict::Incomplete {
            exited: satisfied,
            required: required.len(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wad_analysis::{ANALYSIS_VERSION, MapInfo};
    use crate::wad_stats::MapStats;

    /// Build a synthetic WadAnalysis from `(lump, classification)` pairs.
    fn make_analysis(maps: &[(&str, MapClassification)]) -> WadAnalysis {
        let infos: Vec<MapInfo> = maps
            .iter()
            .map(|(lump, c)| MapInfo {
                lump: lump.to_string(),
                classification: *c,
            })
            .collect();
        let total_maps = infos.len();
        let required_maps = infos
            .iter()
            .filter(|m| m.classification == MapClassification::Required)
            .count();
        let secret_maps: Vec<String> = infos
            .iter()
            .filter(|m| m.classification == MapClassification::OptionalSecret)
            .map(|m| m.lump.clone())
            .collect();
        let terminal_map = infos
            .iter()
            .find(|m| m.classification == MapClassification::OptionalCredits)
            .map(|m| m.lump.clone());
        WadAnalysis {
            version: ANALYSIS_VERSION,
            total_maps,
            required_maps,
            secret_maps,
            terminal_map,
            has_umapinfo: false,
            maps: infos,
        }
    }

    fn make_stats(entries: &[(&str, i32)]) -> WadStats {
        WadStats {
            format: "test".to_string(),
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
    // Basic completion
    // -----------------------------------------------------------------------

    #[test]
    fn complete_when_all_required_exited() {
        let analysis = make_analysis(&[
            ("MAP01", MapClassification::Required),
            ("MAP02", MapClassification::Required),
            ("MAP03", MapClassification::OptionalCredits),
        ]);
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn incomplete_when_required_unexited() {
        let analysis = make_analysis(&[
            ("MAP01", MapClassification::Required),
            ("MAP02", MapClassification::Required),
            ("MAP03", MapClassification::Required),
        ]);
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
    fn optional_credits_not_required_to_exit() {
        // MAP30-style stopper: classifier marks it OptionalCredits, so it
        // doesn't need a recorded exit even though stats show 0.
        let analysis = make_analysis(&[
            ("MAP01", MapClassification::Required),
            ("MAP30", MapClassification::OptionalCredits),
        ]);
        let stats = make_stats(&[("MAP01", 1), ("MAP30", 0)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn optional_secrets_not_required() {
        let analysis = make_analysis(&[
            ("MAP01", MapClassification::Required),
            ("MAP02", MapClassification::Required),
            ("MAP31", MapClassification::OptionalSecret),
            ("MAP32", MapClassification::OptionalSecret),
        ]);
        let stats = make_stats(&[("MAP01", 1), ("MAP02", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn unreachable_maps_ignored() {
        // A lump exists in the WAD but isn't on any flow — verdict ignores it.
        let analysis = make_analysis(&[
            ("MAP01", MapClassification::Required),
            ("MAP18", MapClassification::Unreachable),
        ]);
        let stats = make_stats(&[("MAP01", 1), ("MAP18", 0)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    #[test]
    fn empty_analysis_is_no_analysis() {
        let analysis = make_analysis(&[]);
        let stats = make_stats(&[]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::NoAnalysis
        );
    }

    #[test]
    fn no_required_maps_is_no_analysis() {
        // Single-map WAD where the only map is OptionalCredits → inconclusive
        let analysis = make_analysis(&[("MAP01", MapClassification::OptionalCredits)]);
        let stats = make_stats(&[("MAP01", 1)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::NoAnalysis
        );
    }

    #[test]
    fn missing_stats_for_required_is_incomplete() {
        let analysis = make_analysis(&[
            ("MAP01", MapClassification::Required),
            ("MAP02", MapClassification::Required),
        ]);
        let stats = make_stats(&[]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Incomplete {
                exited: 0,
                required: 2,
            }
        );
    }

    #[test]
    fn multiple_exits_count_once() {
        let analysis = make_analysis(&[("MAP01", MapClassification::Required)]);
        let stats = make_stats(&[("MAP01", 5)]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    // -----------------------------------------------------------------------
    // Vertex Relocation regression: MAP18GZ used in ZMAPINFO flow.
    // The classifier marks MAP18 = Unreachable and MAP18GZ = Required.
    // Player's stats show MAP18=0, MAP18GZ=1. Verdict must be Complete.
    // -----------------------------------------------------------------------

    #[test]
    fn vertex_relocation_zmapinfo_alternate_lump_completes() {
        let analysis = make_analysis(&[
            ("MAP01", MapClassification::Required),
            ("MAP17", MapClassification::Required),
            ("MAP18", MapClassification::Unreachable),
            ("MAP18GZ", MapClassification::Required),
            ("MAP19", MapClassification::Required),
            ("MAP30", MapClassification::OptionalCredits),
            ("MAP31", MapClassification::OptionalSecret),
            ("MAP32", MapClassification::OptionalSecret),
        ]);
        let stats = make_stats(&[
            ("MAP01", 1),
            ("MAP17", 1),
            ("MAP18", 0),
            ("MAP18GZ", 1),
            ("MAP19", 1),
            ("MAP30", 0),
            ("MAP31", 0),
            ("MAP32", 0),
        ]);
        assert_eq!(
            check_completion(&analysis, &stats),
            CompletionVerdict::Complete
        );
    }

    /// Integration: scan all WADs in the user's library and report verdicts.
    /// Run with: cargo test -p caco-core check_real_wads -- --ignored --nocapture
    #[test]
    #[ignore]
    fn check_real_wads() {
        use std::path::PathBuf;

        let db_path = dirs::data_dir().unwrap().join("caco/library.db");
        if !db_path.exists() {
            eprintln!("No library.db found, skipping");
            return;
        }
        let conn = crate::db::open_connection(&db_path).unwrap();
        let wads = crate::db::search_wads(&conn, None, None, false, false, 500).unwrap();
        for wad in &wads {
            let cached = match wad.cached_path.as_deref() {
                Some(p) if std::path::Path::new(p).exists() => p,
                _ => {
                    eprintln!("[{}] {} — no cached file", wad.id, wad.title);
                    continue;
                }
            };
            let path = PathBuf::from(cached);
            let analysis = match crate::wad_analysis::analyze_path(&path) {
                Some(a) => a,
                None => {
                    eprintln!("[{}] {} — analysis returned None", wad.id, wad.title);
                    continue;
                }
            };

            eprintln!(
                "\n[{}] {} — required={} terminal={:?}",
                wad.id, wad.title, analysis.required_maps, analysis.terminal_map
            );
            for m in &analysis.maps {
                eprintln!("  {:10} {:?}", m.lump, m.classification);
            }
            if let Some(ref ss) = wad.stats_snapshot {
                let stats: crate::wad_stats::WadStats = serde_json::from_str(ss).unwrap();
                eprintln!("  Verdict: {:?}", check_completion(&analysis, &stats));
            } else {
                eprintln!("  No stats snapshot");
            }
        }
    }
}
