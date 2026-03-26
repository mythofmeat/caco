//! Save game management — find, backup, restore, and clean save files.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone, Utc};
use regex::Regex;
use zip::write::SimpleFileOptions;
use zip::ZipArchive;

use crate::config::get_backup_dir;
use crate::sourceports::ALL_SAVE_EXTENSIONS;
use crate::utils::sanitize_dirname;

/// Information about a save file.
#[derive(Debug, Clone)]
pub struct SaveFile {
    pub path: PathBuf,
    pub name: String,
    pub rel_path: String,
    pub size: u64,
    pub mtime_iso: String,
}

/// Information about a backup archive.
#[derive(Debug, Clone)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub name: String,
    pub wad_id: Option<i64>,
    pub size: u64,
    pub created_iso: String,
}

/// Find all save files in a WAD data directory.
///
/// Recursively scans for files matching known save extensions (.dsg, .zds).
pub fn find_save_files(data_dir: &Path) -> Vec<SaveFile> {
    if !data_dir.is_dir() {
        return Vec::new();
    }

    let mut saves = Vec::new();
    collect_save_files_recursive(data_dir, data_dir, &mut saves);
    saves.sort_by(|a, b| a.path.cmp(&b.path));
    saves
}

fn collect_save_files_recursive(base: &Path, dir: &Path, saves: &mut Vec<SaveFile>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_save_files_recursive(base, &path, saves);
        } else if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_lowercase()))
                .unwrap_or_default();
            if ALL_SAVE_EXTENSIONS.contains(&ext.as_str())
                && let Ok(meta) = path.metadata() {
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| {
                            let duration = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                            Utc.timestamp_opt(duration.as_secs() as i64, 0).single()
                        })
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default();

                    saves.push(SaveFile {
                        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                        rel_path: path
                            .strip_prefix(base)
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
}

/// Create a zip backup of a WAD's entire data directory.
///
/// Returns the path to the created backup file.
pub fn create_backup(wad_id: i64, title: &str, data_dir: &Path) -> crate::Result<PathBuf> {
    if !data_dir.is_dir() {
        return Err(crate::Error::FileNotFound(format!(
            "Data directory does not exist: {}",
            data_dir.display()
        )));
    }

    let backup_dir = get_backup_dir();
    fs::create_dir_all(&backup_dir)?;

    let timestamp = Local::now().format("%Y%m%d_%H%M%S_%f").to_string();
    let sanitized = sanitize_dirname(title);
    let backup_name = format!("{wad_id}_{sanitized}_{timestamp}.zip");
    let backup_path = backup_dir.join(&backup_name);

    let file = File::create(&backup_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut files = Vec::new();
    collect_all_files_recursive(data_dir, &mut files);
    files.sort();

    for file_path in &files {
        let rel = file_path
            .strip_prefix(data_dir)
            .unwrap_or(file_path)
            .to_string_lossy();
        zip.start_file(rel.as_ref(), options)
            .map_err(io::Error::other)?;
        let mut f = File::open(file_path)?;
        io::copy(&mut f, &mut zip)?;
    }

    zip.finish()
        .map_err(io::Error::other)?;

    Ok(backup_path)
}

fn collect_all_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_all_files_recursive(&path, files);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
}

/// Restore a backup zip into a WAD's data directory.
///
/// Creates the data directory if it doesn't exist.
/// Returns the number of files extracted.
pub fn restore_backup(backup_path: &Path, data_dir: &Path) -> crate::Result<usize> {
    if !backup_path.is_file() {
        return Err(crate::Error::FileNotFound(format!(
            "Backup file not found: {}",
            backup_path.display()
        )));
    }

    fs::create_dir_all(data_dir)?;

    let file = File::open(backup_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let count = archive
        .file_names()
        .filter(|name| !name.ends_with('/'))
        .count();

    // Extract all files
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let outpath = data_dir.join(enclosed);

        if entry.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&outpath)?;
            io::copy(&mut entry, &mut outfile)?;
        }
    }

    Ok(count)
}

/// List existing backups for a specific WAD.
///
/// Sorted by creation time (newest first).
pub fn list_backups(wad_id: i64) -> Vec<BackupInfo> {
    let backup_dir = get_backup_dir();
    if !backup_dir.is_dir() {
        return Vec::new();
    }

    let prefix = format!("{wad_id}_");
    let mut backups = Vec::new();

    if let Ok(entries) = fs::read_dir(&backup_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
                && path.extension().and_then(|e| e.to_str()) == Some("zip")
                && let Ok(meta) = path.metadata() {
                    let created = meta
                        .modified()
                        .ok()
                        .and_then(|t| {
                            let duration = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                            Utc.timestamp_opt(duration.as_secs() as i64, 0).single()
                        })
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default();

                    backups.push(BackupInfo {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        wad_id: Some(wad_id),
                        size: meta.len(),
                        created_iso: created,
                        path,
                    });
                }
        }
    }

    backups.sort_by(|a, b| b.created_iso.cmp(&a.created_iso));
    backups
}

/// List all existing backups across all WADs.
///
/// Sorted by creation time (newest first).
pub fn list_all_backups() -> Vec<BackupInfo> {
    let backup_dir = get_backup_dir();
    if !backup_dir.is_dir() {
        return Vec::new();
    }

    let id_re = Regex::new(r"^(\d+)_").unwrap();
    let mut backups = Vec::new();

    if let Ok(entries) = fs::read_dir(&backup_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("zip") {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let wad_id = id_re
                    .captures(&name)
                    .and_then(|c| c[1].parse::<i64>().ok());

                if let Ok(meta) = path.metadata() {
                    let created = meta
                        .modified()
                        .ok()
                        .and_then(|t| {
                            let duration = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                            Utc.timestamp_opt(duration.as_secs() as i64, 0).single()
                        })
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default();

                    backups.push(BackupInfo {
                        path,
                        name,
                        wad_id,
                        size: meta.len(),
                        created_iso: created,
                    });
                }
            }
        }
    }

    backups.sort_by(|a, b| b.created_iso.cmp(&a.created_iso));
    backups
}

/// Delete save files from a WAD data directory, keeping stats and configs.
///
/// Returns list of deleted file paths.
pub fn clean_save_files(data_dir: &Path) -> Vec<PathBuf> {
    let saves = find_save_files(data_dir);
    let mut deleted = Vec::new();
    for save in saves {
        if fs::remove_file(&save.path).is_ok() {
            deleted.push(save.path);
        }
    }
    deleted
}

/// Resolve a backup argument to a path.
///
/// If `backup_arg` is None, returns the most recent backup for the WAD.
/// If `backup_arg` is a filename, looks it up in the backup directory.
/// If `backup_arg` is an absolute path, returns it directly.
pub fn resolve_backup_path(wad_id: i64, backup_arg: Option<&str>) -> Option<PathBuf> {
    if let Some(arg) = backup_arg {
        let candidate = PathBuf::from(arg);
        if candidate.is_absolute() {
            return if candidate.is_file() {
                Some(candidate)
            } else {
                None
            };
        }

        let candidate = get_backup_dir().join(arg);
        if candidate.is_file() {
            return Some(candidate);
        }
        return None;
    }

    // No arg — use most recent
    let backups = list_backups(wad_id);
    backups.into_iter().next().map(|b| b.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_save_files_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_save_files(dir.path()).is_empty());
    }

    #[test]
    fn test_find_save_files_dsg() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("save1.dsg"), b"data").unwrap();
        fs::write(dir.path().join("save2.dsg"), b"data").unwrap();
        fs::write(dir.path().join("stats.txt"), b"data").unwrap(); // not a save

        let saves = find_save_files(dir.path());
        assert_eq!(saves.len(), 2);
        assert!(saves.iter().all(|s| s.name.ends_with(".dsg")));
    }

    #[test]
    fn test_find_save_files_recursive() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("deep.zds"), b"data").unwrap();

        let saves = find_save_files(dir.path());
        assert_eq!(saves.len(), 1);
        assert!(saves[0].rel_path.contains("subdir"));
    }

    #[test]
    fn test_backup_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        fs::create_dir(&data_dir).unwrap();
        fs::write(data_dir.join("save1.dsg"), b"save data 1").unwrap();
        fs::write(data_dir.join("stats.txt"), b"stats data").unwrap();

        let backup_path = create_backup(42, "Test WAD", &data_dir).unwrap();
        assert!(backup_path.exists());
        assert!(backup_path.to_string_lossy().contains("42_test-wad_"));

        // Restore to a new directory
        let restore_dir = dir.path().join("restored");
        let count = restore_backup(&backup_path, &restore_dir).unwrap();
        assert_eq!(count, 2);
        assert!(restore_dir.join("save1.dsg").exists());
        assert!(restore_dir.join("stats.txt").exists());
    }

    #[test]
    fn test_clean_save_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("save1.dsg"), b"data").unwrap();
        fs::write(dir.path().join("save2.zds"), b"data").unwrap();
        fs::write(dir.path().join("stats.txt"), b"stats").unwrap();

        let deleted = clean_save_files(dir.path());
        assert_eq!(deleted.len(), 2);
        assert!(!dir.path().join("save1.dsg").exists());
        assert!(dir.path().join("stats.txt").exists());
    }

    #[test]
    fn test_resolve_backup_nonexistent() {
        assert!(resolve_backup_path(999, None).is_none());
    }
}
