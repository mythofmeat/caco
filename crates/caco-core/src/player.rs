//! Sourceport launcher and playtime tracking.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use rusqlite::Connection;

use crate::complevel_detect;
use crate::config;
use crate::db;
use crate::demos;
use crate::iwad_detect;
use crate::sourceports;
use crate::wad_stats;

/// Result of a play session.
#[derive(Debug, Clone)]
pub struct PlayResult {
    pub duration: Option<i64>,
    pub exit_code: Option<i32>,
}

impl PlayResult {
    /// True if the sourceport exited with a non-zero code.
    pub fn crashed(&self) -> bool {
        matches!(self.exit_code, Some(code) if code != 0)
    }
}

/// Options for launching a WAD.
#[derive(Debug, Default)]
pub struct PlayOptions {
    pub sourceport: Option<String>,
    pub extra_args: Vec<String>,
    pub record: Option<RecordOption>,
    pub config_profile: Option<String>,
}

/// Demo recording option.
#[derive(Debug, Clone)]
pub enum RecordOption {
    /// Auto-generate a demo name.
    Auto,
    /// Use a specific name (without extension).
    Named(String),
}

/// Play a WAD with the specified sourceport.
///
/// Returns a `PlayResult` with duration and exit code.
pub fn play(conn: &Connection, wad_id: i64, opts: &PlayOptions) -> crate::Result<PlayResult> {
    let wad = db::get_wad(conn, wad_id, false)?
        .ok_or(crate::Error::WadNotFound(wad_id))?;

    // Get WAD file path (must already be cached/linked)
    let wad_path = wad.cached_path.as_deref().and_then(|p| {
        let path = PathBuf::from(p);
        if path.exists() { Some(path) } else { None }
    });

    let wad_path = match wad_path {
        Some(p) => p,
        None => {
            return Err(crate::Error::FileNotFound(format!(
                "No WAD file linked for '{}'. Download and link with: caco modify id:{} --link /path/to/wad",
                wad.title, wad_id
            )));
        }
    };

    // Determine sourceport (CLI > WAD-specific > global config)
    let port = opts
        .sourceport
        .as_deref()
        .or(wad.custom_sourceport.as_deref())
        .map(|s| s.to_string())
        .unwrap_or_else(config::get_default_sourceport);

    if port.is_empty() {
        return Err(crate::Error::Config(
            "No sourceport specified and no default configured".to_string(),
        ));
    }

    let port = config::resolve_sourceport(&port);
    let mut cmd = Command::new(&port);

    // Auto-detect IWAD if not explicitly set
    let mut custom_iwad = wad.custom_iwad.clone();
    if custom_iwad.is_none() && config::get_auto_detect_iwad()
        && let Some(detected) = iwad_detect::detect_iwad(&wad_path) {
            let update = db::WadUpdate::new()
                .set_text("custom_iwad", Some(detected.to_string()))?;
            db::update_wad(conn, wad_id, &update)?;
            custom_iwad = Some(detected.to_string());
        }

    // Auto-detect complevel if not explicitly set
    let mut complevel = wad.complevel;
    if complevel.is_none() && config::get_auto_detect_complevel() {
        // Try COMPLVL lump first (id24 signal)
        if let Some(cl) = iwad_detect::detect_complvl(&wad_path) {
            let update = db::WadUpdate::new()
                .set_int("complevel", Some(cl as i64))?;
            db::update_wad(conn, wad_id, &update)?;
            complevel = Some(cl);
        } else if let Some(cl) = complevel_detect::detect_complevel(&wad_path) {
            let update = db::WadUpdate::new()
                .set_int("complevel", Some(cl as i64))?;
            db::update_wad(conn, wad_id, &update)?;
            complevel = Some(cl);
        }
    }

    // Add IWAD
    let iwad_name = custom_iwad.clone().or_else(|| {
        let iwad = config::get_iwad();
        if iwad.is_empty() { None } else { Some(iwad) }
    });
    if let Some(ref iwad) = iwad_name {
        let db_resolved = db::resolve_iwad_from_db(conn, iwad, None);
        let resolved = config::resolve_iwad_path(iwad, db_resolved.as_deref());
        cmd.args(["-iwad", &resolved]);
    }

    // Add default sourceport args from global config
    let default_args = config::get_sourceport_args();
    if !default_args.is_empty() {
        cmd.args(&default_args);
    }

    // Inject complevel flag if set and not already present in args
    if let Some(cl) = complevel {
        let all_args: Vec<String> = std::iter::once(port.clone())
            .chain(default_args.iter().cloned())
            .chain(opts.extra_args.iter().cloned())
            .collect();
        if !all_args.iter().any(|a| a == "-complevel") {
            let cl_args = sourceports::get_complevel_args(&port, cl);
            if !cl_args.is_empty() {
                cmd.args(&cl_args);
            }
        }
    }

    // Add per-WAD custom args
    if let Some(ref custom_args) = wad.custom_args
        && let Ok(args) = serde_json::from_str::<Vec<String>>(custom_args) {
            cmd.args(&args);
        }

    // Inject managed config profile for dsda-family ports
    let profile_name = opts
        .config_profile
        .as_deref()
        .or(wad.custom_config.as_deref())
        .unwrap_or("default");
    let profile_path = config::get_profile_path(&port, profile_name);
    let config_args = sourceports::get_config_args(&port, &profile_path.to_string_lossy());
    if !config_args.is_empty() {
        if let Some(parent) = profile_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if !profile_path.exists() {
            let _ = std::fs::File::create(&profile_path);
        }
        cmd.args(&config_args);
    }

    // Inject per-WAD data directory args
    let mut wad_data_dir = None;
    if config::get_manage_data_dirs() {
        let data_dir = config::find_wad_data_dir(wad_id)
            .unwrap_or_else(|| config::get_wad_data_dir(wad_id, &wad.title));
        let _ = std::fs::create_dir_all(&data_dir);
        let iwad_for_data = iwad_name.as_deref();
        let data_args = sourceports::get_data_dir_args(
            &port,
            &data_dir.to_string_lossy(),
            iwad_for_data,
            Some(&wad_path.to_string_lossy()),
        );
        if !data_args.is_empty() {
            cmd.args(&data_args);
        }
        wad_data_dir = Some(data_dir);
    }

    // Handle demo recording
    let mut demo_path: Option<String> = None;
    if let Some(ref record) = opts.record {
        let data_dir = wad_data_dir.clone().unwrap_or_else(|| {
            config::find_wad_data_dir(wad_id)
                .unwrap_or_else(|| config::get_wad_data_dir(wad_id, &wad.title))
        });
        let demos_dir = demos::get_demos_dir(&data_dir);
        let _ = std::fs::create_dir_all(&demos_dir);

        let demo_name = match record {
            RecordOption::Named(name) => name.clone(),
            RecordOption::Auto => {
                let stem = wad_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&wad.title);
                demos::generate_demo_name(stem)
            }
        };

        let path = demos_dir.join(&demo_name).to_string_lossy().into_owned();
        cmd.args(["-record", &path]);
        demo_path = Some(path);
    }

    // Build -file list: id24 resources + companion WADs + main WAD
    let mut file_args: Vec<String> = get_id24_resource_args(conn, Some(&wad_path));
    let mut deh_args: Vec<String> = Vec::new();

    if let Some(ref companions) = wad.companion_files
        && let Ok(files) = serde_json::from_str::<Vec<String>>(companions) {
            let deh_extensions = [".deh", ".bex"];
            for comp in files {
                let ext = Path::new(&comp)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| format!(".{}", e.to_lowercase()))
                    .unwrap_or_default();
                if deh_extensions.contains(&ext.as_str()) {
                    if sourceports::uses_deh_flag(&port) {
                        deh_args.extend(["-deh".to_string(), comp]);
                    } else {
                        file_args.push(comp);
                    }
                } else {
                    file_args.push(comp);
                }
            }
        }

    // Add DEH args before -file
    if !deh_args.is_empty() {
        cmd.args(&deh_args);
    }

    // Add -file with id24 resources + companions + main WAD
    file_args.push(wad_path.to_string_lossy().into_owned());
    cmd.arg("-file");
    cmd.args(&file_args);

    // Add extra args from command line (highest priority)
    if !opts.extra_args.is_empty() {
        cmd.args(&opts.extra_args);
    }

    // Capture stats snapshot before play
    let stats_before = read_stats_snapshot(wad_id);

    // Launch sourceport
    cmd.stdin(std::process::Stdio::null());
    let mut child = cmd.spawn().map_err(|e| {
        crate::Error::FileNotFound(format!("Failed to launch sourceport '{}': {}", port, e))
    })?;

    // Start session tracking
    let session_id = db::start_session(conn, wad_id, Some(&port))?;

    let start = Instant::now();
    let status = child.wait()?;
    let _elapsed = start.elapsed().as_secs() as i64;

    // End session
    db::end_session(conn, session_id, None, status.code())?;

    // Auto-track stats
    let stats_after = auto_track_stats(conn, wad_id);

    // Attach before/after snapshots
    if stats_before.is_some() || stats_after.is_some() {
        let _ = db::update_session_stats(
            conn,
            session_id,
            stats_before.as_deref(),
            stats_after.as_deref(),
        );
    }

    // Link recorded demo
    if let Some(ref path) = demo_path {
        let lmp_path = if path.ends_with(".lmp") {
            path.clone()
        } else {
            format!("{path}.lmp")
        };
        if Path::new(&lmp_path).exists() {
            let _ = db::update_session_demo(conn, session_id, &lmp_path);
        }
    }

    // Build result
    let sessions = db::get_sessions(conn, wad_id)?;
    let duration = sessions.first().and_then(|s| s.duration_seconds);

    Ok(PlayResult {
        duration,
        exit_code: status.code(),
    })
}

/// Play an IWAD directly with no PWAD.
pub fn play_iwad(
    conn: &Connection,
    iwad_name: &str,
    sourceport: Option<&str>,
    extra_args: &[String],
    config_profile: Option<&str>,
) -> crate::Result<PlayResult> {
    // Resolve IWAD path
    let db_resolved = db::resolve_iwad_from_db(conn, iwad_name, None);
    let resolved = config::resolve_iwad_path(iwad_name, db_resolved.as_deref());
    if !Path::new(&resolved).exists() {
        return Err(crate::Error::FileNotFound(format!(
            "IWAD '{iwad_name}' not found. Register it with: caco import /path/to/iwad.wad"
        )));
    }

    // Determine sourceport
    let port = sourceport
        .map(|s| s.to_string())
        .unwrap_or_else(config::get_default_sourceport);
    if port.is_empty() {
        return Err(crate::Error::Config(
            "No sourceport specified and no default configured".to_string(),
        ));
    }

    let port = config::resolve_sourceport(&port);
    let mut cmd = Command::new(&port);
    cmd.args(["-iwad", &resolved]);

    // Add default sourceport args
    let default_args = config::get_sourceport_args();
    if !default_args.is_empty() {
        cmd.args(&default_args);
    }

    // Inject config profile
    let profile_name = config_profile.unwrap_or("default");
    let profile_path = config::get_profile_path(&port, profile_name);
    let config_args = sourceports::get_config_args(&port, &profile_path.to_string_lossy());
    if !config_args.is_empty() {
        if let Some(parent) = profile_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if !profile_path.exists() {
            let _ = std::fs::File::create(&profile_path);
        }
        cmd.args(&config_args);
    }

    if !extra_args.is_empty() {
        cmd.args(extra_args);
    }

    // Launch
    cmd.stdin(std::process::Stdio::null());
    let mut child = cmd.spawn().map_err(|e| {
        crate::Error::FileNotFound(format!("Failed to launch sourceport '{}': {}", port, e))
    })?;

    let start = Instant::now();
    let status = child.wait()?;
    let duration = start.elapsed().as_secs() as i64;

    Ok(PlayResult {
        duration: Some(duration),
        exit_code: status.code(),
    })
}

/// Format duration as human-readable string.
pub fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        let secs = seconds % 60;
        format!("{minutes}m {secs}s")
    } else {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        format!("{hours}h {minutes}m")
    }
}

// --- Internal helpers ---

/// Get id24 resource WAD paths to prepend to the -file list.
fn get_id24_resource_args(conn: &Connection, wad_path: Option<&Path>) -> Vec<String> {
    let mut file_args = Vec::new();

    // Check for COMPLVL lump directly (id24 signal)
    let has_complvl = wad_path
        .filter(|p| p.exists())
        .and_then(iwad_detect::detect_complvl)
        .is_some();

    if !has_complvl {
        return file_args;
    }

    // Load id24res.wad for any id24 WAD
    if let Ok(Some(id24res)) = db::get_id24(conn, "id24res")
        && Path::new(&id24res.path).exists() {
            file_args.push(id24res.path);
        }

    // Check if this is id1.wad (Legacy of Rust)
    let is_id1 = wad_path
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("id1"));

    if is_id1 {
        for name in &["id1-res", "id1-tex", "id1-weap", "id1-mus"] {
            if let Ok(Some(entry)) = db::get_id24(conn, name)
                && Path::new(&entry.path).exists() {
                    file_args.push(entry.path);
                }
        }
    }

    file_args
}

/// Search for a stats file in a WAD data directory.
fn find_stats_file(directory: &Path) -> Option<PathBuf> {
    for name in &["stats.txt", "levelstat.txt"] {
        if let Some(found) = find_file_recursive(directory, name) {
            return Some(found);
        }
    }
    None
}

/// Recursively search for a file by name.
fn find_file_recursive(dir: &Path, target: &str) -> Option<PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n == target)
            {
                return Some(path);
            }
            if path.is_dir()
                && let Some(found) = find_file_recursive(&path, target) {
                    return Some(found);
                }
        }
    }
    None
}

/// Read stats from the WAD's data dir, returning JSON string or None.
fn read_stats_snapshot(wad_id: i64) -> Option<String> {
    if !config::get_auto_stats() || !config::get_manage_data_dirs() {
        return None;
    }

    let data_dir = config::find_wad_data_dir(wad_id)?;
    if !data_dir.is_dir() {
        return None;
    }

    let stats_path = find_stats_file(&data_dir)?;
    let stats = wad_stats::parse_stats_file(&stats_path).ok()?;
    wad_stats::stats_to_json(&stats).ok()
}

/// Read stats and store on the WAD record.
fn auto_track_stats(conn: &Connection, wad_id: i64) -> Option<String> {
    let json_str = read_stats_snapshot(wad_id)?;
    let update = db::WadUpdate::new()
        .set_text("stats_snapshot", Some(json_str.clone()))
        .ok()?;
    let _ = db::update_wad(conn, wad_id, &update);
    Some(json_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_play_result_crashed() {
        assert!(PlayResult {
            duration: Some(60),
            exit_code: Some(1)
        }
        .crashed());
        assert!(!PlayResult {
            duration: Some(60),
            exit_code: Some(0)
        }
        .crashed());
        assert!(!PlayResult {
            duration: Some(60),
            exit_code: None
        }
        .crashed());
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn test_find_stats_file() {
        let dir = tempfile::tempdir().unwrap();

        // No stats file
        assert!(find_stats_file(dir.path()).is_none());

        // Create stats.txt
        std::fs::write(dir.path().join("stats.txt"), "1\n0\n").unwrap();
        let found = find_stats_file(dir.path()).unwrap();
        assert!(found.to_string_lossy().contains("stats.txt"));
    }

    #[test]
    fn test_find_stats_file_nested() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nyan_doom_data").join("doom2").join("test");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("stats.txt"), "1\n0\n").unwrap();

        let found = find_stats_file(dir.path()).unwrap();
        assert!(found.to_string_lossy().contains("stats.txt"));
    }
}
