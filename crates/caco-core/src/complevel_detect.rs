//! Auto-detect complevel (compatibility level) from WAD file contents.
//!
//! Conservative heuristics — returns None when ambiguous. Inspects lumps like
//! UMAPINFO and DEHACKED to infer the minimum required complevel.
//!
//! Detection hierarchy:
//! 1. UMAPINFO lump present -> MBF21 (21)
//! 2. DEHACKED with MBF21 codepointers -> MBF21 (21)
//! 3. DEHACKED with MBF codepointers -> MBF (11)
//! 4. DEHACKED without MBF features -> None (ambiguous)
//! 5. ExMy maps only, no DEHACKED/UMAPINFO -> vanilla (2)
//! 6. MAPxx maps without special lumps -> None (could be vanilla doom2 or Boom)

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::utils::{load_wad_data, parse_wad_directory};

/// MBF-specific DeHackEd codepointers (indicate MBF or higher).
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
        "A_FINDTRACER",
        "A_CLEARTARGET",
        "A_JUMPIFHEALTHBELOW",
        "A_JUMPIFFLAGSSET",
        "A_ADDFLAGS",
        "A_REMOVEFLAGS",
    ])
});

/// MBF21 codepointers.
static MBF21_CODEPOINTERS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "A_SEEKERMISSILE",
        "A_FINDTRACER",
        "A_CLEARTARGET",
        "A_JUMPIFHEALTHBELOW",
        "A_JUMPIFFLAGSSET",
        "A_ADDFLAGS",
        "A_REMOVEFLAGS",
        "A_WEAPONPROJECTILE",
        "A_WEAPONBULLETATTACK",
        "A_WEAPONMELEEATTACK",
        "A_WEAPONSOUND",
        "A_WEAPONJUMP",
        "A_CONSUMEAMMO",
        "A_CHECKAMMO",
        "A_REFIRETO",
        "A_GUNFLASHTO",
        "A_WEAPONALERT",
        "A_NOISEALERT",
        "A_HEALCHASE",
        "A_SPAWNOBJECT",
        "A_MONSTERPROJECTILE",
        "A_MONSTERMELEEATTACK",
        "A_MONSTERBULLETATTACK",
        "A_RADIUSDAMAGE",
    ])
});

static EXMY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^E\dM\d$").unwrap());
static MAPXX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^MAP\d\d$").unwrap());

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

    // Check for UMAPINFO -> MBF21
    if lump_names.contains("UMAPINFO") {
        return Some(21);
    }

    // Check DEHACKED lump for MBF codepointers
    if lump_names.contains("DEHACKED")
        && let Some(deh_text) = read_lump_text(&wad_data, &directory, "DEHACKED") {
            if let Some(cl) = detect_from_dehacked(&deh_text) {
                return Some(cl);
            }
            // DEHACKED present but no MBF pointers — ambiguous
            return None;
        }

    // No DEHACKED, no UMAPINFO — check map lump names
    let has_exmy = lump_names.iter().any(|name| EXMY_RE.is_match(name));
    let has_mapxx = lump_names.iter().any(|name| MAPXX_RE.is_match(name));

    if has_exmy && !has_mapxx {
        // ExMy-only maps without special lumps -> vanilla Doom
        return Some(2);
    }

    // MAPxx or mixed — ambiguous
    None
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
/// Checks for MBF21 codepointers first, then MBF codepointers.
/// Returns None if no MBF features found (ambiguous).
fn detect_from_dehacked(deh_text: &str) -> Option<i32> {
    let upper = deh_text.to_uppercase();

    // Check for MBF21 codepointers first (more specific)
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

    // DEHACKED present but no MBF-specific features — ambiguous
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

    #[test]
    fn test_detect_from_dehacked_mbf21() {
        let deh = "Frame 100\nAction A_SpawnObject\n";
        assert_eq!(detect_from_dehacked(deh), Some(21));
    }

    #[test]
    fn test_detect_from_dehacked_mbf() {
        let deh = "Frame 100\nAction A_Mushroom\n";
        assert_eq!(detect_from_dehacked(deh), Some(11));
    }

    #[test]
    fn test_detect_from_dehacked_none() {
        let deh = "Patch File for DeHackEd v3.0\nDoom version = 19\n";
        assert_eq!(detect_from_dehacked(deh), None);
    }

    #[test]
    fn test_detect_umapinfo() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");

        let wad = build_wad(&[("UMAPINFO", b"map MAP01 {}\n"), ("MAP01", &[])]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_complevel(&wad_path), Some(21));
    }

    #[test]
    fn test_detect_exmy_vanilla() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");

        let wad = build_wad(&[("E1M1", &[]), ("E1M2", &[]), ("THINGS", &[])]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_complevel(&wad_path), Some(2));
    }

    #[test]
    fn test_detect_mapxx_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");

        let wad = build_wad(&[("MAP01", &[]), ("MAP02", &[])]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_complevel(&wad_path), None);
    }

    #[test]
    fn test_detect_dehacked_mbf() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");

        let deh = b"Frame 100\nAction A_Mushroom\n";
        let wad = build_wad(&[("DEHACKED", deh), ("MAP01", &[])]);
        std::fs::write(&wad_path, &wad).unwrap();

        assert_eq!(detect_complevel(&wad_path), Some(11));
    }

    #[test]
    fn test_detect_nonexistent() {
        assert_eq!(detect_complevel(Path::new("/nonexistent/test.wad")), None);
    }
}
