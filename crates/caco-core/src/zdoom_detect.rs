//! Auto-detect whether a WAD/PK3 requires a ZDoom-family sourceport.
//!
//! Scans WAD lump names (or PK3 ZIP entries) for lumps that only ZDoom-family
//! sourceports support. Any match means dsda-doom, woof, chocolate-doom, etc.
//! cannot load the file.
//!
//! Detection lumps:
//! - ZSCRIPT  — GZDoom 3.0+ scripting language
//! - DECORATE — ZDoom actor definitions
//! - GLDEFS   — OpenGL rendering definitions
//! - MODELDEF — 3D model support
//! - VOXELDEF — Voxel definitions
//! - TEXTMAP  — UDMF map format (only ZDoom-family fully supports)
//! - SBARINFO — ZDoom custom statusbar definitions
//! - MENUDEF  — ZDoom menu definitions

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use crate::utils::{load_wad_data, parse_wad_directory};

/// Lump names that definitively indicate a ZDoom-family sourceport is required.
static ZDOOM_LUMPS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "ZSCRIPT",
        "DECORATE",
        "GLDEFS",
        "MODELDEF",
        "VOXELDEF",
        "TEXTMAP",
        "SBARINFO",
        "MENUDEF",
    ])
});

/// Detect whether a WAD file requires a ZDoom-family sourceport.
///
/// Returns `true` if any ZDoom-exclusive lumps are found, `false` if none are
/// found, or `None` if the file cannot be read/parsed.
pub fn detect_zdoom_required(wad_path: &Path) -> Option<bool> {
    let ext = wad_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // PK3/PK7 files are ZIP archives — check entry names directly
    if ext == "pk3" || ext == "pk7" {
        return detect_from_pk3(wad_path);
    }

    // WAD files (including ZIP-wrapped WADs)
    let wad_data = load_wad_data(wad_path)?;
    let directory = parse_wad_directory(&wad_data);
    if directory.is_empty() {
        return None;
    }

    let found = directory
        .iter()
        .any(|(name, _, _)| ZDOOM_LUMPS.contains(name.as_str()));
    Some(found)
}

/// Check a PK3/PK7 (ZIP archive) for ZDoom-exclusive entries.
///
/// Matches against basenames of ZIP entries, case-insensitively.
fn detect_from_pk3(pk3_path: &Path) -> Option<bool> {
    use std::fs::File;

    let file = File::open(pk3_path).ok()?;
    let archive = zip::ZipArchive::new(file).ok()?;

    for i in 0..archive.len() {
        let name = match archive.name_for_index(i) {
            Some(n) => n,
            None => continue,
        };

        // Extract basename and strip extension for matching
        let basename = Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_uppercase();

        if ZDOOM_LUMPS.contains(basename.as_str()) {
            return Some(true);
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

    // --- WAD detection tests ---

    #[test]
    fn test_detect_zscript() {
        let (_dir, path) = write_wad(&[("ZSCRIPT", b"version \"4.0\""), ("MAP01", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_decorate() {
        let (_dir, path) = write_wad(&[("DECORATE", b"actor Foo {}"), ("MAP01", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_gldefs() {
        let (_dir, path) = write_wad(&[("GLDEFS", b"brightmap {}"), ("MAP01", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_modeldef() {
        let (_dir, path) = write_wad(&[("MODELDEF", b"Model Foo {}"), ("MAP01", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_voxeldef() {
        let (_dir, path) = write_wad(&[("VOXELDEF", b"voxel test"), ("MAP01", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_textmap() {
        let (_dir, path) = write_wad(&[("TEXTMAP", b"namespace = \"zdoom\";"), ("MAP01", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_sbarinfo() {
        let (_dir, path) = write_wad(&[("SBARINFO", b"base Doom;"), ("MAP01", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_menudef() {
        let (_dir, path) = write_wad(&[("MENUDEF", b"OptionMenu {}"), ("MAP01", &[])]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
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
            ("DEHACKED", b"Doom version = 19"),
            ("UMAPINFO", b"map MAP01 {}"),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_nonexistent() {
        assert_eq!(detect_zdoom_required(Path::new("/nonexistent/test.wad")), None);
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
    fn test_detect_multiple_zdoom_lumps() {
        let (_dir, path) = write_wad(&[
            ("ZSCRIPT", b"version \"4.0\""),
            ("DECORATE", b"actor Foo {}"),
            ("GLDEFS", b"brightmap {}"),
            ("MAP01", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
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
    fn test_detect_pk3_with_zscript() {
        let (_dir, path) = write_pk3(&[
            ("ZSCRIPT.zs", b"version \"4.0\""),
            ("maps/MAP01.wad", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_pk3_with_decorate() {
        let (_dir, path) = write_pk3(&[
            ("DECORATE.txt", b"actor Foo {}"),
            ("maps/MAP01.wad", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_pk3_with_gldefs() {
        let (_dir, path) = write_pk3(&[
            ("GLDEFS.txt", b"brightmap {}"),
            ("maps/MAP01.wad", &[]),
        ]);
        assert_eq!(detect_zdoom_required(&path), Some(true));
    }

    #[test]
    fn test_detect_pk3_vanilla_maps_only() {
        let wad = build_wad(&[("MAP01", &[]), ("THINGS", &[])]);
        let (_dir, path) = write_pk3(&[("maps/MAP01.wad", &wad)]);
        assert_eq!(detect_zdoom_required(&path), Some(false));
    }

    #[test]
    fn test_detect_pk3_nonexistent() {
        assert_eq!(detect_zdoom_required(Path::new("/nonexistent/test.pk3")), None);
    }

    // --- ZIP-wrapped WAD tests ---

    #[test]
    fn test_detect_zip_wrapped_zdoom_wad() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");

        let wad = build_wad(&[("DECORATE", b"actor Foo {}"), ("MAP01", &[])]);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("test.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&wad).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_zdoom_required(&zip_path), Some(true));
    }

    #[test]
    fn test_detect_zip_wrapped_vanilla_wad() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");

        let wad = build_wad(&[("MAP01", &[]), ("MAP02", &[])]);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("test.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&wad).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_zdoom_required(&zip_path), Some(false));
    }
}
