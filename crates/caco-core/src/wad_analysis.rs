//! WAD analysis module for automated completion detection.
//!
//! Two pure layers:
//! - `analyze_wad` / `analyze_pk3` build a directed map graph from the WAD's
//!   ZMAPINFO/UMAPINFO/MAPINFO/vanilla edges, walk the main path from start
//!   to credits-stopper, and assign each map a `MapClassification`.
//! - `completion_detect::check_completion` intersects the Required set with
//!   the player's exit stats. The classifier never sees stats.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::utils::parse_wad_directory;

// ---------------------------------------------------------------------------
// Map name patterns
// ---------------------------------------------------------------------------

static DOOM1_MAP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^E(\d)M(\d)$").unwrap());
static DOOM2_MAP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^MAP([1-9]\d{2}|\d{2})$").unwrap());
/// Playable map names. Accepts standard `MAPxx` / `ExMy` plus alphanumeric
/// suffixes (e.g. `MAP18GZ` referenced from ZMAPINFO).
static PLAYABLE_MAP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(MAP\d{2,}[A-Z0-9]*|E\dM\d[A-Z0-9]*)$").unwrap());

/// Lumps that belong to a map definition (appear after the map marker).
const MAP_LUMPS: &[&str] = &[
    "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES", "SECTORS", "REJECT",
    "BLOCKMAP", "BEHAVIOR", "SCRIPTS", "TEXTMAP", "ENDMAP", "DIALOGUE", "ZNODES",
];

// ---------------------------------------------------------------------------
// Exit linedef specials
// ---------------------------------------------------------------------------

/// Vanilla/Boom normal exit linedef types.
const VANILLA_NORMAL_EXITS: &[u16] = &[11, 52, 197];
/// Vanilla/Boom secret exit linedef types.
const VANILLA_SECRET_EXITS: &[u16] = &[51, 124, 198];

/// Boom generalized exit linedef range: 0x3400..=0x37FF.
const GEN_EXIT_BASE: u16 = 0x3400;
const GEN_EXIT_END: u16 = 0x37FF;
/// Bit 5 within the generalized exit encoding indicates a secret exit.
const GEN_EXIT_SECRET_BIT: u16 = 0x20;

/// Analysis format version. Bump this whenever detection logic changes
/// so that stale cached analyses are automatically invalidated and re-run.
pub const ANALYSIS_VERSION: u32 = 4;

/// UDMF normal exit specials.
const UDMF_NORMAL_EXITS: &[i32] = &[243, 74, 75]; // Exit_Normal, Teleport_NewMap, Teleport_EndGame
/// UDMF secret exit specials.
const UDMF_SECRET_EXITS: &[i32] = &[244]; // Exit_Secret

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// How a map relates to the WAD's main play flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MapClassification {
    /// On the main path from start to credits-stopper. Must be exited for completion.
    Required,
    /// Reachable only via a skippable secret-exit branch. Optional.
    OptionalSecret,
    /// Terminal credits/stopper map. Reaching its predecessor on the main path
    /// proves completion; the stopper itself does not need to be "exited".
    OptionalCredits,
    /// Lump exists but no incoming edge in any flow. Not part of completion.
    Unreachable,
}

/// Per-map analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapInfo {
    /// Map lump name (e.g., "MAP01", "E1M1").
    pub lump: String,
    /// Map has at least one normal exit linedef (diagnostic).
    pub has_normal_exit: bool,
    /// Map has at least one secret exit linedef (diagnostic).
    pub has_secret_exit: bool,
    /// Single source of truth for what this map represents in the play flow.
    #[serde(default = "default_classification")]
    pub classification: MapClassification,
}

fn default_classification() -> MapClassification {
    MapClassification::Required
}

/// Complete WAD analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WadAnalysis {
    /// Analysis format version (defaults to 0 for pre-versioned entries).
    #[serde(default)]
    pub version: u32,
    /// All maps found in the WAD with their classification.
    pub maps: Vec<MapInfo>,
    /// Total number of maps.
    pub total_maps: usize,
    /// Derived: number of maps with `classification == Required`.
    pub required_maps: usize,
    /// Derived: lump names where `classification == OptionalSecret`.
    pub secret_maps: Vec<String>,
    /// Derived: the lump where `classification == OptionalCredits`, if any.
    pub terminal_map: Option<String>,
    /// Whether any structured map-flow data was found (UMAPINFO/MAPINFO/ZMAPINFO).
    pub has_umapinfo: bool,
}

// ---------------------------------------------------------------------------
// Internal: map flow graph
// ---------------------------------------------------------------------------

/// Directed graph of the WAD's map flow.
#[derive(Debug, Default, Clone)]
struct MapGraph {
    /// `lump -> next normal map`. One edge per source.
    edges_normal: HashMap<String, String>,
    /// `lump -> next secret map`. One edge per source.
    edges_secret: HashMap<String, String>,
    /// Maps that explicitly mark game end (`endgame`/`endpic`/etc.).
    has_endgame: HashSet<String>,
}

/// Source of map-flow data, lower priority gets overridden by higher.
/// Vanilla conventions are applied separately (always lowest priority) and
/// don't appear here.
enum FlowSource {
    Umapinfo,
    Mapinfo,
    Zmapinfo,
}

impl FlowSource {
    fn priority(&self) -> u8 {
        match self {
            FlowSource::Umapinfo => 1,
            FlowSource::Mapinfo => 2,
            FlowSource::Zmapinfo => 3,
        }
    }
}

/// Build the map graph by overlaying flow sources in priority order.
///
/// Each source's edges only override existing edges from lower-priority
/// sources. Edges that point to lumps not in `map_set` are dropped.
fn build_graph(
    map_set: &HashSet<&str>,
    is_doom1: bool,
    sources: &[(FlowSource, HashMap<String, MapinfoEdge>)],
) -> MapGraph {
    let mut by_priority: Vec<&(FlowSource, HashMap<String, MapinfoEdge>)> =
        sources.iter().collect();
    by_priority.sort_by_key(|(s, _)| s.priority());

    let mut graph = MapGraph::default();

    // Layer 0: vanilla edges (lowest priority, applied first)
    add_vanilla_edges(&mut graph, map_set, is_doom1);

    // Higher layers: overlay each source in priority order. Each property
    // is only overridden when the higher-priority source explicitly sets it
    // — an empty entry like `MAP MAP01 { }` does NOT clear vanilla edges.
    // Setting endgame=true also clears any normal/secret edge for that map
    // (game ends here, no progression).
    for (_, entries) in &by_priority {
        for (lump, edge) in entries.iter() {
            if !map_set.contains(lump.as_str()) {
                continue;
            }

            if let Some(ref nx) = edge.next {
                if map_set.contains(nx.as_str()) {
                    graph.edges_normal.insert(lump.clone(), nx.clone());
                } else {
                    graph.edges_normal.remove(lump.as_str());
                }
            }
            if let Some(ref sx) = edge.secret_next {
                if map_set.contains(sx.as_str()) {
                    graph.edges_secret.insert(lump.clone(), sx.clone());
                } else {
                    graph.edges_secret.remove(lump.as_str());
                }
            }
            if edge.has_endgame {
                graph.has_endgame.insert(lump.clone());
                graph.edges_normal.remove(lump.as_str());
                graph.edges_secret.remove(lump.as_str());
            }
        }
    }

    graph
}

/// Unified edge representation across all flow sources.
#[derive(Debug, Default, Clone)]
struct MapinfoEdge {
    next: Option<String>,
    secret_next: Option<String>,
    has_endgame: bool,
}

/// Add vanilla map-flow edges based on map naming conventions.
fn add_vanilla_edges(graph: &mut MapGraph, map_set: &HashSet<&str>, is_doom1: bool) {
    if is_doom1 {
        // ExMy → ExM(y+1) for y < 8; ExM3 secret → ExM9; ExM9 → ExM(source+1)
        // is rare and engine-specific, skip.
        for &lump in map_set {
            if let Some(caps) = DOOM1_MAP_RE.captures(lump)
                && let Ok(ep) = caps[1].parse::<u32>()
                && let Ok(mn) = caps[2].parse::<u32>()
            {
                if mn < 8 {
                    let next = format!("E{ep}M{}", mn + 1);
                    if map_set.contains(next.as_str()) {
                        graph.edges_normal.insert(lump.to_string(), next);
                    }
                }
                if mn == 3 {
                    let secret = format!("E{ep}M9");
                    if map_set.contains(secret.as_str()) {
                        graph.edges_secret.insert(lump.to_string(), secret);
                    }
                }
            }
        }
    } else {
        // MAP_n → MAP_(n+1) for n < 30; MAP15 secret → MAP31; MAP31 → MAP32 → MAP16
        for &lump in map_set {
            if let Some(caps) = DOOM2_MAP_RE.captures(lump)
                && let Ok(n) = caps[1].parse::<u32>()
            {
                if n < 30 {
                    let next = format!("MAP{:02}", n + 1);
                    if map_set.contains(next.as_str()) {
                        graph.edges_normal.insert(lump.to_string(), next);
                    }
                }
                if n == 15 && map_set.contains("MAP31") {
                    graph
                        .edges_secret
                        .insert(lump.to_string(), "MAP31".to_string());
                }
                if n == 31 {
                    if map_set.contains("MAP16") {
                        graph
                            .edges_normal
                            .insert(lump.to_string(), "MAP16".to_string());
                    }
                    if map_set.contains("MAP32") {
                        graph
                            .edges_secret
                            .insert(lump.to_string(), "MAP32".to_string());
                    }
                }
                if n == 32 && map_set.contains("MAP16") {
                    graph
                        .edges_normal
                        .insert(lump.to_string(), "MAP16".to_string());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: classification (graph walk)
// ---------------------------------------------------------------------------

/// Walk the graph and assign a classification to each map.
///
/// Algorithm:
/// 1. Pick start = lowest-keyed playable map.
/// 2. Walk the main path, preferring normal edges, falling back to secret
///    edges only when no normal edge exists ("forced secret = true ending").
/// 3. The terminus (last node visited) is reclassified as `OptionalCredits`
///    if it has no outgoing edges and no detected exit linedefs.
/// 4. Walk secret branches off main-path nodes; mark visited nodes as
///    `OptionalSecret`.
/// 5. Anything still unmarked is `Unreachable`.
fn classify_maps(graph: &MapGraph, infos: &mut [MapInfo]) {
    if infos.is_empty() {
        return;
    }

    // Find the canonical start: lowest-keyed playable map.
    let mut sorted: Vec<&str> = infos.iter().map(|m| m.lump.as_str()).collect();
    sorted.sort_by_key(|l| map_sort_key(l));
    let start = sorted[0].to_string();

    // Pre-fill all as Unreachable; we'll upgrade as we walk.
    for m in infos.iter_mut() {
        m.classification = MapClassification::Unreachable;
    }
    let by_lump: HashMap<String, usize> = infos
        .iter()
        .enumerate()
        .map(|(i, m)| (m.lump.clone(), i))
        .collect();

    // 1. Main-path walk
    let mut visited: HashSet<String> = HashSet::new();
    let mut walk_order: Vec<String> = Vec::new();
    let mut current = Some(start);
    while let Some(node) = current {
        if !visited.insert(node.clone()) {
            break; // cycle
        }
        walk_order.push(node.clone());
        if let Some(&idx) = by_lump.get(node.as_str()) {
            infos[idx].classification = MapClassification::Required;
        }
        if graph.has_endgame.contains(&node) {
            break;
        }
        current = match (graph.edges_normal.get(&node), graph.edges_secret.get(&node)) {
            (Some(nx), _) => Some(nx.clone()),
            (None, Some(sx)) => Some(sx.clone()), // forced secret = true ending
            (None, None) => None,
        };
    }

    // 2. Reclassify terminus as OptionalCredits if it really is a stopper
    //    (no outgoing edges + no detected exit linedef). A node with no
    //    outgoing edge but with detected linedef exits is a "false dead-end"
    //    — likely an intermediate map whose exit is ACS/DEH-patched and
    //    doesn't land in the static graph. Leave it as Required so the
    //    verdict layer surfaces the missing chain rather than silently
    //    rewriting it as a stopper.
    if let Some(term) = walk_order.last() {
        let no_normal_out = !graph.edges_normal.contains_key(term);
        let no_secret_out = !graph.edges_secret.contains_key(term);
        let has_endgame_marker = graph.has_endgame.contains(term);
        if let Some(&idx) = by_lump.get(term.as_str()) {
            let info = &infos[idx];
            let no_exit = !info.has_normal_exit && !info.has_secret_exit;
            if no_normal_out && no_secret_out && (no_exit || has_endgame_marker) {
                infos[idx].classification = MapClassification::OptionalCredits;
            }
        }
    }

    // 3. Secret branches off the main path
    let main_path: HashSet<String> = walk_order.iter().cloned().collect();
    let mut sec_queue: VecDeque<String> = VecDeque::new();
    for node in &main_path {
        if let Some(sx) = graph.edges_secret.get(node)
            && !main_path.contains(sx)
        {
            sec_queue.push_back(sx.clone());
        }
    }
    let mut sec_seen: HashSet<String> = HashSet::new();
    while let Some(node) = sec_queue.pop_front() {
        if !sec_seen.insert(node.clone()) {
            continue;
        }
        if let Some(&idx) = by_lump.get(node.as_str())
            && infos[idx].classification == MapClassification::Unreachable
        {
            infos[idx].classification = MapClassification::OptionalSecret;
        }
        if let Some(nx) = graph.edges_normal.get(&node)
            && !main_path.contains(nx)
        {
            sec_queue.push_back(nx.clone());
        }
        if let Some(sx) = graph.edges_secret.get(&node)
            && !main_path.contains(sx)
        {
            sec_queue.push_back(sx.clone());
        }
    }
}

/// Apply hub-WAD classification: every playable map becomes Required.
///
/// In a hub structure (e.g. Hexen-style), maps cross-reference each other
/// and there is no single linear "main path"; the player must visit them
/// all. Detection lives in `detect_hub_structure`.
fn classify_hub(infos: &mut [MapInfo]) {
    for m in infos.iter_mut() {
        m.classification = MapClassification::Required;
    }
}

// ---------------------------------------------------------------------------
// Internal: derived field extraction
// ---------------------------------------------------------------------------

fn finalize(maps: Vec<MapInfo>, has_structured_flow: bool) -> WadAnalysis {
    let total_maps = maps.len();
    let required_maps = maps
        .iter()
        .filter(|m| m.classification == MapClassification::Required)
        .count();
    let secret_maps: Vec<String> = maps
        .iter()
        .filter(|m| m.classification == MapClassification::OptionalSecret)
        .map(|m| m.lump.clone())
        .collect();
    let terminal_map = maps
        .iter()
        .find(|m| m.classification == MapClassification::OptionalCredits)
        .map(|m| m.lump.clone());

    WadAnalysis {
        version: ANALYSIS_VERSION,
        total_maps,
        required_maps,
        secret_maps,
        terminal_map,
        has_umapinfo: has_structured_flow,
        maps,
    }
}

// ---------------------------------------------------------------------------
// Public: WAD analysis
// ---------------------------------------------------------------------------

/// Analyze a WAD file to enumerate maps and classify them.
///
/// Returns `None` if the data is not a valid WAD or contains no maps.
pub fn analyze_wad(wad_data: &[u8]) -> Option<WadAnalysis> {
    let directory = parse_wad_directory(wad_data);
    if directory.is_empty() {
        return None;
    }

    let map_ranges = find_map_ranges(&directory);
    if map_ranges.is_empty() {
        return None;
    }

    // Per-map exit detection
    let mut infos: Vec<MapInfo> = Vec::with_capacity(map_ranges.len());
    for (name, start_idx, end_idx) in &map_ranges {
        let (has_normal, has_secret) = detect_exits(wad_data, &directory, *start_idx, *end_idx);
        infos.push(MapInfo {
            lump: name.clone(),
            has_normal_exit: has_normal,
            has_secret_exit: has_secret,
            classification: MapClassification::Unreachable,
        });
    }

    // Filter to playable map names. Anything else (TITLEMAP, etc.) is
    // ignored for classification purposes.
    let mut infos: Vec<MapInfo> = infos
        .into_iter()
        .filter(|m| PLAYABLE_MAP_RE.is_match(&m.lump))
        .collect();
    if infos.is_empty() {
        return None;
    }

    let lumps: Vec<String> = infos.iter().map(|m| m.lump.clone()).collect();
    let map_set: HashSet<&str> = lumps.iter().map(|s| s.as_str()).collect();
    let is_doom1 = lumps.first().is_some_and(|m| DOOM1_MAP_RE.is_match(m));

    // Collect flow sources from the WAD.
    let mut sources: Vec<(FlowSource, HashMap<String, MapinfoEdge>)> = Vec::new();
    let mut has_structured = false;

    if let Some(text) = read_lump_text(wad_data, &directory, "ZMAPINFO") {
        let entries = parse_mapinfo_to_edges(&crate::mapinfo::parse_mapinfo(&text));
        if !entries.is_empty() {
            has_structured = true;
            sources.push((FlowSource::Zmapinfo, entries));
        }
    }
    if let Some(text) = read_lump_text(wad_data, &directory, "UMAPINFO") {
        let entries = parse_umapinfo_to_edges(&parse_umapinfo(&text));
        if !entries.is_empty() {
            has_structured = true;
            sources.push((FlowSource::Umapinfo, entries));
        }
    }
    if let Some(text) = read_lump_text(wad_data, &directory, "MAPINFO") {
        let entries = parse_mapinfo_to_edges(&crate::mapinfo::parse_mapinfo(&text));
        if !entries.is_empty() {
            has_structured = true;
            sources.push((FlowSource::Mapinfo, entries));
        }
    }

    let graph = build_graph(&map_set, is_doom1, &sources);
    classify_maps(&graph, &mut infos);

    Some(finalize(infos, has_structured))
}

// ---------------------------------------------------------------------------
// Public: PK3 analysis
// ---------------------------------------------------------------------------

/// Analyze a PK3 file (ZDoom ZIP archive).
pub fn analyze_pk3(pk3_path: &std::path::Path) -> Option<WadAnalysis> {
    use std::io::Read;

    let file = std::fs::File::open(pk3_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // --- Step 1: Discover maps and analyze exits ---
    let mut infos: Vec<MapInfo> = Vec::new();

    // Try maps/ directory first (one map per WAD)
    let map_wad_names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            if name.to_lowercase().starts_with("maps/") && name.to_lowercase().ends_with(".wad") {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    if !map_wad_names.is_empty() {
        for entry_name in &map_wad_names {
            let mut entry = archive.by_name(entry_name).ok()?;
            let mut data = Vec::new();
            entry.read_to_end(&mut data).ok()?;

            let stem = std::path::Path::new(entry_name)
                .file_stem()
                .and_then(|s| s.to_str())?
                .to_uppercase();

            let (has_normal, has_secret) = analyze_map_wad_exits(&data);
            infos.push(MapInfo {
                lump: stem,
                has_normal_exit: has_normal,
                has_secret_exit: has_secret,
                classification: MapClassification::Unreachable,
            });
        }
    } else {
        // Fallback: scan root-level WAD files for embedded maps
        let root_wad_names: Vec<String> = (0..archive.len())
            .filter_map(|i| {
                let entry = archive.by_index(i).ok()?;
                let name = entry.name().to_string();
                if !name.contains('/') && name.to_lowercase().ends_with(".wad") {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();

        for entry_name in &root_wad_names {
            let mut entry = archive.by_name(entry_name).ok()?;
            let mut data = Vec::new();
            entry.read_to_end(&mut data).ok()?;

            let directory = parse_wad_directory(&data);
            let ranges = find_map_ranges(&directory);
            for (name, start, end) in &ranges {
                let (has_normal, has_secret) = detect_exits(&data, &directory, *start, *end);
                infos.push(MapInfo {
                    lump: name.to_uppercase(),
                    has_normal_exit: has_normal,
                    has_secret_exit: has_secret,
                    classification: MapClassification::Unreachable,
                });
            }
        }
    }

    // Filter to playable map names
    infos.retain(|m| PLAYABLE_MAP_RE.is_match(&m.lump));
    if infos.is_empty() {
        return None;
    }

    // --- Step 2: Read and parse MAPINFO/ZMAPINFO ---
    let mut mapinfo_text = String::new();
    let mapinfo_names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            let lower = name.to_lowercase();
            if !name.contains('/')
                && !name.ends_with('/')
                && (lower.starts_with("mapinfo") || lower.starts_with("zmapinfo"))
            {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    for entry_name in &mapinfo_names {
        if let Ok(mut entry) = archive.by_name(entry_name) {
            let mut text = String::new();
            if entry.read_to_string(&mut text).is_ok() {
                mapinfo_text.push('\n');
                mapinfo_text.push_str(&text);
            }
        }
    }

    let mapinfo = if !mapinfo_text.is_empty() {
        Some(crate::mapinfo::parse_mapinfo(&mapinfo_text))
    } else {
        None
    };
    let has_structured = mapinfo.is_some();

    // --- Step 3: Hub detection (PK3-specific) ---
    let lumps: Vec<String> = infos.iter().map(|m| m.lump.clone()).collect();
    let is_hub = mapinfo
        .as_ref()
        .is_some_and(|mi| detect_hub_structure(mi, &lumps));

    if is_hub {
        classify_hub(&mut infos);
        return Some(finalize(infos, has_structured));
    }

    // --- Step 4: Build graph + classify (linear/branching path) ---
    let map_set: HashSet<&str> = lumps.iter().map(|s| s.as_str()).collect();
    let is_doom1 = lumps.first().is_some_and(|m| DOOM1_MAP_RE.is_match(m));

    let mut sources: Vec<(FlowSource, HashMap<String, MapinfoEdge>)> = Vec::new();
    if let Some(ref mi) = mapinfo {
        let entries = parse_mapinfo_to_edges(mi);
        if !entries.is_empty() {
            // For PK3 we don't distinguish ZMAPINFO from MAPINFO at this layer;
            // mapinfo.rs already reads both. Treat as Mapinfo priority.
            sources.push((FlowSource::Mapinfo, entries));
        }
    }

    // PK3 maps may also have UMAPINFO embedded inside one of the WAD files.
    // Skip that lookup for now — MAPINFO/ZMAPINFO is the standard for PK3s.

    let graph = build_graph(&map_set, is_doom1, &sources);
    classify_maps(&graph, &mut infos);

    Some(finalize(infos, has_structured))
}

// ---------------------------------------------------------------------------
// Hub detection
// ---------------------------------------------------------------------------

/// Detect whether a MAPINFO describes a hub structure.
///
/// Returns true if either:
/// - A single `next` target receives more than half of all map entries
///   (single dominant hub), OR
/// - Multiple "mini-hub" targets (3+ incoming) collectively cover >50%.
fn detect_hub_structure(
    mapinfo: &HashMap<String, crate::mapinfo::MapinfoEntry>,
    playable_maps: &[String],
) -> bool {
    let playable_set: HashSet<&str> = playable_maps.iter().map(|s| s.as_str()).collect();

    let mut target_counts: HashMap<&str, usize> = HashMap::new();
    for (name, entry) in mapinfo {
        if !PLAYABLE_MAP_RE.is_match(name) {
            continue;
        }
        if let Some(ref next) = entry.next {
            *target_counts.entry(next.as_str()).or_default() += 1;
        }
    }

    let playable_count = playable_maps
        .iter()
        .filter(|m| PLAYABLE_MAP_RE.is_match(m))
        .count();
    if playable_count == 0 {
        return false;
    }

    if target_counts
        .values()
        .max()
        .is_some_and(|&max_count| max_count > playable_count / 2)
    {
        return true;
    }

    let hub_connected: usize = target_counts
        .iter()
        .filter(|&(target, &count)| {
            count >= 3 && playable_set.contains(target.to_uppercase().as_str())
        })
        .map(|(_, &count)| count)
        .sum();
    hub_connected > playable_count / 2
}

// ---------------------------------------------------------------------------
// Map range detection
// ---------------------------------------------------------------------------

/// Find map markers in the directory and their associated lump ranges.
///
/// A lump is a map marker if either:
/// - Its name matches a standard pattern (MAP01, E1M1), OR
/// - It is immediately followed by a map-defining lump (THINGS, LINEDEFS,
///   TEXTMAP, etc.) AND its name plausibly looks like a map (matches
///   `PLAYABLE_MAP_RE`).
///
/// The second rule catches ZDoom-style alternate maps like MAP18GZ that are
/// referenced from ZMAPINFO under arbitrary lump names. The PLAYABLE_MAP_RE
/// guard prevents random zero-size lumps that happen to precede LINEDEFS
/// from being misidentified.
fn find_map_ranges(directory: &[(String, u32, u32)]) -> Vec<(String, usize, usize)> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < directory.len() {
        let name = &directory[i].0;
        let next_is_map_lump = directory.get(i + 1).is_some_and(|(n, _, _)| is_map_lump(n));
        let is_marker = is_map_marker(name) || (next_is_map_lump && PLAYABLE_MAP_RE.is_match(name));
        if is_marker {
            let start = i;
            i += 1;
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

fn is_map_marker(name: &str) -> bool {
    DOOM1_MAP_RE.is_match(name) || DOOM2_MAP_RE.is_match(name)
}

fn is_map_lump(name: &str) -> bool {
    MAP_LUMPS.contains(&name)
}

// ---------------------------------------------------------------------------
// Exit detection
// ---------------------------------------------------------------------------

fn detect_exits(
    wad_data: &[u8],
    directory: &[(String, u32, u32)],
    start_idx: usize,
    end_idx: usize,
) -> (bool, bool) {
    let map_lumps = &directory[start_idx..end_idx];

    if let Some(textmap) = find_lump_data(wad_data, map_lumps, "TEXTMAP") {
        return detect_udmf_exits(&textmap);
    }
    if let Some(linedefs) = find_lump_data(wad_data, map_lumps, "LINEDEFS") {
        return detect_vanilla_exits(&linedefs);
    }
    (false, false)
}

fn analyze_map_wad_exits(wad_data: &[u8]) -> (bool, bool) {
    let directory = parse_wad_directory(wad_data);
    if directory.is_empty() {
        return (false, false);
    }
    let map_ranges = find_map_ranges(&directory);
    if let Some((_, start, end)) = map_ranges.first() {
        detect_exits(wad_data, &directory, *start, *end)
    } else {
        (false, false)
    }
}

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

/// Parse vanilla/Boom LINEDEFS lump for exit specials.
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
        if (GEN_EXIT_BASE..=GEN_EXIT_END).contains(&special) {
            if special & GEN_EXIT_SECRET_BIT != 0 {
                has_secret = true;
            } else {
                has_normal = true;
            }
        }
    }

    (has_normal, has_secret)
}

/// Parse UDMF TEXTMAP for exit specials.
fn detect_udmf_exits(textmap_data: &[u8]) -> (bool, bool) {
    let text = String::from_utf8_lossy(textmap_data);
    let mut has_normal = false;
    let mut has_secret = false;

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

/// Parsed UMAPINFO entry (subset used for flow analysis).
#[derive(Debug, Clone, Default)]
struct UmapinfoEntry {
    next: Option<String>,
    nextsecret: Option<String>,
    has_endgame: bool,
}

/// Parse UMAPINFO text into map entries.
fn parse_umapinfo(text: &str) -> HashMap<String, UmapinfoEntry> {
    static MAP_HEADER_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^\s*MAP\s+(\S+)").unwrap());
    static NEXT_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)^\s*next\s*=\s*"?(\w+)"?"#).unwrap());
    static NEXTSECRET_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)^\s*nextsecret\s*=\s*"?(\w+)"?"#).unwrap());
    static ENDGAME_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^\s*(endgame|endpic|endcast|endbunny)\s*=").unwrap());

    let mut entries = HashMap::new();
    let mut current_map: Option<String> = None;
    let mut current_entry = UmapinfoEntry::default();

    for line in text.lines() {
        if let Some(caps) = MAP_HEADER_RE.captures(line) {
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
    if let Some(map_name) = current_map {
        entries.insert(map_name, current_entry);
    }
    entries
}

// ---------------------------------------------------------------------------
// Flow source adapters
// ---------------------------------------------------------------------------

fn parse_umapinfo_to_edges(umi: &HashMap<String, UmapinfoEntry>) -> HashMap<String, MapinfoEdge> {
    let mut out = HashMap::new();
    for (name, e) in umi {
        out.insert(
            name.clone(),
            MapinfoEdge {
                next: e.next.clone(),
                secret_next: e.nextsecret.clone(),
                has_endgame: e.has_endgame,
            },
        );
    }
    out
}

fn parse_mapinfo_to_edges(
    mi: &HashMap<String, crate::mapinfo::MapinfoEntry>,
) -> HashMap<String, MapinfoEdge> {
    let mut out = HashMap::new();
    for (name, e) in mi {
        out.insert(
            name.clone(),
            MapinfoEdge {
                next: e.next.clone(),
                secret_next: e.secretnext.clone(),
                has_endgame: e.has_endgame,
            },
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Sorting helper
// ---------------------------------------------------------------------------

/// Generate a sort key for map names so MAP02 sorts before MAP10.
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
    (999, 999)
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

    fn make_linedef(special: u16) -> [u8; 14] {
        let mut ld = [0u8; 14];
        ld[6] = (special & 0xFF) as u8;
        ld[7] = (special >> 8) as u8;
        ld
    }

    fn build_linedefs(specials: &[u16]) -> Vec<u8> {
        let mut data = Vec::new();
        for &s in specials {
            data.extend_from_slice(&make_linedef(s));
        }
        data
    }

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

    fn classify_of(a: &WadAnalysis, lump: &str) -> MapClassification {
        a.maps
            .iter()
            .find(|m| m.lump == lump)
            .map(|m| m.classification)
            .expect("lump not found")
    }

    // -----------------------------------------------------------------------
    // Linedef parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_vanilla_normal_exit() {
        let linedefs = build_linedefs(&[0, 0, 11, 0]);
        let (normal, secret) = detect_vanilla_exits(&linedefs);
        assert!(normal);
        assert!(!secret);
    }

    #[test]
    fn test_vanilla_secret_exit() {
        let linedefs = build_linedefs(&[0, 51, 0]);
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
        for &s in VANILLA_NORMAL_EXITS {
            let linedefs = build_linedefs(&[s]);
            let (normal, secret) = detect_vanilla_exits(&linedefs);
            assert!(normal, "special {} should be a normal exit", s);
            assert!(!secret);
        }
        for &s in VANILLA_SECRET_EXITS {
            let linedefs = build_linedefs(&[s]);
            let (normal, secret) = detect_vanilla_exits(&linedefs);
            assert!(!normal);
            assert!(secret, "special {} should be a secret exit", s);
        }
    }

    #[test]
    fn test_boom_generalized_exits() {
        let (normal, _) = detect_vanilla_exits(&build_linedefs(&[0x3489]));
        assert!(normal);
        let (_, secret) = detect_vanilla_exits(&build_linedefs(&[0x3420]));
        assert!(secret);
    }

    // -----------------------------------------------------------------------
    // UDMF parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_udmf_exits() {
        assert_eq!(detect_udmf_exits(&build_textmap(&[243])), (true, false));
        assert_eq!(detect_udmf_exits(&build_textmap(&[244])), (false, true));
        assert_eq!(detect_udmf_exits(&build_textmap(&[74])), (true, false));
        assert_eq!(detect_udmf_exits(&build_textmap(&[75])), (true, false));
        assert_eq!(
            detect_udmf_exits(&build_textmap(&[0, 1, 80])),
            (false, false)
        );
    }

    // -----------------------------------------------------------------------
    // UMAPINFO parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_umapinfo_basic() {
        let text = r#"
MAP MAP01
{
    next = "MAP02"
    nextsecret = "MAP31"
}
"#;
        let entries = parse_umapinfo(text);
        let map01 = &entries["MAP01"];
        assert_eq!(map01.next, Some("MAP02".to_string()));
        assert_eq!(map01.nextsecret, Some("MAP31".to_string()));
    }

    #[test]
    fn test_parse_umapinfo_endgame_variants() {
        for kw in &["endgame", "endpic", "endcast", "endbunny"] {
            let text = format!("MAP MAP30\n{{\n    {} = true\n}}", kw);
            let entries = parse_umapinfo(&text);
            assert!(entries["MAP30"].has_endgame, "failed for {kw}");
        }
    }

    // -----------------------------------------------------------------------
    // Vanilla full-WAD analysis
    // -----------------------------------------------------------------------

    #[test]
    fn test_analyze_wad_vanilla_doom2_short_chain() {
        // MAP01 → MAP02 → MAP03 (no exit, stopper)
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

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(a.total_maps, 3);
        assert_eq!(classify_of(&a, "MAP01"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP02"), MapClassification::Required);
        // MAP03 has no exit, no outgoing edge → stopper
        assert_eq!(classify_of(&a, "MAP03"), MapClassification::OptionalCredits);
        assert_eq!(a.required_maps, 2);
        assert_eq!(a.terminal_map.as_deref(), Some("MAP03"));
    }

    #[test]
    fn test_analyze_wad_vanilla_doom2_full_with_secrets() {
        let ld_normal = build_linedefs(&[11]);
        let ld_secret = build_linedefs(&[51]);

        let mut lumps: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 1..=32 {
            lumps.push((format!("MAP{:02}", i), Vec::new()));
            if i == 15 {
                lumps.push(("LINEDEFS".to_string(), ld_secret.clone()));
            } else {
                lumps.push(("LINEDEFS".to_string(), ld_normal.clone()));
            }
        }
        let lump_refs: Vec<(&str, &[u8])> = lumps
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let wad = build_wad(&lump_refs);

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(a.total_maps, 32);
        // MAP30 is the vanilla terminus and has a normal exit linedef so it's
        // NOT classified as a stopper — it stays Required since the static
        // graph has nothing past it. Classifier doesn't synthesize an
        // endgame-only marker for MAP30 alone.
        // Required = MAP01..MAP30 (no UMAPINFO endgame marker)
        assert_eq!(classify_of(&a, "MAP01"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP30"), MapClassification::Required);
        // MAP31 / MAP32 are reached only via secret edges from MAP15/MAP31
        assert_eq!(classify_of(&a, "MAP31"), MapClassification::OptionalSecret);
        assert_eq!(classify_of(&a, "MAP32"), MapClassification::OptionalSecret);
        assert!(a.secret_maps.contains(&"MAP31".to_string()));
        assert!(a.secret_maps.contains(&"MAP32".to_string()));
    }

    #[test]
    fn test_analyze_wad_doom1_full_episode() {
        // Full E1: E1M1..E1M8 + E1M9 secret. Walk follows vanilla edges all
        // the way to E1M8 (stopper, no outgoing edge). E1M9 is reached from
        // E1M3's secret edge.
        let ld_normal = build_linedefs(&[11]);
        let ld_secret = build_linedefs(&[51]);
        let ld_dead = build_linedefs(&[]);
        let mut lumps: Vec<(String, Vec<u8>)> = Vec::new();
        for m in 1..=8 {
            lumps.push((format!("E1M{m}"), Vec::new()));
            lumps.push((
                "LINEDEFS".to_string(),
                if m == 8 {
                    ld_dead.clone()
                } else if m == 3 {
                    ld_secret.clone()
                } else {
                    ld_normal.clone()
                },
            ));
        }
        lumps.push(("E1M9".to_string(), Vec::new()));
        lumps.push(("LINEDEFS".to_string(), ld_normal.clone()));
        let lump_refs: Vec<(&str, &[u8])> = lumps
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let wad = build_wad(&lump_refs);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(a.total_maps, 9);
        assert_eq!(classify_of(&a, "E1M1"), MapClassification::Required);
        assert_eq!(classify_of(&a, "E1M7"), MapClassification::Required);
        // E1M8 has no exit linedef and no outgoing edge → stopper
        assert_eq!(classify_of(&a, "E1M8"), MapClassification::OptionalCredits);
        // E1M9 reached only via E1M3 secret edge → optional
        assert_eq!(classify_of(&a, "E1M9"), MapClassification::OptionalSecret);
    }

    // -----------------------------------------------------------------------
    // UMAPINFO-driven analysis
    // -----------------------------------------------------------------------

    #[test]
    fn test_analyze_wad_umapinfo_linear_chain() {
        // The Pact-shape: MAP01..MAP06, all linked, MAP06 is stopper.
        let ld_normal = build_linedefs(&[11]);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP02"
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
    next = "MAP05"
}
MAP MAP05
{
    next = "MAP06"
}
MAP MAP06
{
}
"#;
        let mut lumps = Vec::new();
        for i in 1..=6 {
            lumps.push((format!("MAP{:02}", i), Vec::new()));
            lumps.push(("LINEDEFS".to_string(), ld_normal.clone()));
        }
        lumps.push(("UMAPINFO".to_string(), umapinfo.to_vec()));
        let lump_refs: Vec<(&str, &[u8])> = lumps
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let wad = build_wad(&lump_refs);

        let a = analyze_wad(&wad).unwrap();
        assert!(a.has_umapinfo);
        assert_eq!(classify_of(&a, "MAP05"), MapClassification::Required);
        // MAP06 has no outgoing edge but it has a normal exit linedef, so
        // it's a "false dead-end" — classifier keeps it Required rather than
        // silently rewriting it as a stopper.
        assert_eq!(classify_of(&a, "MAP06"), MapClassification::Required);
    }

    #[test]
    fn test_analyze_wad_umapinfo_explicit_endgame() {
        let ld_normal = build_linedefs(&[11]);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP02"
}
MAP MAP02
{
    next = "MAP03"
}
MAP MAP03
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
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        // MAP03 has has_endgame marker → stopper, OptionalCredits
        assert_eq!(classify_of(&a, "MAP03"), MapClassification::OptionalCredits);
        assert_eq!(a.required_maps, 2);
    }

    #[test]
    fn test_analyze_wad_umapinfo_forced_secret_continuation() {
        // Formless Mother shape: MAP04 has only a secret exit to MAP31, and
        // MAP31 is the true ending. MAP31 must be Required.
        let ld_normal = build_linedefs(&[11]);
        let ld_secret = build_linedefs(&[51]);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP02"
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
    nextsecret = "MAP31"
}
MAP MAP31
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
            ("LINEDEFS", &ld_secret),
            ("MAP31", &[]),
            ("LINEDEFS", &ld_normal),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP04"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP31"), MapClassification::OptionalCredits);
        // No secrets — MAP31 was forced, not optional.
        assert!(a.secret_maps.is_empty());
    }

    #[test]
    fn test_analyze_wad_umapinfo_skippable_secret() {
        // MAP15 has both `next = MAP16` and `nextsecret = MAP31`, so MAP31
        // is a skippable secret branch (OptionalSecret).
        let ld = build_linedefs(&[11]);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP15"
}
MAP MAP15
{
    next = "MAP16"
    nextsecret = "MAP31"
}
MAP MAP16
{
    next = "MAP30"
}
MAP MAP30
{
    endgame = true
}
MAP MAP31
{
    next = "MAP16"
}
"#;
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("LINEDEFS", &ld),
            ("MAP15", &[]),
            ("LINEDEFS", &ld),
            ("MAP16", &[]),
            ("LINEDEFS", &ld),
            ("MAP30", &[]),
            ("LINEDEFS", &ld),
            ("MAP31", &[]),
            ("LINEDEFS", &ld),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP15"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP31"), MapClassification::OptionalSecret);
        assert_eq!(classify_of(&a, "MAP30"), MapClassification::OptionalCredits);
    }

    // -----------------------------------------------------------------------
    // ZMAPINFO precedence (Vertex Relocation case)
    // -----------------------------------------------------------------------

    #[test]
    fn test_analyze_wad_zmapinfo_overrides_umapinfo() {
        // UMAPINFO routes MAP17 → MAP18 (vanilla flow).
        // ZMAPINFO routes MAP17 → MAP18GZ. ZMAPINFO must win.
        let ld = build_linedefs(&[11]);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP17"
}
MAP MAP17
{
    next = "MAP18"
}
MAP MAP18
{
    next = "MAP19"
}
MAP MAP19
{
    endgame = true
}
"#;
        let zmapinfo = br#"
map MAP17 "Feline Squire"
{
    next = "MAP18GZ"
}
map MAP18GZ "Biocide"
{
    next = "MAP19"
}
"#;
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("LINEDEFS", &ld),
            ("MAP17", &[]),
            ("LINEDEFS", &ld),
            ("MAP18", &[]),
            ("LINEDEFS", &ld),
            ("MAP18GZ", &[]),
            ("LINEDEFS", &ld),
            ("MAP19", &[]),
            ("LINEDEFS", &ld),
            ("UMAPINFO", umapinfo),
            ("ZMAPINFO", zmapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        // ZMAPINFO routes through MAP18GZ; MAP18 becomes orphan
        assert_eq!(classify_of(&a, "MAP17"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP18GZ"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP18"), MapClassification::Unreachable);
        assert_eq!(classify_of(&a, "MAP19"), MapClassification::OptionalCredits);
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_analyze_wad_no_maps() {
        let wad = build_wad(&[("THINGS", &[]), ("LINEDEFS", &[])]);
        assert!(analyze_wad(&wad).is_none());
    }

    #[test]
    fn test_analyze_wad_invalid_input() {
        assert!(analyze_wad(b"").is_none());
        assert!(analyze_wad(b"NOTAWAD!").is_none());
    }

    #[test]
    fn test_map_sort_key() {
        assert!(map_sort_key("MAP01") < map_sort_key("MAP02"));
        assert!(map_sort_key("MAP09") < map_sort_key("MAP10"));
        assert!(map_sort_key("E1M1") < map_sort_key("E1M8"));
        assert!(map_sort_key("E1M8") < map_sort_key("E2M1"));
    }

    #[test]
    fn test_is_map_marker() {
        assert!(is_map_marker("MAP01"));
        assert!(is_map_marker("MAP32"));
        assert!(is_map_marker("MAP100"));
        assert!(is_map_marker("E1M1"));
        assert!(!is_map_marker("THINGS"));
        assert!(!is_map_marker("MAP001"));
    }

    #[test]
    fn test_classify_unreachable_orphan() {
        // MAP01 → MAP03 via UMAPINFO, MAP02 has no incoming edge.
        let ld = build_linedefs(&[11]);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP03"
}
MAP MAP03
{
    endgame = true
}
"#;
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("LINEDEFS", &ld),
            ("MAP02", &[]),
            ("LINEDEFS", &ld),
            ("MAP03", &[]),
            ("LINEDEFS", &ld),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP02"), MapClassification::Unreachable);
        assert_eq!(classify_of(&a, "MAP03"), MapClassification::OptionalCredits);
    }
}
