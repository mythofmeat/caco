//! Auto-detect complevel (compatibility level) from WAD file contents.
//!
//! Conservative heuristics — returns None when ambiguous. Inspects lumps like
//! COMPLVL, UMAPINFO, and DEHACKED to infer the minimum required complevel.
//!
//! Detection hierarchy:
//! 1. COMPLVL lump (id24 single byte or text string) -> parsed value
//! 2. UMAPINFO lump present -> MBF21 (21)
//! 3. DEHACKED with MBF21 features -> MBF21 (21)
//! 4. DEHACKED with MBF codepointers -> MBF (11)
//! 5. DEHACKED without MBF features + ExMy -> vanilla (2)
//! 6. DEHACKED without MBF features + MAPxx -> vanilla doom2 (4)
//! 7. ExMy maps only -> vanilla (2)
//! 8. MAPxx maps only -> vanilla doom2 (4)
//! 9. No map lumps -> None (resource WAD, can't determine)

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::complevel::parse_complevel;
use crate::utils::{load_wad_data, parse_wad_directory};

/// MBF-specific DeHackEd codepointers (indicate MBF or higher, complevel 11).
static MBF_CODEPOINTERS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "A_MUSHROOM",
        "A_SPAWN",
        "A_TURN",
        "A_FACE",
        "A_SCRATCH",
        "A_PLAYSSOUND",
        "A_RANDOMJUMP",
        "A_LINEEFFECT",
        "A_DIE",
        "A_DETONATE",
        "A_HEALCHASE",
        "A_SEEKERMISSILE",
        "A_SEEKTRACER",
        "A_FINDTRACER",
        "A_CLEARTARGET",
        "A_CLEARTRACER",
    ])
});

/// MBF21-exclusive codepointers (not in original MBF).
static MBF21_CODEPOINTERS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "A_SPAWNOBJECT",
        "A_MONSTERPROJECTILE",
        "A_MONSTERMELEEATTACK",
        "A_MONSTERBULLETATTACK",
        "A_RADIUSDAMAGE",
        "A_NOISEALERT",
        "A_JUMPIFHEALTHBELOW",
        "A_JUMPIFFLAGSSET",
        "A_JUMPIFTARGETINLOS",
        "A_JUMPIFTARGETCLOSER",
        "A_JUMPIFTRACERINLOS",
        "A_JUMPIFTRACERCLOSER",
        "A_ADDFLAGS",
        "A_REMOVEFLAGS",
        "A_WEAPONPROJECTILE",
        "A_WEAPONBULLETATTACK",
        "A_WEAPONMELEEATTACK",
        "A_WEAPONSOUND",
        "A_WEAPONJUMP",
        "A_WEAPONALERT",
        "A_CONSUMEAMMO",
        "A_CHECKAMMO",
        "A_REFIRETO",
        "A_GUNFLASHTO",
    ])
});

static EXMY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^E\dM\d$").unwrap());
static MAPXX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^MAP\d\d$").unwrap());

/// Regex for DEHACKED Doom version field.
static DOOM_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)DOOM\s+VERSION\s*=\s*(\d+)").unwrap());

/// Regex for MBF21 BITS field in DEHACKED.
static MBF21_BITS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)MBF21\s+BITS").unwrap());

/// Detect complevel from the COMPLVL lump only (id24 single byte or text).
///
/// Returns complevel int if a valid COMPLVL lump is found, or None.
pub fn detect_complvl_from_path(wad_path: &Path) -> Option<i32> {
    let wad_data = load_wad_data(wad_path)?;
    if wad_data.len() < 12 {
        return None;
    }
    let magic = &wad_data[..4];
    if magic != b"IWAD" && magic != b"PWAD" {
        return None;
    }
    let directory = parse_wad_directory(&wad_data);
    for (name, offset, size) in &directory {
        if name == "COMPLVL"
            && *size >= 1
            && let Some(cl) = parse_complvl_lump(&wad_data, *offset, *size)
        {
            return Some(cl);
        }
    }
    None
}

/// Detect complevel from WAD file contents.
///
/// Returns complevel int if confidently detected, or None if ambiguous.
pub fn detect_complevel(wad_path: &Path) -> Option<i32> {
    let wad_data = load_wad_data(wad_path)?;

    // Validate WAD magic
    if wad_data.len() < 12 {
        return None;
    }
    let magic = &wad_data[..4];
    if magic != b"IWAD" && magic != b"PWAD" {
        return None;
    }

    let directory = parse_wad_directory(&wad_data);
    let lump_names: HashSet<&str> = directory.iter().map(|(name, _, _)| name.as_str()).collect();

    // 1. Check for COMPLVL lump (id24 single byte or text string)
    for (name, offset, size) in &directory {
        if name == "COMPLVL"
            && *size >= 1
            && let Some(cl) = parse_complvl_lump(&wad_data, *offset, *size)
        {
            return Some(cl);
        }
    }

    // 2. Check for UMAPINFO -> MBF21
    if lump_names.contains("UMAPINFO") {
        return Some(21);
    }

    // 3-4. Check DEHACKED lump for MBF/MBF21 codepointers
    if lump_names.contains("DEHACKED")
        && let Some(deh_text) = read_lump_text(&wad_data, &directory, "DEHACKED")
        && let Some(cl) = detect_from_dehacked(&deh_text)
    {
        return Some(cl);
    }
    // If DEHACKED present but no MBF features — vanilla DEHACKED.
    // Fall through to map lump detection for complevel 2 or 4.

    // 5-8. Check map lump names for vanilla complevel
    let has_exmy = lump_names.iter().any(|name| EXMY_RE.is_match(name));
    let has_mapxx = lump_names.iter().any(|name| MAPXX_RE.is_match(name));

    if has_exmy || has_mapxx {
        // ExMy -> vanilla Doom (2), MAPxx -> vanilla Doom2/Final Doom (4)
        return Some(if has_exmy && !has_mapxx { 2 } else { 4 });
    }

    // 9. No map lumps — resource WAD, can't determine complevel
    None
}

/// Parse a COMPLVL lump, handling both id24 (1-byte) and text formats.
///
/// id24 spec: single byte where the byte value IS the complevel.
/// Some WADs use a text string instead (e.g. "mbf21", "vanilla").
fn parse_complvl_lump(wad_data: &[u8], offset: u32, size: u32) -> Option<i32> {
    let off = offset as usize;
    let sz = size as usize;
    if off + sz > wad_data.len() {
        return None;
    }
    let raw = &wad_data[off..off + sz];

    if sz == 1 {
        // id24 format: single byte = complevel number
        let val = raw[0] as i32;
        if val <= crate::complevel::MAX_COMPLEVEL {
            return Some(val);
        }
        return None;
    }

    // Text format: try to decode as a complevel name/number
    let text: String = raw
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    parse_complevel(text)
}

/// Read a text lump from WAD data.
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
                return Some(
                    wad_data[off..off + sz]
                        .iter()
                        .map(|&b| b as char)
                        .collect(),
                );
            }
        }
    }
    None
}

/// Detect complevel from DEHACKED lump contents.
///
/// Checks Doom version header, MBF21 fields/codepointers, then MBF
/// codepointers. Returns None if no MBF features found (vanilla DEHACKED).
fn detect_from_dehacked(deh_text: &str) -> Option<i32> {
    let upper = deh_text.to_uppercase();

    // Check Doom version field — 2021 is the definitive MBF21 signal
    if let Some(caps) = DOOM_VERSION_RE.captures(&upper)
        && let Ok(version) = caps[1].parse::<i32>()
        && version == 2021
    {
        return Some(21);
    }

    // Check for MBF21-specific DEHACKED fields (thing/weapon/frame flags)
    if MBF21_BITS_RE.is_match(&upper) {
        return Some(21);
    }

    // Check for MBF21-exclusive codepointers first (more specific)
    for cp in MBF21_CODEPOINTERS.iter() {
        if upper.contains(cp) {
            return Some(21);
        }
    }

    // Check for MBF codepointers
    for cp in MBF_CODEPOINTERS.iter() {
        if upper.contains(cp) {
            return Some(11);
        }
    }

    // DEHACKED present but no MBF-specific features — vanilla-compatible
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn write_wad(lumps: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad = build_wad(lumps);
        std::fs::write(&wad_path, &wad).unwrap();
        (dir, wad_path)
    }

    // --- detect_from_dehacked unit tests ---

    #[test]
    fn test_detect_from_dehacked_mbf21_codepointer() {
        let deh = "Frame 100\nAction A_SpawnObject\n";
        assert_eq!(detect_from_dehacked(deh), Some(21));
    }

    #[test]
    fn test_detect_from_dehacked_mbf21_doom_version() {
        let deh = "Patch File for DeHackEd v3.0\nDoom version = 2021\n";
        assert_eq!(detect_from_dehacked(deh), Some(21));
    }

    #[test]
    fn test_detect_from_dehacked_mbf21_bits() {
        let deh = "Thing 1\nMBF21 Bits = 0x00000001\n";
        assert_eq!(detect_from_dehacked(deh), Some(21));
    }

    #[test]
    fn test_detect_from_dehacked_mbf() {
        let deh = "Frame 100\nAction A_Mushroom\n";
        assert_eq!(detect_from_dehacked(deh), Some(11));
    }

    #[test]
    fn test_detect_from_dehacked_vanilla() {
        // Vanilla DEHACKED with no MBF features should return None
        let deh = "Patch File for DeHackEd v3.0\nDoom version = 19\n";
        assert_eq!(detect_from_dehacked(deh), None);
    }

    // --- detect_complevel integration tests ---

    #[test]
    fn test_detect_complvl_lump_id24_byte() {
        // id24 format: single byte = complevel number
        let (_dir, wad_path) = write_wad(&[("COMPLVL", &[21]), ("MAP01", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(21));
    }

    #[test]
    fn test_detect_complvl_lump_text() {
        // Text format: "mbf21" string
        let (_dir, wad_path) = write_wad(&[("COMPLVL", b"mbf21"), ("MAP01", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(21));
    }

    #[test]
    fn test_detect_complvl_lump_text_vanilla() {
        let (_dir, wad_path) = write_wad(&[("COMPLVL", b"vanilla"), ("E1M1", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(2));
    }

    #[test]
    fn test_detect_umapinfo() {
        let (_dir, wad_path) = write_wad(&[("UMAPINFO", b"map MAP01 {}\n"), ("MAP01", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(21));
    }

    #[test]
    fn test_detect_exmy_vanilla() {
        let (_dir, wad_path) = write_wad(&[("E1M1", &[]), ("E1M2", &[]), ("THINGS", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(2));
    }

    #[test]
    fn test_detect_mapxx_vanilla() {
        // MAPxx without DEHACKED or special lumps -> complevel 4
        let (_dir, wad_path) = write_wad(&[("MAP01", &[]), ("MAP02", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(4));
    }

    #[test]
    fn test_detect_dehacked_mbf() {
        let deh = b"Frame 100\nAction A_Mushroom\n";
        let (_dir, wad_path) = write_wad(&[("DEHACKED", deh), ("MAP01", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(11));
    }

    #[test]
    fn test_detect_vanilla_dehacked_exmy() {
        // Vanilla DEHACKED (no MBF codepointers) with ExMy maps -> complevel 2
        let deh = b"Patch File for DeHackEd v3.0\nDoom version = 19\nThing 1\nHit points = 100\n";
        let (_dir, wad_path) = write_wad(&[("DEHACKED", deh), ("E1M1", &[]), ("E1M2", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(2));
    }

    #[test]
    fn test_detect_vanilla_dehacked_mapxx() {
        // Vanilla DEHACKED (no MBF codepointers) with MAPxx maps -> complevel 4
        let deh = b"Patch File for DeHackEd v3.0\nDoom version = 19\nThing 1\nHit points = 100\n";
        let (_dir, wad_path) = write_wad(&[("DEHACKED", deh), ("MAP01", &[]), ("MAP02", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(4));
    }

    #[test]
    fn test_detect_no_maps() {
        // Resource WAD with no map lumps -> None
        let (_dir, wad_path) = write_wad(&[("TEXTURE1", &[0; 4]), ("PNAMES", &[0; 4])]);
        assert_eq!(detect_complevel(&wad_path), None);
    }

    #[test]
    fn test_detect_nonexistent() {
        assert_eq!(detect_complevel(Path::new("/nonexistent/test.wad")), None);
    }

    #[test]
    fn test_complvl_takes_priority_over_umapinfo() {
        // COMPLVL lump should take priority over UMAPINFO
        let (_dir, wad_path) = write_wad(&[
            ("COMPLVL", &[9]), // Boom
            ("UMAPINFO", b"map MAP01 {}\n"),
            ("MAP01", &[]),
        ]);
        assert_eq!(detect_complevel(&wad_path), Some(9));
    }

    #[test]
    fn test_detect_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("empty.wad");
        std::fs::write(&wad_path, b"").unwrap();
        assert_eq!(detect_complevel(&wad_path), None);
    }

    #[test]
    fn test_detect_bad_magic() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("bad.wad");
        std::fs::write(&wad_path, b"NOTAWADFILE!").unwrap();
        assert_eq!(detect_complevel(&wad_path), None);
    }

    #[test]
    fn test_detect_complvl_lump_zero() {
        // id24 byte value 0 should return Some(0)
        let (_dir, wad_path) = write_wad(&[("COMPLVL", &[0]), ("MAP01", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(0));
    }

    #[test]
    fn test_detect_complvl_lump_boom_text() {
        let (_dir, wad_path) = write_wad(&[("COMPLVL", b"boom"), ("MAP01", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(9));
    }

    #[test]
    fn test_detect_from_dehacked_mbf21_seektracer() {
        // A_SEEKTRACER is MBF-only, should return 11
        let deh = "Frame 100\nAction A_SeekTracer\n";
        assert_eq!(detect_from_dehacked(deh), Some(11));
    }

    #[test]
    fn test_detect_from_dehacked_mbf21_refireto() {
        // A_REFIRETO is MBF21-only
        let deh = "Frame 100\nAction A_RefireTo\n";
        assert_eq!(detect_from_dehacked(deh), Some(21));
    }

    #[test]
    fn test_detect_from_dehacked_no_codepointers() {
        // DEHACKED with only thing properties, no codepointers
        let deh = "Thing 1 (Zombieman)\nHit points = 100\nReaction time = 8\n";
        assert_eq!(detect_from_dehacked(deh), None);
    }

    #[test]
    fn test_detect_mixed_maps_vanilla() {
        // Both ExMy and MAPxx maps -> complevel 4 (prefers MAPxx/doom2)
        let (_dir, wad_path) = write_wad(&[("E1M1", &[]), ("MAP01", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(4));
    }

    #[test]
    fn test_detect_dehacked_mbf21_codepointer_integration() {
        // MBF21 codepointer in DEHACKED lump via full detect_complevel
        let deh = b"Frame 100\nAction A_SpawnObject\n";
        let (_dir, wad_path) = write_wad(&[("DEHACKED", deh), ("MAP01", &[])]);
        assert_eq!(detect_complevel(&wad_path), Some(21));
    }

    #[test]
    fn test_complvl_takes_priority_over_dehacked() {
        // COMPLVL lump should take priority over DEHACKED analysis
        let deh = b"Frame 100\nAction A_Mushroom\n"; // Would be MBF (11)
        let (_dir, wad_path) = write_wad(&[
            ("COMPLVL", &[21]),
            ("DEHACKED", deh),
            ("MAP01", &[]),
        ]);
        assert_eq!(detect_complevel(&wad_path), Some(21));
    }
}
