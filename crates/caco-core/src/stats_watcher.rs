//! ZDoom-family stats collection via custom PK3 mod.
//!
//! ZDoom-family sourceports (gzdoom, uzdoom, etc.) don't natively write
//! per-map stats files like dsda-doom does. This module bridges the gap by:
//!
//! 1. Ensuring a small ZScript PK3 mod exists that logs per-map stats via
//!    `Console.PrintfEx(PRINT_LOG, ...)` — written to the ZDoom log file.
//! 2. Injecting `-file <pk3> +logfile <path>` into the sourceport command.
//! 3. After the sourceport exits, parsing the log for `CACOSTATS|…` lines
//!    and writing a `levelstat.txt` that the existing stats infrastructure
//!    can consume.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::config;
use crate::wad_stats::{self, MapStats, TICS_PER_SECOND, WadStats};

// ---------------------------------------------------------------------------
// PK3 mod management
// ---------------------------------------------------------------------------

const ZSCRIPT_ZS: &str = r#"version "4.0"

class CacoStatsReporter : EventHandler
{
    int tickCounter;

    override void WorldTick()
    {
        tickCounter++;
        // Report every 35 ticks (once per second)
        if (tickCounter % 35 == 0)
        {
            ReportStats();
        }
    }

    override void WorldUnloaded(WorldEvent e)
    {
        ReportStats();
    }

    void ReportStats()
    {
        int sk = G_SkillPropertyInt(SKILLP_ACSReturn);
        Console.PrintfEx(PRINT_LOG, "CACOSTATS|%s|%d|%d|%d/%d|%d/%d|%d/%d",
            level.MapName,
            sk,
            level.maptime,
            level.killed_monsters, level.total_monsters,
            level.found_items, level.total_items,
            level.found_secrets, level.total_secrets
        );
    }
}
"#;

const MAPINFO: &str = r#"GameInfo
{
    AddEventHandlers = "CacoStatsReporter"
}
"#;

/// Get the directory where caco stores its mods.
fn get_mods_dir() -> PathBuf {
    config::default_data_dir().join("mods")
}

/// Get the path to the stats reporter PK3 mod.
pub fn get_stats_mod_path() -> PathBuf {
    get_mods_dir().join("caco_stats_reporter.pk3")
}

/// Ensure the stats reporter PK3 mod exists, creating it if necessary.
///
/// Returns the path to the PK3 file.
pub fn ensure_stats_mod() -> crate::Result<PathBuf> {
    let pk3_path = get_stats_mod_path();

    if pk3_path.exists() {
        return Ok(pk3_path);
    }

    let mods_dir = get_mods_dir();
    std::fs::create_dir_all(&mods_dir)?;

    let file = std::fs::File::create(&pk3_path)?;
    let mut zip = zip::ZipWriter::new(file);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("zscript.zs", options)
        .map_err(std::io::Error::other)?;
    zip.write_all(ZSCRIPT_ZS.as_bytes())?;

    zip.start_file("MAPINFO", options)
        .map_err(std::io::Error::other)?;
    zip.write_all(MAPINFO.as_bytes())?;

    zip.finish().map_err(std::io::Error::other)?;

    Ok(pk3_path)
}

// ---------------------------------------------------------------------------
// Launch args
// ---------------------------------------------------------------------------

/// Name of the log file written by ZDoom's `+logfile` command.
pub const LOG_FILENAME: &str = "caco_stats.log";

/// Return extra args to inject for zdoom-family stats collection.
///
/// Returns `["-file", "<pk3_path>", "+logfile", "<log_path>"]` on success,
/// or an empty vec if the mod can't be created.
pub fn get_zdoom_stats_args(data_dir: &Path) -> Vec<String> {
    let pk3_path = match ensure_stats_mod() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let log_path = data_dir.join(LOG_FILENAME);

    vec![
        "-file".to_string(),
        pk3_path.to_string_lossy().into_owned(),
        "+logfile".to_string(),
        log_path.to_string_lossy().into_owned(),
    ]
}

// ---------------------------------------------------------------------------
// Log parsing
// ---------------------------------------------------------------------------

/// Parsed stats for a single map from one CACOSTATS log line.
#[derive(Debug, Clone)]
struct MapLogEntry {
    lump: String,
    #[allow(dead_code)]
    skill: i32,
    time_tics: i32,
    kills: i32,
    total_kills: i32,
    items: i32,
    total_items: i32,
    secrets: i32,
    total_secrets: i32,
}

// CACOSTATS|MAP01|3|1234|50/100|10/20|3/5
static CACOSTATS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"CACOSTATS\|([^|]+)\|(\d+)\|(\d+)\|(\d+)/(\d+)\|(\d+)/(\d+)\|(\d+)/(\d+)").unwrap()
});

/// Parse a ZDoom log file for CACOSTATS lines.
///
/// Returns the last (most up-to-date) entry for each map, preserving
/// the order maps were first seen.
fn parse_log(text: &str) -> Vec<MapLogEntry> {
    let mut latest: HashMap<String, MapLogEntry> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for line in text.lines() {
        if let Some(caps) = CACOSTATS_RE.captures(line) {
            let lump = caps[1].to_string();

            if !latest.contains_key(&lump) {
                order.push(lump.clone());
            }

            latest.insert(
                lump.clone(),
                MapLogEntry {
                    lump,
                    skill: caps[2].parse().unwrap_or(0),
                    time_tics: caps[3].parse().unwrap_or(0),
                    kills: caps[4].parse().unwrap_or(0),
                    total_kills: caps[5].parse().unwrap_or(0),
                    items: caps[6].parse().unwrap_or(0),
                    total_items: caps[7].parse().unwrap_or(0),
                    secrets: caps[8].parse().unwrap_or(0),
                    total_secrets: caps[9].parse().unwrap_or(0),
                },
            );
        }
    }

    order
        .into_iter()
        .filter_map(|lump| latest.remove(&lump))
        .collect()
}

/// Convert parsed log entries to a `WadStats` struct.
fn entries_to_wad_stats(entries: &[MapLogEntry]) -> WadStats {
    let mut maps = Vec::new();
    let mut cumulative_secs = 0.0;

    for entry in entries {
        let map_secs = entry.time_tics as f64 / TICS_PER_SECOND;
        cumulative_secs += map_secs;

        maps.push(MapStats {
            lump: entry.lump.clone(),
            kills: entry.kills,
            total_kills: entry.total_kills,
            items: entry.items,
            total_items: entry.total_items,
            secrets: entry.secrets,
            total_secrets: entry.total_secrets,
            best_skill: entry.skill + 1, // ACS skill is 0-indexed, stats uses 1-indexed
            best_time: entry.time_tics,
            total_exits: 1,
            time_secs: map_secs,
            total_time_secs: cumulative_secs,
            // Fields not available from zdoom log
            episode: 0,
            map_num: 0,
            best_max_time: -1,
            best_nm_time: -1,
            cumulative_kills: 0,
        });
    }

    WadStats {
        format: "levelstat_txt".to_string(),
        maps,
        version: 1,
        header_total_kills: 0,
    }
}

// ---------------------------------------------------------------------------
// Post-play collection
// ---------------------------------------------------------------------------

/// After a zdoom-family sourceport exits, parse the log and write
/// a `levelstat.txt` file in the data directory.
///
/// If a `levelstat.txt` already exists (from a prior session), the new
/// stats are merged with the old — keeping the best values per map, just
/// like dsda-doom's cumulative `stats.txt`.
///
/// Returns `true` if stats were successfully written.
pub fn collect_zdoom_stats(data_dir: &Path) -> bool {
    let log_path = data_dir.join(LOG_FILENAME);
    if !log_path.exists() {
        return false;
    }

    let text = match std::fs::read_to_string(&log_path) {
        Ok(t) => t,
        Err(_) => return false,
    };

    let entries = parse_log(&text);
    if entries.is_empty() {
        return false;
    }

    let new_stats = entries_to_wad_stats(&entries);

    // Merge with existing levelstat.txt if present
    let levelstat_path = data_dir.join("levelstat.txt");
    let merged = if levelstat_path.exists() {
        if let Ok(existing) = wad_stats::parse_stats_file(&levelstat_path) {
            wad_stats::merge_stats(&[existing, new_stats])
        } else {
            new_stats
        }
    } else {
        new_stats
    };

    let output = wad_stats::format_stats(&merged);
    std::fs::write(&levelstat_path, &output).is_ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_log_single_map() {
        let log = "Some engine output\n\
                    CACOSTATS|MAP01|3|3500|50/100|10/20|3/5\n\
                    More output\n";
        let entries = parse_log(log);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].lump, "MAP01");
        assert_eq!(entries[0].skill, 3);
        assert_eq!(entries[0].time_tics, 3500);
        assert_eq!(entries[0].kills, 50);
        assert_eq!(entries[0].total_kills, 100);
        assert_eq!(entries[0].items, 10);
        assert_eq!(entries[0].total_items, 20);
        assert_eq!(entries[0].secrets, 3);
        assert_eq!(entries[0].total_secrets, 5);
    }

    #[test]
    fn test_parse_log_keeps_last_per_map() {
        let log = "CACOSTATS|MAP01|3|1050|10/100|5/20|1/5\n\
                    CACOSTATS|MAP01|3|2100|30/100|8/20|2/5\n\
                    CACOSTATS|MAP01|3|3500|50/100|10/20|3/5\n";
        let entries = parse_log(log);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kills, 50);
        assert_eq!(entries[0].time_tics, 3500);
    }

    #[test]
    fn test_parse_log_multiple_maps() {
        let log = "CACOSTATS|MAP01|3|3500|50/100|10/20|3/5\n\
                    CACOSTATS|MAP02|3|7000|80/80|15/15|2/2\n\
                    CACOSTATS|MAP03|3|1750|20/50|5/10|0/1\n";
        let entries = parse_log(log);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].lump, "MAP01");
        assert_eq!(entries[1].lump, "MAP02");
        assert_eq!(entries[2].lump, "MAP03");
    }

    #[test]
    fn test_parse_log_preserves_map_order() {
        // MAP02 appears first, then MAP01
        let log = "CACOSTATS|MAP02|3|7000|80/80|15/15|2/2\n\
                    CACOSTATS|MAP01|3|3500|50/100|10/20|3/5\n";
        let entries = parse_log(log);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].lump, "MAP02");
        assert_eq!(entries[1].lump, "MAP01");
    }

    #[test]
    fn test_parse_log_empty() {
        assert!(parse_log("").is_empty());
        assert!(parse_log("no stats here\njust noise\n").is_empty());
    }

    #[test]
    fn test_entries_to_wad_stats_single() {
        let entries = vec![MapLogEntry {
            lump: "MAP01".to_string(),
            skill: 3,
            time_tics: 3500, // 100 seconds
            kills: 50,
            total_kills: 100,
            items: 10,
            total_items: 20,
            secrets: 3,
            total_secrets: 5,
        }];
        let stats = entries_to_wad_stats(&entries);
        assert_eq!(stats.maps.len(), 1);
        assert_eq!(stats.maps[0].lump, "MAP01");
        assert_eq!(stats.maps[0].kills, 50);
        assert_eq!(stats.maps[0].total_kills, 100);
        assert_eq!(stats.maps[0].best_skill, 4); // 0-indexed 3 → 1-indexed 4
        assert_eq!(stats.maps[0].total_exits, 1);
        assert_eq!(stats.maps[0].best_time, 3500);
    }

    #[test]
    fn test_entries_to_wad_stats_cumulative_time() {
        let entries = vec![
            MapLogEntry {
                lump: "MAP01".to_string(),
                skill: 3,
                time_tics: 2100, // 60 seconds
                kills: 10,
                total_kills: 10,
                items: 5,
                total_items: 5,
                secrets: 1,
                total_secrets: 1,
            },
            MapLogEntry {
                lump: "MAP02".to_string(),
                skill: 3,
                time_tics: 1050, // 30 seconds
                kills: 20,
                total_kills: 20,
                items: 8,
                total_items: 8,
                secrets: 2,
                total_secrets: 2,
            },
        ];
        let stats = entries_to_wad_stats(&entries);
        assert_eq!(stats.maps.len(), 2);
        assert!((stats.maps[0].time_secs - 60.0).abs() < 0.01);
        assert!((stats.maps[0].total_time_secs - 60.0).abs() < 0.01);
        assert!((stats.maps[1].time_secs - 30.0).abs() < 0.01);
        assert!((stats.maps[1].total_time_secs - 90.0).abs() < 0.01);
    }

    #[test]
    fn test_wad_stats_roundtrips_through_format_parse() {
        let entries = vec![MapLogEntry {
            lump: "MAP01".to_string(),
            skill: 4,
            time_tics: 2100,
            kills: 100,
            total_kills: 100,
            items: 50,
            total_items: 50,
            secrets: 5,
            total_secrets: 5,
        }];
        let stats = entries_to_wad_stats(&entries);
        let output = wad_stats::format_stats(&stats);
        let parsed = wad_stats::parse_stats_text(&output);
        assert!(
            parsed.is_ok(),
            "formatted output should be parseable: {output}"
        );
        let parsed = parsed.unwrap();
        assert_eq!(parsed.format, "levelstat_txt");
        assert_eq!(parsed.maps.len(), 1);
        assert_eq!(parsed.maps[0].lump, "MAP01");
        assert_eq!(parsed.maps[0].kills, 100);
        assert_eq!(parsed.maps[0].total_kills, 100);
    }

    #[test]
    fn test_collect_zdoom_stats_no_log() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!collect_zdoom_stats(dir.path()));
    }

    #[test]
    fn test_collect_zdoom_stats_empty_log() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LOG_FILENAME), "no stats\n").unwrap();
        assert!(!collect_zdoom_stats(dir.path()));
    }

    #[test]
    fn test_collect_zdoom_stats_writes_levelstat() {
        let dir = tempfile::tempdir().unwrap();
        let log = "CACOSTATS|MAP01|3|3500|50/100|10/20|3/5\n\
                    CACOSTATS|MAP02|3|7000|80/80|15/15|2/2\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log).unwrap();

        assert!(collect_zdoom_stats(dir.path()));

        let levelstat_path = dir.path().join("levelstat.txt");
        assert!(levelstat_path.exists());

        let content = std::fs::read_to_string(&levelstat_path).unwrap();
        assert!(content.contains("MAP01"));
        assert!(content.contains("MAP02"));
        assert!(content.contains("K: 50/100"));
        assert!(content.contains("K: 80/80"));

        // Verify it parses
        let stats = crate::wad_stats::parse_stats_text(&content).unwrap();
        assert_eq!(stats.maps.len(), 2);
    }

    #[test]
    fn test_collect_zdoom_stats_merges_across_sessions() {
        let dir = tempfile::tempdir().unwrap();

        // Session 1: play MAP01 and MAP02
        let log1 = "CACOSTATS|MAP01|3|3500|50/100|10/20|3/5\n\
                     CACOSTATS|MAP02|3|7000|80/80|15/15|2/2\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log1).unwrap();
        assert!(collect_zdoom_stats(dir.path()));

        // Session 2: play MAP03 (and replay MAP01 with better stats)
        let log2 = "CACOSTATS|MAP01|3|2000|60/100|12/20|4/5\n\
                     CACOSTATS|MAP03|3|5000|40/40|20/20|1/1\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log2).unwrap();
        assert!(collect_zdoom_stats(dir.path()));

        // Verify all 3 maps are present
        let content = std::fs::read_to_string(dir.path().join("levelstat.txt")).unwrap();
        let stats = wad_stats::parse_stats_text(&content).unwrap();
        assert_eq!(stats.maps.len(), 3);

        let map_lumps: Vec<&str> = stats.maps.iter().map(|m| m.lump.as_str()).collect();
        assert!(map_lumps.contains(&"MAP01"));
        assert!(map_lumps.contains(&"MAP02"));
        assert!(map_lumps.contains(&"MAP03"));

        // MAP01 should have the best (max) kills from session 2
        let map01 = stats.maps.iter().find(|m| m.lump == "MAP01").unwrap();
        assert_eq!(map01.kills, 60); // max of 50, 60
        assert_eq!(map01.secrets, 4); // max of 3, 4
    }

    #[test]
    fn test_ensure_stats_mod_creates_valid_pk3() {
        // Use a temp dir to avoid polluting the real mods dir
        let dir = tempfile::tempdir().unwrap();
        let pk3_path = dir.path().join("test_stats.pk3");

        let file = std::fs::File::create(&pk3_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("zscript.zs", options).unwrap();
        zip.write_all(ZSCRIPT_ZS.as_bytes()).unwrap();
        zip.start_file("MAPINFO", options).unwrap();
        zip.write_all(MAPINFO.as_bytes()).unwrap();
        zip.finish().unwrap();

        // Verify we can read it back
        let archive = zip::ZipArchive::new(std::fs::File::open(&pk3_path).unwrap()).unwrap();
        assert_eq!(archive.len(), 2);
        assert!(archive.name_for_index(0).unwrap().contains("zscript"));
        assert!(archive.name_for_index(1).unwrap().contains("MAPINFO"));
    }
}
