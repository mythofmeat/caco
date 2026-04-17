//! Parser and formatter for sourceport per-map statistics files.
//!
//! Supports two formats:
//! - nyan-doom/dsda-doom stats.txt (binary-ish, 15 fields per map)
//! - dsda-doom levelstat.txt (human-readable, from -levelstat flag)
//!
//! Stats are stored as JSON in the wad_completions.stats_snapshot column.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Doom runs at 35 tics per second.
pub const TICS_PER_SECOND: f64 = 35.0;

/// Human-readable names for skill levels.
pub static SKILL_NAMES: LazyLock<HashMap<i32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (0, "-"),
        (1, "ITYTD"),
        (2, "HNTR"),
        (3, "HMP"),
        (4, "UV"),
        (5, "NM"),
    ])
});

/// Per-map statistics entry (superset of both formats).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapStats {
    pub lump: String,

    // Common to both formats
    #[serde(default)]
    pub kills: i32,
    #[serde(default = "neg_one_i32")]
    pub total_kills: i32,
    #[serde(default)]
    pub items: i32,
    #[serde(default = "neg_one_i32")]
    pub total_items: i32,
    #[serde(default)]
    pub secrets: i32,
    #[serde(default = "neg_one_i32")]
    pub total_secrets: i32,

    // stats.txt specific
    #[serde(default)]
    pub episode: i32,
    #[serde(default)]
    pub map_num: i32,
    #[serde(default)]
    pub best_skill: i32,
    #[serde(default = "neg_one_i32")]
    pub best_time: i32,
    #[serde(default = "neg_one_i32")]
    pub best_max_time: i32,
    #[serde(default = "neg_one_i32")]
    pub best_nm_time: i32,
    #[serde(default)]
    pub total_exits: i32,
    #[serde(default)]
    pub cumulative_kills: i32,

    // levelstat.txt specific
    #[serde(default = "neg_one_f64")]
    pub time_secs: f64,
    #[serde(default = "neg_one_f64")]
    pub total_time_secs: f64,
}

fn neg_one_i32() -> i32 {
    -1
}
fn neg_one_f64() -> f64 {
    -1.0
}

impl MapStats {
    /// Whether this map was actually played.
    pub fn played(&self) -> bool {
        self.best_skill > 0 || self.time_secs >= 0.0
    }
}

/// Complete WAD statistics from a stats file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WadStats {
    pub format: String,
    #[serde(default)]
    pub maps: Vec<MapStats>,

    // stats.txt header fields
    #[serde(default = "default_version")]
    pub version: i32,
    #[serde(default)]
    pub header_total_kills: i32,
}

fn default_version() -> i32 {
    1
}

impl WadStats {
    /// Return only maps that were actually played.
    pub fn played_maps(&self) -> Vec<&MapStats> {
        self.maps.iter().filter(|m| m.played()).collect()
    }

    /// Human-readable total time across all played maps.
    pub fn total_time_display(&self) -> String {
        if self.format == "stats_txt" {
            let total_tics: i32 = self
                .maps
                .iter()
                .filter(|m| m.best_time > 0)
                .map(|m| m.best_time)
                .sum();
            if total_tics > 0 {
                format_time_tics(total_tics)
            } else {
                "-".to_string()
            }
        } else {
            let played = self.played_maps();
            if let Some(last) = played.last()
                && last.total_time_secs >= 0.0
            {
                return format_time_secs(last.total_time_secs);
            }
            "-".to_string()
        }
    }
}

/// Convert tics (35/sec) to human-readable M:SS or H:MM:SS.
pub fn format_time_tics(tics: i32) -> String {
    if tics < 0 {
        return "-".to_string();
    }
    let total_secs = tics as f64 / TICS_PER_SECOND;
    format_seconds(total_secs)
}

/// Convert seconds to human-readable M:SS.CC.
pub fn format_time_secs(secs: f64) -> String {
    if secs < 0.0 {
        return "-".to_string();
    }
    let mins = secs as i64 / 60;
    let remaining = secs - (mins as f64 * 60.0);
    if mins >= 60 {
        let hours = mins / 60;
        let mins = mins % 60;
        format!("{hours}:{mins:02}:{remaining:05.2}")
    } else {
        format!("{mins}:{remaining:05.2}")
    }
}

/// Format seconds as M:SS or H:MM:SS (integer seconds).
fn format_seconds(secs: f64) -> String {
    let total = secs as i64;
    let mins = total / 60;
    let s = total % 60;
    if mins >= 60 {
        let hours = mins / 60;
        let mins = mins % 60;
        format!("{hours}:{mins:02}:{s:02}")
    } else {
        format!("{mins}:{s:02}")
    }
}

/// Get display name for a skill level.
pub fn skill_name(skill: i32) -> &'static str {
    SKILL_NAMES.get(&skill).copied().unwrap_or("-")
}

// --- Parsing ---

// stats.txt: "MAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3"
static STATS_TXT_MAP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\S+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)$",
    )
    .unwrap()
});

// levelstat.txt: "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5"
static LEVELSTAT_MAP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\S+)\s+-\s+(\d+):(\d+(?:\.\d+)?)\s+\((\d+):(\d+(?:\.\d+)?)\)\s+K:\s*(\d+)/(\d+)\s+I:\s*(\d+)/(\d+)\s+S:\s*(\d+)/(\d+)",
    )
    .unwrap()
});

/// Parse a stats file from disk, auto-detecting format.
pub fn parse_stats_file(path: &Path) -> crate::Result<WadStats> {
    let text = std::fs::read_to_string(path)?;
    parse_stats_text(&text)
}

/// Parse stats from text, auto-detecting format.
pub fn parse_stats_text(text: &str) -> crate::Result<WadStats> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return Err(crate::Error::InvalidWadFormat(
            "Empty stats file".to_string(),
        ));
    }

    if is_stats_txt(&lines) {
        Ok(parse_stats_txt(&lines))
    } else if is_levelstat_txt(&lines) {
        Ok(parse_levelstat_txt(&lines))
    } else {
        Err(crate::Error::InvalidWadFormat(
            "Unrecognized stats file format".to_string(),
        ))
    }
}

fn is_stats_txt(lines: &[&str]) -> bool {
    if lines.len() < 3 {
        return false;
    }
    // First two lines should be integers (version, total_kills)
    if lines[0].trim().parse::<i32>().is_err() || lines[1].trim().parse::<i32>().is_err() {
        return false;
    }
    // Third line should match the 15-field map format
    STATS_TXT_MAP_RE.is_match(lines[2].trim())
}

fn is_levelstat_txt(lines: &[&str]) -> bool {
    LEVELSTAT_MAP_RE.is_match(lines[0].trim())
}

fn parse_stats_txt(lines: &[&str]) -> WadStats {
    let version = lines[0].trim().parse::<i32>().unwrap_or(1);
    let total_kills = lines[1].trim().parse::<i32>().unwrap_or(0);

    let mut maps = Vec::new();
    for line in &lines[2..] {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(caps) = STATS_TXT_MAP_RE.captures(line) {
            maps.push(MapStats {
                lump: caps[1].to_string(),
                episode: caps[2].parse().unwrap_or(0),
                map_num: caps[3].parse().unwrap_or(0),
                best_skill: caps[4].parse().unwrap_or(0),
                best_time: caps[5].parse().unwrap_or(-1),
                best_max_time: caps[6].parse().unwrap_or(-1),
                best_nm_time: caps[7].parse().unwrap_or(-1),
                total_exits: caps[8].parse().unwrap_or(0),
                cumulative_kills: caps[9].parse().unwrap_or(0),
                kills: caps[10].parse().unwrap_or(0),
                items: caps[11].parse().unwrap_or(0),
                secrets: caps[12].parse().unwrap_or(0),
                total_kills: caps[13].parse().unwrap_or(-1),
                total_items: caps[14].parse().unwrap_or(-1),
                total_secrets: caps[15].parse().unwrap_or(-1),
                time_secs: -1.0,
                total_time_secs: -1.0,
            });
        }
    }

    WadStats {
        format: "stats_txt".to_string(),
        maps,
        version,
        header_total_kills: total_kills,
    }
}

fn parse_levelstat_txt(lines: &[&str]) -> WadStats {
    let mut maps = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(caps) = LEVELSTAT_MAP_RE.captures(line) {
            let time_mins: f64 = caps[2].parse().unwrap_or(0.0);
            let time_sec: f64 = caps[3].parse().unwrap_or(0.0);
            let total_mins: f64 = caps[4].parse().unwrap_or(0.0);
            let total_sec: f64 = caps[5].parse().unwrap_or(0.0);

            maps.push(MapStats {
                lump: caps[1].to_string(),
                time_secs: time_mins * 60.0 + time_sec,
                total_time_secs: total_mins * 60.0 + total_sec,
                kills: caps[6].parse().unwrap_or(0),
                total_kills: caps[7].parse().unwrap_or(-1),
                items: caps[8].parse().unwrap_or(0),
                total_items: caps[9].parse().unwrap_or(-1),
                secrets: caps[10].parse().unwrap_or(0),
                total_secrets: caps[11].parse().unwrap_or(-1),
                best_skill: 4, // levelstat doesn't record skill; mark as played
                episode: 0,
                map_num: 0,
                best_time: -1,
                best_max_time: -1,
                best_nm_time: -1,
                // Each levelstat line is written only on map exit, so the
                // presence of the line implies at least one exit. Without this,
                // completion detection — which keys off `total_exits >= 1` —
                // would never fire for levelstat-only sourceports (zdoom family).
                total_exits: 1,
                cumulative_kills: 0,
            });
        }
    }

    WadStats {
        format: "levelstat_txt".to_string(),
        maps,
        version: 1,
        header_total_kills: 0,
    }
}

// --- Formatting / Export ---

/// Export WadStats back to original text format.
pub fn format_stats(stats: &WadStats) -> String {
    if stats.format == "stats_txt" {
        format_stats_txt(stats)
    } else {
        format_levelstat_txt(stats)
    }
}

fn format_stats_txt(stats: &WadStats) -> String {
    let mut lines = vec![
        stats.version.to_string(),
        stats.header_total_kills.to_string(),
    ];
    for m in &stats.maps {
        lines.push(format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            m.lump,
            m.episode,
            m.map_num,
            m.best_skill,
            m.best_time,
            m.best_max_time,
            m.best_nm_time,
            m.total_exits,
            m.cumulative_kills,
            m.kills,
            m.items,
            m.secrets,
            m.total_kills,
            m.total_items,
            m.total_secrets,
        ));
    }
    lines.join("\n") + "\n"
}

fn format_levelstat_txt(stats: &WadStats) -> String {
    let mut lines = Vec::new();
    for m in &stats.maps {
        let time_str = secs_to_levelstat(m.time_secs);
        let total_str = secs_to_levelstat(m.total_time_secs);
        lines.push(format!(
            "{} - {} ({})  K: {}/{}  I: {}/{}  S: {}/{}",
            m.lump,
            time_str,
            total_str,
            m.kills,
            m.total_kills,
            m.items,
            m.total_items,
            m.secrets,
            m.total_secrets,
        ));
    }
    lines.join("\n") + "\n"
}

fn secs_to_levelstat(secs: f64) -> String {
    if secs < 0.0 {
        return "0:00.00".to_string();
    }
    let mins = secs as i64 / 60;
    let remaining = secs - (mins as f64 * 60.0);
    format!("{mins}:{remaining:05.2}")
}

// --- JSON serialization ---

/// Serialize WadStats to compact JSON for DB storage.
pub fn stats_to_json(stats: &WadStats) -> crate::Result<String> {
    Ok(serde_json::to_string(stats)?)
}

/// Deserialize WadStats from JSON.
pub fn stats_from_json(json_str: &str) -> crate::Result<WadStats> {
    Ok(serde_json::from_str(json_str)?)
}

// --- Merging ---

/// Return the lesser of two values, treating negatives as unset.
fn min_positive_i32(a: i32, b: i32) -> i32 {
    if a < 0 {
        b
    } else if b < 0 {
        a
    } else {
        a.min(b)
    }
}

fn min_positive_f64(a: f64, b: f64) -> f64 {
    if a < 0.0 {
        b
    } else if b < 0.0 {
        a
    } else {
        a.min(b)
    }
}

/// Merge multiple WadStats into one, keeping the best data per map.
///
/// When IWAD or sourceport changes create different nested directories under
/// `-data`, several stats files can coexist.  Merging keeps the most useful
/// value for every field (highest skill, fastest time, highest counts).
/// Prefers `stats_txt` format when both formats are present.
pub fn merge_stats(stats_list: &[WadStats]) -> WadStats {
    assert!(!stats_list.is_empty(), "No stats to merge");
    if stats_list.len() == 1 {
        return stats_list[0].clone();
    }

    let has_stats_txt = stats_list.iter().any(|s| s.format == "stats_txt");
    let fmt = if has_stats_txt {
        "stats_txt"
    } else {
        &stats_list[0].format
    };

    let mut merged: std::collections::HashMap<String, MapStats> = std::collections::HashMap::new();

    for stats in stats_list {
        for m in &stats.maps {
            if let Some(existing) = merged.get_mut(&m.lump) {
                if m.episode > 0 {
                    existing.episode = m.episode;
                }
                if m.map_num > 0 {
                    existing.map_num = m.map_num;
                }
                existing.best_skill = existing.best_skill.max(m.best_skill);
                existing.total_exits = existing.total_exits.max(m.total_exits);
                existing.cumulative_kills = existing.cumulative_kills.max(m.cumulative_kills);
                existing.best_time = min_positive_i32(existing.best_time, m.best_time);
                existing.best_max_time = min_positive_i32(existing.best_max_time, m.best_max_time);
                existing.best_nm_time = min_positive_i32(existing.best_nm_time, m.best_nm_time);
                existing.time_secs = min_positive_f64(existing.time_secs, m.time_secs);
                existing.total_time_secs =
                    min_positive_f64(existing.total_time_secs, m.total_time_secs);
                existing.kills = existing.kills.max(m.kills);
                existing.items = existing.items.max(m.items);
                existing.secrets = existing.secrets.max(m.secrets);
                existing.total_kills = existing.total_kills.max(m.total_kills);
                existing.total_items = existing.total_items.max(m.total_items);
                existing.total_secrets = existing.total_secrets.max(m.total_secrets);
            } else {
                merged.insert(m.lump.clone(), m.clone());
            }
        }
    }

    // Cross-populate time fields between formats
    for m in merged.values_mut() {
        if m.best_time < 0 && m.time_secs >= 0.0 {
            m.best_time = (m.time_secs * TICS_PER_SECOND).round() as i32;
        }
        if m.time_secs < 0.0 && m.best_time >= 0 {
            m.time_secs = m.best_time as f64 / TICS_PER_SECOND;
        }
    }

    let mut maps: Vec<MapStats> = merged.into_values().collect();
    maps.sort_by(|a, b| a.lump.cmp(&b.lump));

    let version = stats_list.iter().map(|s| s.version).max().unwrap_or(1);
    let header_total_kills = stats_list
        .iter()
        .map(|s| s.header_total_kills)
        .max()
        .unwrap_or(0);

    WadStats {
        format: fmt.to_string(),
        maps,
        version,
        header_total_kills,
    }
}

// --- Delta computation ---

/// Per-map delta information from a play session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapDelta {
    pub lump: String,
    pub new_map: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exits_delta: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kills_delta: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items_delta: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets_delta: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_time_before: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_time_after: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_improved: Option<bool>,
    // levelstat.txt fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kills: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_kills: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_items: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_secrets: Option<i32>,
}

/// Result of computing a stats delta between before/after snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsDelta {
    pub maps_played: Vec<String>,
    pub deltas: Vec<MapDelta>,
}

/// Compute which maps were played in a session by diffing before/after snapshots.
///
/// For stats.txt (persistent/cumulative): a map counts as played iff
/// `total_exits` increased — or, for a brand-new entry, is already > 0.
/// `best_skill`/`time_secs` alone are not enough because merged levelstat
/// data can populate them on idclev (entered but not exited). For
/// levelstat.txt (rewritten each run): all maps in `after` are this session's
/// maps — dsda only writes a levelstat line on actual exit.
pub fn compute_stats_delta(before: Option<&WadStats>, after: &WadStats) -> StatsDelta {
    if after.format == "levelstat_txt" {
        // levelstat.txt is rewritten each run — all maps are this session's
        let maps_played: Vec<String> = after.maps.iter().map(|m| m.lump.clone()).collect();
        let deltas: Vec<MapDelta> = after
            .maps
            .iter()
            .map(|m| MapDelta {
                lump: m.lump.clone(),
                new_map: true,
                time_secs: Some(m.time_secs),
                kills: Some(m.kills),
                total_kills: Some(m.total_kills),
                items: Some(m.items),
                total_items: Some(m.total_items),
                secrets: Some(m.secrets),
                total_secrets: Some(m.total_secrets),
                exits_delta: None,
                kills_delta: None,
                items_delta: None,
                secrets_delta: None,
                best_time_before: None,
                best_time_after: None,
                time_improved: None,
            })
            .collect();
        return StatsDelta {
            maps_played,
            deltas,
        };
    }

    // stats.txt: diff field-by-field
    let before_map: HashMap<&str, &MapStats> = before
        .map(|b| b.maps.iter().map(|m| (m.lump.as_str(), m)).collect())
        .unwrap_or_default();

    let mut maps_played = Vec::new();
    let mut deltas = Vec::new();

    for m in &after.maps {
        match before_map.get(m.lump.as_str()) {
            None => {
                if m.total_exits > 0 {
                    maps_played.push(m.lump.clone());
                    deltas.push(MapDelta {
                        lump: m.lump.clone(),
                        new_map: true,
                        exits_delta: Some(m.total_exits),
                        kills_delta: Some(m.kills),
                        items_delta: Some(m.items),
                        secrets_delta: Some(m.secrets),
                        best_time_before: Some(-1),
                        best_time_after: Some(m.best_time),
                        time_improved: Some(m.best_time > 0),
                        time_secs: None,
                        kills: None,
                        total_kills: None,
                        items: None,
                        total_items: None,
                        secrets: None,
                        total_secrets: None,
                    });
                }
            }
            Some(prev) => {
                let exits_delta = m.total_exits - prev.total_exits;
                if exits_delta > 0 {
                    maps_played.push(m.lump.clone());
                    deltas.push(MapDelta {
                        lump: m.lump.clone(),
                        new_map: false,
                        exits_delta: Some(exits_delta),
                        kills_delta: Some(m.kills - prev.kills),
                        items_delta: Some(m.items - prev.items),
                        secrets_delta: Some(m.secrets - prev.secrets),
                        best_time_before: Some(prev.best_time),
                        best_time_after: Some(m.best_time),
                        time_improved: Some(
                            m.best_time > 0 && (prev.best_time < 0 || m.best_time < prev.best_time),
                        ),
                        time_secs: None,
                        kills: None,
                        total_kills: None,
                        items: None,
                        total_items: None,
                        secrets: None,
                        total_secrets: None,
                    });
                }
            }
        }
    }

    StatsDelta {
        maps_played,
        deltas,
    }
}

// --- Map progress ---

/// Whether a map lump name is a secret map by convention.
///
/// Secret maps: E*M9 (Doom 1), MAP31/MAP32 (Doom 2).
fn is_secret_map(lump: &str) -> bool {
    let lump = lump.to_ascii_uppercase();
    // Doom 1: ExM9
    if lump.len() == 4
        && lump.starts_with('E')
        && lump.as_bytes()[2] == b'M'
        && lump.as_bytes()[1].is_ascii_digit()
        && lump.as_bytes()[3] == b'9'
    {
        return true;
    }
    // Doom 2: MAP31, MAP32
    if let Some(num) = lump.strip_prefix("MAP").and_then(|s| s.parse::<i32>().ok()) {
        return num == 31 || num == 32;
    }
    false
}

/// Summary of map completion progress.
#[derive(Debug, Clone)]
pub struct MapProgress {
    pub played: usize,
    /// Total map count (None for levelstat format where total is unknown).
    pub total: Option<usize>,
    pub secret_played: usize,
    pub secret_total: Option<usize>,
}

/// Compute map progress from WAD stats.
///
/// Secret maps are counted separately from normal maps.
pub fn compute_map_progress(stats: &WadStats) -> MapProgress {
    if stats.format == "levelstat_txt" {
        let secret_played = stats.maps.iter().filter(|m| is_secret_map(&m.lump)).count();
        MapProgress {
            played: stats.maps.len() - secret_played,
            total: None,
            secret_played,
            secret_total: None,
        }
    } else {
        let secret_total = stats.maps.iter().filter(|m| is_secret_map(&m.lump)).count();
        let played_maps = stats.played_maps();
        let secret_played = played_maps
            .iter()
            .filter(|m| is_secret_map(&m.lump))
            .count();
        MapProgress {
            played: played_maps.len() - secret_played,
            total: Some(stats.maps.len() - secret_total),
            secret_played,
            secret_total: Some(secret_total),
        }
    }
}

/// Render a text progress bar for map completion.
///
/// Returns `None` when total is unknown (levelstat format) or no maps exist.
/// Example: `"▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░ 9/30 | 1/2 secret"`
fn format_progress_bar(progress: &MapProgress, width: usize) -> Option<String> {
    let total = progress.total?;
    if total == 0 {
        return None;
    }
    let filled = (progress.played as f64 / total as f64 * width as f64).round() as usize;
    let filled = filled.min(width);
    let bar: String = "▓".repeat(filled) + &"░".repeat(width - filled);
    let mut result = format!("{bar} {}/{total}", progress.played);
    if let Some(secret_total) = progress.secret_total
        && secret_total > 0
    {
        result.push_str(&format!(
            " | {}/{secret_total} secret",
            progress.secret_played
        ));
    }
    Some(result)
}

/// Format map progress as plain text.
///
/// Returns `None` if no maps were played.
fn format_map_progress(progress: &MapProgress) -> Option<String> {
    if let Some(total) = progress.total {
        if total == 0 && progress.secret_total.is_none_or(|s| s == 0) {
            return None;
        }
        let base = format!("{}/{total} maps", progress.played);
        if let Some(secret_total) = progress.secret_total
            && secret_total > 0
        {
            return Some(format!(
                "{base} | {}/{secret_total} secret",
                progress.secret_played
            ));
        }
        return Some(base);
    }
    // levelstat: no total known
    if progress.played == 0 && progress.secret_played == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if progress.played > 0 {
        parts.push(format!("{} maps", progress.played));
    }
    if progress.secret_played > 0 {
        parts.push(format!("{} secret", progress.secret_played));
    }
    Some(format!("{} played", parts.join(" | ")))
}

/// Get the best progress display string for a stats JSON snapshot.
///
/// Returns a progress bar when total is known (stats.txt), otherwise a text
/// summary (levelstat.txt). Returns `None` on missing/invalid input or empty
/// progress.
pub fn get_progress_display(stats_json: Option<&str>) -> Option<String> {
    let json_str = stats_json?;
    let stats = stats_from_json(json_str).ok()?;
    let progress = compute_map_progress(&stats);
    format_progress_bar(&progress, 20).or_else(|| format_map_progress(&progress))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_time_tics() {
        assert_eq!(format_time_tics(-1), "-");
        assert_eq!(format_time_tics(0), "0:00");
        assert_eq!(format_time_tics(35 * 65), "1:05"); // 65 seconds
        assert_eq!(format_time_tics(35 * 3661), "1:01:01"); // 1h 1m 1s
    }

    #[test]
    fn test_format_time_secs() {
        assert_eq!(format_time_secs(-1.0), "-");
        assert_eq!(format_time_secs(32.97), "0:32.97");
        assert_eq!(format_time_secs(125.50), "2:05.50");
        assert_eq!(format_time_secs(3725.00), "1:02:05.00");
    }

    #[test]
    fn test_skill_name() {
        assert_eq!(skill_name(0), "-");
        assert_eq!(skill_name(4), "UV");
        assert_eq!(skill_name(5), "NM");
        assert_eq!(skill_name(99), "-");
    }

    #[test]
    fn test_parse_stats_txt() {
        let text = "1\n150\nMAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3\nMAP02 1 2 4 5000 3000 -1 2 300 200 10 3 200 12 5\n";
        let stats = parse_stats_text(text).unwrap();
        assert_eq!(stats.format, "stats_txt");
        assert_eq!(stats.version, 1);
        assert_eq!(stats.header_total_kills, 150);
        assert_eq!(stats.maps.len(), 2);
        assert_eq!(stats.maps[0].lump, "MAP01");
        assert_eq!(stats.maps[0].best_skill, 3);
        assert_eq!(stats.maps[0].best_time, 23193);
        assert_eq!(stats.maps[0].kills, 127);
        assert_eq!(stats.maps[1].lump, "MAP02");
        assert_eq!(stats.maps[1].total_exits, 2);
    }

    #[test]
    fn test_parse_levelstat_txt() {
        let text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\nMAP02 - 1:15.50 (1:48.47)  K: 80/120  I: 30/45  S: 2/3\n";
        let stats = parse_stats_text(text).unwrap();
        assert_eq!(stats.format, "levelstat_txt");
        assert_eq!(stats.maps.len(), 2);
        assert_eq!(stats.maps[0].lump, "MAP01");
        assert!((stats.maps[0].time_secs - 32.97).abs() < 0.01);
        assert_eq!(stats.maps[0].kills, 100);
        assert_eq!(stats.maps[0].total_kills, 100);
        assert_eq!(stats.maps[0].total_exits, 1); // every levelstat line implies an exit
        assert_eq!(stats.maps[1].lump, "MAP02");
        assert_eq!(stats.maps[1].items, 30);
        assert_eq!(stats.maps[1].total_exits, 1);
    }

    #[test]
    fn test_parse_empty() {
        assert!(parse_stats_text("").is_err());
    }

    #[test]
    fn test_parse_unrecognized() {
        assert!(parse_stats_text("garbage data here").is_err());
    }

    #[test]
    fn test_stats_json_roundtrip() {
        let text = "1\n0\nMAP01 1 1 4 1000 -1 -1 1 100 50 3 2 100 5 3\n";
        let stats = parse_stats_text(text).unwrap();
        let json = stats_to_json(&stats).unwrap();
        let restored = stats_from_json(&json).unwrap();
        assert_eq!(restored.format, "stats_txt");
        assert_eq!(restored.maps.len(), 1);
        assert_eq!(restored.maps[0].lump, "MAP01");
        assert_eq!(restored.maps[0].best_skill, 4);
    }

    #[test]
    fn test_format_stats_txt_roundtrip() {
        let text = "1\n150\nMAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3\n";
        let stats = parse_stats_text(text).unwrap();
        let output = format_stats(&stats);
        let reparsed = parse_stats_text(&output).unwrap();
        assert_eq!(reparsed.maps.len(), 1);
        assert_eq!(reparsed.maps[0].best_time, 23193);
    }

    #[test]
    fn test_compute_delta_levelstat() {
        let after_text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\n";
        let after = parse_stats_text(after_text).unwrap();
        let delta = compute_stats_delta(None, &after);
        assert_eq!(delta.maps_played, vec!["MAP01"]);
        assert_eq!(delta.deltas.len(), 1);
        assert!(delta.deltas[0].new_map);
    }

    #[test]
    fn test_compute_delta_stats_txt_new_map() {
        let after_text = "1\n0\nMAP01 1 1 4 1000 -1 -1 1 100 50 3 2 100 5 3\n";
        let after = parse_stats_text(after_text).unwrap();
        let delta = compute_stats_delta(None, &after);
        assert_eq!(delta.maps_played, vec!["MAP01"]);
        assert!(delta.deltas[0].new_map);
    }

    #[test]
    fn test_compute_delta_stats_txt_exits_increased() {
        let before_text = "1\n0\nMAP01 1 1 4 1000 -1 -1 1 100 50 3 2 100 5 3\n";
        let after_text = "1\n0\nMAP01 1 1 4 900 -1 -1 2 150 60 4 3 100 5 3\n";
        let before = parse_stats_text(before_text).unwrap();
        let after = parse_stats_text(after_text).unwrap();
        let delta = compute_stats_delta(Some(&before), &after);
        assert_eq!(delta.maps_played, vec!["MAP01"]);
        assert!(!delta.deltas[0].new_map);
        assert_eq!(delta.deltas[0].exits_delta, Some(1));
    }

    #[test]
    fn test_compute_delta_no_change() {
        let text = "1\n0\nMAP01 1 1 4 1000 -1 -1 1 100 50 3 2 100 5 3\n";
        let stats = parse_stats_text(text).unwrap();
        let delta = compute_stats_delta(Some(&stats), &stats);
        assert!(delta.maps_played.is_empty());
        assert!(delta.deltas.is_empty());
    }

    #[test]
    fn test_map_played() {
        let played = MapStats {
            lump: "MAP01".to_string(),
            best_skill: 4,
            ..MapStats {
                lump: String::new(),
                kills: 0,
                total_kills: -1,
                items: 0,
                total_items: -1,
                secrets: 0,
                total_secrets: -1,
                episode: 0,
                map_num: 0,
                best_skill: 0,
                best_time: -1,
                best_max_time: -1,
                best_nm_time: -1,
                total_exits: 0,
                cumulative_kills: 0,
                time_secs: -1.0,
                total_time_secs: -1.0,
            }
        };
        assert!(played.played());

        let unplayed = MapStats {
            lump: "MAP01".to_string(),
            best_skill: 0,
            time_secs: -1.0,
            ..played.clone()
        };
        assert!(!unplayed.played());
    }

    #[test]
    fn test_is_secret_map() {
        assert!(is_secret_map("E1M9"));
        assert!(is_secret_map("E2M9"));
        assert!(is_secret_map("MAP31"));
        assert!(is_secret_map("MAP32"));
        assert!(!is_secret_map("E1M1"));
        assert!(!is_secret_map("MAP01"));
        assert!(!is_secret_map("MAP30"));
        assert!(!is_secret_map("MAP33"));
    }

    #[test]
    fn test_compute_map_progress_stats_txt() {
        // 3 maps total (MAP01 played, MAP02 played, MAP31 secret+played)
        let text = concat!(
            "1\n0\n",
            "MAP01 1 1 4 1000 -1 -1 1 100 50 3 2 100 5 3\n",
            "MAP02 1 2 0 -1 -1 -1 0 0 0 0 0 0 0 0\n",
            "MAP31 1 31 3 2000 -1 -1 1 50 20 1 0 50 2 1\n",
        );
        let stats = parse_stats_text(text).unwrap();
        let progress = compute_map_progress(&stats);
        assert_eq!(progress.played, 1); // MAP01 played (MAP02 unplayed)
        assert_eq!(progress.total, Some(2)); // 3 total - 1 secret = 2
        assert_eq!(progress.secret_played, 1); // MAP31
        assert_eq!(progress.secret_total, Some(1));
    }

    #[test]
    fn test_compute_map_progress_levelstat() {
        let text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\nMAP02 - 1:15.50 (1:48.47)  K: 80/120  I: 30/45  S: 2/3\n";
        let stats = parse_stats_text(text).unwrap();
        let progress = compute_map_progress(&stats);
        assert_eq!(progress.played, 2);
        assert!(progress.total.is_none());
        assert_eq!(progress.secret_played, 0);
    }

    #[test]
    fn test_format_progress_bar() {
        let progress = MapProgress {
            played: 9,
            total: Some(30),
            secret_played: 1,
            secret_total: Some(2),
        };
        let bar = format_progress_bar(&progress, 20).unwrap();
        assert!(bar.contains("9/30"));
        assert!(bar.contains("1/2 secret"));
        // Should have 6 filled (9/30 * 20 = 6)
        assert!(bar.starts_with("▓▓▓▓▓▓░"));
    }

    #[test]
    fn test_format_progress_bar_no_total() {
        let progress = MapProgress {
            played: 5,
            total: None,
            secret_played: 0,
            secret_total: None,
        };
        assert!(format_progress_bar(&progress, 20).is_none());
    }

    #[test]
    fn test_get_progress_display_none() {
        assert!(get_progress_display(None).is_none());
        assert!(get_progress_display(Some("invalid json")).is_none());
    }

    #[test]
    fn test_get_progress_display_stats_txt() {
        let text = "1\n0\nMAP01 1 1 4 1000 -1 -1 1 100 50 3 2 100 5 3\nMAP02 1 2 0 -1 -1 -1 0 0 0 0 0 0 0 0\n";
        let stats = parse_stats_text(text).unwrap();
        let json = stats_to_json(&stats).unwrap();
        let display = get_progress_display(Some(&json)).unwrap();
        assert!(display.contains("1/2")); // 1 played out of 2 total
    }

    #[test]
    fn test_get_progress_display_levelstat() {
        let text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\n";
        let stats = parse_stats_text(text).unwrap();
        let json = stats_to_json(&stats).unwrap();
        let display = get_progress_display(Some(&json)).unwrap();
        assert!(display.contains("1 maps played"));
    }

    // --- Detailed parsing tests ---

    #[test]
    fn test_parse_stats_txt_unplayed_map() {
        let text = "1\n0\nMAP01 1 1 0 -1 -1 -1 0 0 0 0 0 -1 -1 -1\n";
        let stats = parse_stats_text(text).unwrap();
        let m = &stats.maps[0];
        assert_eq!(m.lump, "MAP01");
        assert_eq!(m.best_skill, 0);
        assert_eq!(m.best_time, -1);
        assert_eq!(m.total_exits, 0);
        assert!(!m.played());
    }

    #[test]
    fn test_played_maps_filter() {
        let text = concat!(
            "1\n0\n",
            "MAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3\n",
            "MAP02 1 2 4 5000 -1 -1 2 300 200 10 3 200 12 5\n",
            "MAP31 1 31 0 -1 -1 -1 0 0 0 0 0 -1 -1 -1\n",
        );
        let stats = parse_stats_text(text).unwrap();
        let played = stats.played_maps();
        assert_eq!(played.len(), 2); // MAP01, MAP02 (MAP31 unplayed)
        assert!(played.iter().all(|m| m.played()));
    }

    #[test]
    fn test_stats_txt_total_time_display() {
        let text = "1\n0\nMAP01 1 1 3 1050 -1 -1 1 0 0 0 0 0 0 0\nMAP02 1 2 4 2100 -1 -1 1 0 0 0 0 0 0 0\n";
        let stats = parse_stats_text(text).unwrap();
        let display = stats.total_time_display();
        assert_ne!(display, "-");
        // 1050 + 2100 = 3150 tics / 35 = 90 seconds = 1:30
        assert_eq!(display, "1:30");
    }

    #[test]
    fn test_levelstat_time_accumulation() {
        let text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\nMAP02 - 1:23.45 (1:56.42)  K: 80/100  I: 40/50  S: 3/5\n";
        let stats = parse_stats_text(text).unwrap();
        let m2 = &stats.maps[1];
        assert!((m2.time_secs - 83.45).abs() < 0.01);
        assert!((m2.total_time_secs - 116.42).abs() < 0.01);
    }

    #[test]
    fn test_levelstat_total_time_display() {
        let text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\nMAP02 - 1:23.45 (1:56.42)  K: 80/100  I: 40/50  S: 3/5\nMAP03 - 2:10.00 (4:06.42)  K: 60/60  I: 20/20  S: 2/2\n";
        let stats = parse_stats_text(text).unwrap();
        assert_eq!(stats.total_time_display(), "4:06.42");
    }

    #[test]
    fn test_format_levelstat_roundtrip() {
        let text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\nMAP02 - 1:15.50 (1:48.47)  K: 80/120  I: 30/45  S: 2/3\n";
        let stats = parse_stats_text(text).unwrap();
        let output = format_stats(&stats);
        let reparsed = parse_stats_text(&output).unwrap();
        assert_eq!(reparsed.maps.len(), 2);
        assert!((reparsed.maps[0].time_secs - 32.97).abs() < 0.01);
        assert_eq!(reparsed.maps[0].kills, 100);
        assert_eq!(reparsed.maps[1].lump, "MAP02");
    }

    #[test]
    fn test_json_roundtrip_levelstat() {
        let text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\n";
        let stats = parse_stats_text(text).unwrap();
        let json = stats_to_json(&stats).unwrap();
        let restored = stats_from_json(&json).unwrap();
        assert_eq!(restored.format, "levelstat_txt");
        assert_eq!(restored.maps.len(), 1);
        assert!((restored.maps[0].time_secs - 32.97).abs() < 0.01);
    }

    #[test]
    fn test_json_full_roundtrip_to_text() {
        let text = "1\n150\nMAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3\n";
        let stats = parse_stats_text(text).unwrap();
        let json = stats_to_json(&stats).unwrap();
        let restored = stats_from_json(&json).unwrap();
        let text1 = format_stats(&stats);
        let text2 = format_stats(&restored);
        assert_eq!(text1, text2);
    }

    // --- Format map progress tests ---

    #[test]
    fn test_format_map_progress_with_secrets() {
        let p = MapProgress {
            played: 9,
            total: Some(30),
            secret_played: 0,
            secret_total: Some(2),
        };
        let result = format_map_progress(&p).unwrap();
        assert_eq!(result, "9/30 maps | 0/2 secret");
    }

    #[test]
    fn test_format_map_progress_no_secrets() {
        let p = MapProgress {
            played: 5,
            total: Some(10),
            secret_played: 0,
            secret_total: Some(0),
        };
        let result = format_map_progress(&p).unwrap();
        assert_eq!(result, "5/10 maps");
    }

    #[test]
    fn test_format_map_progress_levelstat() {
        let p = MapProgress {
            played: 9,
            total: None,
            secret_played: 1,
            secret_total: None,
        };
        let result = format_map_progress(&p).unwrap();
        assert_eq!(result, "9 maps | 1 secret played");
    }

    #[test]
    fn test_format_map_progress_levelstat_no_secrets() {
        let p = MapProgress {
            played: 5,
            total: None,
            secret_played: 0,
            secret_total: None,
        };
        let result = format_map_progress(&p).unwrap();
        assert_eq!(result, "5 maps played");
    }

    #[test]
    fn test_format_map_progress_empty() {
        let p = MapProgress {
            played: 0,
            total: Some(0),
            secret_played: 0,
            secret_total: Some(0),
        };
        assert!(format_map_progress(&p).is_none());
    }

    #[test]
    fn test_format_map_progress_levelstat_empty() {
        let p = MapProgress {
            played: 0,
            total: None,
            secret_played: 0,
            secret_total: None,
        };
        assert!(format_map_progress(&p).is_none());
    }

    // --- Progress bar tests ---

    #[test]
    fn test_format_progress_bar_half() {
        let p = MapProgress {
            played: 5,
            total: Some(10),
            secret_played: 0,
            secret_total: Some(0),
        };
        let bar = format_progress_bar(&p, 10).unwrap();
        assert_eq!(bar, "▓▓▓▓▓░░░░░ 5/10");
    }

    #[test]
    fn test_format_progress_bar_full() {
        let p = MapProgress {
            played: 30,
            total: Some(30),
            secret_played: 0,
            secret_total: Some(0),
        };
        let bar = format_progress_bar(&p, 10).unwrap();
        assert_eq!(bar, "▓▓▓▓▓▓▓▓▓▓ 30/30");
    }

    #[test]
    fn test_format_progress_bar_zero_total() {
        let p = MapProgress {
            played: 0,
            total: Some(0),
            secret_played: 0,
            secret_total: Some(0),
        };
        assert!(format_progress_bar(&p, 10).is_none());
    }

    // --- Time formatting edge cases ---

    #[test]
    fn test_format_time_tics_zero() {
        assert_eq!(format_time_tics(0), "0:00");
    }

    #[test]
    fn test_format_time_secs_zero() {
        assert_eq!(format_time_secs(0.0), "0:00.00");
    }

    #[test]
    fn test_skill_name_all_known() {
        assert_eq!(skill_name(1), "ITYTD");
        assert_eq!(skill_name(2), "HNTR");
        assert_eq!(skill_name(3), "HMP");
    }

    // --- Delta computation: before_none (first play) ---

    #[test]
    fn test_compute_delta_first_play_unplayed_maps() {
        // First play with mix of played and unplayed maps
        let text = "1\n0\nMAP01 1 1 4 1000 -1 -1 1 100 50 3 2 100 5 3\nMAP02 1 2 0 -1 -1 -1 0 0 0 0 0 -1 -1 -1\n";
        let after = parse_stats_text(text).unwrap();
        let delta = compute_stats_delta(None, &after);
        // Only MAP01 was played (MAP02 unplayed)
        assert_eq!(delta.maps_played, vec!["MAP01"]);
        assert_eq!(delta.deltas.len(), 1);
        assert!(delta.deltas[0].new_map);
    }

    #[test]
    fn test_compute_delta_first_play_idclev_new_map() {
        // Regression: a new map appears with best_skill/best_time populated
        // from merged levelstat data, but total_exits == 0 (user idclev'd
        // through without exiting). Must NOT count as played.
        let text = "1\n0\nMAP01 1 1 4 314 -1 -1 0 0 0 0 0 -1 -1 -1\n";
        let after = parse_stats_text(text).unwrap();
        let delta = compute_stats_delta(None, &after);
        assert!(delta.maps_played.is_empty());
        assert!(delta.deltas.is_empty());
    }

    #[test]
    fn test_compute_delta_levelstat_all_maps() {
        let text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\nMAP02 - 1:23.45 (1:56.42)  K: 80/100  I: 40/50  S: 3/5\n";
        let after = parse_stats_text(text).unwrap();
        let delta = compute_stats_delta(None, &after);
        assert_eq!(delta.maps_played, vec!["MAP01", "MAP02"]);
        assert_eq!(delta.deltas.len(), 2);
        assert!((delta.deltas[0].time_secs.unwrap() - 32.97).abs() < 0.01);
    }

    #[test]
    fn test_compute_delta_levelstat_ignores_before() {
        // levelstat before is irrelevant — all after maps are this session's
        let before_text = "MAP01 - 0:10.00 (0:10.00)  K: 50/100  I: 25/50  S: 2/5\n";
        let after_text = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\nMAP02 - 1:23.45 (1:56.42)  K: 80/100  I: 40/50  S: 3/5\n";
        let before = parse_stats_text(before_text).unwrap();
        let after = parse_stats_text(after_text).unwrap();
        let delta = compute_stats_delta(Some(&before), &after);
        assert_eq!(delta.maps_played, vec!["MAP01", "MAP02"]);
    }

    #[test]
    fn test_compute_delta_time_improved() {
        let before_text = "1\n0\nMAP01 1 1 4 2000 -1 -1 1 100 50 3 2 100 5 3\n";
        let after_text = "1\n0\nMAP01 1 1 4 1500 -1 -1 2 150 60 4 3 100 5 3\n";
        let before = parse_stats_text(before_text).unwrap();
        let after = parse_stats_text(after_text).unwrap();
        let delta = compute_stats_delta(Some(&before), &after);
        assert_eq!(delta.deltas[0].time_improved, Some(true));
        assert_eq!(delta.deltas[0].best_time_before, Some(2000));
        assert_eq!(delta.deltas[0].best_time_after, Some(1500));
    }

    // --- Compute map progress from real data ---

    #[test]
    fn test_compute_map_progress_sample_stats_txt() {
        let text = concat!(
            "1\n34663\n",
            "MAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3\n",
            "MAP02 1 2 3 26043 -1 -1 1 91 83 71 2 83 137 5\n",
            "MAP31 1 31 0 -1 -1 -1 0 0 0 0 0 -1 -1 -1\n",
            "MAP35 1 35 4 294 294 -1 1 0 0 0 0 0 0 0\n",
        );
        let stats = parse_stats_text(text).unwrap();
        let progress = compute_map_progress(&stats);
        // MAP01 played, MAP02 played, MAP31 secret+unplayed, MAP35 played
        assert_eq!(progress.total, Some(3)); // 4 total - 1 secret = 3
        assert_eq!(progress.played, 3); // MAP01, MAP02, MAP35
        assert_eq!(progress.secret_total, Some(1)); // MAP31
        assert_eq!(progress.secret_played, 0); // MAP31 unplayed
    }

    #[test]
    fn test_is_secret_map_non_standard() {
        assert!(!is_secret_map("INTRO"));
        assert!(!is_secret_map("TITLEMAP"));
        assert!(!is_secret_map("MAP33"));
    }

    #[test]
    fn test_merge_stats_single() {
        let stats = parse_stats_text("1\n0\nMAP01 1 1 4 9734 -1 -1 1 128 102 1 0 113 9 1").unwrap();
        let merged = merge_stats(std::slice::from_ref(&stats));
        assert_eq!(merged.maps.len(), stats.maps.len());
        assert_eq!(merged.maps[0].best_skill, 4);
    }

    #[test]
    fn test_merge_stats_zeros_with_real_data() {
        // Simulates the WAD 100 bug: one stats file with zeros, one with real data
        let zeros = parse_stats_text(
            "1\n0\nMAP01 1 1 0 -1 -1 -1 0 0 0 0 0 -1 -1 -1\nMAP02 1 2 0 -1 -1 -1 0 0 0 0 0 -1 -1 -1",
        ).unwrap();
        let real = parse_stats_text(
            "1\n200\nMAP01 1 1 4 9734 -1 -1 1 128 102 1 0 113 9 1\nMAP02 1 2 4 11748 -1 -1 1 77 70 8 2 72 18 3",
        ).unwrap();

        let merged = merge_stats(&[zeros, real]);
        assert_eq!(merged.maps.len(), 2);
        // Should keep the real data, not the zeros
        let map01 = &merged.maps[0];
        assert_eq!(map01.best_skill, 4);
        assert_eq!(map01.best_time, 9734);
        assert_eq!(map01.kills, 102);
        assert_eq!(map01.total_kills, 113);
        assert_eq!(merged.header_total_kills, 200);
    }

    #[test]
    fn test_merge_stats_best_time_wins() {
        let fast = parse_stats_text("1\n0\nMAP01 1 1 4 5000 -1 -1 1 50 50 3 1 50 3 1").unwrap();
        let slow = parse_stats_text("1\n0\nMAP01 1 1 4 9000 -1 -1 2 50 50 3 1 50 3 1").unwrap();
        let merged = merge_stats(&[fast, slow]);
        assert_eq!(merged.maps[0].best_time, 5000); // fastest
        assert_eq!(merged.maps[0].total_exits, 2); // highest
    }
}
