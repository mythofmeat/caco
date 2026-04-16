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
use crate::stats_watcher;
use crate::wad_stats;
use crate::zdoom_detect;

/// Parses a custom args string into a validated JSON array for DB storage.
/// Accepts either a JSON array (`["--fast", "--nomusic"]`) or space-separated flags.
/// Returns the normalised JSON string.
pub fn normalize_custom_args(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok("[]".to_string());
    }
    if trimmed.starts_with('[') {
        let args: Vec<String> =
            serde_json::from_str(trimmed).map_err(|e| format!("Invalid JSON args: {e}"))?;
        serde_json::to_string(&args).map_err(|e| e.to_string())
    } else {
        let args: Vec<String> = trimmed.split_whitespace().map(|s| s.to_string()).collect();
        serde_json::to_string(&args).map_err(|e| e.to_string())
    }
}

/// Whether auto-completion detection triggered after play.
#[derive(Debug, Clone, PartialEq)]
pub enum AutoCompleteResult {
    /// All required maps have been exited — playthrough was auto-completed.
    Completed,
    /// Not all maps exited yet.
    Incomplete { exited: usize, required: usize },
    /// Could not determine (no analysis or no stats).
    Unknown,
}

/// Result of a play session.
#[derive(Debug, Clone)]
pub struct PlayResult {
    pub duration: Option<i64>,
    pub exit_code: Option<i32>,
    pub auto_complete: AutoCompleteResult,
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
    /// Start a fresh playthrough (resets stats tracking for completed WADs).
    pub new_playthrough: bool,
}

/// Demo recording option.
#[derive(Debug, Clone)]
pub enum RecordOption {
    /// Auto-generate a demo name.
    Auto,
    /// Use a specific name (without extension).
    Named(String),
}

/// Start a new playthrough for a completed/abandoned WAD.
///
/// Creates a new playthrough record, sets status to in-progress,
/// and clears the live stats snapshot so progress tracks fresh.
/// The prior playthrough's stats are preserved on its own record.
pub fn start_new_playthrough(conn: &Connection, wad_id: i64) -> crate::Result<i64> {
    db::get_wad(conn, wad_id, false)?.ok_or(crate::Error::WadNotFound(wad_id))?;

    // Guard: must not already have an active playthrough
    if db::get_active_playthrough(conn, wad_id)?.is_some() {
        return Err(crate::Error::Config(
            "WAD already has an active playthrough".to_string(),
        ));
    }

    // Start new playthrough (sets status → in-progress)
    let pt_id = db::start_playthrough(conn, wad_id)?;

    // Clear live stats snapshot so auto-completion tracks fresh
    let update = db::WadUpdate::new().set_text("stats_snapshot", None)?;
    db::update_wad(conn, wad_id, &update)?;

    Ok(pt_id)
}

/// Play a WAD with the specified sourceport.
///
/// Returns a `PlayResult` with duration and exit code.
pub fn play(conn: &Connection, wad_id: i64, opts: &PlayOptions) -> crate::Result<PlayResult> {
    let wad = db::get_wad(conn, wad_id, false)?.ok_or(crate::Error::WadNotFound(wad_id))?;

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

    // Auto-detect zdoom_required if not already set
    let zdoom_required = match wad.zdoom_required {
        Some(v) => v != 0,
        None => {
            if let Some(detected) = zdoom_detect::detect_zdoom_required(&wad_path) {
                let update =
                    db::WadUpdate::new().set_int("zdoom_required", Some(i64::from(detected)))?;
                db::update_wad(conn, wad_id, &update)?;
                detected
            } else {
                false
            }
        }
    };

    // If zdoom required but current port isn't zdoom-family, force to zdoom sourceport
    let port = if zdoom_required {
        let is_zdoom_family = sourceports::family_name(&port).is_some_and(|name| name == "zdoom");
        if is_zdoom_family {
            port
        } else {
            let zdoom_port = config::get_zdoom_sourceport();
            eprintln!("WAD requires ZDoom-family sourceport, using {zdoom_port} instead of {port}");
            zdoom_port
        }
    } else {
        port
    };

    let port = config::resolve_sourceport(&port);
    let mut cmd = Command::new(&port);

    // Auto-detect IWAD if not explicitly set
    let mut custom_iwad = wad.custom_iwad.clone();
    if custom_iwad.is_none()
        && config::get_auto_detect_iwad()
        && let Some(detected) = iwad_detect::detect_iwad(&wad_path)
    {
        let update = db::WadUpdate::new().set_text("custom_iwad", Some(detected.to_string()))?;
        db::update_wad(conn, wad_id, &update)?;
        custom_iwad = Some(detected.to_string());
    }

    // Auto-detect complevel if not explicitly set
    let mut complevel = wad.complevel;
    if complevel.is_none() && config::get_auto_detect_complevel() {
        // Try COMPLVL lump first (id24 signal)
        if let Some(cl) = iwad_detect::detect_complvl(&wad_path) {
            let update = db::WadUpdate::new().set_int("complevel", Some(cl as i64))?;
            db::update_wad(conn, wad_id, &update)?;
            complevel = Some(cl);
        } else if let Some(cl) = complevel_detect::detect_complevel(&wad_path) {
            let update = db::WadUpdate::new().set_int("complevel", Some(cl as i64))?;
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

    // Add per-WAD custom args (tolerant read: silently skip malformed data already stored)
    if let Some(ref custom_args) = wad.custom_args
        && let Ok(args) = serde_json::from_str::<Vec<String>>(custom_args)
    {
        cmd.args(&args);
    }

    // Inject managed config profile
    let profile_name = opts
        .config_profile
        .as_deref()
        .or(wad.custom_config.as_deref())
        .unwrap_or("default");
    let profile_path = config::get_profile_path(&port, profile_name);
    let config_args = sourceports::get_config_args(&port, &profile_path.to_string_lossy());
    if !config_args.is_empty() {
        if let Some(parent) = profile_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("failed to create sourceport config dir {parent:?}: {e}");
        }
        if !profile_path.exists()
            && let Err(e) = std::fs::File::create(&profile_path)
        {
            tracing::warn!("failed to create sourceport config file {profile_path:?}: {e}");
        }
        cmd.args(&config_args);
    }

    // Inject per-WAD data directory args
    let mut wad_data_dir = None;
    if config::get_manage_data_dirs() {
        let data_dir = config::find_wad_data_dir(wad_id)
            .unwrap_or_else(|| config::get_wad_data_dir(wad_id, &wad.title));
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            tracing::warn!("failed to create WAD data dir {data_dir:?}: {e}");
        }
        let iwad_for_data = iwad_name.as_deref();
        // For dsda-family ports, ensure the nested save directory exists
        if let (Some(iw), Some(family)) = (iwad_for_data, sourceports::identify_family(&port))
            && family.name == "dsda"
        {
            let save_dir = sourceports::get_dsda_save_dir(
                &port,
                &data_dir.to_string_lossy(),
                iw,
                &wad_path.to_string_lossy(),
            );
            if let Err(e) = std::fs::create_dir_all(&save_dir) {
                tracing::warn!("failed to create dsda save dir {save_dir:?}: {e}");
            }
        }
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
        if let Err(e) = std::fs::create_dir_all(&demos_dir) {
            tracing::warn!("failed to create demos dir {demos_dir:?}: {e}");
        }

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

    // Build -file list: id24 resources + companion files + stats mod + main WAD
    let mut file_args: Vec<String> = get_id24_resource_args(conn, Some(&wad_path));
    let mut deh_args: Vec<String> = Vec::new();

    // Load enabled companions from DB
    if let Ok(companions) = db::get_companions_for_wad(conn, wad_id) {
        for comp in companions {
            if !comp.enabled {
                continue;
            }
            let comp_path = Path::new(&comp.path);
            if !comp_path.exists() {
                continue;
            }
            if crate::companion_service::is_deh_bex(comp_path) {
                if sourceports::uses_deh_flag(&port) {
                    deh_args.extend(["-deh".to_string(), comp.path]);
                } else {
                    file_args.push(comp.path);
                }
            } else {
                file_args.push(comp.path);
            }
        }
    }

    // For zdoom-family ports, inject the stats reporter PK3 mod
    let is_zdoom = sourceports::family_name(&port) == Some("zdoom");
    if is_zdoom
        && config::get_auto_stats()
        && let Ok(pk3_path) = stats_watcher::ensure_stats_mod()
    {
        file_args.push(pk3_path.to_string_lossy().into_owned());
    }

    // Add DEH args before -file
    if !deh_args.is_empty() {
        cmd.args(&deh_args);
    }

    // Add -file with id24 resources + companions + stats mod + main WAD
    file_args.push(wad_path.to_string_lossy().into_owned());
    cmd.arg("-file");
    cmd.args(&file_args);

    // Add extra args from command line (highest priority)
    if !opts.extra_args.is_empty() {
        cmd.args(&opts.extra_args);
    }

    // For zdoom-family ports, set up logfile for stats collection
    if is_zdoom
        && config::get_auto_stats()
        && let Some(ref data_dir) = wad_data_dir
    {
        let log_path = data_dir.join(stats_watcher::LOG_FILENAME);
        cmd.args(["+logfile", &log_path.to_string_lossy()]);
    }

    // Handle --new-playthrough: start fresh before launching
    if opts.new_playthrough {
        start_new_playthrough(conn, wad_id)?;
    }

    // Capture stats snapshot before play
    let stats_before = read_stats_snapshot(wad_id);

    // Launch sourceport
    cmd.stdin(std::process::Stdio::null());
    let mut child = cmd.spawn().map_err(|e| {
        crate::Error::FileNotFound(format!("Failed to launch sourceport '{}': {}", port, e))
    })?;

    // Ensure a playthrough exists (sets status → in-progress on first play), start
    // a session, and link the session to the playthrough — all atomically so we
    // never persist an unlinked session.
    let session_id = db::with_transaction(conn, |tx| {
        let playthrough_id = db::ensure_playthrough(tx, wad_id)?;
        let session_id = db::start_session(tx, wad_id, Some(&port))?;
        tx.execute(
            "UPDATE sessions SET playthrough_id = ?1 WHERE id = ?2",
            rusqlite::params![playthrough_id, session_id],
        )?;
        Ok(session_id)
    })?;

    let start = Instant::now();
    let status = child.wait()?;
    let elapsed = start.elapsed().as_secs() as i64;

    // End session
    db::end_session(conn, session_id, None, status.code())?;

    // For zdoom-family ports, parse the log and write levelstat.txt
    if is_zdoom
        && config::get_auto_stats()
        && let Some(ref data_dir) = wad_data_dir
    {
        stats_watcher::collect_zdoom_stats(data_dir);
    }

    // Auto-track stats
    let stats_after = auto_track_stats(conn, wad_id);

    // Attach before/after snapshots
    if stats_before.is_some() || stats_after.is_some() {
        db::update_session_stats(
            conn,
            session_id,
            stats_before.as_deref(),
            stats_after.as_deref(),
        )?;
    }

    // Link recorded demo
    if let Some(ref path) = demo_path {
        let lmp_path = if path.ends_with(".lmp") {
            path.clone()
        } else {
            format!("{path}.lmp")
        };
        if Path::new(&lmp_path).exists() {
            db::update_session_demo(conn, session_id, &lmp_path)?;
        }
    }

    // Auto-completion detection: check if all required maps have been beaten
    let auto_complete = check_auto_completion(conn, wad_id, &wad_path, stats_after.as_deref());

    Ok(PlayResult {
        duration: Some(elapsed),
        exit_code: status.code(),
        auto_complete,
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
        if let Some(parent) = profile_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("failed to create sourceport config dir {parent:?}: {e}");
        }
        if !profile_path.exists()
            && let Err(e) = std::fs::File::create(&profile_path)
        {
            tracing::warn!("failed to create sourceport config file {profile_path:?}: {e}");
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
        auto_complete: AutoCompleteResult::Unknown,
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
        && Path::new(&id24res.path).exists()
    {
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
                && Path::new(&entry.path).exists()
            {
                file_args.push(entry.path);
            }
        }
    }

    file_args
}

/// Find all stats files (stats.txt / levelstat.txt) in a WAD data directory.
///
/// dsda-family sourceports can create multiple nested directories under
/// `-data` (e.g. when the IWAD or sourceport changes), so multiple stats
/// files may coexist.  Returns all of them sorted by path.
fn find_all_stats_files(directory: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    collect_stats_files_recursive(directory, &mut results);
    results.sort();
    results
}

fn collect_stats_files_recursive(dir: &Path, results: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && (name == "stats.txt" || name == "levelstat.txt")
            {
                results.push(path);
            }
        } else if path.is_dir() {
            collect_stats_files_recursive(&path, results);
        }
    }
}

/// Read stats from the WAD's data dir, returning JSON string or None.
///
/// When multiple stats files exist (e.g. from IWAD/sourceport changes),
/// merges them keeping the best data per map.
fn read_stats_snapshot(wad_id: i64) -> Option<String> {
    if !config::get_auto_stats() || !config::get_manage_data_dirs() {
        return None;
    }

    let data_dir = config::find_wad_data_dir(wad_id)?;
    if !data_dir.is_dir() {
        return None;
    }

    let stats_files = find_all_stats_files(&data_dir);
    if stats_files.is_empty() {
        return None;
    }

    let parsed: Vec<wad_stats::WadStats> = stats_files
        .iter()
        .filter_map(|p| wad_stats::parse_stats_file(p).ok())
        .collect();

    if parsed.is_empty() {
        return None;
    }

    let merged = wad_stats::merge_stats(&parsed);
    wad_stats::stats_to_json(&merged).ok()
}

/// Read stats and store on the WAD record.
fn auto_track_stats(conn: &Connection, wad_id: i64) -> Option<String> {
    let json_str = read_stats_snapshot(wad_id)?;
    let update = db::WadUpdate::new()
        .set_text("stats_snapshot", Some(json_str.clone()))
        .ok()?;
    if let Err(e) = db::update_wad(conn, wad_id, &update) {
        tracing::warn!("failed to persist stats snapshot for wad {wad_id}: {e}");
    }
    Some(json_str)
}

/// Check if a WAD has been completed based on map exit stats vs WAD analysis.
///
/// On first play, analyzes the WAD file and caches the result. After each
/// session, compares the analysis against cumulative stats to detect completion.
/// If all required maps have been exited, auto-completes the active playthrough.
fn check_auto_completion(
    conn: &Connection,
    wad_id: i64,
    wad_path: &Path,
    stats_json: Option<&str>,
) -> AutoCompleteResult {
    use crate::completion_detect::{self, CompletionVerdict};
    use crate::wad_analysis;

    // Only check WADs that are currently being played
    let wad = match db::get_wad(conn, wad_id, false) {
        Ok(Some(w)) if w.status == "in-progress" => w,
        _ => return AutoCompleteResult::Unknown,
    };

    // Get or create WAD analysis
    let analysis = match db::get_analysis(conn, wad_id) {
        Ok(Some(a)) => a,
        Ok(None) => {
            // First time: analyze the file (PK3 or WAD)
            let is_pk3 = wad_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("pk3"));

            let analysis = if is_pk3 {
                wad_analysis::analyze_pk3(wad_path)
            } else {
                let wad_data = match crate::utils::load_wad_data(wad_path) {
                    Some(d) => d,
                    None => return AutoCompleteResult::Unknown,
                };
                wad_analysis::analyze_wad(&wad_data)
            };

            let analysis = match analysis {
                Some(a) => a,
                None => return AutoCompleteResult::Unknown,
            };
            if let Err(e) = db::save_analysis(conn, wad_id, &analysis) {
                tracing::warn!("failed to save wad analysis for wad {wad_id}: {e}");
            }
            analysis
        }
        Err(_) => return AutoCompleteResult::Unknown,
    };

    // Get current stats (prefer fresh stats_json, fall back to DB)
    let stats_str = stats_json.map(|s| s.to_string()).or(wad.stats_snapshot);
    let stats_str = match stats_str {
        Some(s) => s,
        None => return AutoCompleteResult::Unknown,
    };
    let stats: wad_stats::WadStats = match serde_json::from_str(&stats_str) {
        Ok(s) => s,
        Err(_) => return AutoCompleteResult::Unknown,
    };

    // Run completion check
    match completion_detect::check_completion(&analysis, &stats) {
        CompletionVerdict::Complete => {
            // Auto-complete the active playthrough
            if let Ok(Some(pt)) = db::get_active_playthrough(conn, wad_id)
                && let Err(e) = db::complete_playthrough(conn, pt.id, Some(&stats_str), None)
            {
                tracing::warn!("failed to auto-complete playthrough {}: {e}", pt.id);
            }
            AutoCompleteResult::Completed
        }
        CompletionVerdict::Incomplete { exited, required } => {
            AutoCompleteResult::Incomplete { exited, required }
        }
        CompletionVerdict::NoAnalysis => AutoCompleteResult::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_play_result_crashed() {
        assert!(
            PlayResult {
                duration: Some(60),
                exit_code: Some(1),
                auto_complete: AutoCompleteResult::Unknown,
            }
            .crashed()
        );
        assert!(
            !PlayResult {
                duration: Some(60),
                exit_code: Some(0),
                auto_complete: AutoCompleteResult::Unknown,
            }
            .crashed()
        );
        assert!(
            !PlayResult {
                duration: Some(60),
                exit_code: None,
                auto_complete: AutoCompleteResult::Unknown,
            }
            .crashed()
        );
    }

    #[test]
    fn test_normalize_custom_args_json_array() {
        let result = normalize_custom_args(r#"["--fast", "--nomusic"]"#).unwrap();
        assert_eq!(result, r#"["--fast","--nomusic"]"#);
    }

    #[test]
    fn test_normalize_custom_args_space_separated() {
        let result = normalize_custom_args("--fast --nomusic").unwrap();
        assert_eq!(result, r#"["--fast","--nomusic"]"#);
    }

    #[test]
    fn test_normalize_custom_args_empty() {
        assert_eq!(normalize_custom_args("").unwrap(), "[]");
        assert_eq!(normalize_custom_args("  ").unwrap(), "[]");
    }

    #[test]
    fn test_normalize_custom_args_malformed_json() {
        assert!(normalize_custom_args("[bad").is_err());
        assert!(normalize_custom_args(r#"["ok", 42]"#).is_err());
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn test_find_all_stats_files_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_all_stats_files(dir.path()).is_empty());
    }

    #[test]
    fn test_find_all_stats_files_single() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("stats.txt"), "1\n0\n").unwrap();
        let found = find_all_stats_files(dir.path());
        assert_eq!(found.len(), 1);
        assert!(found[0].to_string_lossy().contains("stats.txt"));
    }

    #[test]
    fn test_find_all_stats_files_nested() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nyan_doom_data").join("doom2").join("test");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("stats.txt"), "1\n0\n").unwrap();

        let found = find_all_stats_files(dir.path());
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_find_all_stats_files_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let dir_a = dir
            .path()
            .join("nyan_doom_data")
            .join("tnt")
            .join("100_tnt2")
            .join("tnt2bmus");
        let dir_b = dir
            .path()
            .join("nyan_doom_data")
            .join("tnt")
            .join("tnt2_1_2")
            .join("tnt2bmus");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        std::fs::write(dir_a.join("stats.txt"), "1\n0\n").unwrap();
        std::fs::write(dir_b.join("stats.txt"), "1\n0\n").unwrap();

        let found = find_all_stats_files(dir.path());
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn test_new_playthrough_clears_stats_snapshot() {
        use crate::db::SourceType;
        use crate::db::{self, init_db, open_memory};

        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        // Create a WAD and complete a playthrough
        let wad_id = db::add_wad(&conn, &db::NewWad::new("Test WAD", SourceType::Local)).unwrap();
        let pt_id = db::start_playthrough(&conn, wad_id).unwrap();

        // Set a stats snapshot on the WAD
        let update = db::WadUpdate::new()
            .set_text("stats_snapshot", Some(r#"{"maps":{}}"#.to_string()))
            .unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        // Complete the playthrough
        db::complete_playthrough(&conn, pt_id, Some(r#"{"maps":{}}"#), None).unwrap();

        // Verify WAD is completed with a snapshot
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, "completed");
        assert!(wad.stats_snapshot.is_some());

        // Start a new playthrough (simulating --new-playthrough)
        start_new_playthrough(&conn, wad_id).unwrap();

        // Stats snapshot should be cleared
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, "in-progress");
        assert!(wad.stats_snapshot.is_none());

        // A new active playthrough should exist
        let active = db::get_active_playthrough(&conn, wad_id).unwrap();
        assert!(active.is_some());
        assert_ne!(active.unwrap().id, pt_id);
    }

    #[test]
    fn test_new_playthrough_rejects_active() {
        use crate::db::SourceType;
        use crate::db::{self, init_db, open_memory};

        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();

        let wad_id = db::add_wad(&conn, &db::NewWad::new("Test WAD", SourceType::Local)).unwrap();
        db::start_playthrough(&conn, wad_id).unwrap();

        // Should fail — already has an active playthrough
        let result = start_new_playthrough(&conn, wad_id);
        assert!(result.is_err());
    }
}
