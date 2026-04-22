//! Auto-detect required IWAD family from WAD file contents.
//!
//! Inspects a PWAD's TEXTURE1/TEXTURE2 lumps, PNAMES, and SIDEDEFS to
//! determine which IWAD (base game) it requires. Detection only counts
//! Plutonia-only / TNT-only patches that are reachable from a texture
//! actually used by some sidedef — patches sitting in PNAMES that no
//! map references don't count, which avoids false positives from bulk-
//! imported TEXTURE1 lumps that include unused entries.
//!
//! Priority order:
//! 1. SIDEDEF-reachable patch analysis — patches needed by used textures
//! 2. Map name format — ExMy (doom) vs MAPxx (doom2)
//! 3. None if no signal found

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::utils::{load_wad_data, parse_wad_directory};

// Map lump name patterns
static DOOM1_MAP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^E[1-9]M[0-9]$").unwrap());
static DOOM2_MAP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^MAP[0-9][0-9]$").unwrap());

// 197 patch names present in TNT.WAD but not in DOOM2.WAD
static TNT_ONLY_PATCHES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "ALTAQUA", "ASPHALT", "BCRAT16", "BCRAT32", "BIGMURAL", "BIGWALL", "BLOD128A", "BLOD128B",
        "BLOD64A", "BLOD64B", "BLUTNT", "BRNOPEN", "BTNTCRAT", "BUL64A", "BUL64B", "BUL64C",
        "BUL64D", "CARLLF1", "CARLLF2", "CARLRT1", "CARLRT2", "CAVERN1", "CAVERN4", "CAVERN5",
        "CAVERN6", "CAVERN7", "CLWDVS3", "CRLWDH6", "CRLWDH6B", "CRLWDL12", "CRLWDL6", "CRLWDL6B",
        "CRLWDL6C", "CRLWDL6D", "CRLWDL6E", "CRLWDS6", "CRLWDT3", "CRWDH6", "CRWDH6B", "CRWDL12",
        "CRWDL6B", "CRWDL6C", "CRWDL6D", "CRWDS6", "CRWDT3", "CRWDVS3", "CYAN", "DISASTER",
        "DOBIGTVA", "DOBIGTVB", "DOBIGTVC", "DOBIGTVD", "DOBLIP1A", "DOBLIP2A", "DOBLIP3A",
        "DOBLIP4A", "DOBWIRE", "DOBWIRE2", "DOEDAY", "DOEHELL", "DOENITE", "DOGLDIR", "DOGLPANL",
        "DOGRID", "DOGRMSC", "DOGRNMEN", "DOKGRIR", "DOKODO1B", "DOKODO2B", "DONDAY", "DONHELL",
        "DONNITE", "DOPUNK4", "DORED", "DOSDAY", "DOSHA1", "DOSHB1", "DOSHC1", "DOSHD1", "DOSHE1",
        "DOSHELL", "DOSHF1", "DOSLVR11", "DOSLVR12", "DOSLVR13", "DOSLVR14", "DOSLVR21",
        "DOSLVR22", "DOSLVR23", "DOSLVR24", "DOSNITE", "DOSPI1B", "DOSPI2B", "DOSPI3B", "DOSPI4B",
        "DOSW1", "DOSW1C", "DOSW2", "DOSW2C", "DOSW3", "DOSW3C", "DOSW4", "DOSW4C", "DOSWX1",
        "DOSWX1C", "DOSWX2", "DOSWX2C", "DOSWX3", "DOSWX3C", "DOSWX4", "DOSWX4C", "DOTNTDR",
        "DOTV1B", "DOTV2B", "DOTV3B", "DOTV4B", "DOWDAY", "DOWEBL", "DOWEBR", "DOWHELL",
        "DOWINDOW", "DOWNITE", "DRFRONT", "DRSIDE1", "DRSIDE2", "DRTOPFR", "DRTOPSID", "EGGREENI",
        "EGREDI", "FENCE4", "FENCE5", "GRNLIT1", "GRNOPEN", "LONGWALL", "MTNT2", "MURAL1",
        "MURAL2", "PBLAK", "PCWINL", "PILLAR", "PIVY3", "PL_01", "PL_05", "PL_10", "PL_18",
        "PL_19", "PL_20", "PL_25", "PL_31", "PL_A", "PL_C", "PL_N", "PL_T", "PL_U", "PREEL1",
        "PREEL2", "PREEL3", "PREEL4", "PREEL5", "PREEL6", "PREEL7", "PSTON2", "REDLITE1",
        "REDLITE2", "REDOPEN", "REDTNT2", "ROMERO1", "SAW1", "SAW1SD", "SAW2", "SAW2SD", "SAW3",
        "SAW3SD", "SAW4", "SAW4SD", "SAW5", "SAW5SD", "SAW6", "SAW6SD", "SKIRTING", "SMCRATG",
        "SMFILLER", "STONEW1", "STONEW5", "STWALL", "TFOGF0", "TFOGI0", "TYUNDER1", "TYWFALL1",
        "TYWFALL2", "TYWFALL3", "TYWFALL4", "TYWHEEL1", "YELLITE1", "YELLITE2", "YELLITE3",
        "YELTNT",
    ])
});

// 72 patch names present in Plutonia but not in DOOM.WAD, DOOM2.WAD, or TNT.WAD
static PLUTONIA_ONLY_PATCHES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "AROCK2", "AROCK3", "AROCK4", "AROCK5", "BOSFA0", "BRBRICK", "BRBRICK2", "BRICK", "BRICK1",
        "BRICK2", "BROCK2", "BROWN1", "BROWN2", "BROWN3", "BROWN5", "CAMO1", "CAMO4", "CAMO5",
        "CONCRETE", "DARKROCK", "DIRBRI1", "DIRBRI2", "FIREBLU1", "FIREBLU2", "GRATE", "MARBLE1",
        "MC1", "MC10", "MC11", "MC12", "MC13", "MC14", "MC15", "MC16", "MC17", "MC18", "MC19",
        "MC2", "MC20", "MC3", "MC4", "MC5", "MC6", "MC7", "MC8", "MOSROK2", "MOSSBRIK", "MOSSROCK",
        "MOULD", "MUD", "MYWOOD", "NATROCK", "POISON", "RAILING", "REDROCK", "ROCK", "SKY2A",
        "SKY2B", "SKY2C", "SKY2D", "SKY3A", "SKY3B", "SW1SKULL", "SW2SKULL", "TILE", "VINES1",
        "WFALL1", "WFALL2", "WFALL3", "WFALL4", "WOOD", "YELLOW",
    ])
});

/// Detect the required IWAD family for a PWAD file.
///
/// Returns an IWAD family name ("doom", "doom2", "tnt", "plutonia")
/// or None if detection is inconclusive.
pub fn detect_iwad(wad_path: &Path) -> Option<&'static str> {
    let wad_data = load_wad_data(wad_path)?;
    let directory = parse_wad_directory(&wad_data);
    if directory.is_empty() {
        return None;
    }

    let lump_names: HashSet<&str> = directory.iter().map(|(name, _, _)| name.as_str()).collect();

    // Priority 1: PNAMES analysis restricted to patches reachable from a
    // texture actually used by some sidedef. This avoids false positives
    // from bulk-imported TEXTURE1 entries that no map references.
    if let Some(pnames) = parse_pnames(&wad_data, &directory) {
        let mut textures = parse_textures(&wad_data, &directory, "TEXTURE1");
        let tex2 = parse_textures(&wad_data, &directory, "TEXTURE2");
        textures.extend(tex2);
        if !textures.is_empty() {
            let used = collect_used_textures(&wad_data, &directory);
            if let Some(result) = detect_from_used_textures(&pnames, &textures, &used, &lump_names)
            {
                return Some(result);
            }
        }
    }

    // Priority 2: Map name format (Doom 1 vs Doom 2 family)
    detect_from_maps(&directory)
}

/// Detect complevel from a WAD file's COMPLVL lump.
///
/// The COMPLVL lump is an id24 signal — a single byte indicating the
/// compatibility level the WAD was designed for.
///
/// Returns the complevel as an integer, or None if no COMPLVL lump found.
pub fn detect_complvl(wad_path: &Path) -> Option<i32> {
    crate::complevel_detect::detect_complvl_from_path(wad_path)
}

fn read_lump_name(bytes: &[u8]) -> String {
    bytes
        .split(|&b| b == 0)
        .next()
        .unwrap_or(b"")
        .iter()
        .map(|&b| (b as char).to_ascii_uppercase())
        .collect()
}

/// Parse the PNAMES lump into an ordered list. Index matters because
/// TEXTURE1/2 entries reference patches by position.
fn parse_pnames(wad_data: &[u8], directory: &[(String, u32, u32)]) -> Option<Vec<String>> {
    for (name, offset, size) in directory {
        if name == "PNAMES" && *size >= 4 {
            let off = *offset as usize;
            let sz = *size as usize;

            if off + 4 > wad_data.len() {
                return None;
            }

            let count = i32::from_le_bytes(wad_data[off..off + 4].try_into().ok()?) as usize;
            if 4 + count * 8 > sz {
                return None;
            }

            let mut patches = Vec::with_capacity(count);
            for i in 0..count {
                let pname_offset = off + 4 + i * 8;
                if pname_offset + 8 > wad_data.len() {
                    break;
                }
                patches.push(read_lump_name(&wad_data[pname_offset..pname_offset + 8]));
            }
            return Some(patches);
        }
    }
    None
}

/// Parse a TEXTURE1 / TEXTURE2 lump.
///
/// Returns a map from texture name → ordered list of patch indices into
/// PNAMES. Returns an empty map if the lump is missing or malformed.
fn parse_textures(
    wad_data: &[u8],
    directory: &[(String, u32, u32)],
    lump_name: &str,
) -> HashMap<String, Vec<i16>> {
    let mut out = HashMap::new();
    let Some((_, offset, size)) = directory.iter().find(|(n, _, _)| n == lump_name) else {
        return out;
    };
    let off = *offset as usize;
    let sz = *size as usize;
    if sz < 4 || off + sz > wad_data.len() {
        return out;
    }

    let lump = &wad_data[off..off + sz];
    let count = i32::from_le_bytes(lump[0..4].try_into().unwrap_or([0; 4])) as usize;
    if 4 + count.saturating_mul(4) > sz {
        return out;
    }

    out.reserve(count);
    for i in 0..count {
        let off_pos = 4 + i * 4;
        let tex_off =
            i32::from_le_bytes(lump[off_pos..off_pos + 4].try_into().unwrap_or([0; 4])) as usize;
        // Texture def header is 22 bytes: name(8), masked(4), w(2), h(2), columndir(4), npatches(2)
        if tex_off + 22 > sz {
            continue;
        }
        let name = read_lump_name(&lump[tex_off..tex_off + 8]);
        let npatches = i16::from_le_bytes(
            lump[tex_off + 20..tex_off + 22]
                .try_into()
                .unwrap_or([0; 2]),
        );
        if npatches < 0 {
            continue;
        }
        let npatches = npatches as usize;
        let patches_start = tex_off + 22;
        if patches_start + npatches.saturating_mul(10) > sz {
            continue;
        }
        let mut patches = Vec::with_capacity(npatches);
        for p in 0..npatches {
            // Each patch entry: originx(2), originy(2), patch_idx(2), stepdir(2), colormap(2)
            let p_off = patches_start + p * 10 + 4;
            let idx = i16::from_le_bytes(lump[p_off..p_off + 2].try_into().unwrap_or([0; 2]));
            patches.push(idx);
        }
        out.insert(name, patches);
    }
    out
}

/// Walk all map markers in the WAD and collect every texture name (upper /
/// middle / lower) referenced by their SIDEDEFS lumps.
///
/// UDMF maps (TEXTMAP lump) are skipped — that format stores sidedef
/// textures as text fields and is overwhelmingly zdoom-only, where IWAD
/// matching is loose.
fn collect_used_textures(wad_data: &[u8], directory: &[(String, u32, u32)]) -> HashSet<String> {
    let mut used = HashSet::new();
    let mut i = 0;
    while i < directory.len() {
        let name = &directory[i].0;
        if !is_map_marker(name) {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < directory.len() && is_map_lump(&directory[end].0) {
            end += 1;
        }
        let map_lumps = &directory[start..end];
        let is_udmf = map_lumps.iter().any(|(n, _, _)| n == "TEXTMAP");
        if !is_udmf && let Some((_, off, sz)) = map_lumps.iter().find(|(n, _, _)| n == "SIDEDEFS") {
            parse_sidedef_textures(wad_data, *off as usize, *sz as usize, &mut used);
        }
        i = end.max(i + 1);
    }
    used
}

/// Each sidedef record is 30 bytes: x_off(2), y_off(2), upper(8), lower(8),
/// middle(8), sector(2). "-" means "no texture" — skipped.
fn parse_sidedef_textures(wad_data: &[u8], off: usize, sz: usize, out: &mut HashSet<String>) {
    if sz < 30 || off.saturating_add(sz) > wad_data.len() {
        return;
    }
    let lump = &wad_data[off..off + sz];
    let count = sz / 30;
    for i in 0..count {
        let base = i * 30;
        for tex_off in [4usize, 12, 20] {
            let name = read_lump_name(&lump[base + tex_off..base + tex_off + 8]);
            if !name.is_empty() && name != "-" {
                out.insert(name);
            }
        }
    }
}

fn is_map_marker(name: &str) -> bool {
    DOOM1_MAP_RE.is_match(name) || DOOM2_MAP_RE.is_match(name)
}

fn is_map_lump(name: &str) -> bool {
    matches!(
        name,
        "THINGS"
            | "LINEDEFS"
            | "SIDEDEFS"
            | "VERTEXES"
            | "SEGS"
            | "SSECTORS"
            | "NODES"
            | "SECTORS"
            | "REJECT"
            | "BLOCKMAP"
            | "BEHAVIOR"
            | "SCRIPTS"
            | "TEXTMAP"
            | "ZNODES"
            | "ENDMAP"
            | "DIALOGUE"
    )
}

/// Count Plutonia-only / TNT-only patches that are (a) referenced by a
/// texture actually used by a sidedef, and (b) not shipped as a lump in
/// the PWAD (which would override the IWAD's patch).
fn detect_from_used_textures(
    pnames: &[String],
    textures: &HashMap<String, Vec<i16>>,
    used_textures: &HashSet<String>,
    lump_names: &HashSet<&str>,
) -> Option<&'static str> {
    let mut needed_tnt: HashSet<&str> = HashSet::new();
    let mut needed_plutonia: HashSet<&str> = HashSet::new();

    for tex_name in used_textures {
        let Some(patch_idxs) = textures.get(tex_name) else {
            continue;
        };
        for &idx in patch_idxs {
            if idx < 0 || (idx as usize) >= pnames.len() {
                continue;
            }
            let pname = pnames[idx as usize].as_str();
            if lump_names.contains(pname) {
                continue;
            }
            if let Some(&p) = TNT_ONLY_PATCHES.get(pname) {
                needed_tnt.insert(p);
            } else if let Some(&p) = PLUTONIA_ONLY_PATCHES.get(pname) {
                needed_plutonia.insert(p);
            }
        }
    }

    if !needed_tnt.is_empty() && needed_plutonia.is_empty() {
        return Some("tnt");
    }
    if !needed_plutonia.is_empty() && needed_tnt.is_empty() {
        return Some("plutonia");
    }
    None
}

/// Detect IWAD family from verified map markers.
///
/// Only counts lumps that match the ExMy / MAPxx pattern *and* are
/// immediately followed by a known map data lump. This avoids false
/// positives from non-map lumps (textures, music, custom data) that
/// happen to use names like "E1M1".
fn detect_from_maps(directory: &[(String, u32, u32)]) -> Option<&'static str> {
    let mut has_doom1 = false;
    let mut has_doom2 = false;

    for i in 0..directory.len() {
        let name = &directory[i].0;
        if !is_map_marker(name) {
            continue;
        }
        // Verify this is an actual map marker by checking if the next
        // entry is a known map data lump.
        if i + 1 < directory.len() && is_map_lump(&directory[i + 1].0) {
            if DOOM1_MAP_RE.is_match(name) {
                has_doom1 = true;
            } else if DOOM2_MAP_RE.is_match(name) {
                has_doom2 = true;
            }
        }
    }

    if has_doom1 && !has_doom2 {
        return Some("doom");
    }
    if has_doom2 && !has_doom1 {
        return Some("doom2");
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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

    /// Build a PNAMES lump with the given patch names.
    fn build_pnames(names: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(names.len() as i32).to_le_bytes());
        for name in names {
            let mut buf = [0u8; 8];
            for (i, &b) in name.as_bytes().iter().take(8).enumerate() {
                buf[i] = b;
            }
            out.extend_from_slice(&buf);
        }
        out
    }

    /// Build a TEXTURE1 lump containing one texture per entry.
    /// Each entry: (texture_name, list of patch indices).
    fn build_texture_lump(entries: &[(&str, &[i16])]) -> Vec<u8> {
        let mut header = Vec::new();
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        let count = entries.len() as i32;
        header.extend_from_slice(&count.to_le_bytes());

        // Compute body sizes and offsets
        let header_size = 4 + 4 * entries.len();
        let mut offsets = Vec::with_capacity(entries.len());
        let mut running = header_size as i32;
        for (_, patches) in entries {
            offsets.push(running);
            let body_size = 22 + patches.len() * 10;
            running += body_size as i32;
        }
        for off in &offsets {
            header.extend_from_slice(&off.to_le_bytes());
        }

        for (name, patches) in entries {
            let mut body = Vec::new();
            let mut name_bytes = [0u8; 8];
            for (i, &b) in name.as_bytes().iter().take(8).enumerate() {
                name_bytes[i] = b;
            }
            body.extend_from_slice(&name_bytes);
            body.extend_from_slice(&[0u8; 4]); // masked
            body.extend_from_slice(&64i16.to_le_bytes()); // width
            body.extend_from_slice(&128i16.to_le_bytes()); // height
            body.extend_from_slice(&[0u8; 4]); // columndir
            body.extend_from_slice(&(patches.len() as i16).to_le_bytes());
            for &idx in *patches {
                body.extend_from_slice(&0i16.to_le_bytes()); // originx
                body.extend_from_slice(&0i16.to_le_bytes()); // originy
                body.extend_from_slice(&idx.to_le_bytes());
                body.extend_from_slice(&0i16.to_le_bytes()); // stepdir
                body.extend_from_slice(&0i16.to_le_bytes()); // colormap
            }
            bodies.push(body);
        }

        let mut out = header;
        for body in bodies {
            out.extend_from_slice(&body);
        }
        out
    }

    /// Build a SIDEDEFS lump given (upper, lower, middle) texture names.
    fn build_sidedefs(sidedefs: &[(&str, &str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (upper, lower, middle) in sidedefs {
            out.extend_from_slice(&0i16.to_le_bytes()); // x_off
            out.extend_from_slice(&0i16.to_le_bytes()); // y_off
            for tex in [upper, lower, middle] {
                let mut buf = [0u8; 8];
                for (i, &b) in tex.as_bytes().iter().take(8).enumerate() {
                    buf[i] = b;
                }
                out.extend_from_slice(&buf);
            }
            out.extend_from_slice(&0i16.to_le_bytes()); // sector
        }
        out
    }

    #[test]
    fn test_detect_from_maps_doom1() {
        let directory = vec![
            ("E1M1".to_string(), 0, 0),
            ("THINGS".to_string(), 0, 0),
            ("E1M2".to_string(), 0, 0),
            ("THINGS".to_string(), 0, 0),
            ("E1M3".to_string(), 0, 0),
            ("THINGS".to_string(), 0, 0),
        ];
        assert_eq!(detect_from_maps(&directory), Some("doom"));
    }

    #[test]
    fn test_detect_from_maps_doom2() {
        let directory = vec![
            ("MAP01".to_string(), 0, 0),
            ("THINGS".to_string(), 0, 0),
            ("MAP02".to_string(), 0, 0),
            ("THINGS".to_string(), 0, 0),
            ("MAP03".to_string(), 0, 0),
            ("THINGS".to_string(), 0, 0),
        ];
        assert_eq!(detect_from_maps(&directory), Some("doom2"));
    }

    #[test]
    fn test_detect_from_maps_mixed() {
        let directory = vec![
            ("E1M1".to_string(), 0, 0),
            ("THINGS".to_string(), 0, 0),
            ("MAP01".to_string(), 0, 0),
            ("THINGS".to_string(), 0, 0),
        ];
        assert_eq!(detect_from_maps(&directory), None);
    }

    #[test]
    fn test_detect_from_maps_neither() {
        let directory = vec![("THINGS".to_string(), 0, 0), ("LINEDEFS".to_string(), 0, 0)];
        assert_eq!(detect_from_maps(&directory), None);
    }

    #[test]
    fn test_detect_from_maps_ignores_non_map_lumps() {
        // A non-map lump named "E1M1" should not be counted as a doom1 map.
        let directory = vec![
            ("E1M1".to_string(), 0, 10),  // Non-map lump (followed by non-map data)
            ("MAP01".to_string(), 0, 0),  // Map marker
            ("THINGS".to_string(), 0, 0), // Map lump
        ];
        assert_eq!(detect_from_maps(&directory), Some("doom2"));
    }

    #[test]
    fn test_detect_from_maps_non_map_lump_only() {
        // A WAD with only a non-map lump named "E1M1" (no actual maps).
        let directory = vec![("E1M1".to_string(), 0, 10)];
        assert_eq!(detect_from_maps(&directory), None);
    }

    #[test]
    fn test_used_textures_signal_plutonia() {
        // PNAMES references AROCK2 (Plutonia-only); TEXTURE1 has texture
        // PLUTEX using AROCK2; sidedef uses PLUTEX → plutonia signal.
        let pnames = vec!["AROCK2".to_string(), "STARTAN1".to_string()];
        let mut textures = HashMap::new();
        textures.insert("PLUTEX".to_string(), vec![0i16]);
        textures.insert("UNUSED".to_string(), vec![0i16]); // also references AROCK2 but no sidedef uses it
        let used: HashSet<String> = ["PLUTEX".to_string()].into_iter().collect();
        let lump_names: HashSet<&str> = HashSet::new();

        assert_eq!(
            detect_from_used_textures(&pnames, &textures, &used, &lump_names),
            Some("plutonia")
        );
    }

    #[test]
    fn test_used_textures_signal_tnt() {
        let pnames = vec!["ALTAQUA".to_string()];
        let mut textures = HashMap::new();
        textures.insert("TNTEX".to_string(), vec![0i16]);
        let used: HashSet<String> = ["TNTEX".to_string()].into_iter().collect();
        let lump_names: HashSet<&str> = HashSet::new();

        assert_eq!(
            detect_from_used_textures(&pnames, &textures, &used, &lump_names),
            Some("tnt")
        );
    }

    #[test]
    fn test_used_textures_unused_plutonia_no_signal() {
        // Apophis-style: TEXTURE1 has BROWN5 referencing the BROWN5 patch,
        // but no sidedef uses the BROWN5 texture → no plutonia signal.
        let pnames = vec!["BROWN5".to_string(), "STARTAN1".to_string()];
        let mut textures = HashMap::new();
        textures.insert("BROWN5".to_string(), vec![0i16]);
        textures.insert("WALL01".to_string(), vec![1i16]);
        let used: HashSet<String> = ["WALL01".to_string()].into_iter().collect();
        let lump_names: HashSet<&str> = HashSet::new();

        assert_eq!(
            detect_from_used_textures(&pnames, &textures, &used, &lump_names),
            None
        );
    }

    #[test]
    fn test_used_textures_self_contained() {
        // WAD ships its own AROCK2 patch as a lump → not detected.
        let pnames = vec!["AROCK2".to_string()];
        let mut textures = HashMap::new();
        textures.insert("PLUTEX".to_string(), vec![0i16]);
        let used: HashSet<String> = ["PLUTEX".to_string()].into_iter().collect();
        let lump_names: HashSet<&str> = ["AROCK2"].into_iter().collect();

        assert_eq!(
            detect_from_used_textures(&pnames, &textures, &used, &lump_names),
            None
        );
    }

    #[test]
    fn test_used_textures_doom_patch_no_signal() {
        // A texture using a DOOM/DOOM2 patch (e.g. STARTAN1) is fine.
        let pnames = vec!["STARTAN1".to_string()];
        let mut textures = HashMap::new();
        textures.insert("WALL".to_string(), vec![0i16]);
        let used: HashSet<String> = ["WALL".to_string()].into_iter().collect();
        let lump_names: HashSet<&str> = HashSet::new();

        assert_eq!(
            detect_from_used_textures(&pnames, &textures, &used, &lump_names),
            None
        );
    }

    #[test]
    fn test_used_textures_both_tnt_and_plutonia_no_signal() {
        // If the WAD references both, we can't pick one → None.
        let pnames = vec!["AROCK2".to_string(), "ALTAQUA".to_string()];
        let mut textures = HashMap::new();
        textures.insert("MIXED".to_string(), vec![0i16, 1i16]);
        let used: HashSet<String> = ["MIXED".to_string()].into_iter().collect();
        let lump_names: HashSet<&str> = HashSet::new();

        assert_eq!(
            detect_from_used_textures(&pnames, &textures, &used, &lump_names),
            None
        );
    }

    #[test]
    fn test_parse_pnames_ordered() {
        let wad = build_wad(&[("PNAMES", &build_pnames(&["ALTAQUA", "BIGMURAL"]))]);
        let directory = parse_wad_directory(&wad);
        let result = parse_pnames(&wad, &directory).unwrap();
        assert_eq!(result, vec!["ALTAQUA".to_string(), "BIGMURAL".to_string()]);
    }

    #[test]
    fn test_parse_pnames_empty() {
        let wad = build_wad(&[("PNAMES", &build_pnames(&[]))]);
        let directory = parse_wad_directory(&wad);
        let result = parse_pnames(&wad, &directory).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_textures() {
        let lump = build_texture_lump(&[("TEX1", &[0, 1]), ("TEX2", &[2])]);
        let wad = build_wad(&[("TEXTURE1", &lump)]);
        let directory = parse_wad_directory(&wad);
        let textures = parse_textures(&wad, &directory, "TEXTURE1");
        assert_eq!(textures.len(), 2);
        assert_eq!(textures.get("TEX1"), Some(&vec![0i16, 1]));
        assert_eq!(textures.get("TEX2"), Some(&vec![2i16]));
    }

    #[test]
    fn test_parse_sidedef_textures() {
        let sidedefs = build_sidedefs(&[("UPPER1", "LOWER1", "MIDDLE1"), ("UPPER2", "-", "-")]);
        let mut out = HashSet::new();
        parse_sidedef_textures(&sidedefs, 0, sidedefs.len(), &mut out);
        assert!(out.contains("UPPER1"));
        assert!(out.contains("LOWER1"));
        assert!(out.contains("MIDDLE1"));
        assert!(out.contains("UPPER2"));
        assert!(!out.contains("-"));
    }

    #[test]
    fn test_collect_used_textures() {
        let sidedefs = build_sidedefs(&[("BROWN5", "-", "-")]);
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &[]),
            ("LINEDEFS", &[]),
            ("SIDEDEFS", &sidedefs),
            ("VERTEXES", &[]),
        ]);
        let directory = parse_wad_directory(&wad);
        let used = collect_used_textures(&wad, &directory);
        assert!(used.contains("BROWN5"));
    }

    #[test]
    fn test_collect_used_textures_skips_udmf() {
        let sidedefs = build_sidedefs(&[("BROWN5", "-", "-")]);
        let wad = build_wad(&[
            ("MAP01", &[]),
            ("TEXTMAP", &[]),
            ("SIDEDEFS", &sidedefs), // ignored because TEXTMAP marks UDMF
        ]);
        let directory = parse_wad_directory(&wad);
        let used = collect_used_textures(&wad, &directory);
        assert!(used.is_empty());
    }

    // --- Integration tests (full detect_iwad flow) ---

    #[test]
    fn test_detect_iwad_apophis_style_no_false_plutonia() {
        // Reproduces Apophis: TEXTURE1 has BROWN5 referencing Plutonia patch
        // but no sidedef uses BROWN5 → fall through to map name detection
        // and return doom2.
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("apophis_style.wad");

        let pnames = build_pnames(&["BROWN5", "STARTAN1"]);
        let texture1 = build_texture_lump(&[("BROWN5", &[0]), ("WALL01", &[1])]);
        let sidedefs = build_sidedefs(&[("WALL01", "-", "-"), ("WALL01", "WALL01", "-")]);

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("THINGS", &[]),
            ("LINEDEFS", &[]),
            ("SIDEDEFS", &sidedefs),
            ("VERTEXES", &[]),
            ("PNAMES", &pnames),
            ("TEXTURE1", &texture1),
        ]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_iwad(&wad_path), Some("doom2"));
    }

    #[test]
    fn test_detect_iwad_plutonia_actually_used() {
        // A real Plutonia mod that uses Plutonia textures in maps.
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("plutonia_mod.wad");

        let pnames = build_pnames(&["AROCK2", "BRBRICK"]);
        let texture1 = build_texture_lump(&[("AROCK2", &[0]), ("BRBRICK", &[1])]);
        let sidedefs = build_sidedefs(&[("AROCK2", "BRBRICK", "-")]);

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("SIDEDEFS", &sidedefs),
            ("PNAMES", &pnames),
            ("TEXTURE1", &texture1),
        ]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_iwad(&wad_path), Some("plutonia"));
    }

    #[test]
    fn test_detect_iwad_tnt_actually_used() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("tnt_mod.wad");

        let pnames = build_pnames(&["ALTAQUA", "BIGMURAL"]);
        let texture1 = build_texture_lump(&[("ALTAQUA", &[0]), ("BIGMURAL", &[1])]);
        let sidedefs = build_sidedefs(&[("ALTAQUA", "-", "-"), ("BIGMURAL", "-", "-")]);

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("SIDEDEFS", &sidedefs),
            ("PNAMES", &pnames),
            ("TEXTURE1", &texture1),
        ]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_iwad(&wad_path), Some("tnt"));
    }

    #[test]
    fn test_detect_iwad_doom2_maps() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");

        let wad = build_wad(&[("MAP01", &[]), ("MAP02", &[]), ("THINGS", &[])]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_iwad(&wad_path), Some("doom2"));
    }

    #[test]
    fn test_detect_iwad_doom1_maps() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");

        let wad = build_wad(&[("E1M1", &[]), ("E1M2", &[]), ("THINGS", &[])]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_iwad(&wad_path), Some("doom"));
    }

    #[test]
    fn test_detect_iwad_nonexistent() {
        assert_eq!(detect_iwad(Path::new("/nonexistent/test.wad")), None);
    }

    #[test]
    fn test_detect_iwad_zip_wrapped() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");

        let wad = build_wad(&[("MAP01", &[]), ("MAP02", &[]), ("THINGS", &[])]);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("test.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&wad).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_iwad(&zip_path), Some("doom2"));
    }

    #[test]
    fn test_detect_iwad_multi_wad_zip() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("multi.zip");

        let resource_wad = build_wad(&[("THINGS", &[1, 2, 3]), ("LINEDEFS", &[4, 5])]);
        let maps_wad = build_wad(&[("MAP01", &[]), ("MAP02", &[]), ("THINGS", &[])]);

        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("resources.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&resource_wad).unwrap();
        zip.start_file("maps.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&maps_wad).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_iwad(&zip_path), Some("doom2"));
    }

    #[test]
    fn test_detect_iwad_multi_wad_zip_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("nomaps.zip");

        let wad1 = build_wad(&[("THINGS", &[1, 2])]);
        let wad2 = build_wad(&[("LINEDEFS", &[3, 4])]);

        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("a.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&wad1).unwrap();
        zip.start_file("b.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&wad2).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_iwad(&zip_path), None);
    }

    #[test]
    fn test_patch_sets_disjoint() {
        let overlap: Vec<&&str> = TNT_ONLY_PATCHES
            .iter()
            .filter(|p| PLUTONIA_ONLY_PATCHES.contains(*p))
            .collect();
        assert!(
            overlap.is_empty(),
            "TNT and Plutonia sets overlap: {:?}",
            overlap
        );
    }

    #[test]
    fn test_patch_set_sizes() {
        assert_eq!(TNT_ONLY_PATCHES.len(), 197);
        assert_eq!(PLUTONIA_ONLY_PATCHES.len(), 72);
    }

    #[test]
    fn test_plutonia_patches_exclude_doom_wad() {
        let doom_patches = ["HELL6_1", "HELL8_3", "SKY1", "W109_1", "W109_2", "W110_1"];
        for patch in &doom_patches {
            assert!(
                !PLUTONIA_ONLY_PATCHES.contains(patch),
                "{patch} is in DOOM.WAD and should not be in PLUTONIA_ONLY_PATCHES"
            );
        }
    }

    #[test]
    fn test_detect_iwad_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("empty.wad");
        std::fs::write(&wad_path, b"").unwrap();
        assert_eq!(detect_iwad(&wad_path), None);
    }

    #[test]
    fn test_detect_iwad_too_small() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("small.wad");
        std::fs::write(&wad_path, b"PWAD").unwrap();
        assert_eq!(detect_iwad(&wad_path), None);
    }

    #[test]
    fn test_detect_iwad_bad_magic() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("bad.wad");
        std::fs::write(&wad_path, b"NOTAWADFILE!").unwrap();
        assert_eq!(detect_iwad(&wad_path), None);
    }

    #[test]
    fn test_detect_complvl() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad = build_wad(&[("COMPLVL", &[21])]);
        std::fs::write(&wad_path, &wad).unwrap();
        assert_eq!(detect_complvl(&wad_path), Some(21));
    }

    #[test]
    fn test_detect_complvl_missing() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad = build_wad(&[("MAP01", &[])]);
        std::fs::write(&wad_path, &wad).unwrap();
        assert_eq!(detect_complvl(&wad_path), None);
    }

    #[test]
    fn test_detect_complvl_zero() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad = build_wad(&[("COMPLVL", &[0])]);
        std::fs::write(&wad_path, &wad).unwrap();
        assert_eq!(detect_complvl(&wad_path), Some(0));
    }

    #[test]
    fn test_detect_complvl_nonexistent() {
        assert_eq!(detect_complvl(Path::new("/nonexistent/test.wad")), None);
    }
}
