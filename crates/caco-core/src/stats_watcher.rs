//! Stats collection for sourceports that don't write `stats.txt` natively.
//!
//! ZDoom-family sourceports (gzdoom, uzdoom, etc.) don't natively write
//! per-map stats files like dsda-doom does. This module bridges the gap by:
//!
//! 1. Ensuring a small ZScript PK3 mod exists that logs per-map exit stats via
//!    `Console.PrintfEx(PRINT_LOG, ...)` — written to the ZDoom log file.
//! 2. Injecting `-file <pk3> +logfile <path>` into the sourceport command.
//! 3. After the sourceport exits, parsing the log for `CACOSTATS|…` lines
//!    and writing a `stats.txt` that the existing stats infrastructure
//!    can consume.
//!
//! Helion supports `-levelstat`, which appends one line per map exit to a
//! single global `levelstat.txt` in its user data folder (not the `-savedir`).
//! The format looks like dsda's but the time fraction is unpadded
//! milliseconds rather than zero-padded centiseconds, so it gets its own
//! parser here. After each session the file is converted, merged into the
//! WAD's managed `stats.txt`, and truncated.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::config;
use crate::wad_stats::{self, MapStats, TICS_PER_SECOND, WadStats};

// ---------------------------------------------------------------------------
// PK3 mod management
// ---------------------------------------------------------------------------

// uzdoom 4.15pre+1355 (May 2026) regressed `GameInfo.AddEventHandlers` so the
// handler is recreated per-map and `WorldUnloaded` no longer fires for some
// transition paths (e.g. the `nextmap` console command). The handler keeps its
// `WorldUnloaded` reporting for the normal case, but also snapshots stats into
// user CVars and emits a fallback EXIT from `WorldLoaded` for the *previous*
// map when WorldUnloaded never fired. CVars are required because per-map
// handler instances cannot share state directly.
const ZSCRIPT_ZS: &str = r#"version "4.0"

class CacoStatsReporter : EventHandler
{
    transient CVar cvMap;
    transient CVar cvSkill;
    transient CVar cvMaptime;
    transient CVar cvKills, cvTotalKills;
    transient CVar cvItems, cvTotalItems;
    transient CVar cvSecrets, cvTotalSecrets;
    transient CVar cvReported;

    void InitCVars()
    {
        if (cvMap == null) cvMap = CVar.FindCVar("caco_prev_map");
        if (cvSkill == null) cvSkill = CVar.FindCVar("caco_prev_skill");
        if (cvMaptime == null) cvMaptime = CVar.FindCVar("caco_prev_maptime");
        if (cvKills == null) cvKills = CVar.FindCVar("caco_prev_kills");
        if (cvTotalKills == null) cvTotalKills = CVar.FindCVar("caco_prev_totalkills");
        if (cvItems == null) cvItems = CVar.FindCVar("caco_prev_items");
        if (cvTotalItems == null) cvTotalItems = CVar.FindCVar("caco_prev_totalitems");
        if (cvSecrets == null) cvSecrets = CVar.FindCVar("caco_prev_secrets");
        if (cvTotalSecrets == null) cvTotalSecrets = CVar.FindCVar("caco_prev_totalsecrets");
        if (cvReported == null) cvReported = CVar.FindCVar("caco_prev_reported");
    }

    void ReportExit(string mapName, int skill, int maptime, int k, int tk, int it, int tit, int sec, int tsec)
    {
        Console.PrintfEx(PRINT_LOG, "CACOSTATS|EXIT|%s|%d|%d|%d/%d|%d/%d|%d/%d",
            mapName, skill, maptime, k, tk, it, tit, sec, tsec);
    }

    override void WorldLoaded(WorldEvent e)
    {
        InitCVars();
        if (cvMap == null) return;

        string prevMap = cvMap.GetString();

        // Fallback: a previous map was tracked, this transition isn't a save
        // load, and WorldUnloaded never reported it - so emit EXIT for it now
        // using the last snapshot we captured in WorldTick.
        if (prevMap.Length() > 0
            && prevMap != level.MapName
            && !e.IsSaveGame
            && !e.IsReopen
            && !cvReported.GetBool())
        {
            ReportExit(prevMap,
                cvSkill.GetInt(),
                cvMaptime.GetInt(),
                cvKills.GetInt(), cvTotalKills.GetInt(),
                cvItems.GetInt(), cvTotalItems.GetInt(),
                cvSecrets.GetInt(), cvTotalSecrets.GetInt());
        }

        // Rebase tracking to the new map. Done for save loads too so the next
        // real exit on this map fires correctly.
        cvMap.SetString(level.MapName);
        cvReported.SetBool(false);
        cvSkill.SetInt(0);
        cvMaptime.SetInt(0);
        cvKills.SetInt(0); cvTotalKills.SetInt(0);
        cvItems.SetInt(0); cvTotalItems.SetInt(0);
        cvSecrets.SetInt(0); cvTotalSecrets.SetInt(0);
    }

    override void WorldTick()
    {
        InitCVars();
        if (cvMap == null || level == null) return;
        // Throttle to once per second to keep CVar writes off the hot path.
        if (level.maptime % 35 != 0) return;
        // Only update when the tracked map matches the current map.
        if (cvMap.GetString() != level.MapName) return;

        cvSkill.SetInt(G_SkillPropertyInt(SKILLP_ACSReturn));
        cvMaptime.SetInt(level.maptime);
        cvKills.SetInt(level.killed_monsters);
        cvTotalKills.SetInt(level.total_monsters);
        cvItems.SetInt(level.found_items);
        cvTotalItems.SetInt(level.total_items);
        cvSecrets.SetInt(level.found_secrets);
        cvTotalSecrets.SetInt(level.total_secrets);
    }

    override void WorldUnloaded(WorldEvent e)
    {
        InitCVars();
        // Save/reopen transitions are not player exits.
        if (e.IsSaveGame || e.IsReopen) return;

        ReportExit(level.MapName,
            G_SkillPropertyInt(SKILLP_ACSReturn),
            level.maptime,
            level.killed_monsters, level.total_monsters,
            level.found_items, level.total_items,
            level.found_secrets, level.total_secrets);

        if (cvReported != null) cvReported.SetBool(true);
    }
}
"#;

const MAPINFO: &str = r#"GameInfo
{
    AddEventHandlers = "CacoStatsReporter"
}
"#;

const CVARINFO: &str = r#"user noarchive string caco_prev_map = "";
user noarchive int caco_prev_skill = 0;
user noarchive int caco_prev_maptime = 0;
user noarchive int caco_prev_kills = 0;
user noarchive int caco_prev_totalkills = 0;
user noarchive int caco_prev_items = 0;
user noarchive int caco_prev_totalitems = 0;
user noarchive int caco_prev_secrets = 0;
user noarchive int caco_prev_totalsecrets = 0;
user noarchive bool caco_prev_reported = false;
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

    if pk3_path.exists() && !stats_mod_needs_refresh(&pk3_path) {
        return Ok(pk3_path);
    }

    let mods_dir = get_mods_dir();
    std::fs::create_dir_all(&mods_dir)?;

    write_stats_mod(&pk3_path)?;

    Ok(pk3_path)
}

fn read_pk3_lump(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut file = zip.by_name(name).ok()?;
    let mut out = String::new();
    file.read_to_string(&mut out).ok()?;
    Some(out)
}

fn stats_mod_needs_refresh(pk3_path: &Path) -> bool {
    let file = match std::fs::File::open(pk3_path) {
        Ok(file) => file,
        Err(_) => return true,
    };
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(_) => return true,
    };

    let Some(zscript) = read_pk3_lump(&mut zip, "zscript.zs") else {
        return true;
    };
    let Some(mapinfo) = read_pk3_lump(&mut zip, "MAPINFO") else {
        return true;
    };
    let Some(cvarinfo) = read_pk3_lump(&mut zip, "CVARINFO") else {
        return true;
    };

    zscript != ZSCRIPT_ZS || mapinfo != MAPINFO || cvarinfo != CVARINFO
}

fn write_stats_mod(pk3_path: &Path) -> crate::Result<()> {
    let file = std::fs::File::create(pk3_path)?;
    let mut zip = zip::ZipWriter::new(file);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("zscript.zs", options)
        .map_err(std::io::Error::other)?;
    zip.write_all(ZSCRIPT_ZS.as_bytes())?;

    zip.start_file("MAPINFO", options)
        .map_err(std::io::Error::other)?;
    zip.write_all(MAPINFO.as_bytes())?;

    zip.start_file("CVARINFO", options)
        .map_err(std::io::Error::other)?;
    zip.write_all(CVARINFO.as_bytes())?;

    zip.finish().map_err(std::io::Error::other)?;

    Ok(())
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

// CACOSTATS|EXIT|MAP01|3|1234|50/100|10/20|3/5
static CACOSTATS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"CACOSTATS\|EXIT\|([^|]+)\|(\d+)\|(\d+)\|(\d+)/(\d+)\|(\d+)/(\d+)\|(\d+)/(\d+)")
        .unwrap()
});

// uzdoom's own map header lines, e.g. "MAP03 - the lower depths" or "E1M1 - Hangar".
// We use these as a fallback transition detector when the EventHandler-based
// reporter can't see what's happening (e.g. after a save load in current uzdoom,
// where WorldLoaded / WorldUnloaded / WorldTick all stop firing).
static MAP_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Z][A-Z0-9_]{2,15}) - \S").unwrap());

/// Build a `MapLogEntry` for a map we know the player exited but for which the
/// ZScript reporter never emitted a CACOSTATS|EXIT line. Stats are marked
/// unknown (-1 totals) so the merge keeps any prior real numbers.
fn synthetic_entry(lump: String) -> MapLogEntry {
    MapLogEntry {
        lump,
        skill: -1,
        time_tics: -1,
        kills: 0,
        total_kills: -1,
        items: 0,
        total_items: -1,
        secrets: 0,
        total_secrets: -1,
    }
}

/// Parse a ZDoom log file for CACOSTATS lines, plus synthesise EXIT entries
/// for transitions seen in uzdoom's own map headers but missed by the reporter.
///
/// Returns the last (most up-to-date) entry for each map, preserving
/// the order maps were first seen.
fn parse_log(text: &str) -> Vec<MapLogEntry> {
    let mut latest: HashMap<String, MapLogEntry> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut visit_order: Vec<String> = Vec::new();

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
        } else if let Some(caps) = MAP_HEADER_RE.captures(line) {
            let lump = caps[1].to_string();
            // Collapse consecutive duplicates (quickload prints the same header).
            if visit_order.last().map(String::as_str) != Some(lump.as_str()) {
                visit_order.push(lump);
            }
        }
    }

    // Fallback: any visit_order[i] whose successor is a different map and which
    // didn't get a CACOSTATS|EXIT line was almost certainly exited - the player
    // had to leave it to reach the next one. We exclude TITLEMAP as a source
    // because the engine routes through it on game start, not via player exit.
    for pair in visit_order.windows(2) {
        let from = &pair[0];
        let to = &pair[1];
        if from == to || from == "TITLEMAP" {
            continue;
        }
        if !latest.contains_key(from) {
            order.push(from.clone());
            latest.insert(from.clone(), synthetic_entry(from.clone()));
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

    for entry in entries {
        // Negative tics means the entry is synthesised from a header-only
        // transition - no real time was captured. Keep time/totals unknown, but
        // mark best_skill=4 (matching the levelstat parser's "played at unknown
        // skill" convention) so `played_maps()` and the levelstats display
        // recognise the map as played.
        let (best_time, time_secs, best_skill) = if entry.time_tics < 0 {
            (-1, -1.0, 4)
        } else {
            (
                entry.time_tics,
                entry.time_tics as f64 / TICS_PER_SECOND,
                entry.skill + 1,
            )
        };

        maps.push(MapStats {
            lump: entry.lump.clone(),
            kills: entry.kills,
            total_kills: entry.total_kills,
            items: entry.items,
            total_items: entry.total_items,
            secrets: entry.secrets,
            total_secrets: entry.total_secrets,
            best_skill,
            best_time,
            total_exits: 1,
            time_secs,
            total_time_secs: -1.0,
            // Fields not available from zdoom log
            episode: 0,
            map_num: 0,
            best_max_time: -1,
            best_nm_time: -1,
            cumulative_kills: 0,
        });
    }

    WadStats {
        // ZDoom stats are cumulative once merged below, so store them in the
        // stats.txt-shaped format. Session deltas can diff total_exits instead
        // of treating the entire cumulative file as this session.
        format: "stats_txt".to_string(),
        maps,
        version: 1,
        header_total_kills: 0,
    }
}

/// Merge a session's exit entries into existing cumulative stats.
///
/// Duplicate lumps *within* the session also fold together here: exit counts
/// sum, counts keep the max, times keep the best. Shared by the zdoom log
/// collector and the helion levelstat collector.
fn merge_session_stats(existing: Option<WadStats>, session: WadStats) -> WadStats {
    let mut maps_by_lump: HashMap<String, MapStats> = existing
        .map(|stats| {
            stats
                .maps
                .into_iter()
                .map(|m| (m.lump.clone(), m))
                .collect()
        })
        .unwrap_or_default();

    for map in session.maps {
        match maps_by_lump.get_mut(&map.lump) {
            Some(existing) => {
                existing.best_skill = existing.best_skill.max(map.best_skill);
                existing.total_exits = existing.total_exits.max(0) + map.total_exits.max(0);
                existing.best_time = min_positive_i32(existing.best_time, map.best_time);
                existing.best_max_time =
                    min_positive_i32(existing.best_max_time, map.best_max_time);
                existing.best_nm_time = min_positive_i32(existing.best_nm_time, map.best_nm_time);
                existing.time_secs = min_positive_f64(existing.time_secs, map.time_secs);
                existing.total_time_secs = -1.0;
                existing.kills = existing.kills.max(map.kills);
                existing.items = existing.items.max(map.items);
                existing.secrets = existing.secrets.max(map.secrets);
                existing.total_kills = existing.total_kills.max(map.total_kills);
                existing.total_items = existing.total_items.max(map.total_items);
                existing.total_secrets = existing.total_secrets.max(map.total_secrets);
            }
            None => {
                maps_by_lump.insert(map.lump.clone(), map);
            }
        }
    }

    let mut maps: Vec<MapStats> = maps_by_lump.into_values().collect();
    maps.sort_by(|a, b| a.lump.cmp(&b.lump));

    for map in &mut maps {
        if map.best_time < 0 && map.time_secs >= 0.0 {
            map.best_time = (map.time_secs * TICS_PER_SECOND).round() as i32;
        }
        if map.time_secs < 0.0 && map.best_time >= 0 {
            map.time_secs = map.best_time as f64 / TICS_PER_SECOND;
        }
    }

    WadStats {
        format: "stats_txt".to_string(),
        maps,
        version: 1,
        header_total_kills: 0,
    }
}

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

// ---------------------------------------------------------------------------
// Post-play collection
// ---------------------------------------------------------------------------

/// After a zdoom-family sourceport exits, parse the log and write
/// a `stats.txt` file in the data directory.
///
/// If prior stats exist (from an older or current Caco version), the new
/// stats are merged with the old — keeping the best values per map, just
/// like dsda-doom's cumulative `stats.txt`, while incrementing exit counts
/// for maps completed in this ZDoom session.
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

    let stats_path = data_dir.join("stats.txt");
    let legacy_levelstat_path = data_dir.join("levelstat.txt");
    let existing = if stats_path.exists() {
        wad_stats::parse_stats_file(&stats_path).ok()
    } else if legacy_levelstat_path.exists() {
        wad_stats::parse_stats_file(&legacy_levelstat_path).ok()
    } else {
        None
    };

    let merged = merge_session_stats(existing, new_stats);
    let output = wad_stats::format_stats(&merged);
    if std::fs::write(&stats_path, &output).is_err() {
        return false;
    }

    // Retire the old generated levelstat.txt so stale periodic snapshots from
    // previous Caco versions do not keep getting merged into live progress.
    if legacy_levelstat_path.exists() {
        let _ = std::fs::remove_file(&legacy_levelstat_path);
    }

    true
}

// ---------------------------------------------------------------------------
// Helion levelstat collection
// ---------------------------------------------------------------------------

// Helion levelstat line: "MAP01 - 1:5.40 (1:5)  K: 59/59  I: 38/38  S: 1/3"
// Written via C# `$"{t.Minutes}:{t.Seconds}.{t.Milliseconds}"`: seconds and
// the fraction are unpadded, and the fraction is a raw milliseconds component
// (0-999) — unlike dsda's zero-padded centiseconds — so dsda's levelstat
// parser would silently misread these times. Minutes wrap at 60 (hours are
// lost by Helion itself; nothing to recover here).
static HELION_LEVELSTAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\S+)\s+-\s+(\d+):(\d+)\.(\d+)\s+\((\d+):(\d+)(?:\.\d+)?\)\s+K:\s*(\d+)/(\d+)\s+I:\s*(\d+)/(\d+)\s+S:\s*(\d+)/(\d+)",
    )
    .unwrap()
});

/// Path to Helion's global `levelstat.txt`.
///
/// Helion writes it to its user data folder regardless of `-savedir`:
/// `$XDG_CONFIG_HOME/Helion` on Linux, preferring a legacy lowercase
/// `helion` folder when one exists. Windows ("Saved Games") and portable
/// installs are not resolved; collection just no-ops for those.
pub fn helion_levelstat_path() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    let legacy = config_home.join("helion");
    let dir = if legacy.exists() {
        legacy
    } else {
        config_home.join("Helion")
    };
    Some(dir.join("levelstat.txt"))
}

/// Parse Helion levelstat text into a stats.txt-shaped `WadStats`.
///
/// One entry per exit line (duplicates fold later in `merge_session_stats`),
/// `total_exits = 1`, and `best_skill = 4` — levelstat carries no skill, so
/// mark "played at unknown skill" like the dsda levelstat parser does.
fn parse_helion_levelstat(text: &str) -> WadStats {
    let mut maps = Vec::new();
    for line in text.lines() {
        let Some(caps) = HELION_LEVELSTAT_RE.captures(line.trim()) else {
            continue;
        };
        let mins: f64 = caps[2].parse().unwrap_or(0.0);
        let secs: f64 = caps[3].parse().unwrap_or(0.0);
        let millis: f64 = caps[4].parse().unwrap_or(0.0);
        let time_secs = mins * 60.0 + secs + millis / 1000.0;

        maps.push(MapStats {
            lump: caps[1].to_string(),
            kills: caps[7].parse().unwrap_or(0),
            total_kills: caps[8].parse().unwrap_or(-1),
            items: caps[9].parse().unwrap_or(0),
            total_items: caps[10].parse().unwrap_or(-1),
            secrets: caps[11].parse().unwrap_or(0),
            total_secrets: caps[12].parse().unwrap_or(-1),
            best_skill: 4,
            best_time: (time_secs * TICS_PER_SECOND).round() as i32,
            total_exits: 1,
            time_secs,
            total_time_secs: -1.0,
            episode: 0,
            map_num: 0,
            best_max_time: -1,
            best_nm_time: -1,
            cumulative_kills: 0,
        });
    }

    WadStats {
        format: "stats_txt".to_string(),
        maps,
        version: 1,
        header_total_kills: 0,
    }
}

/// After a Helion session, consume the global levelstat file into the WAD's
/// managed `stats.txt` in `data_dir`.
///
/// Returns `true` if stats were merged and written.
pub fn collect_helion_stats(data_dir: &Path) -> bool {
    match helion_levelstat_path() {
        Some(path) => collect_helion_stats_from(&path, data_dir),
        None => false,
    }
}

fn collect_helion_stats_from(levelstat_path: &Path, data_dir: &Path) -> bool {
    let text = match std::fs::read_to_string(levelstat_path) {
        Ok(t) => t,
        Err(_) => return false,
    };

    let session = parse_helion_levelstat(&text);
    if session.maps.is_empty() {
        return false;
    }

    let stats_path = data_dir.join("stats.txt");
    let existing = if stats_path.exists() {
        wad_stats::parse_stats_file(&stats_path).ok()
    } else {
        None
    };

    let merged = merge_session_stats(existing, session);
    let output = wad_stats::format_stats(&merged);
    if std::fs::write(&stats_path, &output).is_err() {
        return false;
    }

    // Truncate the consumed file: it is global to Helion and only cleared on
    // the *next* `-levelstat` launch, so a later collect (any WAD) would
    // otherwise double-count these exits.
    let _ = std::fs::write(levelstat_path, "");

    true
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
                    CACOSTATS|EXIT|MAP01|3|3500|50/100|10/20|3/5\n\
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
        let log = "CACOSTATS|EXIT|MAP01|3|1050|10/100|5/20|1/5\n\
                    CACOSTATS|EXIT|MAP01|3|2100|30/100|8/20|2/5\n\
                    CACOSTATS|EXIT|MAP01|3|3500|50/100|10/20|3/5\n";
        let entries = parse_log(log);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kills, 50);
        assert_eq!(entries[0].time_tics, 3500);
    }

    #[test]
    fn test_parse_log_multiple_maps() {
        let log = "CACOSTATS|EXIT|MAP01|3|3500|50/100|10/20|3/5\n\
                    CACOSTATS|EXIT|MAP02|3|7000|80/80|15/15|2/2\n\
                    CACOSTATS|EXIT|MAP03|3|1750|20/50|5/10|0/1\n";
        let entries = parse_log(log);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].lump, "MAP01");
        assert_eq!(entries[1].lump, "MAP02");
        assert_eq!(entries[2].lump, "MAP03");
    }

    #[test]
    fn test_parse_log_preserves_map_order() {
        // MAP02 appears first, then MAP01
        let log = "CACOSTATS|EXIT|MAP02|3|7000|80/80|15/15|2/2\n\
                    CACOSTATS|EXIT|MAP01|3|3500|50/100|10/20|3/5\n";
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
    fn test_parse_log_header_fallback_save_load_scenario() {
        // Mirrors the actual uzdoom regression: TITLEMAP loads, save load
        // suppresses every EventHandler event for MAP03, exit to MAP04 fires
        // WorldLoaded for MAP04 with stale state -> reporter logs TITLEMAP
        // by mistake. MAP03 has no CACOSTATS line. The header fallback must
        // synthesise a MAP03 EXIT entry.
        let log = "TITLEMAP - Unnamed\n\
                   MAP03 - the lower depths\n\
                   Picked up a clip.\n\
                   MAP03 - the lower depths\n\
                   Picked up a clip.\n\
                   MAP04 - in the valley\n\
                   CACOSTATS|EXIT|TITLEMAP|2|105|0/0|0/0|0/0\n";
        let entries = parse_log(log);
        let lumps: Vec<&str> = entries.iter().map(|e| e.lump.as_str()).collect();
        assert!(
            lumps.contains(&"MAP03"),
            "expected MAP03 synthesised, got {:?}",
            lumps
        );
        let map03 = entries.iter().find(|e| e.lump == "MAP03").unwrap();
        assert_eq!(
            map03.time_tics, -1,
            "synthetic entry should mark time unknown"
        );
        assert_eq!(
            map03.total_kills, -1,
            "synthetic entry should mark totals unknown"
        );
    }

    #[test]
    fn test_parse_log_header_fallback_skips_titlemap_as_source() {
        // TITLEMAP -> MAP01 is just menu->play, not a player exit.
        let log = "TITLEMAP - Unnamed\nMAP01 - Entryway\n";
        let entries = parse_log(log);
        assert!(
            !entries.iter().any(|e| e.lump == "TITLEMAP"),
            "TITLEMAP must not be credited as exited"
        );
    }

    #[test]
    fn test_parse_log_header_fallback_collapses_quickloads() {
        // Repeated MAP03 headers (each quickload) must not produce a self-transition.
        let log = "MAP03 - foo\nMAP03 - foo\nMAP03 - foo\n";
        let entries = parse_log(log);
        assert!(entries.is_empty(), "no transition, nothing to synthesise");
    }

    #[test]
    fn test_parse_log_header_fallback_yields_to_real_cacostats() {
        // When a real CACOSTATS line exists for the source map, the fallback
        // must not overwrite it with a synthetic entry.
        let log = "MAP01 - Entryway\n\
                   CACOSTATS|EXIT|MAP01|3|3500|50/100|10/20|3/5\n\
                   MAP02 - Underhalls\n";
        let entries = parse_log(log);
        let map01 = entries.iter().find(|e| e.lump == "MAP01").unwrap();
        assert_eq!(
            map01.time_tics, 3500,
            "real CACOSTATS must win over fallback"
        );
        assert_eq!(map01.kills, 50);
    }

    #[test]
    fn test_reporter_source_is_exit_only() {
        // The reporter has WorldUnloaded (primary) and WorldLoaded (fallback)
        // and snapshots stats in WorldTick into CVars. It must only emit
        // CACOSTATS|EXIT lines (no live/periodic snapshots that would fool
        // completion detection).
        assert!(ZSCRIPT_ZS.contains("WorldUnloaded"));
        assert!(ZSCRIPT_ZS.contains("WorldLoaded"));
        assert!(ZSCRIPT_ZS.contains("CACOSTATS|EXIT|"));
        // The only Console.PrintfEx call must be the EXIT one (no LIVE/SNAPSHOT
        // variants).
        let printf_count = ZSCRIPT_ZS.matches("Console.PrintfEx").count();
        assert_eq!(printf_count, 1, "expected a single CACOSTATS emitter");
    }

    #[test]
    fn test_parse_log_ignores_untagged_snapshots() {
        let log = "CACOSTATS|MAP03|3|175|1/10|0/1|0/0\n\
                   CACOSTATS|LIVE|MAP03|3|210|2/10|0/1|0/0\n";
        assert!(parse_log(log).is_empty());
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
    fn test_entries_to_wad_stats_records_exit_times() {
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
        assert_eq!(stats.maps[0].total_time_secs, -1.0);
        assert!((stats.maps[1].time_secs - 30.0).abs() < 0.01);
        assert_eq!(stats.maps[1].total_time_secs, -1.0);
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
        assert_eq!(parsed.format, "stats_txt");
        assert_eq!(parsed.maps.len(), 1);
        assert_eq!(parsed.maps[0].lump, "MAP01");
        assert_eq!(parsed.maps[0].kills, 100);
        assert_eq!(parsed.maps[0].total_kills, 100);
        assert_eq!(parsed.maps[0].best_time, 2100);
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
    fn test_collect_zdoom_stats_writes_stats_txt() {
        let dir = tempfile::tempdir().unwrap();
        let log = "CACOSTATS|EXIT|MAP01|3|3500|50/100|10/20|3/5\n\
                    CACOSTATS|EXIT|MAP02|3|7000|80/80|15/15|2/2\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log).unwrap();

        assert!(collect_zdoom_stats(dir.path()));

        let stats_path = dir.path().join("stats.txt");
        assert!(stats_path.exists());

        let content = std::fs::read_to_string(&stats_path).unwrap();
        assert!(content.contains("MAP01"));
        assert!(content.contains("MAP02"));

        // Verify it parses
        let stats = crate::wad_stats::parse_stats_text(&content).unwrap();
        assert_eq!(stats.format, "stats_txt");
        assert_eq!(stats.maps.len(), 2);
        assert_eq!(stats.maps[0].best_time, 3500);
        assert_eq!(stats.maps[1].best_time, 7000);
    }

    #[test]
    fn test_collect_zdoom_stats_ignores_current_map_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let log = "CACOSTATS|EXIT|MAP04|3|7000|80/80|15/15|2/2\n\
                   CACOSTATS|MAP05|3|140|2/80|0/15|0/2\n\
                   CACOSTATS|LIVE|MAP05|3|175|3/80|0/15|0/2\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log).unwrap();

        assert!(collect_zdoom_stats(dir.path()));

        let content = std::fs::read_to_string(dir.path().join("stats.txt")).unwrap();
        let stats = wad_stats::parse_stats_text(&content).unwrap();
        assert_eq!(stats.maps.len(), 1);
        assert_eq!(stats.maps[0].lump, "MAP04");
        assert_eq!(stats.maps[0].best_time, 7000);
        assert!(!stats.maps.iter().any(|m| m.lump == "MAP05"));
    }

    #[test]
    fn test_collect_zdoom_stats_merges_across_sessions() {
        let dir = tempfile::tempdir().unwrap();

        // Session 1: play MAP01 and MAP02
        let log1 = "CACOSTATS|EXIT|MAP01|3|3500|50/100|10/20|3/5\n\
                     CACOSTATS|EXIT|MAP02|3|7000|80/80|15/15|2/2\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log1).unwrap();
        assert!(collect_zdoom_stats(dir.path()));

        // Session 2: play MAP03 (and replay MAP01 with better stats)
        let log2 = "CACOSTATS|EXIT|MAP01|3|2000|60/100|12/20|4/5\n\
                     CACOSTATS|EXIT|MAP03|3|5000|40/40|20/20|1/1\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log2).unwrap();
        assert!(collect_zdoom_stats(dir.path()));

        // Verify all 3 maps are present
        let content = std::fs::read_to_string(dir.path().join("stats.txt")).unwrap();
        let stats = wad_stats::parse_stats_text(&content).unwrap();
        assert_eq!(stats.format, "stats_txt");
        assert_eq!(stats.maps.len(), 3);

        let map_lumps: Vec<&str> = stats.maps.iter().map(|m| m.lump.as_str()).collect();
        assert!(map_lumps.contains(&"MAP01"));
        assert!(map_lumps.contains(&"MAP02"));
        assert!(map_lumps.contains(&"MAP03"));

        // MAP01 should have the best (max) kills from session 2
        let map01 = stats.maps.iter().find(|m| m.lump == "MAP01").unwrap();
        assert_eq!(map01.kills, 60); // max of 50, 60
        assert_eq!(map01.secrets, 4); // max of 3, 4
        assert_eq!(map01.best_time, 2000); // best time from session 2
        assert_eq!(map01.total_exits, 2); // replay increments exits
    }

    #[test]
    fn test_collect_zdoom_stats_delta_does_not_replay_history() {
        let dir = tempfile::tempdir().unwrap();

        let log1 = "CACOSTATS|EXIT|MAP01|3|3500|50/100|10/20|3/5\n\
                    CACOSTATS|EXIT|MAP02|3|3600|60/100|11/20|3/5\n\
                    CACOSTATS|EXIT|MAP03|3|3700|70/100|12/20|3/5\n\
                    CACOSTATS|EXIT|MAP04|3|3800|80/100|13/20|3/5\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log1).unwrap();
        assert!(collect_zdoom_stats(dir.path()));
        let before_text = std::fs::read_to_string(dir.path().join("stats.txt")).unwrap();
        let before = wad_stats::parse_stats_text(&before_text).unwrap();

        let log2 = "CACOSTATS|EXIT|MAP05|3|3900|90/100|14/20|3/5\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log2).unwrap();
        assert!(collect_zdoom_stats(dir.path()));
        let after_text = std::fs::read_to_string(dir.path().join("stats.txt")).unwrap();
        let after = wad_stats::parse_stats_text(&after_text).unwrap();

        let delta = wad_stats::compute_stats_delta(Some(&before), &after);
        assert_eq!(delta.maps_played, vec!["MAP05"]);
    }

    #[test]
    fn test_collect_zdoom_stats_retires_legacy_levelstat() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("levelstat.txt"),
            "MAP01 - 1:00.00 (1:00.00)  K: 10/10  I: 1/1  S: 0/0\n",
        )
        .unwrap();

        let log = "CACOSTATS|EXIT|MAP02|3|3500|20/20|2/2|1/1\n";
        std::fs::write(dir.path().join(LOG_FILENAME), log).unwrap();
        assert!(collect_zdoom_stats(dir.path()));

        assert!(dir.path().join("stats.txt").exists());
        assert!(!dir.path().join("levelstat.txt").exists());
    }

    #[test]
    fn test_stats_mod_refresh_detects_stale_pk3() {
        let dir = tempfile::tempdir().unwrap();
        let pk3_path = dir.path().join("test_stats.pk3");

        let file = std::fs::File::create(&pk3_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("zscript.zs", options).unwrap();
        zip.write_all(b"old reporter").unwrap();
        zip.start_file("MAPINFO", options).unwrap();
        zip.write_all(MAPINFO.as_bytes()).unwrap();
        zip.start_file("CVARINFO", options).unwrap();
        zip.write_all(CVARINFO.as_bytes()).unwrap();
        zip.finish().unwrap();

        assert!(stats_mod_needs_refresh(&pk3_path));
        write_stats_mod(&pk3_path).unwrap();
        assert!(!stats_mod_needs_refresh(&pk3_path));
    }

    #[test]
    fn test_stats_mod_refresh_detects_missing_cvarinfo() {
        // A pk3 from before the WorldLoaded-fallback rewrite has no CVARINFO
        // lump. It must be flagged for refresh so users get the fixed reporter.
        let dir = tempfile::tempdir().unwrap();
        let pk3_path = dir.path().join("legacy.pk3");

        let file = std::fs::File::create(&pk3_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("zscript.zs", options).unwrap();
        zip.write_all(ZSCRIPT_ZS.as_bytes()).unwrap();
        zip.start_file("MAPINFO", options).unwrap();
        zip.write_all(MAPINFO.as_bytes()).unwrap();
        zip.finish().unwrap();

        assert!(stats_mod_needs_refresh(&pk3_path));
    }

    // --- Helion levelstat ---

    #[test]
    fn test_parse_helion_levelstat_basic() {
        // Helion writes unpadded seconds/millis: 1:5.40 = 65.040s, not 65.40s
        let text = "MAP01 - 1:5.40 (1:5)  K: 59/59  I: 38/38  S: 1/3\n";
        let stats = parse_helion_levelstat(text);
        assert_eq!(stats.format, "stats_txt");
        assert_eq!(stats.maps.len(), 1);
        let m = &stats.maps[0];
        assert_eq!(m.lump, "MAP01");
        assert!((m.time_secs - 65.040).abs() < 0.001);
        assert_eq!(m.best_time, (65.040f64 * TICS_PER_SECOND).round() as i32);
        assert_eq!(m.kills, 59);
        assert_eq!(m.total_kills, 59);
        assert_eq!(m.items, 38);
        assert_eq!(m.total_items, 38);
        assert_eq!(m.secrets, 1);
        assert_eq!(m.total_secrets, 3);
        assert_eq!(m.total_exits, 1);
        assert_eq!(m.best_skill, 4);
    }

    #[test]
    fn test_parse_helion_levelstat_millis_not_centis() {
        // ".97" is 97 milliseconds in Helion's format, not dsda's 0.97s
        let text = "E1M1 - 0:32.97 (0:32)  K: 10/10  I: 5/5  S: 0/0\n";
        let stats = parse_helion_levelstat(text);
        assert!((stats.maps[0].time_secs - 32.097).abs() < 0.001);
    }

    #[test]
    fn test_parse_helion_levelstat_total_time_with_fraction() {
        // Defensive: accept a fractional total time even though current
        // Helion omits it.
        let text = "MAP02 - 2:10.5 (3:15.40)  K: 1/2  I: 0/0  S: 0/0\n";
        let stats = parse_helion_levelstat(text);
        assert_eq!(stats.maps.len(), 1);
        assert!((stats.maps[0].time_secs - 130.005).abs() < 0.001);
    }

    #[test]
    fn test_parse_helion_levelstat_ignores_noise() {
        let text = "garbage line\nMAP01 - 0:30.0 (0:30)  K: 1/1  I: 0/0  S: 0/0\n\n";
        let stats = parse_helion_levelstat(text);
        assert_eq!(stats.maps.len(), 1);
    }

    #[test]
    fn test_collect_helion_stats_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("levelstat.txt");
        assert!(!collect_helion_stats_from(&src, dir.path()));
    }

    #[test]
    fn test_collect_helion_stats_writes_and_truncates_source() {
        let src_dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("levelstat.txt");
        std::fs::write(
            &src,
            "MAP01 - 1:5.40 (1:5)  K: 59/59  I: 38/38  S: 1/3\n\
             MAP02 - 0:45.2 (1:50)  K: 30/40  I: 10/10  S: 0/2\n",
        )
        .unwrap();

        assert!(collect_helion_stats_from(&src, data_dir.path()));

        let content = std::fs::read_to_string(data_dir.path().join("stats.txt")).unwrap();
        let stats = wad_stats::parse_stats_text(&content).unwrap();
        assert_eq!(stats.format, "stats_txt");
        assert_eq!(stats.maps.len(), 2);
        assert_eq!(stats.maps[0].lump, "MAP01");
        assert_eq!(stats.maps[0].total_exits, 1);
        assert_eq!(stats.maps[1].lump, "MAP02");

        // Source must be truncated so a later collect can't double-count
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "");
        assert!(!collect_helion_stats_from(&src, data_dir.path()));
    }

    #[test]
    fn test_collect_helion_stats_repeat_exits_fold() {
        // The same map exited twice in one session sums exits, keeps best time
        let src_dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("levelstat.txt");
        std::fs::write(
            &src,
            "MAP01 - 1:30.0 (1:30)  K: 40/59  I: 20/38  S: 0/3\n\
             MAP01 - 1:5.40 (2:35)  K: 59/59  I: 38/38  S: 1/3\n",
        )
        .unwrap();

        assert!(collect_helion_stats_from(&src, data_dir.path()));

        let content = std::fs::read_to_string(data_dir.path().join("stats.txt")).unwrap();
        let stats = wad_stats::parse_stats_text(&content).unwrap();
        assert_eq!(stats.maps.len(), 1);
        let m = &stats.maps[0];
        assert_eq!(m.total_exits, 2);
        assert_eq!(m.kills, 59);
        assert_eq!(m.secrets, 1);
        assert_eq!(m.best_time, (65.040f64 * TICS_PER_SECOND).round() as i32);
    }

    #[test]
    fn test_collect_helion_stats_merges_across_sessions() {
        let src_dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("levelstat.txt");

        // Session 1: MAP01
        std::fs::write(&src, "MAP01 - 1:30.0 (1:30)  K: 40/59  I: 20/38  S: 0/3\n").unwrap();
        assert!(collect_helion_stats_from(&src, data_dir.path()));

        // Session 2: replay MAP01 (better), plus MAP02
        std::fs::write(
            &src,
            "MAP01 - 1:5.40 (1:5)  K: 59/59  I: 38/38  S: 1/3\n\
             MAP02 - 0:45.2 (1:50)  K: 30/40  I: 10/10  S: 0/2\n",
        )
        .unwrap();
        assert!(collect_helion_stats_from(&src, data_dir.path()));

        let content = std::fs::read_to_string(data_dir.path().join("stats.txt")).unwrap();
        let after = wad_stats::parse_stats_text(&content).unwrap();
        assert_eq!(after.maps.len(), 2);
        let map01 = after.maps.iter().find(|m| m.lump == "MAP01").unwrap();
        assert_eq!(map01.total_exits, 2);
        assert_eq!(map01.kills, 59);
        assert_eq!(
            map01.best_time,
            (65.040f64 * TICS_PER_SECOND).round() as i32
        );

        // Session deltas key off total_exits diffs, so only MAP02 is "new"
        // relative to a snapshot taken after session 1 plus the MAP01 replay.
        let map02 = after.maps.iter().find(|m| m.lump == "MAP02").unwrap();
        assert_eq!(map02.total_exits, 1);
    }

    #[test]
    fn test_ensure_stats_mod_creates_valid_pk3() {
        // Use a temp dir to avoid polluting the real mods dir
        let dir = tempfile::tempdir().unwrap();
        let pk3_path = dir.path().join("test_stats.pk3");

        write_stats_mod(&pk3_path).unwrap();

        let archive = zip::ZipArchive::new(std::fs::File::open(&pk3_path).unwrap()).unwrap();
        let names: Vec<&str> = (0..archive.len())
            .map(|i| archive.name_for_index(i).unwrap())
            .collect();
        assert!(names.contains(&"zscript.zs"));
        assert!(names.contains(&"MAPINFO"));
        assert!(names.contains(&"CVARINFO"));
    }
}
