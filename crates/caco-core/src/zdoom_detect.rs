//! Auto-detect whether a WAD/PK3 requires a ZDoom-family sourceport.
//!
//! The only reliable file-level signal is **UDMF map format**: maps stored as
//! TEXTMAP lumps instead of standard THINGS/LINEDEFS/etc. dsda-doom and other
//! Boom-lineage ports cannot parse UDMF maps at all.
//!
//! Other ZDoom-associated lumps (ZSCRIPT, DECORATE, GLDEFS, MENUDEF, MODELDEF,
//! VOXELDEF, SBARINFO) are intentionally NOT checked — WAD authors commonly
//! include these as optional GZDoom enhancements while the core gameplay runs
//! fine in dsda-doom, which silently ignores them. Confirmed with real-world
//! false positives: Neon Overdrive (DECORATE), Eviternity II (ZSCRIPT+MENUDEF).
//!
//! For PK3 files, maps live as individual WADs under `maps/` — each is checked
//! for UDMF format.

use std::path::Path;

use regex::Regex;
use std::sync::LazyLock;

use crate::utils::{load_wad_data, parse_wad_directory};

static MAP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(E[1-9]M[0-9]|MAP[0-9][0-9])$").unwrap());

/// Detect whether a WAD file requires a ZDoom-family sourceport.
///
/// Returns `true` if any maps use UDMF format (TEXTMAP), `false` if all maps
/// use standard format, or `None` if the file cannot be read/parsed.
pub fn detect_zdoom_required(wad_path: &Path) -> Option<bool> {
    let ext = wad_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // PK3/PK7 files are ZIP archives with maps/ directory
    if ext == "pk3" || ext == "pk7" {
        return detect_from_pk3(wad_path);
    }

    // WAD files (including ZIP-wrapped WADs)
    let wad_data = load_wad_data(wad_path)?;
    let directory = parse_wad_directory(&wad_data);
    if directory.is_empty() {
        return None;
    }

    Some(has_udmf_maps(&directory))
}

/// Check if any maps in a WAD directory use UDMF format.
///
/// UDMF maps have TEXTMAP as the first lump after the map marker:
///   MAP01 → TEXTMAP → ... → ENDMAP
/// Standard maps have THINGS/LINEDEFS/etc.:
///   MAP01 → THINGS → LINEDEFS → ...
fn has_udmf_maps(directory: &[(String, u32, u32)]) -> bool {
    let names: Vec<&str> = directory.iter().map(|(n, _, _)| n.as_str()).collect();
    for (i, name) in names.iter().enumerate() {
        if MAP_RE.is_match(name)
            && let Some(&next) = names.get(i + 1)
            && next == "TEXTMAP"
        {
            return true;
        }
    }
    false
}

/// Check a PK3/PK7 (ZIP archive) for UDMF-format maps.
///
/// PK3 maps are stored as individual WAD files under `maps/` (e.g.,
/// `maps/MAP01.wad`). Each map WAD is checked for UDMF format.
fn detect_from_pk3(pk3_path: &Path) -> Option<bool> {
    use std::fs::File;
    use std::io::Read;

    let file = File::open(pk3_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    for i in 0..archive.len() {
        let is_map_wad = archive
            .name_for_index(i)
            .is_some_and(|n| {
                let lower = n.to_lowercase();
                lower.starts_with("maps/") && lower.ends_with(".wad")
            });

        if is_map_wad {
            let mut entry = match archive.by_index(i) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let mut buf = Vec::with_capacity(entry.size() as usize);
            if entry.read_to_end(&mut buf).is_err() {
                continue;
            }
            let directory = parse_wad_directory(&buf);
            if has_udmf_maps(&directory) {
                return Some(true);
            }
        }
    }

    Some(false)
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

    fn write_wad(lumps: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let wad = build_wad(lumps);
        std::fs::write(&wad_path, &wad).unwrap();
        (dir, wad_path)
    }

    // --- UDMF detection ---

    #[test]
    fn test_detect_udmf_map() {
        // MAP01 followed by TEXTMAP = UDMF format
        let (_dir, path) = write_wad(&[
            ("MAP01", &[]),
            ("TEXTMAP", b"namespace = \"zdoom\";"),
            ("ENDMAP", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_udmf_exmy() {
        let (_dir, path) = write_wad(&[
            ("E1M1", &[]),
            ("TEXTMAP", b"namespace = \"zdoom\";"),
            ("ENDMAP", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_udmf_multiple_maps() {
        let (_dir, path) = write_wad(&[
            ("MAP01", &[]),
            ("TEXTMAP", b"namespace = \"zdoom\";"),
            ("ENDMAP", &[]),
            ("MAP02", &[]),
            ("TEXTMAP", b"namespace = \"zdoom\";"),
            ("ENDMAP", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    // --- Standard format (not zdoom) ---

    #[test]
    fn test_detect_standard_maps() {
        let (_dir, path) = write_wad(&[
            ("MAP01", &[]),
            ("THINGS", &[1, 2]),
            ("LINEDEFS", &[3, 4]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_vanilla_wad() {
        let (_dir, path) = write_wad(&[("MAP01", &[]), ("MAP02", &[]), ("THINGS", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_boom_wad() {
        let (_dir, path) = write_wad(&[
            ("MAP01", &[]),
            ("THINGS", &[]),
            ("DEHACKED", b"Doom version = 19"),
            ("UMAPINFO", b"map MAP01 {}"),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    // --- Optional zdoom lumps should NOT trigger ---

    #[test]
    fn test_detect_zscript_not_definitive() {
        // ZSCRIPT without UDMF maps — optional enhancement, not required
        let (_dir, path) = write_wad(&[
            ("MAP01", &[]),
            ("THINGS", &[]),
            ("ZSCRIPT", b"version \"4.0\""),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_decorate_not_definitive() {
        let (_dir, path) = write_wad(&[
            ("MAP01", &[]),
            ("THINGS", &[]),
            ("DECORATE", b"actor Foo {}"),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_menudef_not_definitive() {
        let (_dir, path) = write_wad(&[
            ("MAP01", &[]),
            ("THINGS", &[]),
            ("MENUDEF", b"OptionMenu {}"),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_gldefs_not_definitive() {
        let (_dir, path) = write_wad(&[
            ("MAP01", &[]),
            ("THINGS", &[]),
            ("GLDEFS", b"brightmap {}"),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    // --- Mixed: UDMF maps + optional lumps ---

    #[test]
    fn test_detect_udmf_with_zscript() {
        let (_dir, path) = write_wad(&[
            ("ZSCRIPT", b"version \"4.0\""),
            ("MAP01", &[]),
            ("TEXTMAP", b"namespace = \"zdoom\";"),
            ("ENDMAP", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    // --- TEXTMAP not after map marker should NOT trigger ---

    #[test]
    fn test_detect_textmap_not_after_map_marker() {
        // TEXTMAP as a standalone lump (not following a map marker) is not UDMF
        let (_dir, path) = write_wad(&[
            ("TEXTMAP", b"some data"),
            ("MAP01", &[]),
            ("THINGS", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    // --- Edge cases ---

    #[test]
    fn test_detect_nonexistent() {
        assert_eq!(
            detect_zdoom_required(Path::new("/nonexistent/test.wad")),
            None
        );
    }

    #[test]
    fn test_detect_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.wad");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(detect_zdoom_required(&path), None);
    }

    #[test]
    fn test_detect_bad_magic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.wad");
        std::fs::write(&path, b"NOTAWADFILE!").unwrap();
        assert_eq!(detect_zdoom_required(&path), None);
    }

    #[test]
    fn test_detect_no_maps() {
        // Resource WAD with no map markers
        let (_dir, path) = write_wad(&[("TEXTURE1", &[0; 4]), ("PNAMES", &[0; 4])]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    // --- PK3 detection tests ---

    fn write_pk3(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let pk3_path = dir.path().join("test.pk3");
        let file = std::fs::File::create(&pk3_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, data) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
        (dir, pk3_path)
    }

    #[test]
    fn test_detect_pk3_udmf_map() {
        let map_wad = build_wad(&[
            ("MAP01", &[]),
            ("TEXTMAP", b"namespace = \"zdoom\";"),
            ("ENDMAP", &[]),
        ]);
        let (_dir, path) = write_pk3(&[("maps/MAP01.wad", &map_wad)]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_pk3_standard_map() {
        let map_wad = build_wad(&[("MAP01", &[]), ("THINGS", &[1, 2])]);
        let (_dir, path) = write_pk3(&[("maps/MAP01.wad", &map_wad)]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_pk3_no_maps() {
        let (_dir, path) = write_pk3(&[("ZSCRIPT.zs", b"version \"4.0\"")]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_pk3_nonexistent() {
        assert_eq!(
            detect_zdoom_required(Path::new("/nonexistent/test.pk3")),
            None
        );
    }

    // --- ZIP-wrapped WAD tests ---

    #[test]
    fn test_detect_zip_wrapped_udmf_wad() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");

        let wad = build_wad(&[
            ("MAP01", &[]),
            ("TEXTMAP", b"namespace = \"zdoom\";"),
            ("ENDMAP", &[]),
        ]);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("test.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&wad).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_zdoom_required(&zip_path), Some(true));
    }

    #[test]
    fn test_detect_zip_wrapped_standard_wad() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");

        let wad = build_wad(&[("MAP01", &[]), ("THINGS", &[])]);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("test.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&wad).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_zdoom_required(&zip_path), Some(false));
    }
}
