//! Demo file management — find, clean, and name demo recordings.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;

use crate::utils::{sanitize_name, system_time_to_rfc3339};

/// Demo file extension.
pub const DEMO_EXTENSION: &str = ".lmp";

/// Information about a demo file.
#[derive(Debug, Clone)]
pub struct DemoFile {
    pub path: PathBuf,
    pub name: String,
    pub rel_path: String,
    pub size: u64,
    pub mtime_iso: String,
}

/// Return the demos subdirectory within a WAD's data directory.
pub fn get_demos_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("demos")
}

/// Find all demo files in a WAD's demos directory.
pub fn find_demo_files(data_dir: &Path) -> Vec<DemoFile> {
    let demos_dir = get_demos_dir(data_dir);
    if !demos_dir.is_dir() {
        return Vec::new();
    }

    let mut demos = Vec::new();
    if let Ok(entries) = fs::read_dir(&demos_dir) {
        let mut paths: Vec<_> = entries
            .flatten()
            .filter(|e| {
                let path = e.path();
                path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| format!(".{}", ext.to_lowercase()) == DEMO_EXTENSION)
                        .unwrap_or(false)
            })
            .collect();
        paths.sort_by_key(|e| e.file_name());

        for entry in paths {
            let path = entry.path();
            if let Ok(meta) = path.metadata() {
                let mtime = meta.modified()
                    .map(system_time_to_rfc3339)
                    .unwrap_or_default();

                demos.push(DemoFile {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    rel_path: path
                        .strip_prefix(data_dir)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                    size: meta.len(),
                    mtime_iso: mtime,
                    path,
                });
            }
        }
    }

    demos
}

/// Delete demo files from a WAD's demos directory.
///
/// Returns list of deleted file paths.
pub fn clean_demo_files(data_dir: &Path) -> Vec<PathBuf> {
    let demos = find_demo_files(data_dir);
    let mut deleted = Vec::new();
    for demo in demos {
        if fs::remove_file(&demo.path).is_ok() {
            deleted.push(demo.path);
        }
    }
    deleted
}

/// Generate a timestamped demo filename (without extension).
///
/// Sourceports append .lmp automatically when recording, so this returns
/// the base name only.
pub fn generate_demo_name(wad_stem: &str) -> String {
    let sanitized = sanitize_name(wad_stem, 48);
    let sanitized = if sanitized.is_empty() {
        "demo".to_string()
    } else {
        sanitized
    };
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    format!("{sanitized}_{timestamp}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_demos_dir() {
        let dir = get_demos_dir(Path::new("/data/42_scythe-2"));
        assert_eq!(dir, PathBuf::from("/data/42_scythe-2/demos"));
    }

    #[test]
    fn test_find_demo_files_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_demo_files(dir.path()).is_empty());
    }

    #[test]
    fn test_find_demo_files() {
        let dir = tempfile::tempdir().unwrap();
        let demos_dir = dir.path().join("demos");
        fs::create_dir(&demos_dir).unwrap();
        fs::write(demos_dir.join("demo1.lmp"), b"data").unwrap();
        fs::write(demos_dir.join("demo2.lmp"), b"data").unwrap();
        fs::write(demos_dir.join("other.txt"), b"data").unwrap();

        let demos = find_demo_files(dir.path());
        assert_eq!(demos.len(), 2);
        assert!(demos.iter().all(|d| d.name.ends_with(".lmp")));
    }

    #[test]
    fn test_clean_demo_files() {
        let dir = tempfile::tempdir().unwrap();
        let demos_dir = dir.path().join("demos");
        fs::create_dir(&demos_dir).unwrap();
        fs::write(demos_dir.join("demo1.lmp"), b"data").unwrap();
        fs::write(demos_dir.join("demo2.lmp"), b"data").unwrap();

        let deleted = clean_demo_files(dir.path());
        assert_eq!(deleted.len(), 2);
        assert!(find_demo_files(dir.path()).is_empty());
    }

    #[test]
    fn test_generate_demo_name() {
        let name = generate_demo_name("Scythe 2");
        assert!(name.starts_with("scythe-2_"));
        assert!(name.len() > 10); // has timestamp
    }

    #[test]
    fn test_generate_demo_name_empty() {
        let name = generate_demo_name("");
        assert!(name.starts_with("demo_"));
    }

    #[test]
    fn test_generate_demo_name_special_chars() {
        let name = generate_demo_name("!!!Test WAD!!!");
        assert!(name.starts_with("test-wad_"));
    }
}
