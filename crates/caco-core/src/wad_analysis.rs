//! WAD analysis module for automated completion detection.
//!
//! Analyzes a WAD file to enumerate maps, detect exit linedefs, classify
//! secret/terminal/dead-end maps, and compute required map counts. Supports
//! vanilla/Boom LINEDEFS, UDMF TEXTMAP, and UMAPINFO overrides.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::utils::parse_wad_directory;

// ---------------------------------------------------------------------------
// Map name patterns
// ---------------------------------------------------------------------------

static DOOM1_MAP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^E(\d)M(\d)$").unwrap());
static DOOM2_MAP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^MAP(\d\d)$").unwrap());

/// Lumps that belong to a map definition (appear after the map marker).
const MAP_LUMPS: &[&str] = &[
    "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES",
    "SECTORS", "REJECT", "BLOCKMAP", "BEHAVIOR", "SCRIPTS", "TEXTMAP", "ENDMAP",
    "DIALOGUE", "ZNODES",
];

// ---------------------------------------------------------------------------
// Exit linedef specials
// ---------------------------------------------------------------------------

/// Vanilla/Boom normal exit linedef types.
const VANILLA_NORMAL_EXITS: &[u16] = &[11, 52, 197];
/// Vanilla/Boom secret exit linedef types.
const VANILLA_SECRET_EXITS: &[u16] = &[51, 124, 198];

/// UDMF normal exit specials.
const UDMF_NORMAL_EXITS: &[i32] = &[243, 74, 75]; // Exit_Normal, Teleport_NewMap, Teleport_EndGame
/// UDMF secret exit specials.
const UDMF_SECRET_EXITS: &[i32] = &[244]; // Exit_Secret

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-map analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapInfo {
    /// Map lump name (e.g., "MAP01", "E1M1").
    pub lump: String,
    /// Map has at least one normal exit linedef.
    pub has_normal_exit: bool,
    /// Map has at least one secret exit linedef.
    pub has_secret_exit: bool,
    /// Map is classified as a secret map.
    pub is_secret: bool,
    /// Map has no exit linedefs at all.
    pub is_dead_end: bool,
    /// Map is identified as the end-of-wad map.
    pub is_terminal: bool,
    /// Map is reachable through normal gameplay (not just via warp).
    /// Maps beyond the vanilla flow (e.g. MAP33+ in Doom 2) are unreachable.
    #[serde(default = "default_true")]
    pub reachable: bool,
}

fn default_true() -> bool {
    true
}

/// Complete WAD analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WadAnalysis {
    /// All maps found in the WAD with their analysis.
    pub maps: Vec<MapInfo>,
    /// Total number of maps.
    pub total_maps: usize,
    /// Number of maps required for completion (total - secret - dead-end).
    pub required_maps: usize,
    /// Lump names of secret maps.
    pub secret_maps: Vec<String>,
    /// Lump names of dead-end maps (no exits).
    pub dead_end_maps: Vec<String>,
    /// Lump name of the terminal (final) map, if identified.
    pub terminal_map: Option<String>,
    /// Whether the WAD contains a UMAPINFO lump.
    pub has_umapinfo: bool,
}

// ---------------------------------------------------------------------------
// UMAPINFO parsed structures
// ---------------------------------------------------------------------------

/// Parsed data from a single UMAPINFO map block.
#[derive(Debug, Clone, Default)]
struct UmapinfoEntry {
    next: Option<String>,
    nextsecret: Option<String>,
    has_endgame: bool,
}

// ---------------------------------------------------------------------------
// Core analysis function
// ---------------------------------------------------------------------------

/// Analyze a WAD file to enumerate maps and detect completion requirements.
///
/// Returns `None` if the data is not a valid WAD or contains no maps.
pub fn analyze_wad(wad_data: &[u8]) -> Option<WadAnalysis> {
    let directory = parse_wad_directory(wad_data);
    if directory.is_empty() {
        return None;
    }

    // Find map markers and their associated lumps
    let map_ranges = find_map_ranges(&directory);
    if map_ranges.is_empty() {
        return None;
    }

    // Check for UMAPINFO
    let umapinfo = parse_umapinfo_from_directory(wad_data, &directory);
    let has_umapinfo = umapinfo.is_some();

    // Determine map format (ExMy vs MAPxx)
    let first_map = &map_ranges[0].0;
    let is_doom1 = DOOM1_MAP_RE.is_match(first_map);

    // Collect all map lumps
    let all_map_lumps: Vec<String> = map_ranges.iter().map(|(name, _, _)| name.clone()).collect();

    // Analyze each map for exits
    let mut map_infos: Vec<MapInfo> = Vec::with_capacity(map_ranges.len());
    for (name, start_idx, end_idx) in &map_ranges {
        let (has_normal, has_secret) =
            detect_exits(wad_data, &directory, *start_idx, *end_idx);
        map_infos.push(MapInfo {
            lump: name.clone(),
            has_normal_exit: has_normal,
            has_secret_exit: has_secret,
            is_secret: false,
            is_dead_end: !has_normal && !has_secret,
            is_terminal: false,
            reachable: true, // default, refined below
        });
    }

    // Classify secret maps
    let secret_set = classify_secrets(&all_map_lumps, is_doom1, &umapinfo);
    for info in &mut map_infos {
        if secret_set.contains(&info.lump) {
            info.is_secret = true;
        }
    }

    // Identify terminal map
    let terminal = identify_terminal(&all_map_lumps, is_doom1, &umapinfo, &map_infos);
    if let Some(ref term) = terminal {
        for info in &mut map_infos {
            if info.lump == *term {
                info.is_terminal = true;
            }
        }
    }

    // Compute reachability: mark maps that can't be reached through normal play
    let reachable_set = compute_reachability(&all_map_lumps, is_doom1, &umapinfo, &map_infos);
    for info in &mut map_infos {
        info.reachable = reachable_set.contains(&info.lump);
    }

    // Build result vectors
    let secret_maps: Vec<String> = map_infos
        .iter()
        .filter(|m| m.is_secret)
        .map(|m| m.lump.clone())
        .collect();

    // Dead-end maps: no exits AND not secret AND not the terminal map
    // (terminal map with no exits is expected — it's the credits stopper)
    let dead_end_maps: Vec<String> = map_infos
        .iter()
        .filter(|m| m.is_dead_end && !m.is_secret && !m.is_terminal)
        .map(|m| m.lump.clone())
        .collect();

    let total_maps = map_infos.len();
    // Required = reachable, non-secret, non-dead-end, non-terminal-dead-end.
    // Unreachable count excludes maps already subtracted by other categories
    // (secret, dead-end, terminal dead-end) to avoid double-counting.
    let terminal_excluded = map_infos
        .iter()
        .any(|m| m.is_terminal && m.is_dead_end);
    let unreachable_count = map_infos
        .iter()
        .filter(|m| !m.reachable && !m.is_secret && !m.is_dead_end)
        .count();
    let required_maps = total_maps
        - secret_maps.len()
        - dead_end_maps.len()
        - unreachable_count
        - if terminal_excluded { 1 } else { 0 };

    Some(WadAnalysis {
        maps: map_infos,
        total_maps,
        required_maps,
        secret_maps,
        dead_end_maps,
        terminal_map: terminal,
        has_umapinfo,
    })
}

// ---------------------------------------------------------------------------
// Map range detection
// ---------------------------------------------------------------------------

/// Find map markers in the directory and their associated lump ranges.
/// Returns `(map_name, start_index, end_index)` where start_index is the
/// map marker's position and end_index is exclusive.
fn find_map_ranges(directory: &[(String, u32, u32)]) -> Vec<(String, usize, usize)> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < directory.len() {
        let name = &directory[i].0;
        if is_map_marker(name) {
            let start = i;
            i += 1;
            // Consume all map-associated lumps
            while i < directory.len() && is_map_lump(&directory[i].0) {
                i += 1;
            }
            ranges.push((name.clone(), start, i));
        } else {
            i += 1;
        }
    }
    ranges
}

/// Check if a lump name is a map marker (ExMy or MAPxx).
fn is_map_marker(name: &str) -> bool {
    DOOM1_MAP_RE.is_match(name) || DOOM2_MAP_RE.is_match(name)
}

/// Check if a lump name is a map-associated lump.
fn is_map_lump(name: &str) -> bool {
    MAP_LUMPS.contains(&name)
}

// ---------------------------------------------------------------------------
// Exit detection
// ---------------------------------------------------------------------------

/// Detect normal and secret exits for a map.
/// Returns `(has_normal_exit, has_secret_exit)`.
fn detect_exits(
    wad_data: &[u8],
    directory: &[(String, u32, u32)],
    start_idx: usize,
    end_idx: usize,
) -> (bool, bool) {
    let map_lumps = &directory[start_idx..end_idx];

    // Check for UDMF (TEXTMAP lump)
    if let Some(textmap) = find_lump_data(wad_data, map_lumps, "TEXTMAP") {
        return detect_udmf_exits(&textmap);
    }

    // Vanilla/Boom: parse LINEDEFS
    if let Some(linedefs) = find_lump_data(wad_data, map_lumps, "LINEDEFS") {
        return detect_vanilla_exits(&linedefs);
    }

    (false, false)
}

/// Extract lump data by name from a slice of directory entries.
fn find_lump_data(
    wad_data: &[u8],
    lumps: &[(String, u32, u32)],
    lump_name: &str,
) -> Option<Vec<u8>> {
    for (name, offset, size) in lumps {
        if name == lump_name && *size > 0 {
            let off = *offset as usize;
            let sz = *size as usize;
            if off + sz <= wad_data.len() {
                return Some(wad_data[off..off + sz].to_vec());
            }
        }
    }
    None
}

/// Parse vanilla/Boom LINEDEFS lump for exit specials.
///
/// Each linedef is 14 bytes: v1(2), v2(2), flags(2), special(2), tag(2), front(2), back(2).
fn detect_vanilla_exits(linedefs: &[u8]) -> (bool, bool) {
    let mut has_normal = false;
    let mut has_secret = false;

    let linedef_size = 14;
    let count = linedefs.len() / linedef_size;

    for i in 0..count {
        let base = i * linedef_size;
        if base + 8 > linedefs.len() {
            break;
        }
        let special = u16::from_le_bytes([linedefs[base + 6], linedefs[base + 7]]);

        if VANILLA_NORMAL_EXITS.contains(&special) {
            has_normal = true;
        }
        if VANILLA_SECRET_EXITS.contains(&special) {
            has_secret = true;
        }
    }

    (has_normal, has_secret)
}

/// Parse UDMF TEXTMAP for exit specials.
///
/// Simple regex approach: look for `special = NNN` within linedef blocks.
fn detect_udmf_exits(textmap_data: &[u8]) -> (bool, bool) {
    let text = String::from_utf8_lossy(textmap_data);
    let mut has_normal = false;
    let mut has_secret = false;

    // Match linedef blocks and extract specials
    static LINEDEF_BLOCK_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)linedef\s*\{([^}]*)\}").unwrap());
    static SPECIAL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)special\s*=\s*(\d+)").unwrap());

    for block in LINEDEF_BLOCK_RE.captures_iter(&text) {
        let body = &block[1];
        if let Some(caps) = SPECIAL_RE.captures(body)
            && let Ok(special) = caps[1].parse::<i32>()
        {
            if UDMF_NORMAL_EXITS.contains(&special) {
                has_normal = true;
            }
            if UDMF_SECRET_EXITS.contains(&special) {
                has_secret = true;
            }
        }
    }

    (has_normal, has_secret)
}

// ---------------------------------------------------------------------------
// UMAPINFO parsing
// ---------------------------------------------------------------------------

/// Read a text lump from the WAD directory.
fn read_lump_text(
    wad_data: &[u8],
    directory: &[(String, u32, u32)],
    lump_name: &str,
) -> Option<String> {
    for (name, offset, size) in directory {
        if name == lump_name && *size > 0 {
            let off = *offset as usize;
            let sz = *size as usize;
            if off + sz <= wad_data.len() {
                return Some(String::from_utf8_lossy(&wad_data[off..off + sz]).to_string());
            }
        }
    }
    None
}

/// Parse UMAPINFO lump from WAD directory.
fn parse_umapinfo_from_directory(
    wad_data: &[u8],
    directory: &[(String, u32, u32)],
) -> Option<HashMap<String, UmapinfoEntry>> {
    let text = read_lump_text(wad_data, directory, "UMAPINFO")?;
    Some(parse_umapinfo(&text))
}

/// Parse UMAPINFO text into map entries.
///
/// Format:
/// ```text
/// MAP MAP01
/// {
///     next = "MAP02"
///     nextsecret = "MAP31"
///     endgame = true
/// }
/// ```
fn parse_umapinfo(text: &str) -> HashMap<String, UmapinfoEntry> {
    static MAP_HEADER_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^\s*MAP\s+(\S+)").unwrap());
    static NEXT_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)^\s*next\s*=\s*"?(\w+)"?"#).unwrap());
    static NEXTSECRET_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)^\s*nextsecret\s*=\s*"?(\w+)"?"#).unwrap());
    static ENDGAME_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^\s*(endgame|endpic|endcast|endbunny)\s*=").unwrap()
    });

    let mut entries = HashMap::new();
    let mut current_map: Option<String> = None;
    let mut current_entry = UmapinfoEntry::default();

    for line in text.lines() {
        if let Some(caps) = MAP_HEADER_RE.captures(line) {
            // Save previous entry
            if let Some(map_name) = current_map.take() {
                entries.insert(map_name, current_entry);
                current_entry = UmapinfoEntry::default();
            }
            current_map = Some(caps[1].to_uppercase());
            continue;
        }

        if current_map.is_some() {
            if let Some(caps) = NEXT_RE.captures(line) {
                current_entry.next = Some(caps[1].to_uppercase());
            } else if let Some(caps) = NEXTSECRET_RE.captures(line) {
                current_entry.nextsecret = Some(caps[1].to_uppercase());
            } else if ENDGAME_RE.is_match(line) {
                current_entry.has_endgame = true;
            }
        }
    }

    // Save last entry
    if let Some(map_name) = current_map {
        entries.insert(map_name, current_entry);
    }

    entries
}

// ---------------------------------------------------------------------------
// Secret map classification
// ---------------------------------------------------------------------------

/// Classify which maps are secret.
fn classify_secrets(
    all_maps: &[String],
    is_doom1: bool,
    umapinfo: &Option<HashMap<String, UmapinfoEntry>>,
) -> HashSet<String> {
    if let Some(umi) = umapinfo {
        classify_secrets_umapinfo(all_maps, umi)
    } else {
        classify_secrets_vanilla(all_maps, is_doom1)
    }
}

/// Classify secret maps using vanilla conventions.
///
/// Doom 2 (MAPxx): MAP31 and MAP32 are secret if they exist.
/// Doom 1 (ExMy): E*M9 is secret for each episode.
fn classify_secrets_vanilla(all_maps: &[String], is_doom1: bool) -> HashSet<String> {
    let map_set: HashSet<&str> = all_maps.iter().map(|s| s.as_str()).collect();
    let mut secrets = HashSet::new();

    if is_doom1 {
        // ExM9 maps are secret
        for map in all_maps {
            if let Some(caps) = DOOM1_MAP_RE.captures(map)
                && &caps[2] == "9"
                && map_set.contains(map.as_str())
            {
                secrets.insert(map.clone());
            }
        }
    } else {
        // MAP31 and MAP32 are secret
        for name in &["MAP31", "MAP32"] {
            if map_set.contains(*name) {
                secrets.insert(name.to_string());
            }
        }
    }

    secrets
}

/// Classify secret maps using UMAPINFO reachability analysis.
///
/// A map is secret ONLY IF it is reachable exclusively via `nextsecret`
/// and NEVER appears in any `next` chain. This avoids the Poogers edge
/// case where `nextsecret` is used for normal progression.
fn classify_secrets_umapinfo(
    all_maps: &[String],
    umapinfo: &HashMap<String, UmapinfoEntry>,
) -> HashSet<String> {
    let map_set: HashSet<&str> = all_maps.iter().map(|s| s.as_str()).collect();

    // Build sets of maps reachable via `next` and `nextsecret`
    let mut reached_by_next: HashSet<String> = HashSet::new();
    let mut reached_by_nextsecret: HashSet<String> = HashSet::new();

    for entry in umapinfo.values() {
        if let Some(ref next) = entry.next {
            reached_by_next.insert(next.clone());
        }
        if let Some(ref ns) = entry.nextsecret {
            reached_by_nextsecret.insert(ns.clone());
        }
    }

    // A map is secret if it's reachable ONLY via nextsecret, never via next,
    // AND it actually exists in the WAD
    let mut secrets = HashSet::new();
    for map in &reached_by_nextsecret {
        if !reached_by_next.contains(map) && map_set.contains(map.as_str()) {
            secrets.insert(map.clone());
        }
    }

    secrets
}

// ---------------------------------------------------------------------------
// Terminal map identification
// ---------------------------------------------------------------------------

/// Identify the terminal (final) map.
///
/// Priority:
/// 1. UMAPINFO: map has endgame/endpic/endcast/endbunny
/// 2. UMAPINFO: map's next points to itself (self-loop)
/// 3. UMAPINFO: highest map number with no next field defined
/// 4. Standard conventions: MAP30 for MAPxx, E*M8 for ExMy
/// 5. Fallback: highest-numbered non-secret map
fn identify_terminal(
    all_maps: &[String],
    is_doom1: bool,
    umapinfo: &Option<HashMap<String, UmapinfoEntry>>,
    map_infos: &[MapInfo],
) -> Option<String> {
    let secret_set: HashSet<&str> = map_infos
        .iter()
        .filter(|m| m.is_secret)
        .map(|m| m.lump.as_str())
        .collect();

    if let Some(umi) = umapinfo {
        // Priority 1: endgame/endpic/endcast/endbunny
        for (map_name, entry) in umi {
            if entry.has_endgame && all_maps.contains(map_name) {
                return Some(map_name.clone());
            }
        }

        // Priority 2: self-loop (next points to itself)
        for (map_name, entry) in umi {
            if let Some(next) = &entry.next
                && next == map_name
                && all_maps.contains(map_name)
            {
                return Some(map_name.clone());
            }
        }

        // Priority 3: highest map in UMAPINFO with no `next` defined
        // (among maps that actually exist in the WAD and aren't secret)
        let mut candidates: Vec<&String> = umi
            .iter()
            .filter(|(name, entry)| {
                entry.next.is_none()
                    && !entry.has_endgame
                    && all_maps.contains(name)
                    && !secret_set.contains(name.as_str())
            })
            .map(|(name, _)| name)
            .collect();
        candidates.sort_by_key(|a| map_sort_key(a));
        if let Some(last) = candidates.last() {
            return Some((*last).clone());
        }
    }

    let map_set: HashSet<&str> = all_maps.iter().map(|s| s.as_str()).collect();

    // Priority 4: standard conventions
    if is_doom1 {
        // Find highest episode's E*M8
        let mut best: Option<String> = None;
        for map in all_maps {
            if let Some(caps) = DOOM1_MAP_RE.captures(map)
                && &caps[2] == "8"
                && best.as_ref().is_none_or(|b| map > b)
            {
                best = Some(map.clone());
            }
        }
        if let Some(term) = best {
            return Some(term);
        }
    } else if map_set.contains("MAP30") && !secret_set.contains("MAP30") {
        return Some("MAP30".to_string());
    }

    // Priority 5: highest-numbered non-secret map
    let mut non_secret: Vec<&String> = all_maps
        .iter()
        .filter(|m| !secret_set.contains(m.as_str()))
        .collect();
    non_secret.sort_by_key(|a| map_sort_key(a));
    non_secret.last().map(|m| (*m).clone())
}

/// Generate a sort key for map names to enable numeric sorting.
fn map_sort_key(name: &str) -> (u32, u32) {
    if let Some(caps) = DOOM2_MAP_RE.captures(name)
        && let Ok(num) = caps[1].parse::<u32>()
    {
        return (0, num);
    }
    if let Some(caps) = DOOM1_MAP_RE.captures(name)
        && let Ok(ep) = caps[1].parse::<u32>()
        && let Ok(map) = caps[2].parse::<u32>()
    {
        return (ep, map);
    }
    (999, 999) // Unknown format sorts last
}

// ---------------------------------------------------------------------------
// Reachability analysis
// ---------------------------------------------------------------------------

/// Compute the set of maps reachable from the start through normal gameplay.
///
/// For vanilla WADs (no UMAPINFO), the map flow is hardcoded by the engine:
/// - Doom 2 (MAPxx): MAP01→MAP02→...→MAP30; MAP15 secret→MAP31; MAP31 secret→MAP32; MAP32→MAP16
/// - Doom 1 (ExMy): E1M1→...→E1M8; ExMy secret→ExM9; ExM9→ExM(y+1) where y was the source
///
/// For UMAPINFO WADs, reachability follows the explicit `next`/`nextsecret` chains.
fn compute_reachability(
    all_maps: &[String],
    is_doom1: bool,
    umapinfo: &Option<HashMap<String, UmapinfoEntry>>,
    map_infos: &[MapInfo],
) -> HashSet<String> {
    let map_set: HashSet<&str> = all_maps.iter().map(|s| s.as_str()).collect();

    if let Some(umi) = umapinfo {
        return compute_reachability_umapinfo(all_maps, umi, map_infos);
    }

    if is_doom1 {
        compute_reachability_doom1(&map_set, map_infos)
    } else {
        compute_reachability_doom2(&map_set, map_infos)
    }
}

/// Vanilla Doom 2 reachability: MAP01→MAP02→...→MAP30, secret exits to MAP31/32.
fn compute_reachability_doom2(
    map_set: &HashSet<&str>,
    map_infos: &[MapInfo],
) -> HashSet<String> {
    let info_map: HashMap<&str, &MapInfo> = map_infos.iter().map(|m| (m.lump.as_str(), m)).collect();
    let mut reachable = HashSet::new();

    // Linear progression MAP01 → MAP30
    for num in 1..=30 {
        let name = format!("MAP{num:02}");
        if map_set.contains(name.as_str()) {
            reachable.insert(name);
        }
    }

    // Secret maps: MAP15 secret exit → MAP31, MAP31 secret exit → MAP32
    if let Some(m15) = info_map.get("MAP15")
        && m15.has_secret_exit
        && map_set.contains("MAP31")
    {
        reachable.insert("MAP31".to_string());
    }
    if let Some(m31) = info_map.get("MAP31")
        && m31.has_secret_exit
        && map_set.contains("MAP32")
    {
        reachable.insert("MAP32".to_string());
    }

    reachable
}

/// Vanilla Doom 1 reachability: ExM1→...→ExM8, secret exit → ExM9.
fn compute_reachability_doom1(
    map_set: &HashSet<&str>,
    map_infos: &[MapInfo],
) -> HashSet<String> {
    let info_map: HashMap<&str, &MapInfo> = map_infos.iter().map(|m| (m.lump.as_str(), m)).collect();
    let mut reachable = HashSet::new();

    // Find all episodes present
    let episodes: HashSet<u32> = map_infos
        .iter()
        .filter_map(|m| DOOM1_MAP_RE.captures(&m.lump))
        .filter_map(|caps| caps[1].parse::<u32>().ok())
        .collect();

    for ep in &episodes {
        // Linear progression ExM1 → ExM8
        for map in 1..=8 {
            let name = format!("E{ep}M{map}");
            if map_set.contains(name.as_str()) {
                reachable.insert(name);
            }
        }
        // Secret map: any map in the episode with a secret exit → ExM9
        let has_secret = (1..=8).any(|m| {
            let name = format!("E{ep}M{m}");
            info_map.get(name.as_str()).is_some_and(|i| i.has_secret_exit)
        });
        if has_secret {
            let secret = format!("E{ep}M9");
            if map_set.contains(secret.as_str()) {
                reachable.insert(secret);
            }
        }
    }

    reachable
}

/// UMAPINFO reachability: follow `next`/`nextsecret` chains from the first map.
///
/// Many WADs only define UMAPINFO for a few maps (e.g. to set endgame on the
/// final map). Maps without a UMAPINFO entry, or with an entry but no `next`
/// field, fall back to vanilla map progression conventions.
fn compute_reachability_umapinfo(
    all_maps: &[String],
    umapinfo: &HashMap<String, UmapinfoEntry>,
    map_infos: &[MapInfo],
) -> HashSet<String> {
    let map_set: HashSet<&str> = all_maps.iter().map(|s| s.as_str()).collect();
    let info_map: HashMap<&str, &MapInfo> = map_infos.iter().map(|m| (m.lump.as_str(), m)).collect();
    let mut reachable = HashSet::new();
    let mut queue = std::collections::VecDeque::new();

    // Start from the first map
    if let Some(first) = all_maps.first() {
        queue.push_back(first.clone());
    }

    while let Some(current) = queue.pop_front() {
        if !map_set.contains(current.as_str()) || reachable.contains(&current) {
            continue;
        }
        reachable.insert(current.clone());

        let entry = umapinfo.get(&current);

        // Check for endgame — this map terminates the chain
        if entry.is_some_and(|e| e.has_endgame) {
            continue;
        }

        // Follow explicit UMAPINFO next if defined
        let mut has_explicit_next = false;
        if let Some(e) = entry {
            if let Some(ref next) = e.next
                && next != &current
            {
                queue.push_back(next.clone());
                has_explicit_next = true;
            }
            if let Some(ref ns) = e.nextsecret
                && info_map.get(current.as_str()).is_some_and(|m| m.has_secret_exit)
            {
                queue.push_back(ns.clone());
            }
        }

        // Fall back to vanilla conventions when no explicit next is defined
        if !has_explicit_next {
            if let Some(caps) = DOOM2_MAP_RE.captures(&current)
                && let Ok(num) = caps[1].parse::<u32>()
                && num < 30
            {
                let next = format!("MAP{:02}", num + 1);
                queue.push_back(next);
            }
            if let Some(caps) = DOOM1_MAP_RE.captures(&current)
                && let Ok(ep) = caps[1].parse::<u32>()
                && let Ok(map) = caps[2].parse::<u32>()
                && map < 8
            {
                let next = format!("E{ep}M{}", map + 1);
                queue.push_back(next);
            }
        }

        // Vanilla secret exits (always check, even for UMAPINFO maps,
        // unless nextsecret is explicitly defined)
        let has_nextsecret = entry.is_some_and(|e| e.nextsecret.is_some());
        if !has_nextsecret {
            if current == "MAP15"
                && info_map.get("MAP15").is_some_and(|m| m.has_secret_exit)
            {
                queue.push_back("MAP31".to_string());
            }
            if current == "MAP31"
                && info_map.get("MAP31").is_some_and(|m| m.has_secret_exit)
            {
                queue.push_back("MAP32".to_string());
            }
        }
    }

    reachable
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // WAD construction helpers
    // -----------------------------------------------------------------------

    /// Build a minimal WAD with specific lumps.
    fn build_wad(lumps: &[(&str, &[u8])]) -> Vec<u8> {
        let mut wad = Vec::new();
        let num_lumps = lumps.len() as i32;
        let header_size = 12;
        let mut data_start = header_size;
        let mut entries: Vec<(String, u32, u32)> = Vec::new();
        let mut data_blob = Vec::new();

        for (name, data) in lumps {
            entries.push((name.to_string(), data_start as u32, data.len() as u32));
            data_blob.extend_from_slice(data);
            data_start += data.len();
        }

        let dir_offset = data_start as i32;
        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&num_lumps.to_le_bytes());
        wad.extend_from_slice(&dir_offset.to_le_bytes());
        wad.extend_from_slice(&data_blob);

        for (name, offset, size) in &entries {
            wad.extend_from_slice(&offset.to_le_bytes());
            wad.extend_from_slice(&size.to_le_bytes());
            let mut name_bytes = [0u8; 8];
            for (i, &b) in name.as_bytes().iter().take(8).enumerate() {
                name_bytes[i] = b;
            }
            wad.extend_from_slice(&name_bytes);
        }

        wad
    }

    /// Build a vanilla linedef with the given special type.
    fn make_linedef(special: u16) -> [u8; 14] {
        let mut ld = [0u8; 14];
        ld[6] = (special & 0xFF) as u8;
        ld[7] = (special >> 8) as u8;
        ld
    }

    /// Build a LINEDEFS lump from multiple linedef specials.
    fn build_linedefs(specials: &[u16]) -> Vec<u8> {
        let mut data = Vec::new();
        for &s in specials {
            data.extend_from_slice(&make_linedef(s));
        }
        data
    }

    /// Build a TEXTMAP lump with linedef specials.
    fn build_textmap(specials: &[i32]) -> Vec<u8> {
        let mut text = String::new();
        text.push_str("namespace = \"zdoom\";\n");
        for &s in specials {
            text.push_str(&format!(
                "linedef {{\n  special = {};\n  v1 = 0;\n  v2 = 1;\n}}\n",
                s
            ));
        }
        text.into_bytes()
    }

    // -----------------------------------------------------------------------
    // Linedef parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_vanilla_normal_exit() {
        let linedefs = build_linedefs(&[0, 0, 11, 0]); // one normal exit
        let (normal, secret) = detect_vanilla_exits(&linedefs);
        assert!(normal);
        assert!(!secret);
    }

    #[test]
    fn test_vanilla_secret_exit() {
        let linedefs = build_linedefs(&[0, 51, 0]); // one secret exit
        let (normal, secret) = detect_vanilla_exits(&linedefs);
        assert!(!normal);
        assert!(secret);
    }

    #[test]
    fn test_vanilla_both_exits() {
        let linedefs = build_linedefs(&[11, 51]);
        let (normal, secret) = detect_vanilla_exits(&linedefs);
        assert!(normal);
        assert!(secret);
    }

    #[test]
    fn test_vanilla_no_exits() {
        let linedefs = build_linedefs(&[0, 1, 2, 3, 4, 5]);
        let (normal, secret) = detect_vanilla_exits(&linedefs);
        assert!(!normal);
        assert!(!secret);
    }

    #[test]
    fn test_vanilla_all_exit_types() {
        // Normal: 11, 52, 197
        // Secret: 51, 124, 198
        for &s in VANILLA_NORMAL_EXITS {
            let linedefs = build_linedefs(&[s]);
            let (normal, secret) = detect_vanilla_exits(&linedefs);
            assert!(normal, "special {} should be a normal exit", s);
            assert!(!secret, "special {} should not be a secret exit", s);
        }
        for &s in VANILLA_SECRET_EXITS {
            let linedefs = build_linedefs(&[s]);
            let (normal, secret) = detect_vanilla_exits(&linedefs);
            assert!(!normal, "special {} should not be a normal exit", s);
            assert!(secret, "special {} should be a secret exit", s);
        }
    }

    #[test]
    fn test_vanilla_empty_linedefs() {
        let (normal, secret) = detect_vanilla_exits(&[]);
        assert!(!normal);
        assert!(!secret);
    }

    // -----------------------------------------------------------------------
    // UDMF parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_udmf_normal_exit() {
        let textmap = build_textmap(&[243]); // Exit_Normal
        let (normal, secret) = detect_udmf_exits(&textmap);
        assert!(normal);
        assert!(!secret);
    }

    #[test]
    fn test_udmf_secret_exit() {
        let textmap = build_textmap(&[244]); // Exit_Secret
        let (normal, secret) = detect_udmf_exits(&textmap);
        assert!(!normal);
        assert!(secret);
    }

    #[test]
    fn test_udmf_teleport_newmap() {
        let textmap = build_textmap(&[74]); // Teleport_NewMap
        let (normal, secret) = detect_udmf_exits(&textmap);
        assert!(normal);
        assert!(!secret);
    }

    #[test]
    fn test_udmf_teleport_endgame() {
        let textmap = build_textmap(&[75]); // Teleport_EndGame
        let (normal, secret) = detect_udmf_exits(&textmap);
        assert!(normal);
        assert!(!secret);
    }

    #[test]
    fn test_udmf_no_exits() {
        let textmap = build_textmap(&[0, 1, 80]);
        let (normal, secret) = detect_udmf_exits(&textmap);
        assert!(!normal);
        assert!(!secret);
    }

    // -----------------------------------------------------------------------
    // UMAPINFO parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_umapinfo_basic() {
        let text = r#"
MAP MAP01
{
    next = "MAP02"
    nextsecret = "MAP31"
}

MAP MAP02
{
    next = "MAP03"
}
"#;
        let entries = parse_umapinfo(text);
        assert_eq!(entries.len(), 2);

        let map01 = &entries["MAP01"];
        assert_eq!(map01.next, Some("MAP02".to_string()));
        assert_eq!(map01.nextsecret, Some("MAP31".to_string()));
        assert!(!map01.has_endgame);

        let map02 = &entries["MAP02"];
        assert_eq!(map02.next, Some("MAP03".to_string()));
        assert_eq!(map02.nextsecret, None);
    }

    #[test]
    fn test_parse_umapinfo_endgame() {
        let text = r#"
MAP MAP30
{
    endgame = true
}
"#;
        let entries = parse_umapinfo(text);
        assert!(entries["MAP30"].has_endgame);
    }

    #[test]
    fn test_parse_umapinfo_endpic() {
        let text = r#"
MAP MAP08
{
    endpic = "BOSSBACK"
}
"#;
        let entries = parse_umapinfo(text);
        assert!(entries["MAP08"].has_endgame); // endpic counts as endgame
    }

    #[test]
    fn test_parse_umapinfo_endcast() {
        let text = r#"
MAP MAP30
{
    endcast = true
}
"#;
        let entries = parse_umapinfo(text);
        assert!(entries["MAP30"].has_endgame);
    }

    #[test]
    fn test_parse_umapinfo_endbunny() {
        let text = r#"
MAP MAP15
{
    endbunny = true
}
"#;
        let entries = parse_umapinfo(text);
        assert!(entries["MAP15"].has_endgame);
    }

    #[test]
    fn test_parse_umapinfo_no_quotes() {
        let text = r#"
MAP MAP01
{
    next = MAP02
    nextsecret = MAP31
}
"#;
        let entries = parse_umapinfo(text);
        let map01 = &entries["MAP01"];
        assert_eq!(map01.next, Some("MAP02".to_string()));
        assert_eq!(map01.nextsecret, Some("MAP31".to_string()));
    }

    #[test]
    fn test_parse_umapinfo_empty() {
        let entries = parse_umapinfo("");
        assert!(entries.is_empty());
    }

    // -----------------------------------------------------------------------
    // Secret classification tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_secrets_vanilla_doom2() {
        let maps: Vec<String> = (1..=32).map(|i| format!("MAP{:02}", i)).collect();
        let secrets = classify_secrets_vanilla(&maps, false);
        assert!(secrets.contains("MAP31"));
        assert!(secrets.contains("MAP32"));
        assert_eq!(secrets.len(), 2);
    }

    #[test]
    fn test_secrets_vanilla_doom2_short_wad() {
        // WAD with only MAP01-MAP10 — no MAP31/32 to be secret
        let maps: Vec<String> = (1..=10).map(|i| format!("MAP{:02}", i)).collect();
        let secrets = classify_secrets_vanilla(&maps, false);
        assert!(secrets.is_empty());
    }

    #[test]
    fn test_secrets_vanilla_doom1() {
        let maps = vec![
            "E1M1", "E1M2", "E1M3", "E1M4", "E1M5", "E1M6", "E1M7", "E1M8", "E1M9",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        let secrets = classify_secrets_vanilla(&maps, true);
        assert!(secrets.contains("E1M9"));
        assert_eq!(secrets.len(), 1);
    }

    #[test]
    fn test_secrets_vanilla_doom1_no_m9() {
        let maps = vec!["E1M1", "E1M2", "E1M3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let secrets = classify_secrets_vanilla(&maps, true);
        assert!(secrets.is_empty());
    }

    #[test]
    fn test_secrets_vanilla_doom1_multi_episode() {
        let maps = vec!["E1M1", "E1M9", "E2M1", "E2M9", "E3M1"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let secrets = classify_secrets_vanilla(&maps, true);
        assert!(secrets.contains("E1M9"));
        assert!(secrets.contains("E2M9"));
        assert_eq!(secrets.len(), 2);
    }

    #[test]
    fn test_secrets_umapinfo_basic() {
        let mut umi = HashMap::new();
        umi.insert(
            "MAP01".into(),
            UmapinfoEntry {
                next: Some("MAP02".into()),
                nextsecret: Some("MAP31".into()),
                has_endgame: false,
            },
        );
        umi.insert(
            "MAP02".into(),
            UmapinfoEntry {
                next: Some("MAP03".into()),
                nextsecret: None,
                has_endgame: false,
            },
        );
        umi.insert(
            "MAP31".into(),
            UmapinfoEntry {
                next: Some("MAP02".into()),
                nextsecret: None,
                has_endgame: false,
            },
        );

        let maps: Vec<String> = vec!["MAP01", "MAP02", "MAP03", "MAP31"]
            .into_iter()
            .map(String::from)
            .collect();
        let secrets = classify_secrets_umapinfo(&maps, &umi);
        // MAP31 is reached via nextsecret from MAP01, and also via next from MAP31.
        // But MAP31 is NOT in any other map's `next` chain (MAP31's own next goes to MAP02).
        // Wait — MAP31 is pointed to by MAP01's nextsecret. MAP31 is NOT in any `next`.
        // MAP02 is in MAP01.next and MAP31.next.
        // So MAP31 should be secret.
        assert!(secrets.contains("MAP31"));
        assert_eq!(secrets.len(), 1);
    }

    #[test]
    fn test_secrets_umapinfo_poogers_edge_case() {
        // Poogers uses nextsecret for normal progression:
        // MAP01 -> next=MAP02, nextsecret=MAP03
        // MAP02 -> next=MAP04
        // MAP03 -> next=MAP04 (MAP03 also in next chain from itself)
        // MAP03 is reachable via nextsecret BUT also appears in MAP03.next chain?
        // Actually the rule is: MAP03 is reached by nextsecret AND also by next.
        // Let's make it clearer: MAP03 is the target of MAP01's nextsecret,
        // but MAP03 is also the target of some other map's `next`.
        let mut umi = HashMap::new();
        umi.insert(
            "MAP01".into(),
            UmapinfoEntry {
                next: Some("MAP02".into()),
                nextsecret: Some("MAP03".into()),
                has_endgame: false,
            },
        );
        umi.insert(
            "MAP02".into(),
            UmapinfoEntry {
                next: Some("MAP03".into()),
                nextsecret: None,
                has_endgame: false,
            },
        );
        umi.insert(
            "MAP03".into(),
            UmapinfoEntry {
                next: Some("MAP04".into()),
                nextsecret: None,
                has_endgame: false,
            },
        );

        let maps: Vec<String> = vec!["MAP01", "MAP02", "MAP03", "MAP04"]
            .into_iter()
            .map(String::from)
            .collect();
        let secrets = classify_secrets_umapinfo(&maps, &umi);
        // MAP03 is reached by MAP01's nextsecret BUT also by MAP02's next.
        // Therefore MAP03 is NOT secret.
        assert!(
            !secrets.contains("MAP03"),
            "Poogers edge case: MAP03 should NOT be secret"
        );
        assert!(secrets.is_empty());
    }

    #[test]
    fn test_secrets_umapinfo_nextsecret_only() {
        // MAP31 is reachable ONLY via nextsecret, never via next
        let mut umi = HashMap::new();
        umi.insert(
            "MAP15".into(),
            UmapinfoEntry {
                next: Some("MAP16".into()),
                nextsecret: Some("MAP31".into()),
                has_endgame: false,
            },
        );
        umi.insert(
            "MAP31".into(),
            UmapinfoEntry {
                next: Some("MAP16".into()),
                nextsecret: Some("MAP32".into()),
                has_endgame: false,
            },
        );
        umi.insert(
            "MAP32".into(),
            UmapinfoEntry {
                next: Some("MAP16".into()),
                nextsecret: None,
                has_endgame: false,
            },
        );

        let maps: Vec<String> = vec!["MAP15", "MAP16", "MAP31", "MAP32"]
            .into_iter()
            .map(String::from)
            .collect();
        let secrets = classify_secrets_umapinfo(&maps, &umi);
        assert!(secrets.contains("MAP31"));
        assert!(secrets.contains("MAP32"));
        assert_eq!(secrets.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Terminal map identification tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_terminal_umapinfo_endgame() {
        let mut umi = HashMap::new();
        umi.insert(
            "MAP07".into(),
            UmapinfoEntry {
                next: None,
                nextsecret: None,
                has_endgame: true,
            },
        );
        let maps = vec!["MAP01", "MAP02", "MAP07"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let infos: Vec<MapInfo> = maps
            .iter()
            .map(|m| MapInfo {
                lump: m.clone(),
                has_normal_exit: true,
                has_secret_exit: false,
                is_secret: false,
                is_dead_end: false,
                is_terminal: false,
                reachable: true,
            })
            .collect();

        let term = identify_terminal(&maps, false, &Some(umi), &infos);
        assert_eq!(term, Some("MAP07".to_string()));
    }

    #[test]
    fn test_terminal_umapinfo_self_loop() {
        let mut umi = HashMap::new();
        umi.insert(
            "MAP05".into(),
            UmapinfoEntry {
                next: Some("MAP05".into()), // self-loop
                nextsecret: None,
                has_endgame: false,
            },
        );
        let maps = vec!["MAP01", "MAP05"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let infos: Vec<MapInfo> = maps
            .iter()
            .map(|m| MapInfo {
                lump: m.clone(),
                has_normal_exit: true,
                has_secret_exit: false,
                is_secret: false,
                is_dead_end: false,
                is_terminal: false,
                reachable: true,
            })
            .collect();

        let term = identify_terminal(&maps, false, &Some(umi), &infos);
        assert_eq!(term, Some("MAP05".to_string()));
    }

    #[test]
    fn test_terminal_umapinfo_no_next() {
        let mut umi = HashMap::new();
        umi.insert(
            "MAP01".into(),
            UmapinfoEntry {
                next: Some("MAP02".into()),
                nextsecret: None,
                has_endgame: false,
            },
        );
        umi.insert(
            "MAP02".into(),
            UmapinfoEntry {
                next: Some("MAP03".into()),
                nextsecret: None,
                has_endgame: false,
            },
        );
        // MAP03 has an entry in UMAPINFO but no `next` field
        umi.insert("MAP03".into(), UmapinfoEntry::default());

        let maps = vec!["MAP01", "MAP02", "MAP03"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let infos: Vec<MapInfo> = maps
            .iter()
            .map(|m| MapInfo {
                lump: m.clone(),
                has_normal_exit: true,
                has_secret_exit: false,
                is_secret: false,
                is_dead_end: false,
                is_terminal: false,
                reachable: true,
            })
            .collect();

        let term = identify_terminal(&maps, false, &Some(umi), &infos);
        assert_eq!(term, Some("MAP03".to_string()));
    }

    #[test]
    fn test_terminal_vanilla_map30() {
        let maps: Vec<String> = (1..=32).map(|i| format!("MAP{:02}", i)).collect();
        let infos: Vec<MapInfo> = maps
            .iter()
            .map(|m| MapInfo {
                lump: m.clone(),
                has_normal_exit: true,
                has_secret_exit: false,
                is_secret: m == "MAP31" || m == "MAP32",
                is_dead_end: false,
                is_terminal: false,
                reachable: true,
            })
            .collect();

        let term = identify_terminal(&maps, false, &None, &infos);
        assert_eq!(term, Some("MAP30".to_string()));
    }

    #[test]
    fn test_terminal_vanilla_doom1_e3m8() {
        let maps = vec!["E3M1", "E3M2", "E3M8", "E3M9"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let infos: Vec<MapInfo> = maps
            .iter()
            .map(|m| MapInfo {
                lump: m.clone(),
                has_normal_exit: true,
                has_secret_exit: false,
                is_secret: m == "E3M9",
                is_dead_end: false,
                is_terminal: false,
                reachable: true,
            })
            .collect();

        let term = identify_terminal(&maps, true, &None, &infos);
        assert_eq!(term, Some("E3M8".to_string()));
    }

    #[test]
    fn test_terminal_fallback_highest() {
        // No MAP30, no UMAPINFO, no ExMy convention — fallback to highest
        let maps = vec!["MAP01", "MAP02", "MAP10"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let infos: Vec<MapInfo> = maps
            .iter()
            .map(|m| MapInfo {
                lump: m.clone(),
                has_normal_exit: true,
                has_secret_exit: false,
                is_secret: false,
                is_dead_end: false,
                is_terminal: false,
                reachable: true,
            })
            .collect();

        let term = identify_terminal(&maps, false, &None, &infos);
        assert_eq!(term, Some("MAP10".to_string()));
    }

    // -----------------------------------------------------------------------
    // Full WAD analysis integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_analyze_wad_simple_doom2() {
        // MAP01: normal exit, MAP02: normal exit, MAP03: no exits (dead end)
        let ld_normal = build_linedefs(&[11]);
        let ld_dead = build_linedefs(&[0, 1, 2]);

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP02", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP03", &[]),
            ("LINEDEFS", &ld_dead),
        ]);

        let analysis = analyze_wad(&wad).unwrap();
        assert_eq!(analysis.total_maps, 3);
        assert!(!analysis.has_umapinfo);
        assert!(analysis.secret_maps.is_empty());

        // MAP03 is both dead-end and highest → terminal
        assert_eq!(analysis.terminal_map, Some("MAP03".to_string()));
        // Dead-end maps exclude the terminal map
        assert!(analysis.dead_end_maps.is_empty());
        // required = 3 - 0 secrets - 0 dead-end - 1 terminal (dead-end) = 2
        assert_eq!(analysis.required_maps, 2);
    }

    #[test]
    fn test_analyze_wad_with_secret_maps() {
        let ld_normal = build_linedefs(&[11]);
        let ld_secret = build_linedefs(&[51]);

        // Build a 32-map WAD skeleton (only LINEDEFS for a few)
        let mut lumps: Vec<(&str, Vec<u8>)> = Vec::new();
        for i in 1..=32 {
            let name = format!("MAP{:02}", i);
            lumps.push((Box::leak(name.into_boxed_str()), vec![]));
            if i == 15 {
                // MAP15 has secret exit
                lumps.push(("LINEDEFS", ld_secret.clone()));
            } else {
                lumps.push(("LINEDEFS", ld_normal.clone()));
            }
        }

        let lump_refs: Vec<(&str, &[u8])> =
            lumps.iter().map(|(n, d)| (n.as_ref(), d.as_slice())).collect();
        let wad = build_wad(&lump_refs);

        let analysis = analyze_wad(&wad).unwrap();
        assert_eq!(analysis.total_maps, 32);
        assert!(analysis.secret_maps.contains(&"MAP31".to_string()));
        assert!(analysis.secret_maps.contains(&"MAP32".to_string()));
        assert_eq!(analysis.terminal_map, Some("MAP30".to_string()));
    }

    #[test]
    fn test_analyze_wad_doom1() {
        let ld_normal = build_linedefs(&[11]);

        let wad = build_wad(&[
            ("E1M1", &[]),
            ("LINEDEFS", &ld_normal),
            ("E1M2", &[]),
            ("LINEDEFS", &ld_normal),
            ("E1M8", &[]),
            ("LINEDEFS", &ld_normal),
            ("E1M9", &[]),
            ("LINEDEFS", &ld_normal),
        ]);

        let analysis = analyze_wad(&wad).unwrap();
        assert_eq!(analysis.total_maps, 4);
        assert!(analysis.secret_maps.contains(&"E1M9".to_string()));
        assert_eq!(analysis.terminal_map, Some("E1M8".to_string()));
        // required = 4 - 1 secret = 3
        assert_eq!(analysis.required_maps, 3);
    }

    #[test]
    fn test_analyze_wad_with_umapinfo() {
        let ld_normal = build_linedefs(&[11]);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP02"
}

MAP MAP02
{
    next = "MAP03"
    nextsecret = "MAP31"
}

MAP MAP03
{
    endgame = true
}

MAP MAP31
{
    next = "MAP03"
}
"#;

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP02", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP03", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP31", &[]),
            ("LINEDEFS", &ld_normal),
            ("UMAPINFO", umapinfo),
        ]);

        let analysis = analyze_wad(&wad).unwrap();
        assert!(analysis.has_umapinfo);
        assert_eq!(analysis.total_maps, 4);
        assert!(analysis.secret_maps.contains(&"MAP31".to_string()));
        assert_eq!(analysis.terminal_map, Some("MAP03".to_string()));
        // required = 4 - 1 secret = 3 (MAP01, MAP02, MAP03)
        assert_eq!(analysis.required_maps, 3);
    }

    #[test]
    fn test_analyze_wad_umapinfo_poogers_edge_case() {
        // Poogers-style: nextsecret used for normal progression
        let ld_normal = build_linedefs(&[11]);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP02"
    nextsecret = "MAP03"
}

MAP MAP02
{
    next = "MAP03"
}

MAP MAP03
{
    next = "MAP04"
}

MAP MAP04
{
    endgame = true
}
"#;

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP02", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP03", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP04", &[]),
            ("LINEDEFS", &ld_normal),
            ("UMAPINFO", umapinfo),
        ]);

        let analysis = analyze_wad(&wad).unwrap();
        // MAP03 is reached via MAP02's next AND MAP01's nextsecret
        // Therefore MAP03 should NOT be secret
        assert!(
            !analysis.secret_maps.contains(&"MAP03".to_string()),
            "MAP03 should not be secret in Poogers-style progression"
        );
        assert!(analysis.secret_maps.is_empty());
        assert_eq!(analysis.terminal_map, Some("MAP04".to_string()));
        // All 4 maps are required
        assert_eq!(analysis.required_maps, 4);
    }

    #[test]
    fn test_analyze_wad_udmf() {
        let textmap_normal = build_textmap(&[243]); // Exit_Normal
        let textmap_both = build_textmap(&[243, 244]); // both exits

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("TEXTMAP", &textmap_both),
            ("ENDMAP", &[]),
            ("MAP02", &[]),
            ("TEXTMAP", &textmap_normal),
            ("ENDMAP", &[]),
        ]);

        let analysis = analyze_wad(&wad).unwrap();
        assert_eq!(analysis.total_maps, 2);
        assert!(analysis.maps[0].has_normal_exit);
        assert!(analysis.maps[0].has_secret_exit);
        assert!(analysis.maps[1].has_normal_exit);
        assert!(!analysis.maps[1].has_secret_exit);
    }

    #[test]
    fn test_analyze_wad_no_maps() {
        let wad = build_wad(&[("THINGS", &[]), ("LINEDEFS", &[])]);
        assert!(analyze_wad(&wad).is_none());
    }

    #[test]
    fn test_analyze_wad_invalid() {
        assert!(analyze_wad(b"").is_none());
        assert!(analyze_wad(b"NOTAWAD!").is_none());
    }

    #[test]
    fn test_analyze_wad_dead_end_non_terminal() {
        // MAP01: normal exit, MAP02: no exits (dead end, not terminal), MAP03: normal exit
        let ld_normal = build_linedefs(&[11]);
        let ld_dead = build_linedefs(&[0, 1]);

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("LINEDEFS", &ld_normal),
            ("MAP02", &[]),
            ("LINEDEFS", &ld_dead),
            ("MAP03", &[]),
            ("LINEDEFS", &ld_normal),
        ]);

        let analysis = analyze_wad(&wad).unwrap();
        assert_eq!(analysis.total_maps, 3);
        // MAP02 is dead-end but not terminal (MAP03 is higher)
        assert!(analysis.dead_end_maps.contains(&"MAP02".to_string()));
        assert_eq!(analysis.terminal_map, Some("MAP03".to_string()));
        // required = 3 - 0 secret - 1 dead_end = 2
        assert_eq!(analysis.required_maps, 2);
    }

    #[test]
    fn test_map_sort_key() {
        assert!(map_sort_key("MAP01") < map_sort_key("MAP02"));
        assert!(map_sort_key("MAP09") < map_sort_key("MAP10"));
        assert!(map_sort_key("MAP10") < map_sort_key("MAP30"));
        assert!(map_sort_key("E1M1") < map_sort_key("E1M8"));
        assert!(map_sort_key("E1M8") < map_sort_key("E2M1"));
    }

    #[test]
    fn test_is_map_marker() {
        assert!(is_map_marker("MAP01"));
        assert!(is_map_marker("MAP32"));
        assert!(is_map_marker("E1M1"));
        assert!(is_map_marker("E4M9"));
        assert!(!is_map_marker("THINGS"));
        assert!(!is_map_marker("LINEDEFS"));
        assert!(!is_map_marker("MAP001"));
        assert!(!is_map_marker(""));
    }
}
