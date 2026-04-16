//! Auto-detect required IWAD family from WAD file contents.
//!
//! Inspects a PWAD's PNAMES lump and map lump names to determine which
//! IWAD (base game) it requires. Detection is unambiguous: TNT-only and
//! Plutonia-only patch sets are disjoint.
//!
//! Priority order:
//! 1. PNAMES analysis — patches unique to TNT or Plutonia (strongest signal)
//! 2. Map name format — ExMy (doom) vs MAPxx (doom2)
//! 3. None if no signal found

use std::collections::HashSet;
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

    // Priority 1: PNAMES analysis (strongest signal for TNT/Plutonia vs Doom 2)
    if let Some(pnames) = parse_pnames(&wad_data, &directory)
        && let Some(result) = detect_from_pnames(&pnames, &lump_names)
    {
        return Some(result);
    }

    // Priority 2: Map name format (Doom 1 vs Doom 2 family)
    detect_from_maps(&lump_names)
}

/// Detect complevel from a WAD file's COMPLVL lump.
///
/// The COMPLVL lump is an id24 signal — a single byte indicating the
/// compatibility level the WAD was designed for.
///
/// Returns the complevel as an integer, or None if no COMPLVL lump found.
pub fn detect_complvl(wad_path: &Path) -> Option<i32> {
    // Delegate to the full COMPLVL parser in complevel_detect which handles
    // both id24 single-byte and text formats correctly.
    crate::complevel_detect::detect_complvl_from_path(wad_path)
}

/// Extract patch names from the PNAMES lump.
fn parse_pnames(wad_data: &[u8], directory: &[(String, u32, u32)]) -> Option<HashSet<String>> {
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

            let mut patches = HashSet::with_capacity(count);
            for i in 0..count {
                let pname_offset = off + 4 + i * 8;
                if pname_offset + 8 > wad_data.len() {
                    break;
                }
                let pname = &wad_data[pname_offset..pname_offset + 8];
                let pname = pname
                    .split(|&b| b == 0)
                    .next()
                    .unwrap_or(b"")
                    .iter()
                    .map(|&b| (b as char).to_ascii_uppercase())
                    .collect::<String>();
                patches.insert(pname);
            }
            return Some(patches);
        }
    }
    None
}

/// Check if PNAMES references TNT-only or Plutonia-only patches.
///
/// Only counts patches that are NOT also present as lumps within the PWAD
/// itself — a self-contained WAD that includes the patches doesn't need
/// the specific IWAD.
fn detect_from_pnames(
    pnames: &HashSet<String>,
    lump_names: &HashSet<&str>,
) -> Option<&'static str> {
    let needed_tnt: usize = pnames
        .iter()
        .filter(|p| TNT_ONLY_PATCHES.contains(p.as_str()) && !lump_names.contains(p.as_str()))
        .count();
    let needed_plutonia: usize = pnames
        .iter()
        .filter(|p| PLUTONIA_ONLY_PATCHES.contains(p.as_str()) && !lump_names.contains(p.as_str()))
        .count();

    if needed_tnt > 0 && needed_plutonia == 0 {
        return Some("tnt");
    }
    if needed_plutonia > 0 && needed_tnt == 0 {
        return Some("plutonia");
    }

    None
}

/// Detect IWAD family from map lump naming convention.
fn detect_from_maps(lump_names: &HashSet<&str>) -> Option<&'static str> {
    let has_doom1 = lump_names.iter().any(|name| DOOM1_MAP_RE.is_match(name));
    let has_doom2 = lump_names.iter().any(|name| DOOM2_MAP_RE.is_match(name));

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

        // Calculate data offsets
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

        // Write header
        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&num_lumps.to_le_bytes());
        wad.extend_from_slice(&dir_offset.to_le_bytes());

        // Write lump data
        wad.extend_from_slice(&data_blob);

        // Write directory
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

    #[test]
    fn test_detect_from_maps_doom1() {
        let names: HashSet<&str> = HashSet::from(["E1M1", "E1M2", "E1M3", "THINGS"]);
        assert_eq!(detect_from_maps(&names), Some("doom"));
    }

    #[test]
    fn test_detect_from_maps_doom2() {
        let names: HashSet<&str> = HashSet::from(["MAP01", "MAP02", "MAP03", "THINGS"]);
        assert_eq!(detect_from_maps(&names), Some("doom2"));
    }

    #[test]
    fn test_detect_from_maps_mixed() {
        let names: HashSet<&str> = HashSet::from(["E1M1", "MAP01"]);
        assert_eq!(detect_from_maps(&names), None);
    }

    #[test]
    fn test_detect_from_maps_neither() {
        let names: HashSet<&str> = HashSet::from(["THINGS", "LINEDEFS"]);
        assert_eq!(detect_from_maps(&names), None);
    }

    #[test]
    fn test_detect_from_pnames_tnt() {
        let mut pnames = HashSet::new();
        pnames.insert("ALTAQUA".to_string());
        pnames.insert("BIGMURAL".to_string());
        pnames.insert("NORMAL_PATCH".to_string());
        let lump_names: HashSet<&str> = HashSet::from(["MAP01"]);
        assert_eq!(detect_from_pnames(&pnames, &lump_names), Some("tnt"));
    }

    #[test]
    fn test_detect_from_pnames_plutonia() {
        let mut pnames = HashSet::new();
        pnames.insert("AROCK2".to_string());
        pnames.insert("BRBRICK".to_string());
        let lump_names: HashSet<&str> = HashSet::from(["MAP01"]);
        assert_eq!(detect_from_pnames(&pnames, &lump_names), Some("plutonia"));
    }

    #[test]
    fn test_detect_from_pnames_self_contained() {
        // WAD provides its own TNT patches as lumps → not detected
        let mut pnames = HashSet::new();
        pnames.insert("ALTAQUA".to_string());
        let lump_names: HashSet<&str> = HashSet::from(["ALTAQUA", "MAP01"]);
        assert_eq!(detect_from_pnames(&pnames, &lump_names), None);
    }

    #[test]
    fn test_parse_pnames() {
        // Build a PNAMES lump with 2 patches
        let mut pnames_data = Vec::new();
        pnames_data.extend_from_slice(&2_i32.to_le_bytes()); // count
        pnames_data.extend_from_slice(b"ALTAQUA\0"); // 8 bytes
        pnames_data.extend_from_slice(b"BIGMURAL"); // 8 bytes

        let wad = build_wad(&[("PNAMES", &pnames_data)]);
        let directory = parse_wad_directory(&wad);
        let result = parse_pnames(&wad, &directory).unwrap();
        assert!(result.contains("ALTAQUA"));
        assert!(result.contains("BIGMURAL"));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_detect_complvl() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");

        // Build WAD with COMPLVL lump containing byte 21
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

        // Build a WAD and wrap it in a ZIP
        let wad = build_wad(&[("MAP01", &[]), ("MAP02", &[])]);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("test.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&wad).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_iwad(&zip_path), Some("doom2"));
    }

    #[test]
    fn test_plutonia_patches_exclude_doom_wad() {
        // HELL6_1, HELL8_3, SKY1, W109_1, W109_2, W110_1 are in DOOM.WAD
        // and must NOT be in the Plutonia-only set (would cause false positives)
        let doom_patches = ["HELL6_1", "HELL8_3", "SKY1", "W109_1", "W109_2", "W110_1"];
        for patch in &doom_patches {
            assert!(
                !PLUTONIA_ONLY_PATCHES.contains(patch),
                "{patch} is in DOOM.WAD and should not be in PLUTONIA_ONLY_PATCHES"
            );
        }
    }

    #[test]
    fn test_detect_from_pnames_no_false_plutonia_with_doom_patches() {
        // A WAD referencing HELL6_1 (in DOOM.WAD) should NOT detect as Plutonia
        let mut pnames = HashSet::new();
        pnames.insert("HELL6_1".to_string());
        pnames.insert("SKY1".to_string());
        let lump_names: HashSet<&str> = HashSet::from(["MAP01"]);
        assert_eq!(detect_from_pnames(&pnames, &lump_names), None);
    }

    #[test]
    fn test_detect_from_pnames_self_contained_plutonia() {
        // WAD provides its own Plutonia patches as lumps → not detected
        let mut pnames = HashSet::new();
        pnames.insert("AROCK2".to_string());
        pnames.insert("BRBRICK".to_string());
        let lump_names: HashSet<&str> = HashSet::from(["AROCK2", "BRBRICK", "MAP01"]);
        assert_eq!(detect_from_pnames(&pnames, &lump_names), None);
    }

    #[test]
    fn test_detect_iwad_multi_wad_zip() {
        // ZIP with multiple WADs: a resource WAD (no maps) and a maps WAD
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("multi.zip");

        let resource_wad = build_wad(&[("THINGS", &[1, 2, 3]), ("LINEDEFS", &[4, 5])]);
        let maps_wad = build_wad(&[("MAP01", &[]), ("MAP02", &[]), ("THINGS", &[])]);

        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        // Resource WAD listed first — old code would pick this one
        zip.start_file("resources.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&resource_wad).unwrap();
        zip.start_file("maps.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&maps_wad).unwrap();
        zip.finish().unwrap();

        // Should detect from the WAD with maps, not the resource WAD
        assert_eq!(detect_iwad(&zip_path), Some("doom2"));
    }

    #[test]
    fn test_detect_iwad_multi_wad_zip_fallback() {
        // ZIP with multiple WADs, none containing maps — should fall back to first
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

        // No maps in either WAD, so detection returns None (falls back to first WAD,
        // but map detection finds nothing)
        assert_eq!(detect_iwad(&zip_path), None);
    }

    #[test]
    fn test_patch_sets_disjoint() {
        // TNT and Plutonia patch sets must have zero overlap
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
    fn test_parse_pnames_empty() {
        // PNAMES lump with count=0
        let mut pnames_data = Vec::new();
        pnames_data.extend_from_slice(&0_i32.to_le_bytes()); // count = 0

        let wad = build_wad(&[("PNAMES", &pnames_data)]);
        let directory = parse_wad_directory(&wad);
        let result = parse_pnames(&wad, &directory).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_detect_from_pnames_no_signal() {
        // PNAMES with only generic patches (not in TNT or Plutonia sets)
        let mut pnames = HashSet::new();
        pnames.insert("STARTAN1".to_string());
        pnames.insert("DOOR1".to_string());
        pnames.insert("FLAT10".to_string());
        let lump_names: HashSet<&str> = HashSet::from(["MAP01"]);
        assert_eq!(detect_from_pnames(&pnames, &lump_names), None);
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
        std::fs::write(&wad_path, b"PWAD").unwrap(); // Only 4 bytes, need 12
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
    fn test_detect_iwad_with_tnt_pnames() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("tnt_wad.wad");

        // Build PNAMES lump with TNT-only patches
        let mut pnames_data = Vec::new();
        pnames_data.extend_from_slice(&2_i32.to_le_bytes());
        pnames_data.extend_from_slice(b"ALTAQUA\0");
        pnames_data.extend_from_slice(b"BIGMURAL");

        let wad = build_wad(&[("MAP01", &[]), ("PNAMES", &pnames_data)]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_iwad(&wad_path), Some("tnt"));
    }

    #[test]
    fn test_detect_iwad_pnames_priority_over_maps() {
        // PNAMES detection should take priority over map lump detection
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("priority.wad");

        let mut pnames_data = Vec::new();
        pnames_data.extend_from_slice(&1_i32.to_le_bytes());
        pnames_data.extend_from_slice(b"AROCK2\0\0");

        let wad = build_wad(&[("MAP01", &[]), ("PNAMES", &pnames_data)]);
        std::fs::write(&wad_path, &wad).unwrap();

        // Should detect plutonia from PNAMES, not doom2 from MAPxx
        assert_eq!(detect_iwad(&wad_path), Some("plutonia"));
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
