//! WAD analysis module for automated completion detection.
//!
//! Two pure layers:
//! - `analyze_path` gathers the maps of whatever a launch loads for a WAD's
//!   file, builds a directed map graph from their
//!   ZMAPINFO/UMAPINFO/MAPINFO/vanilla edges, walks the main path, then peels
//!   trailing zero-monster maps off the tail as `OptionalCredits`.
//! - `completion_detect::check_completion` intersects the Required set with
//!   the player's exit stats. The classifier never sees stats.
//!
//! We deliberately do not try to detect exit linedefs, boss-brain exits,
//! sector death-exits, ACS exits, or any other "how does this map end"
//! mechanism. WAD designers have too many ways to wire up an exit, and any
//! detection we add is brittle. Instead we lean on a single robust signal:
//! a playable map has monsters, a credits map does not.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::companion_service::is_deh_bex;
use crate::mapinfo::MapinfoEntry;
use crate::utils::{MAX_ZIP_ENTRY_SIZE, parse_wad_directory, zip_entry_names};
use crate::{identity, unpack};

// ---------------------------------------------------------------------------
// Map name patterns
// ---------------------------------------------------------------------------

static DOOM1_MAP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^E(\d)M(\d)$").unwrap());
static DOOM2_MAP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^MAP([1-9]\d{2}|\d{2})$").unwrap());

/// The lump a map starts with, in the original format and in UDMF.
const FIRST_MAP_LUMPS: &[&str] = &["THINGS", "TEXTMAP"];

/// Lumps that belong to a map definition (appear after the map marker).
const MAP_LUMPS: &[&str] = &[
    "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES", "SECTORS", "REJECT",
    "BLOCKMAP", "BEHAVIOR", "SCRIPTS", "TEXTMAP", "ENDMAP", "DIALOGUE", "ZNODES",
];

/// Analysis format version. Bump this whenever detection logic changes
/// so that stale cached analyses are automatically invalidated and re-run.
pub const ANALYSIS_VERSION: u32 = 14;

/// Tail maps with fewer than this many monster things are peeled off as
/// `OptionalCredits`. Threshold is "more than one" so a single decorative
/// imp on a credits screen doesn't make the map count as playable.
const MIN_MONSTERS_FOR_PLAYABLE: usize = 2;

/// Doom thing types that count as monsters for the "is this map playable"
/// heuristic. Editor numbers, not DEH mobjinfo slots. Boss Brain (88) is the
/// Romero head — not strictly COUNTKILL, but it's the only target on an Icon
/// of Sin map and we want those classified as playable.
const MONSTER_THING_TYPES: &[u16] = &[
    7,    // Spider Mastermind
    9,    // Shotgun Guy
    16,   // Cyberdemon
    58,   // Spectre
    64,   // Archvile
    65,   // Heavy Weapon Dude
    66,   // Revenant
    67,   // Mancubus
    68,   // Arachnotron
    69,   // Hell Knight
    71,   // Pain Elemental
    72,   // Commander Keen
    84,   // Wolfenstein SS
    88,   // Boss Brain (Romero head)
    3001, // Imp
    3002, // Demon
    3003, // Baron of Hell
    3004, // Former Human
    3005, // Cacodemon
    3006, // Lost Soul
];

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
    /// Reached from the episode menu, on a route that joins another partway:
    /// After Doom's bonus maps. Another way into an ending already counted, so
    /// optional.
    OptionalBonus,
    /// Lump exists but no incoming edge in any flow. Not part of completion.
    Unreachable,
}

/// Per-map analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapInfo {
    /// Map lump name (e.g., "MAP01", "E1M1").
    pub lump: String,
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
    /// Derived: the first map (in directory order) where
    /// `classification == OptionalCredits`, if any.
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
    sources: &[(FlowSource, HashMap<String, MapinfoEntry>)],
) -> MapGraph {
    let mut by_priority: Vec<&(FlowSource, HashMap<String, MapinfoEntry>)> =
        sources.iter().collect();
    by_priority.sort_by_key(|(s, _)| s.priority());

    let mut graph = MapGraph::default();

    // Layer 0: vanilla edges (lowest priority, applied first)
    add_vanilla_edges(&mut graph, map_set, is_doom1);

    // Higher layers: overlay each source in priority order. Each property
    // is only overridden when the higher-priority source explicitly sets it
    // — an empty entry like `MAP MAP01 { }` does NOT clear vanilla edges.
    // Setting endgame=true also clears any normal/secret edge for that map
    // (game ends here, no progression). A self-loop in `next`/`nextsecret`
    // (`map MAP10 { next = "MAP10" }`) is the established UMAPINFO idiom for
    // "stops here" — treat it as endgame so the tail-peel can promote it to
    // OptionalCredits when it has no monsters.
    for (_, entries) in &by_priority {
        for (lump, edge) in entries.iter() {
            if !map_set.contains(lump.as_str()) {
                continue;
            }

            let normal_self_loop = edge.next.as_deref() == Some(lump.as_str());
            let secret_self_loop = edge.secretnext.as_deref() == Some(lump.as_str());

            if let Some(ref nx) = edge.next {
                if normal_self_loop {
                    graph.edges_normal.remove(lump.as_str());
                } else if map_set.contains(nx.as_str()) {
                    graph.edges_normal.insert(lump.clone(), nx.clone());
                } else {
                    graph.edges_normal.remove(lump.as_str());
                }
            }
            if let Some(ref sx) = edge.secretnext {
                if secret_self_loop {
                    graph.edges_secret.remove(lump.as_str());
                } else if map_set.contains(sx.as_str()) {
                    graph.edges_secret.insert(lump.clone(), sx.clone());
                } else {
                    graph.edges_secret.remove(lump.as_str());
                }
            }
            // Stopper if explicit endgame, or if `next = self` and the only
            // alternative (secretnext) is absent or also self-looping.
            // `next = self` paired with a real `nextsecret = OTHER` is NOT a
            // stopper — the secret edge is the path forward.
            let stops_here = edge.has_endgame
                || (normal_self_loop && (secret_self_loop || edge.secretnext.is_none()));
            if stops_here {
                graph.has_endgame.insert(lump.clone());
                // The normal exit ends the episode; drop any normal edge
                // (including a vanilla one inherited from the map name).
                graph.edges_normal.remove(lump.as_str());
                // A secret branch survives only when *this* entry explicitly
                // declared a valid one: `next = endpic` with `secretnext =
                // MAP31` ends the episode on the normal exit but still branches
                // to a secret level. An inherited vanilla secret edge (e.g. a
                // Doom II MAP15 -> MAP31 the map name implies) is not a real
                // branch once the episode ends here, so drop it.
                let has_explicit_secret = edge
                    .secretnext
                    .as_deref()
                    .is_some_and(|sx| !secret_self_loop && map_set.contains(sx));
                if !has_explicit_secret {
                    graph.edges_secret.remove(lump.as_str());
                }
            }
        }
    }

    graph
}

/// Add vanilla map-flow edges based on map naming conventions.
///
/// These are the edges the Doom/Doom 2 engine implies from map names alone,
/// independent of any linedef inspection. We synthesize them whenever both
/// source and destination maps exist in the WAD — exit-linedef detection is
/// too brittle (Boom generalized specials, voodoo plumbing, DEH-patched
/// codepointers, ACS scripts all bypass it). If a map happens to have no
/// reachable exit, that's a WAD-design issue we can't see from static lump
/// inspection; the tail-peel step handles the case where the designer
/// genuinely intends the last map to be a credits screen.
fn add_vanilla_edges(graph: &mut MapGraph, map_set: &HashSet<&str>, is_doom1: bool) {
    if is_doom1 {
        // ExMy → ExM(y+1) for y < 8; ExM3 secret → ExM9.
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
        // MAP_n → MAP_(n+1) for n < 30; MAP15 secret → MAP31;
        // MAP31 normal → MAP16 + secret → MAP32; MAP32 normal → MAP16.
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
// Internal: classification (graph walk + tail peel)
// ---------------------------------------------------------------------------

/// Walk the graph and assign a classification to each map.
///
/// Algorithm:
/// 1. Roots = the canonical start (the lowest map with a standard name, where
///    a new game begins) plus every `episode` entry point. A multi-episode WAD
///    has no single linear path through all maps, so each episode's first map
///    is walked independently. A WAD whose maps all have names of its own
///    starts where its episodes do, and with none of those, at its first map.
/// 2. From each root, in that order, walk the main path, preferring normal
///    edges, falling back to secret edges only when no normal edge exists
///    ("forced secret = true ending"). A node already claimed by an earlier
///    walk stops the walk, and where it stopped decides what the walk was
///    ([`WalkEnd`]): an episode of its own, whose maps are `Required`, or
///    another way into an ending already counted, whose maps are
///    `OptionalBonus`.
/// 3. **Tail peel** (per required walk): starting from the end, demote each
///    map to `OptionalCredits` while its monster count is below the playable
///    threshold. The deepest map with real monsters is the playable terminus.
///    A walk's *first* map (a root) is never peeled — we always require at
///    least one Required map so an empty/test WAD doesn't auto-complete.
/// 4. Walk secret branches off every walked node; mark what they reach off
///    the required path as `OptionalSecret`.
/// 5. Anything still unmarked is `Unreachable`.
fn classify_maps(
    graph: &MapGraph,
    infos: &mut [MapInfo],
    monsters: &HashMap<String, usize>,
    episode_roots: &[String],
) {
    if infos.is_empty() {
        return;
    }

    // The canonical start: the lowest map with a standard name.
    let start = infos
        .iter()
        .map(|m| m.lump.as_str())
        .filter(|lump| is_map_marker(lump))
        .min_by_key(|lump| map_sort_key(lump))
        .map(str::to_string);

    // Pre-fill all as Unreachable; we'll upgrade as we walk.
    for m in infos.iter_mut() {
        m.classification = MapClassification::Unreachable;
    }
    let by_lump: HashMap<String, usize> = infos
        .iter()
        .enumerate()
        .map(|(i, m)| (m.lump.clone(), i))
        .collect();

    // Roots: canonical start first, then each episode entry point that exists
    // as a playable map. Order matters — the start owns shared tails; episode
    // walks pick up only the maps the start didn't already claim.
    let mut roots: Vec<String> = start.into_iter().collect();
    for r in episode_roots {
        if by_lump.contains_key(r) && !roots.contains(r) {
            roots.push(r.clone());
        }
    }
    // With neither, the first map in order is the start, so that the WAD still
    // has a map to finish.
    if roots.is_empty()
        && let Some(first) = infos
            .iter()
            .map(|m| m.lump.as_str())
            .min_by(|a, b| map_order(a, b))
    {
        roots.push(first.to_string());
    }

    // 1. Main-path walk from every root. A map belongs to the first walk to
    //    reach it, so a walk that runs into an earlier one stops there.
    let mut claimed_by: HashMap<String, usize> = HashMap::new();
    // Each walk's maps in the order it reached them, and whether they are
    // required. A root an earlier walk already reached walks nothing.
    let mut walks: Vec<(Vec<String>, bool)> = Vec::new();
    for root in &roots {
        let this = walks.len();
        let mut walk_order: Vec<String> = Vec::new();
        let mut end = WalkEnd::OwnEnding;
        let mut current = Some(root.clone());
        while let Some(node) = current {
            if let Some(&earlier) = claimed_by.get(&node) {
                end = if earlier == this {
                    WalkEnd::OwnEnding // came back round on itself
                } else if walks[earlier].0.first() == Some(&node) {
                    WalkEnd::IntoStartOf(earlier)
                } else {
                    WalkEnd::Joins
                };
                break;
            }
            claimed_by.insert(node.clone(), this);
            walk_order.push(node.clone());
            if graph.has_endgame.contains(&node) {
                break;
            }
            current = match (graph.edges_normal.get(&node), graph.edges_secret.get(&node)) {
                (Some(nx), _) => Some(nx.clone()),
                (None, Some(sx)) => Some(sx.clone()), // forced secret = true ending
                (None, None) => None,
            };
        }
        let required = match end {
            WalkEnd::OwnEnding => true,
            WalkEnd::IntoStartOf(earlier) => walks[earlier].1,
            WalkEnd::Joins => false,
        };
        walks.push((walk_order, required));
    }

    let mut classify = |lump: &str, classification: MapClassification| {
        if let Some(&idx) = by_lump.get(lump) {
            infos[idx].classification = classification;
        }
    };

    for (walk_order, required) in &walks {
        if !required {
            for lump in walk_order {
                classify(lump, MapClassification::OptionalBonus);
            }
            continue;
        }
        for lump in walk_order {
            classify(lump, MapClassification::Required);
        }
        // 2. Tail peel this walk: walk back from the end demoting low-monster
        //    maps to OptionalCredits. Stop at the first map that has real
        //    monsters — that's the playable terminus. Never peel index 0 (the
        //    root) so a tiny/empty WAD still has at least one Required map.
        for lump in walk_order.iter().skip(1).rev() {
            if monsters.get(lump).copied().unwrap_or(0) >= MIN_MONSTERS_FOR_PLAYABLE {
                break;
            }
            classify(lump, MapClassification::OptionalCredits);
        }
    }

    // 3. Secret branches off any walked node. A branch stops at the required
    //    path, and a bonus map it reaches is a secret one: that the episode
    //    menu also leads there doesn't make it any less of one.
    let required_path: HashSet<&String> = walks
        .iter()
        .filter(|(_, required)| *required)
        .flat_map(|(walk_order, _)| walk_order)
        .collect();
    let mut sec_queue: VecDeque<String> = VecDeque::new();
    for node in claimed_by.keys() {
        if let Some(sx) = graph.edges_secret.get(node)
            && !required_path.contains(sx)
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
            && matches!(
                infos[idx].classification,
                MapClassification::Unreachable | MapClassification::OptionalBonus
            )
        {
            infos[idx].classification = MapClassification::OptionalSecret;
        }
        for next in [graph.edges_normal.get(&node), graph.edges_secret.get(&node)] {
            if let Some(nx) = next
                && !required_path.contains(nx)
            {
                sec_queue.push_back(nx.clone());
            }
        }
    }
}

/// How a walk from one root ended, which decides whether its maps are needed
/// to finish the WAD.
///
/// An `episode` entry in MAPINFO is a way into the game from the episode menu,
/// and nothing more is promised: Eisberg's tiers are each an arc with an ending
/// of its own, where After Doom lists every map, two of them bonus maps whose
/// `next` rejoins the main route a map later. Finishing a WAD is reaching each
/// of its endings, so a walk that joins another partway adds no ending to
/// reach, and its maps are optional.
enum WalkEnd {
    /// An ending, a map with nowhere to go, or back on itself: an episode of
    /// its own.
    OwnEnding,
    /// Into the first map of an earlier walk, as a prologue leads into an
    /// episode, so as needed as that walk is. The earlier walk only went first
    /// because its episode is declared first.
    IntoStartOf(usize),
    /// Into an earlier walk partway: another way into an ending already
    /// counted.
    Joins,
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
// Public: analysis of a WAD's file
// ---------------------------------------------------------------------------

/// Analyze what a launch hands the sourceport for the file at `wad_path`.
///
/// Goes by what the file is, not what it is called, as a launch does. A zip
/// that only wraps loadable files ([`identity::is_container_archive`]) is never
/// handed over itself: [`crate::unpack`] gives the sourceport the files inside,
/// so those are what is analyzed, together and in the order they load. Any
/// other zip is a resource archive played whole, whatever its extension, and
/// anything else is a WAD. A container used to be searched for a `.wad` and
/// nothing else, so The Warrens: Burrow of the Were, a `.pk3` in a zip, had no
/// analysis, and finishing it finished nothing.
///
/// Centralises the dispatch so `player.rs`, the GUI's lazy re-analysis
/// worker, and the diagnostic harness in `completion_detect.rs` agree on
/// what "analyze the file" means.
pub fn analyze_path(wad_path: &Path) -> Option<WadAnalysis> {
    let mut file = File::open(wad_path).ok()?;
    let mut magic = [0u8; 4];
    let is_zip = file.read_exact(&mut magic).is_ok() && magic == identity::ZIP_MAGIC;
    file.rewind().ok()?;
    let gathered = if is_zip {
        let mut archive = ZipArchive::new(file).ok()?;
        if identity::is_container_archive(&archive) {
            gather_container(&mut archive)?
        } else {
            gather_archive(&mut archive)?
        }
    } else {
        let mut data = Vec::new();
        file.read_to_end(&mut data).ok()?;
        gather_wad(&data)?
    };
    gathered.classify()
}

/// What the files a launch loads say about their maps, before any of it is
/// classified.
///
/// Several files gather into one as a sourceport reads them, in load order: a
/// later file's map replaces an earlier one's of the same name, and its
/// MAPINFO is read after theirs.
#[derive(Default)]
struct Gathered {
    /// Each map once, where it was first found.
    maps: Vec<String>,
    /// How many monsters each map has, from the last file to have it.
    monsters: HashMap<String, usize>,
    sources: Vec<(FlowSource, HashMap<String, MapinfoEntry>)>,
    episode_roots: Vec<String>,
    /// MAPINFO read from a resource archive, the only kind whose flow is
    /// checked for hubs.
    archive_mapinfo: Option<HashMap<String, MapinfoEntry>>,
    /// Whether any structured map flow was found (UMAPINFO/MAPINFO/ZMAPINFO).
    has_structured: bool,
}

impl Gathered {
    fn add_map(&mut self, name: String, monsters: usize) {
        if self.monsters.insert(name.clone(), monsters).is_none() {
            self.maps.push(name);
        }
    }

    /// `later` read after what is here, as a sourceport reads a file loaded
    /// after the others.
    fn extend(&mut self, later: Gathered) {
        let Gathered {
            maps,
            mut monsters,
            sources,
            episode_roots,
            archive_mapinfo,
            has_structured,
        } = later;
        for name in maps {
            let count = monsters.remove(&name).unwrap_or(0);
            self.add_map(name, count);
        }
        self.sources.extend(sources);
        self.episode_roots.extend(episode_roots);
        if let Some(mapinfo) = archive_mapinfo {
            self.archive_mapinfo.get_or_insert_default().extend(mapinfo);
        }
        self.has_structured |= has_structured;
    }

    /// Classify the maps. `None` when there is no map a game is played on.
    fn classify(self) -> Option<WadAnalysis> {
        let mut infos: Vec<MapInfo> = self
            .maps
            .into_iter()
            .filter(|lump| is_playable(lump))
            .map(|lump| MapInfo {
                lump,
                classification: MapClassification::Unreachable,
            })
            .collect();
        if infos.is_empty() {
            return None;
        }
        let lumps: Vec<String> = infos.iter().map(|m| m.lump.clone()).collect();

        if self
            .archive_mapinfo
            .as_ref()
            .is_some_and(|mapinfo| detect_hub_structure(mapinfo, &lumps))
        {
            classify_hub(&mut infos);
            return Some(finalize(infos, self.has_structured));
        }

        let map_set: HashSet<&str> = lumps.iter().map(String::as_str).collect();
        let is_doom1 = lumps
            .iter()
            .find(|lump| is_map_marker(lump))
            .is_some_and(|lump| DOOM1_MAP_RE.is_match(lump));
        let graph = build_graph(&map_set, is_doom1, &self.sources);
        classify_maps(&graph, &mut infos, &self.monsters, &self.episode_roots);
        Some(finalize(infos, self.has_structured))
    }
}

/// Whether a map is one a game is played on. Any name a map has counts, since
/// a ZDoom WAD can name its own (The Warrens' only map is BTW01), and whether
/// the game ever reaches one is for its flow to say. The title screen's map is
/// the one that is never played.
fn is_playable(lump: &str) -> bool {
    !lump.eq_ignore_ascii_case("TITLEMAP")
}

/// What a WAD holds: its maps, and the flow its MAPINFO lumps give them.
/// `None` for what is not a WAD.
fn gather_wad(wad_data: &[u8]) -> Option<Gathered> {
    let directory = parse_wad_directory(wad_data);
    if directory.is_empty() {
        return None;
    }

    let mut gathered = Gathered::default();
    for (name, start_idx, end_idx) in find_map_ranges(&directory) {
        let count = count_monsters_in_range(wad_data, &directory, start_idx, end_idx);
        gathered.add_map(name, count);
    }

    // An include in a WAD names another lump.
    let mapinfo = |lump: &str| {
        let text = read_lump_text(wad_data, &directory, lump)?;
        let mut read = |name: &str| read_lump_text(wad_data, &directory, &name.to_uppercase());
        Some(crate::mapinfo::parse_mapinfo(&text, &mut read))
    };
    if let Some(mapinfo) = mapinfo("ZMAPINFO") {
        if !mapinfo.maps.is_empty() {
            gathered.has_structured = true;
            gathered.sources.push((FlowSource::Zmapinfo, mapinfo.maps));
        }
        gathered.episode_roots.extend(mapinfo.episodes);
    }
    if let Some(text) = read_lump_text(wad_data, &directory, "UMAPINFO") {
        let entries = crate::mapinfo::parse_umapinfo(&text);
        if !entries.is_empty() {
            gathered.has_structured = true;
            gathered.sources.push((FlowSource::Umapinfo, entries));
        }
    }
    if let Some(mapinfo) = mapinfo("MAPINFO") {
        if !mapinfo.maps.is_empty() {
            gathered.has_structured = true;
            gathered.sources.push((FlowSource::Mapinfo, mapinfo.maps));
        }
        gathered.episode_roots.extend(mapinfo.episodes);
    }

    Some(gathered)
}

/// What a resource archive (a PK3, or any zip played whole) holds, as ZDoom
/// reads one: a map per WAD under `maps/`, named after its file, the MAPINFO at
/// its root, and then each WAD at its root as if it had been loaded after it.
///
/// `None` when an entry it needs cannot be read, rather than an analysis short
/// of a map, which could have the WAD finished early.
fn gather_archive<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Option<Gathered> {
    let names: Vec<(usize, String)> = zip_entry_names(archive)
        .map(|(index, name)| (index, name.into_owned()))
        .collect();
    let mut gathered = Gathered::default();

    for (index, name) in &names {
        let lower = name.to_lowercase();
        if !(lower.starts_with("maps/") && lower.ends_with(".wad")) {
            continue;
        }
        let data = read_entry(archive, *index)?;
        let stem = Path::new(name).file_stem()?.to_str()?.to_uppercase();
        gathered.add_map(stem, count_monsters_in_map_wad(&data));
    }

    let mut mapinfo_text = String::new();
    for (index, name) in &names {
        let lower = name.to_lowercase();
        if !name.contains('/') && (lower.starts_with("mapinfo") || lower.starts_with("zmapinfo")) {
            let text = read_entry(archive, *index)?;
            mapinfo_text.push('\n');
            mapinfo_text.push_str(&String::from_utf8_lossy(&text));
        }
    }
    if !mapinfo_text.is_empty() {
        // An include in an archive names an entry by its path, or a lump at the root
        // by its name, which is its file name up to the first dot.
        let mut read = |wanted: &str| {
            let wanted = wanted.replace('\\', "/");
            let (index, _) = names.iter().find(|(_, name)| {
                name.eq_ignore_ascii_case(&wanted)
                    || (!name.contains('/')
                        && name
                            .split('.')
                            .next()
                            .is_some_and(|lump| lump.eq_ignore_ascii_case(&wanted)))
            })?;
            let text = read_entry(archive, *index)?;
            Some(String::from_utf8_lossy(&text).into_owned())
        };
        let mapinfo = crate::mapinfo::parse_mapinfo(&mapinfo_text, &mut read);
        if !mapinfo.maps.is_empty() {
            // ZMAPINFO and MAPINFO are not told apart here, since mapinfo.rs reads
            // both: the archive's is a Mapinfo source.
            gathered
                .sources
                .push((FlowSource::Mapinfo, mapinfo.maps.clone()));
        }
        gathered.episode_roots.extend(mapinfo.episodes);
        gathered.archive_mapinfo = Some(mapinfo.maps);
        gathered.has_structured = true;
    }

    let mut root_wads: Vec<&(usize, String)> = names
        .iter()
        .filter(|(_, name)| !name.contains('/') && name.to_lowercase().ends_with(".wad"))
        .collect();
    root_wads.sort_by_cached_key(|(_, name)| unpack::load_order(name));
    for (index, _) in root_wads {
        let data = read_entry(archive, *index)?;
        if let Some(wad) = gather_wad(&data) {
            gathered.extend(wad);
        }
    }

    Some(gathered)
}

/// What a container hands the sourceport, which is each of its loadable files,
/// in the order [`crate::unpack`] loads them. Patches hold no maps.
fn gather_container<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Option<Gathered> {
    let mut files = identity::loadable_entries(archive);
    files.sort_by_cached_key(|(_, name)| unpack::load_order(name));

    let mut gathered = Gathered::default();
    for (index, name) in files {
        if is_deh_bex(Path::new(&name)) {
            continue;
        }
        let data = read_entry(archive, index)?;
        let found = if data.starts_with(&identity::ZIP_MAGIC) {
            gather_archive(&mut ZipArchive::new(Cursor::new(data)).ok()?)
        } else {
            gather_wad(&data)
        };
        if let Some(found) = found {
            gathered.extend(found);
        }
    }
    Some(gathered)
}

/// A zip entry's bytes, or `None` for one that cannot be read or is larger
/// than any WAD.
fn read_entry<R: Read + Seek>(archive: &mut ZipArchive<R>, index: usize) -> Option<Vec<u8>> {
    let entry = archive.by_index(index).ok()?;
    if entry.size() > MAX_ZIP_ENTRY_SIZE {
        return None;
    }
    let mut data = Vec::with_capacity(entry.size() as usize);
    // The size is what the zip says, which a damaged one can get wrong.
    entry
        .take(MAX_ZIP_ENTRY_SIZE + 1)
        .read_to_end(&mut data)
        .ok()?;
    (data.len() as u64 <= MAX_ZIP_ENTRY_SIZE).then_some(data)
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
fn detect_hub_structure(mapinfo: &HashMap<String, MapinfoEntry>, playable_maps: &[String]) -> bool {
    let playable_set: HashSet<&str> = playable_maps.iter().map(|s| s.as_str()).collect();

    let mut target_counts: HashMap<&str, usize> = HashMap::new();
    for (name, entry) in mapinfo {
        if !is_playable(name) {
            continue;
        }
        if let Some(ref next) = entry.next {
            *target_counts.entry(next.as_str()).or_default() += 1;
        }
    }

    let playable_count = playable_maps.len();
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
/// - The lump after it is the one a map starts with, THINGS or TEXTMAP,
///   whatever its own name.
///
/// The second rule is how ZDoom finds a map, and catches the names ZDoom maps
/// have: ZMAPINFO's alternates (MAP18GZ) and a WAD's own (BTW01). It asks for
/// a map's first lump and not any of them, because SCRIPTS, BEHAVIOR and
/// DIALOGUE can also stand on their own, after a lump that is no map.
fn find_map_ranges(directory: &[(String, u32, u32)]) -> Vec<(String, usize, usize)> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < directory.len() {
        let name = &directory[i].0;
        let starts_a_map = directory
            .get(i + 1)
            .is_some_and(|(n, _, _)| FIRST_MAP_LUMPS.contains(&n.as_str()));
        let is_marker = is_map_marker(name) || starts_a_map;
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
// Monster counting
// ---------------------------------------------------------------------------

fn count_monsters_in_range(
    wad_data: &[u8],
    directory: &[(String, u32, u32)],
    start_idx: usize,
    end_idx: usize,
) -> usize {
    let map_lumps = &directory[start_idx..end_idx];

    if let Some(textmap) = find_lump_data(wad_data, map_lumps, "TEXTMAP") {
        return count_udmf_monsters(&textmap);
    }

    if let Some(things) = find_lump_data(wad_data, map_lumps, "THINGS") {
        return count_vanilla_monsters(&things);
    }

    0
}

fn count_monsters_in_map_wad(wad_data: &[u8]) -> usize {
    let directory = parse_wad_directory(wad_data);
    if directory.is_empty() {
        return 0;
    }
    let map_ranges = find_map_ranges(&directory);
    if let Some((_, start, end)) = map_ranges.first() {
        count_monsters_in_range(wad_data, &directory, *start, *end)
    } else {
        0
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

/// Count monster things in a vanilla/Boom THINGS lump.
fn count_vanilla_monsters(things: &[u8]) -> usize {
    let thing_size = 10;
    let count = things.len() / thing_size;
    let mut n = 0;
    for i in 0..count {
        let base = i * thing_size;
        if base + 8 > things.len() {
            break;
        }
        let thing_type = u16::from_le_bytes([things[base + 6], things[base + 7]]);
        if MONSTER_THING_TYPES.contains(&thing_type) {
            n += 1;
        }
    }
    n
}

/// Count monster things in a UDMF TEXTMAP lump.
fn count_udmf_monsters(textmap_data: &[u8]) -> usize {
    let text = String::from_utf8_lossy(textmap_data);

    static THING_BLOCK_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)\bthing\b(?:\s*//[^\r\n]*)?\s*\{([^}]*)\}").unwrap());
    static TYPE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\btype\s*=\s*(\d+)").unwrap());

    let mut n = 0;
    for block in THING_BLOCK_RE.captures_iter(&text) {
        let body = &block[1];
        if let Some(caps) = TYPE_RE.captures(body)
            && let Ok(t) = caps[1].parse::<u16>()
            && MONSTER_THING_TYPES.contains(&t)
        {
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// Sorting helpers
// ---------------------------------------------------------------------------

/// The order to list a WAD's maps in for a person to read. Numbers go by value wherever
/// they are in a name, so MAP02 comes before MAP10, E1M8 before E2M1 and MAP99 before
/// MAP100, and a name with something after its number (`MAP18GZ`) comes straight after the
/// map it goes with.
///
/// Neither list a view has is in this order. [`WadAnalysis::maps`] is in the order the file
/// had them, which for a PK3 is the order its maps were zipped in (Purgatory's starts MAP22,
/// MAP16, MAP15), and stats are in the order they were merged in. It is not
/// `map_sort_key` either: that knows standard names only, and is how the first map is
/// found.
pub fn map_order(a: &str, b: &str) -> Ordering {
    let (mut left, mut right) = (a, b);
    loop {
        let (l, r) = (leading_run(left), leading_run(right));
        if l.is_empty() || r.is_empty() {
            // The shorter name first, and names that differ only in case or in leading
            // zeros still have an order, so the same maps always list the same way.
            return l.len().cmp(&r.len()).then_with(|| a.cmp(b));
        }
        let by_run = if is_number(l) && is_number(r) {
            // By length once the zeros are off, so a number of any size can be compared.
            let (l, r) = (l.trim_start_matches('0'), r.trim_start_matches('0'));
            l.len().cmp(&r.len()).then_with(|| l.cmp(r))
        } else {
            let upper = |c: char| c.to_ascii_uppercase();
            l.chars().map(upper).cmp(r.chars().map(upper))
        };
        if by_run != Ordering::Equal {
            return by_run;
        }
        left = &left[l.len()..];
        right = &right[r.len()..];
    }
}

/// The digits a name starts with, or everything before its first digit.
fn leading_run(name: &str) -> &str {
    let digits = is_number(name);
    let end = name
        .find(|c: char| c.is_ascii_digit() != digits)
        .unwrap_or(name.len());
    &name[..end]
}

fn is_number(run: &str) -> bool {
    run.starts_with(|c: char| c.is_ascii_digit())
}

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

    /// Build a THINGS lump from a list of thing types.
    fn build_things(types: &[u16]) -> Vec<u8> {
        let mut data = Vec::new();
        for &thing_type in types {
            let mut thing = [0u8; 10];
            thing[6] = (thing_type & 0xFF) as u8;
            thing[7] = (thing_type >> 8) as u8;
            data.extend_from_slice(&thing);
        }
        data
    }

    /// Build a THINGS lump with `n` imps (a "playable" map).
    fn playable_things(n: usize) -> Vec<u8> {
        build_things(&vec![3001u16; n])
    }

    /// Build a UDMF TEXTMAP with the given thing types.
    fn build_textmap_things(types: &[u16]) -> Vec<u8> {
        let mut text = String::from("namespace = \"zdoom\";\n");
        for &t in types {
            text.push_str(&format!(
                "thing {{\n  type = {};\n  x = 0;\n  y = 0;\n}}\n",
                t
            ));
        }
        text.into_bytes()
    }

    fn analyze_wad(wad_data: &[u8]) -> Option<WadAnalysis> {
        gather_wad(wad_data)?.classify()
    }

    fn classify_of(a: &WadAnalysis, lump: &str) -> MapClassification {
        a.maps
            .iter()
            .find(|m| m.lump == lump)
            .map(|m| m.classification)
            .expect("lump not found")
    }

    // -----------------------------------------------------------------------
    // Monster counting
    // -----------------------------------------------------------------------

    #[test]
    fn test_count_vanilla_monsters_basic() {
        // Three imps and a player start
        let things = build_things(&[3001, 3001, 3001, 1]);
        assert_eq!(count_vanilla_monsters(&things), 3);
    }

    #[test]
    fn test_count_vanilla_monsters_excludes_decorations() {
        // Player start, lamp, key — all non-monsters
        let things = build_things(&[1, 2028, 5]);
        assert_eq!(count_vanilla_monsters(&things), 0);
    }

    #[test]
    fn test_count_vanilla_monsters_full_bestiary() {
        // One of every monster type the heuristic knows about
        let types: Vec<u16> = MONSTER_THING_TYPES.to_vec();
        let things = build_things(&types);
        assert_eq!(count_vanilla_monsters(&things), MONSTER_THING_TYPES.len());
    }

    #[test]
    fn test_count_udmf_monsters_basic() {
        let textmap = build_textmap_things(&[3001, 3001, 1, 2028]);
        assert_eq!(count_udmf_monsters(&textmap), 2);
    }

    #[test]
    fn test_count_udmf_monsters_with_comment_header() {
        // UDB-style comment between block keyword and brace.
        let textmap = br#"
namespace = "zdoom";

thing // 42
{
  type = 3001;
  x = 0;
  y = 0;
}
"#;
        assert_eq!(count_udmf_monsters(textmap), 1);
    }

    // -----------------------------------------------------------------------
    // Vanilla full-WAD analysis
    // -----------------------------------------------------------------------

    #[test]
    fn test_vanilla_doom2_chain_with_empty_terminus() {
        // MAP01 → MAP02 → MAP03. MAP03 has no monsters → peeled as credits.
        let monsters = playable_things(5);
        let no_monsters = playable_things(0);
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &monsters),
            ("MAP02", &[]),
            ("THINGS", &monsters),
            ("MAP03", &[]),
            ("THINGS", &no_monsters),
        ]);

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(a.total_maps, 3);
        assert_eq!(classify_of(&a, "MAP01"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP02"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP03"), MapClassification::OptionalCredits);
        assert_eq!(a.required_maps, 2);
        assert_eq!(a.terminal_map.as_deref(), Some("MAP03"));
    }

    #[test]
    fn test_pgr_shape_no_exit_with_monsters_stays_required() {
        // Regression for Perdition's Gate Resurgence (WAD #133). MAP29 has
        // no detectable exit linedef but is a real playable map with many
        // monsters. Old algorithm peeled it as terminal; new algorithm trusts
        // the monster count and keeps the whole chain required.
        let monsters = playable_things(50);
        let mut lumps: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 1..=30 {
            lumps.push((format!("MAP{:02}", i), Vec::new()));
            lumps.push(("THINGS".to_string(), monsters.clone()));
        }
        let lump_refs: Vec<(&str, &[u8])> = lumps
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let wad = build_wad(&lump_refs);

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(a.required_maps, 30);
        assert_eq!(classify_of(&a, "MAP29"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP30"), MapClassification::Required);
        assert_eq!(a.terminal_map, None);
    }

    #[test]
    fn test_sewerlust_shape_trailing_credits_peeled() {
        // Adjacent no-monster credits maps after the real finale must all
        // demote to OptionalCredits. MAP23 is the playable terminus.
        let monsters = playable_things(20);
        let empty = playable_things(0);
        let wad = build_wad(&[
            ("MAP23", &[]),
            ("THINGS", &monsters),
            ("MAP24", &[]),
            ("THINGS", &empty),
            ("MAP25", &[]),
            ("THINGS", &empty),
        ]);

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP23"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP24"), MapClassification::OptionalCredits);
        assert_eq!(classify_of(&a, "MAP25"), MapClassification::OptionalCredits);
        assert_eq!(a.required_maps, 1);
        // terminal_map is the first OptionalCredits in directory order.
        assert_eq!(a.terminal_map.as_deref(), Some("MAP24"));
    }

    #[test]
    fn test_single_decorative_monster_does_not_save_terminus() {
        // One lone cyberdemon for atmosphere isn't a playable map.
        let monsters = playable_things(10);
        let one_monster = build_things(&[16]);
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &monsters),
            ("MAP02", &[]),
            ("THINGS", &one_monster),
        ]);

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP01"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP02"), MapClassification::OptionalCredits);
    }

    #[test]
    fn test_start_map_never_peeled() {
        // Even if every map has no monsters, MAP01 stays Required so an
        // empty/test WAD doesn't auto-complete.
        let empty = playable_things(0);
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &empty),
            ("MAP02", &[]),
            ("THINGS", &empty),
        ]);

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP01"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP02"), MapClassification::OptionalCredits);
        assert_eq!(a.required_maps, 1);
    }

    #[test]
    fn test_vanilla_doom2_full_with_secrets() {
        let monsters = playable_things(20);

        let mut lumps: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 1..=32 {
            lumps.push((format!("MAP{:02}", i), Vec::new()));
            lumps.push(("THINGS".to_string(), monsters.clone()));
        }
        let lump_refs: Vec<(&str, &[u8])> = lumps
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let wad = build_wad(&lump_refs);

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(a.total_maps, 32);
        // MAP01..MAP30 all required (no UMAPINFO endgame).
        assert_eq!(classify_of(&a, "MAP01"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP30"), MapClassification::Required);
        // MAP31 / MAP32 reached only via vanilla secret edges.
        assert_eq!(classify_of(&a, "MAP31"), MapClassification::OptionalSecret);
        assert_eq!(classify_of(&a, "MAP32"), MapClassification::OptionalSecret);
        assert!(a.secret_maps.contains(&"MAP31".to_string()));
        assert!(a.secret_maps.contains(&"MAP32".to_string()));
    }

    #[test]
    fn test_doom1_full_episode_with_bosses() {
        // E1M8 has bosses → playable terminus stays Required.
        // E1M9 reached only via E1M3's vanilla secret edge.
        let monsters = playable_things(20);

        let mut lumps: Vec<(String, Vec<u8>)> = Vec::new();
        for m in 1..=8 {
            lumps.push((format!("E1M{m}"), Vec::new()));
            lumps.push(("THINGS".to_string(), monsters.clone()));
        }
        lumps.push(("E1M9".to_string(), Vec::new()));
        lumps.push(("THINGS".to_string(), monsters.clone()));
        let lump_refs: Vec<(&str, &[u8])> = lumps
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let wad = build_wad(&lump_refs);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(a.total_maps, 9);
        assert_eq!(classify_of(&a, "E1M1"), MapClassification::Required);
        assert_eq!(classify_of(&a, "E1M7"), MapClassification::Required);
        assert_eq!(classify_of(&a, "E1M8"), MapClassification::Required);
        assert_eq!(classify_of(&a, "E1M9"), MapClassification::OptionalSecret);
    }

    // -----------------------------------------------------------------------
    // UMAPINFO-driven analysis
    // -----------------------------------------------------------------------

    #[test]
    fn test_umapinfo_linear_chain_with_playable_terminus() {
        // MAP01..MAP06, all linked, MAP06 has monsters so it's a real
        // playable terminus.
        let monsters = playable_things(15);
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
            lumps.push(("THINGS".to_string(), monsters.clone()));
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
        assert_eq!(classify_of(&a, "MAP06"), MapClassification::Required);
    }

    #[test]
    fn test_umapinfo_endgame_with_monsters_stays_required() {
        // MAP03 has `endgame = true` AND lots of monsters → it's the real
        // finale and must be exited.
        let monsters = playable_things(10);
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
            ("THINGS", &monsters),
            ("MAP02", &[]),
            ("THINGS", &monsters),
            ("MAP03", &[]),
            ("THINGS", &monsters),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP03"), MapClassification::Required);
        assert_eq!(a.required_maps, 3);
        assert_eq!(a.terminal_map, None);
    }

    #[test]
    fn test_umapinfo_endgame_without_monsters_is_credits() {
        // MAP03 declares `endgame = true` and has no monsters — it's a
        // credits screen, demote it.
        let monsters = playable_things(10);
        let empty = playable_things(0);
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
            ("THINGS", &monsters),
            ("MAP02", &[]),
            ("THINGS", &monsters),
            ("MAP03", &[]),
            ("THINGS", &empty),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP03"), MapClassification::OptionalCredits);
        assert_eq!(a.required_maps, 2);
        assert_eq!(a.terminal_map.as_deref(), Some("MAP03"));
    }

    #[test]
    fn test_umapinfo_self_loop_without_monsters_is_credits() {
        // ][vydotwad shape: credits map uses `next = SELF` as the "stops
        // here" idiom and has no monsters. Should peel to OptionalCredits.
        let monsters = playable_things(10);
        let empty = playable_things(0);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP02"
}
MAP MAP02
{
    next = "MAP10"
}
MAP MAP10
{
    next = "MAP10"
}
"#;
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &monsters),
            ("MAP02", &[]),
            ("THINGS", &monsters),
            ("MAP10", &[]),
            ("THINGS", &empty),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP01"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP02"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP10"), MapClassification::OptionalCredits);
        assert_eq!(a.required_maps, 2);
        assert_eq!(a.terminal_map.as_deref(), Some("MAP10"));
    }

    #[test]
    fn test_umapinfo_self_loop_with_real_secret_not_stopper() {
        // Defensive case: `next = SELF` but a real `nextsecret = OTHER` means
        // the secret IS the path forward.
        let monsters = playable_things(15);
        let umapinfo = br#"
MAP MAP01
{
    next = "MAP02"
}
MAP MAP02
{
    next = "MAP02"
    nextsecret = "MAP31"
}
MAP MAP31
{
    next = "MAP31"
}
"#;
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &monsters),
            ("MAP02", &[]),
            ("THINGS", &monsters),
            ("MAP31", &[]),
            ("THINGS", &monsters),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP02"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP31"), MapClassification::Required);
        assert_eq!(a.terminal_map, None);
    }

    #[test]
    fn test_umapinfo_forced_secret_continuation() {
        // Formless Mother shape: MAP04 has only a secret exit to MAP31, and
        // MAP31 is the true ending. With monsters on MAP31 it stays Required.
        let monsters = playable_things(15);
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
            ("THINGS", &monsters),
            ("MAP02", &[]),
            ("THINGS", &monsters),
            ("MAP03", &[]),
            ("THINGS", &monsters),
            ("MAP04", &[]),
            ("THINGS", &monsters),
            ("MAP31", &[]),
            ("THINGS", &monsters),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP04"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP31"), MapClassification::Required);
        // No secrets — MAP31 was forced, not optional.
        assert!(a.secret_maps.is_empty());
    }

    #[test]
    fn test_umapinfo_skippable_secret() {
        // MAP15 has both `next = MAP16` and `nextsecret = MAP31`, so MAP31
        // is a skippable secret branch (OptionalSecret).
        let monsters = playable_things(15);
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
            ("THINGS", &monsters),
            ("MAP15", &[]),
            ("THINGS", &monsters),
            ("MAP16", &[]),
            ("THINGS", &monsters),
            ("MAP30", &[]),
            ("THINGS", &monsters),
            ("MAP31", &[]),
            ("THINGS", &monsters),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP15"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP31"), MapClassification::OptionalSecret);
        assert_eq!(classify_of(&a, "MAP30"), MapClassification::Required);
    }

    // -----------------------------------------------------------------------
    // ZMAPINFO precedence (Vertex Relocation case)
    // -----------------------------------------------------------------------

    #[test]
    fn test_zmapinfo_overrides_umapinfo() {
        // UMAPINFO routes MAP17 → MAP18 (vanilla flow).
        // ZMAPINFO routes MAP17 → MAP18GZ. ZMAPINFO must win.
        let monsters = playable_things(15);
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
            ("THINGS", &monsters),
            ("MAP17", &[]),
            ("THINGS", &monsters),
            ("MAP18", &[]),
            ("THINGS", &monsters),
            ("MAP18GZ", &[]),
            ("THINGS", &monsters),
            ("MAP19", &[]),
            ("THINGS", &monsters),
            ("UMAPINFO", umapinfo),
            ("ZMAPINFO", zmapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP17"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP18GZ"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP18"), MapClassification::Unreachable);
        assert_eq!(classify_of(&a, "MAP19"), MapClassification::Required);
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Multi-episode WADs (Eisberg regression)
    // -----------------------------------------------------------------------

    #[test]
    fn test_eisberg_shape_episodes_all_required() {
        // Eisberg #149: an episodic megawad. Each tier is a 3-map arc ending
        // in `next = Endpic, CREDIT`, reachable from its own `episode` marker.
        // Before the fix the walk stopped at MAP03 and MAP04-06 were
        // Unreachable, so the WAD auto-completed after one episode.
        let monsters = playable_things(15);
        let mapinfo = br#"
episode MAP01
{
    name = "Tier 1"
}
episode MAP04
{
    name = "Tier 2"
}
map MAP01 "A"
{
    next = Map02
}
map MAP02 "B"
{
    next = Map03
}
map MAP03 "C"
{
    next = Endpic, CREDIT
}
map MAP04 "D"
{
    next = Map05
}
map MAP05 "E"
{
    next = Map06
}
map MAP06 "F"
{
    next = Endpic, CREDIT
}
"#;
        let mut lumps: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 1..=6 {
            lumps.push((format!("MAP{:02}", i), Vec::new()));
            lumps.push(("THINGS".to_string(), monsters.clone()));
        }
        lumps.push(("MAPINFO".to_string(), mapinfo.to_vec()));
        let lump_refs: Vec<(&str, &[u8])> = lumps
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let wad = build_wad(&lump_refs);

        let a = analyze_wad(&wad).unwrap();
        assert_eq!(a.required_maps, 6, "all six maps across both episodes");
        for i in 1..=6 {
            assert_eq!(
                classify_of(&a, &format!("MAP{:02}", i)),
                MapClassification::Required,
            );
        }
        assert_eq!(a.terminal_map, None);
    }

    #[test]
    fn test_episode_end_keeps_secret_branch_optional() {
        // A tier whose finale ends the episode on the normal exit but branches
        // to a secret map: `next = endpic` + `secretnext = MAP31`. MAP31 must
        // be OptionalSecret, not a forced Required continuation.
        let monsters = playable_things(15);
        let mapinfo = br#"
episode MAP13
{
    name = "Tier 5"
}
map MAP13 "A"
{
    next = Map14
}
map MAP14 "B"
{
    next = Map15
}
map MAP15 "C"
{
    next = Endpic, CREDIT
    secretnext = MAP31
}
map MAP31 "S"
{
    next = Endpic, CREDIT
}
"#;
        let wad = build_wad(&[
            ("MAP13", &[]),
            ("THINGS", &monsters),
            ("MAP14", &[]),
            ("THINGS", &monsters),
            ("MAP15", &[]),
            ("THINGS", &monsters),
            ("MAP31", &[]),
            ("THINGS", &monsters),
            ("MAPINFO", mapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP15"), MapClassification::Required);
        assert_eq!(classify_of(&a, "MAP31"), MapClassification::OptionalSecret);
        assert!(a.secret_maps.contains(&"MAP31".to_string()));
    }

    /// The analysis of a WAD with these maps, each with this many monsters,
    /// and this MAPINFO.
    fn analyze_with_mapinfo(maps: &[(&str, usize)], mapinfo: &str) -> WadAnalysis {
        let things: Vec<Vec<u8>> = maps.iter().map(|&(_, n)| playable_things(n)).collect();
        let mut lumps: Vec<(&str, &[u8])> = Vec::new();
        for (&(name, _), things) in maps.iter().zip(&things) {
            lumps.push((name, &[]));
            lumps.push(("THINGS", things));
        }
        lumps.push(("MAPINFO", mapinfo.as_bytes()));
        analyze_wad(&build_wad(&lumps)).unwrap()
    }

    #[test]
    fn test_an_episode_that_rejoins_the_route_partway_is_a_bonus() {
        // After Doom, as v7.6 has it: every map is on the episode menu, and two of them are
        // bonus maps whose `next` rejoins the route a map later. Each episode used to be walked
        // as one to finish, so both were required, and finishing the WAD finished nothing.
        let mapinfo = r#"
map MAP33 "A"
{
    next = "map34"
}
map MAP34 "B"
{
    next = "map35"
}
map MAP35 "C"
{
    next = "map36"
}
map MAP36 "D"
{
    next = "map38"
}
map MAP37 "Bonus"
{
    next = "map38"
}
map MAP38 "E"
{
    next = "map39"
}
map MAP39 "F"
{
    next = "map41"
}
map MAP39B "Bonus"
{
    next = "map41"
}
map MAP40 "CAST"
{
    next = "EndGameC"
}
map MAP41 "G"
{
    next = "map40"
}
episode map33
{
    name = "A"
}
episode map34
{
    name = "B"
}
episode map35
{
    name = "C"
}
episode map36
{
    name = "D"
}
episode map37
{
    name = "Bonus (BONUS MAP)"
}
episode map38
{
    name = "E"
}
episode map39
{
    name = "F"
}
episode map39b
{
    name = "Bonus (BONUS MAP)"
}
episode map41
{
    name = "G"
}
"#;
        let maps = [
            ("MAP33", 9),
            ("MAP34", 9),
            ("MAP35", 9),
            ("MAP36", 9),
            ("MAP37", 9),
            ("MAP38", 9),
            ("MAP39", 9),
            ("MAP39B", 9),
            ("MAP40", 0),
            ("MAP41", 9),
        ];

        let a = analyze_with_mapinfo(&maps, mapinfo);

        assert_eq!(
            classified(&a),
            [
                ("MAP33", MapClassification::Required),
                ("MAP34", MapClassification::Required),
                ("MAP35", MapClassification::Required),
                ("MAP36", MapClassification::Required),
                ("MAP37", MapClassification::OptionalBonus),
                ("MAP38", MapClassification::Required),
                ("MAP39", MapClassification::Required),
                ("MAP39B", MapClassification::OptionalBonus),
                ("MAP40", MapClassification::OptionalCredits),
                ("MAP41", MapClassification::Required),
            ]
        );
        assert!(a.secret_maps.is_empty(), "a bonus map is not a secret one");
    }

    #[test]
    fn test_an_episode_leading_into_one_declared_before_it_is_still_required() {
        // The second episode runs on into the third, which is declared first and so walked
        // first. Running into the start of an earlier walk is a prologue, not a shortcut.
        let mapinfo = r#"
map MAP01 "A"
{
    next = "MAP02"
}
map MAP02 "B"
{
    next = "EndGameC"
}
map MAP03 "C"
{
    next = "MAP04"
}
map MAP04 "D"
{
    next = "MAP05"
}
map MAP05 "E"
{
    next = "MAP06"
}
map MAP06 "F"
{
    next = "EndGameC"
}
episode MAP01
{
    name = "One"
}
episode MAP05
{
    name = "Three"
}
episode MAP03
{
    name = "Two"
}
"#;
        let maps: Vec<(String, usize)> = (1..=6).map(|n| (format!("MAP{n:02}"), 9)).collect();
        let maps: Vec<(&str, usize)> = maps.iter().map(|(m, n)| (m.as_str(), *n)).collect();

        let a = analyze_with_mapinfo(&maps, mapinfo);

        assert_eq!(a.required_maps, 6, "{:?}", classified(&a));
    }

    #[test]
    fn test_a_bonus_map_joining_a_later_episode_leaves_that_episode_required() {
        let mapinfo = r#"
map MAP01 "A"
{
    next = "MAP02"
}
map MAP02 "B"
{
    next = "EndGameC"
}
map MAP03 "C"
{
    next = "MAP04"
}
map MAP04 "D"
{
    next = "MAP05"
}
map MAP05 "E"
{
    next = "EndGameC"
}
map MAP06 "Bonus"
{
    next = "MAP04"
}
episode MAP01
{
    name = "One"
}
episode MAP03
{
    name = "Two"
}
episode MAP06
{
    name = "Bonus"
}
"#;
        let maps: Vec<(String, usize)> = (1..=6).map(|n| (format!("MAP{n:02}"), 9)).collect();
        let maps: Vec<(&str, usize)> = maps.iter().map(|(m, n)| (m.as_str(), *n)).collect();

        let a = analyze_with_mapinfo(&maps, mapinfo);

        for lump in ["MAP01", "MAP02", "MAP03", "MAP04", "MAP05"] {
            assert_eq!(classify_of(&a, lump), MapClassification::Required, "{lump}");
        }
        assert_eq!(classify_of(&a, "MAP06"), MapClassification::OptionalBonus);
    }

    #[test]
    fn test_a_secret_map_on_the_episode_menu_is_still_a_secret() {
        // MAP31 is where MAP15's secret exit goes, and the episode menu offers it too.
        let mapinfo = r#"
map MAP14 "A"
{
    next = "MAP15"
}
map MAP15 "B"
{
    next = "MAP16"
    secretnext = "MAP31"
}
map MAP16 "C"
{
    next = "EndGameC"
}
map MAP31 "Secret"
{
    next = "MAP16"
}
episode MAP14
{
    name = "Main"
}
episode MAP31
{
    name = "Secret"
}
"#;
        let maps = [("MAP14", 9), ("MAP15", 9), ("MAP16", 9), ("MAP31", 9)];

        let a = analyze_with_mapinfo(&maps, mapinfo);

        assert_eq!(classify_of(&a, "MAP31"), MapClassification::OptionalSecret);
        assert_eq!(a.secret_maps, ["MAP31"]);
        assert_eq!(a.required_maps, 3);
    }

    #[test]
    fn test_a_mapinfo_in_the_older_format_is_followed() {
        // Triangulum goes on from MAP30 to MAP33 and MAP34, in a MAPINFO with no
        // braces. None of it used to be read, so MAP30 ended the game by the map
        // order, MAP33 and MAP34 were unreachable, and the WAD was finished two
        // maps early.
        let mapinfo = "map MAP29 \"Divine wind\"\r\nnext MAP30\r\n\
                       map MAP30 \"Shielding aura\"\r\ncluster 1\r\nnext MAP33\r\n\
                       map MAP33 \"Ghorrock\"\r\nnext MAP34\r\n\
                       map MAP34 \"Attack on Titan\"\r\nnext EndGameC\r\n";
        let maps = [("MAP29", 9), ("MAP30", 9), ("MAP33", 9), ("MAP34", 9)];

        let a = analyze_with_mapinfo(&maps, mapinfo);

        for lump in ["MAP29", "MAP30", "MAP33", "MAP34"] {
            assert_eq!(classify_of(&a, lump), MapClassification::Required, "{lump}");
        }
    }

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

    fn in_map_order(mut names: Vec<&str>) -> Vec<&str> {
        names.sort_by(|a, b| map_order(a, b));
        names
    }

    #[test]
    fn test_map_order_goes_by_the_number_and_not_the_text() {
        assert_eq!(
            in_map_order(vec!["MAP10", "MAP100", "MAP02", "MAP99", "MAP01"]),
            ["MAP01", "MAP02", "MAP10", "MAP99", "MAP100"]
        );
        // E1M10 is a name UMAPINFO allows, and as text it is before E1M2.
        assert_eq!(
            in_map_order(vec!["E2M1", "E1M10", "E1M8", "E1M2"]),
            ["E1M2", "E1M8", "E1M10", "E2M1"]
        );
    }

    #[test]
    fn test_map_order_puts_a_variant_after_the_map_it_goes_with() {
        assert_eq!(
            in_map_order(vec!["MAP19", "MAP18GZ", "MAP18", "MAP17"]),
            ["MAP17", "MAP18", "MAP18GZ", "MAP19"]
        );
    }

    #[test]
    fn test_map_order_takes_names_that_are_not_a_map_number() {
        // What a zdoom log can name, for the table of stats with no analysis to go by.
        assert_eq!(
            in_map_order(vec!["TITLEMAP", "LEVEL10", "level2", "HUB", "LEVEL1"]),
            ["HUB", "LEVEL1", "level2", "LEVEL10", "TITLEMAP"]
        );
        // A number of any length, which a PK3's file names are free to have.
        assert_eq!(
            map_order("MAP99999999999999999999998", "MAP99999999999999999999999"),
            Ordering::Less
        );
    }

    #[test]
    fn test_map_order_never_calls_two_names_equal() {
        // Or which of them came first would be up to the order they arrived in.
        for (a, b) in [("MAP01", "MAP1"), ("map01", "MAP01"), ("MAP01", "MAP01A")] {
            assert_ne!(map_order(a, b), Ordering::Equal, "{a} and {b}");
            assert_eq!(map_order(a, b), map_order(b, a).reverse(), "{a} and {b}");
        }
        assert_eq!(map_order("MAP01", "MAP01"), Ordering::Equal);
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
        let monsters = playable_things(10);
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
            ("THINGS", &monsters),
            ("MAP02", &[]),
            ("THINGS", &monsters),
            ("MAP03", &[]),
            ("THINGS", &monsters),
            ("UMAPINFO", umapinfo),
        ]);
        let a = analyze_wad(&wad).unwrap();
        assert_eq!(classify_of(&a, "MAP02"), MapClassification::Unreachable);
        assert_eq!(classify_of(&a, "MAP03"), MapClassification::Required);
    }

    // -----------------------------------------------------------------------
    // PK3
    // -----------------------------------------------------------------------

    /// A zip holding one entry whose name is raw CP437, as archivers that predate zip's UTF-8 flag
    /// wrote it. `ZipWriter` only writes UTF-8, so the entry goes in under an ASCII stand-in of the
    /// same length and the raw bytes are swapped in afterwards: nothing in a zip checksums a name.
    fn zip_with_raw_name(stand_in: &str, raw: &[u8], data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        assert_eq!(stand_in.len(), raw.len());
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(stand_in, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(data).unwrap();
        let mut bytes = writer.finish().unwrap().into_inner();

        // Once in the local header, once in the central directory.
        let mut replaced = 0;
        while let Some(at) = bytes
            .windows(stand_in.len())
            .position(|w| w == stand_in.as_bytes())
        {
            bytes[at..at + raw.len()].copy_from_slice(raw);
            replaced += 1;
        }
        assert_eq!(replaced, 2);
        bytes
    }

    #[test]
    fn test_pk3_reads_wad_with_cp437_name() {
        // zip keys entries by their raw name bytes but decodes this one to "MAPä.wad", so a lookup
        // by the decoded name finds nothing and the PK3 would read as having no maps.
        let things = playable_things(5);
        let wad = build_wad(&[("MAP01", &[]), ("THINGS", &things)]);
        let pk3 = zip_with_raw_name("MAP#.wad", b"MAP\x84.wad", &wad);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cp437.pk3");
        std::fs::write(&path, pk3).unwrap();

        let a = analyze_path(&path).expect("the WAD's maps are found");
        assert_eq!(classify_of(&a, "MAP01"), MapClassification::Required);
    }

    // -----------------------------------------------------------------------
    // What a launch loads
    // -----------------------------------------------------------------------

    /// A zip of `entries`, in that order.
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// A WAD of one map, `name`, with `imps` imps in it, as a PK3's `maps/` holds one.
    fn map_wad(name: &str, imps: usize) -> Vec<u8> {
        build_wad(&[(name, &[]), ("THINGS", &playable_things(imps))])
    }

    /// `bytes`, as a file called `name`, analyzed as a launch loads it.
    fn analyze_file(name: &str, bytes: &[u8]) -> Option<WadAnalysis> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        analyze_path(&path)
    }

    fn classified(a: &WadAnalysis) -> Vec<(&str, MapClassification)> {
        a.maps
            .iter()
            .map(|m| (m.lump.as_str(), m.classification))
            .collect()
    }

    /// The Warrens: Burrow of the Were's PK3, cut down to what decides its maps: one named
    /// BTW01 and a title map, and a ZMAPINFO of nothing but includes, as 1.4.3 has them.
    fn warrens_pk3() -> Vec<u8> {
        let maps = b"DefaultMap\n{\n\tEvenLighting\n}\n\n\
            Map TITLEMAP \"The Warrens\"\n{\n}\n\n\
            Map BTW01 \"Burrow of The Were\"\n{\n\tLevelNum = 666\n\tNext = \"MAP01\"\n\tNoIntermission\n}\n";
        let episodes =
            b"clearepisodes\n\nEpisode\tBTW01 \n{\n\tname = \"Burrow of the Were\"\n}\n\n\
            Episode MAP01\n{\n\tname = \"Doom II: Hell on Earth\"\n}\n";
        zip_of(&[
            (
                "ZMAPINFO",
                b"include \"mapinfo/Maps\"\ninclude \"mapinfo/Episodes\"\n",
            ),
            ("mapinfo/Maps", maps),
            ("mapinfo/Episodes", episodes),
            ("maps/BTW01.wad", &map_wad("BTW01", 150)),
            // A title map can have monsters of its own to look at.
            ("maps/TITLEMAP.wad", &map_wad("TITLEMAP", 9)),
        ])
    }

    #[test]
    fn test_a_pk3_in_a_zip_is_analyzed_as_the_pk3() {
        // The Warrens as it comes: a zip that only wraps its PK3, which a launch unpacks. Its
        // analysis used to look in the zip for a `.wad`, find none, and give up, so finishing
        // BTW01 finished nothing.
        let zip = zip_of(&[
            ("README.txt", b"read me"),
            ("BurrowOfTheWere_1.4.3.pk3", &warrens_pk3()),
            ("CREDITS.txt", b"thanks"),
        ]);

        let a = analyze_file("TheWarrensBOTW_1.4.3.zip", &zip).expect("the PK3's maps are found");

        assert_eq!(classified(&a), [("BTW01", MapClassification::Required)]);
        assert!(a.has_umapinfo);
    }

    #[test]
    fn test_a_zip_played_whole_is_a_pk3_whatever_it_is_called() {
        let pk3 = warrens_pk3();

        let as_pk3 = analyze_file("warrens.pk3", &pk3).unwrap();
        let as_zip = analyze_file("warrens.zip", &pk3).unwrap();

        assert_eq!(classified(&as_pk3), classified(&as_zip));
    }

    #[test]
    fn test_maps_with_names_of_their_own_start_where_the_episodes_say() {
        // AAAA comes first by name, and nothing leads to it. The game starts at START, which
        // only the included file says.
        let pk3 = zip_of(&[
            (
                "ZMAPINFO",
                b"include \"mapinfo/episodes\"\nmap START \"Start\"\n{\n\tnext = \"FINALE\"\n}\n",
            ),
            ("MapInfo/Episodes", b"clearepisodes\nepisode START\n{\n}\n"),
            ("maps/AAAA.wad", &map_wad("AAAA", 5)),
            ("maps/START.wad", &map_wad("START", 5)),
            ("maps/FINALE.wad", &map_wad("FINALE", 5)),
        ]);

        let a = analyze_file("game.pk3", &pk3).unwrap();

        assert_eq!(classify_of(&a, "START"), MapClassification::Required);
        assert_eq!(classify_of(&a, "FINALE"), MapClassification::Required);
        assert_eq!(classify_of(&a, "AAAA"), MapClassification::Unreachable);
    }

    #[test]
    fn test_a_map_with_a_name_of_its_own_is_one_to_finish_once_play_reaches_it() {
        // MAP02 leads to a map the WAD named itself, and on to MAP03. ARENA used to be no map,
        // which left MAP02 the last, and the WAD finished two maps early.
        let zmapinfo = b"map MAP02 \"Two\"\n{\n\tnext = \"ARENA\"\n}\n\
            map ARENA \"Arena\"\n{\n\tnext = \"MAP03\"\n}\n";
        let things = playable_things(5);
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &things),
            ("MAP02", &[]),
            ("THINGS", &things),
            ("ARENA", &[]),
            ("THINGS", &things),
            ("MAP03", &[]),
            ("THINGS", &things),
            ("ZMAPINFO", zmapinfo),
        ]);

        let a = analyze_wad(&wad).unwrap();

        for lump in ["MAP01", "MAP02", "ARENA", "MAP03"] {
            assert_eq!(classify_of(&a, lump), MapClassification::Required, "{lump}");
        }
    }

    #[test]
    fn test_only_a_lump_before_a_maps_first_lump_is_a_map() {
        // SCRIPTS and BEHAVIOR can stand on their own, so a lump before one is no map.
        let things = playable_things(5);
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &things),
            ("LOADACS", b"lib"),
            ("SCRIPTS", b"#library \"lib\""),
            ("BEHAVIOR", b"ACS\0"),
            ("TITLEMAP", &[]),
            ("THINGS", &things),
        ]);

        let a = analyze_wad(&wad).unwrap();

        assert_eq!(classified(&a), [("MAP01", MapClassification::Required)]);
    }

    #[test]
    fn test_a_containers_files_are_analyzed_together_in_the_order_they_load() {
        // The first to load ends on an empty MAP02, and the second replaces it with a real one.
        // They are zipped the other way round, and a launch loads them by name.
        let things = playable_things(5);
        let first = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &things),
            ("MAP02", &[]),
            ("THINGS", &[]),
        ]);
        let second = map_wad("MAP02", 5);
        let zip = zip_of(&[
            ("pack/b.wad", &second),
            ("pack/A.wad", &first),
            ("pack/readme.txt", b"read me"),
        ]);

        let a = analyze_file("pack.zip", &zip).unwrap();

        assert_eq!(
            classified(&a),
            [
                ("MAP01", MapClassification::Required),
                ("MAP02", MapClassification::Required),
            ]
        );
    }
}
