//! Sourceport launcher and playtime tracking.

use std::collections::HashMap;
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

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn sourceport_is_family(sourceport: &str, family: &str) -> bool {
    sourceports::family_name(sourceport).is_some_and(|name| name == family)
}

fn choose_family_sourceport(
    required_family: &str,
    default_sourceport: &str,
    zdoom_sourceport: &str,
    sourceport_preferences: &HashMap<String, String>,
    installed_sourceports: &[(String, String)],
) -> Option<String> {
    let family = sourceports::FAMILIES
        .iter()
        .find(|family| family.name == required_family)?;

    if let Some(preferred) = sourceport_preferences
        .get(required_family)
        .map(String::as_str)
        .filter(|preferred| !preferred.trim().is_empty())
        && sourceport_is_family(preferred, required_family)
    {
        return Some(preferred.to_string());
    }

    if required_family == "zdoom" && !zdoom_sourceport.trim().is_empty() {
        return Some(zdoom_sourceport.to_string());
    }

    if !default_sourceport.trim().is_empty()
        && sourceport_is_family(default_sourceport, required_family)
    {
        return Some(default_sourceport.to_string());
    }

    if let Some((exe, _)) = installed_sourceports
        .iter()
        .find(|(_, family)| family == required_family)
    {
        return Some(exe.clone());
    }

    family.executables.first().map(|exe| (*exe).to_string())
}

fn select_sourceport(
    cli_sourceport: Option<&str>,
    custom_sourceport: Option<&str>,
    required_sourceport_family: Option<&str>,
    default_sourceport: &str,
    zdoom_sourceport: &str,
    sourceport_preferences: &HashMap<String, String>,
    installed_sourceports: &[(String, String)],
) -> String {
    if let Some(port) = non_empty(cli_sourceport) {
        return port.to_string();
    }
    if let Some(port) = non_empty(custom_sourceport) {
        return port.to_string();
    }
    if let Some(family) = non_empty(required_sourceport_family)
        && let Some(port) = choose_family_sourceport(
            family,
            default_sourceport,
            zdoom_sourceport,
            sourceport_preferences,
            installed_sourceports,
        )
    {
        return port;
    }
    default_sourceport.to_string()
}

fn detect_and_persist_zdoom_required_if_missing(
    conn: &Connection,
    wad_id: i64,
    persisted_zdoom_required: Option<i32>,
    wad_path: &Path,
) -> crate::Result<()> {
    if persisted_zdoom_required.is_none()
        && let Some(detected) = zdoom_detect::detect_zdoom_required(wad_path)
    {
        let update = db::WadUpdate::new().set_int("zdoom_required", Some(i64::from(detected)));
        db::update_wad(conn, wad_id, &update)?;
    };
    Ok(())
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
    let update = db::WadUpdate::new().set_text("stats_snapshot", None);
    db::update_wad(conn, wad_id, &update)?;

    // Clearing the DB snapshot alone is not enough: every stats read path
    // prefers the on-disk stats files, so reconcile_stats would absorb the
    // prior playthrough's stats.txt right back and instantly auto-complete
    // the new playthrough. Archive the disk files too; the prior
    // playthrough's final stats already live on its own DB record.
    if let Some(data_dir) = config::find_wad_data_dir(wad_id) {
        archive_stats_files(&data_dir);
    }

    Ok(pt_id)
}

/// Rename all stats files under `data_dir` so the stats reader no longer
/// sees them (`stats.txt` → `stats.txt.archived`, numbered on collision).
/// The sourceport then starts a fresh stats file on the next session.
fn archive_stats_files(data_dir: &Path) {
    for path in find_all_stats_files(data_dir) {
        let base = format!("{}.archived", path.to_string_lossy());
        let mut target = PathBuf::from(&base);
        let mut n = 1;
        while target.exists() {
            n += 1;
            target = PathBuf::from(format!("{base}{n}"));
        }
        if let Err(e) = std::fs::rename(&path, &target) {
            tracing::warn!("failed to archive stats file {path:?}: {e}");
        }
    }
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

    // Auto-detect zdoom_required if not already set. This remains metadata only:
    // the visible compatibility family field is the launch constraint, so a
    // blank family means "use the default sourceport".
    detect_and_persist_zdoom_required_if_missing(conn, wad_id, wad.zdoom_required, &wad_path)?;

    // Determine sourceport (CLI > user override > compatibility family > global default).
    let cfg = config::load_config();
    let zdoom_sourceport = config::get_zdoom_sourceport();
    let installed_sourceports: Vec<(String, String)> = sourceports::detect_sourceports()
        .into_iter()
        .map(|(exe, _path, family)| (exe.to_string(), family.to_string()))
        .collect();
    let port = select_sourceport(
        opts.sourceport.as_deref(),
        wad.custom_sourceport.as_deref(),
        wad.required_sourceport_family.as_deref(),
        &cfg.sourceport,
        &zdoom_sourceport,
        &cfg.sourceport_preferences,
        &installed_sourceports,
    );

    if port.is_empty() {
        return Err(crate::Error::Config(
            "No sourceport specified and no default configured".to_string(),
        ));
    }

    let port = config::resolve_sourceport(&port);
    let mut cmd = Command::new(&port);

    // Auto-detect IWAD if not explicitly set
    let mut custom_iwad = wad.custom_iwad.clone();
    if custom_iwad.is_none()
        && config::get_auto_detect_iwad()
        && let Some(detected) = iwad_detect::detect_iwad(&wad_path)
    {
        let update = db::WadUpdate::new().set_text("custom_iwad", Some(detected.to_string()));
        db::update_wad(conn, wad_id, &update)?;
        custom_iwad = Some(detected.to_string());
    }

    // Auto-detect complevel if not explicitly set
    let mut complevel = wad.complevel;
    if complevel.is_none() && config::get_auto_detect_complevel() {
        // Try COMPLVL lump first (id24 signal)
        if let Some(cl) = iwad_detect::detect_complvl(&wad_path) {
            let update = db::WadUpdate::new().set_int("complevel", Some(cl as i64));
            db::update_wad(conn, wad_id, &update)?;
            complevel = Some(cl);
        } else if let Some(cl) = complevel_detect::detect_complevel(&wad_path) {
            let update = db::WadUpdate::new().set_int("complevel", Some(cl as i64));
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
    let is_helion = sourceports::family_name(&port) == Some("helion");
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
        // Avoid parsing stale lines if the sourceport appends to an existing log.
        let _ = std::fs::remove_file(&log_path);
        cmd.args(["+logfile", &log_path.to_string_lossy()]);
    }

    // For helion, enable the native global levelstat file
    if is_helion && config::get_auto_stats() {
        cmd.arg("-levelstat");
        // Helion clears the file itself at launch, but remove it up front so
        // a crash before init can't leave stale exits to be absorbed later.
        if let Some(path) = stats_watcher::helion_levelstat_path() {
            let _ = std::fs::remove_file(&path);
        }
    }

    // Handle --new-playthrough: start fresh before launching
    if opts.new_playthrough {
        start_new_playthrough(conn, wad_id)?;
    }

    // Absorb any on-disk stats progress the DB doesn't yet know about
    // (e.g. from an orphaned session where caco exited before the
    // sourceport did, or a manual sourceport launch). Must run before
    // stats_before is captured so this session's delta is measured
    // against the reconciled baseline rather than stale DB state.
    reconcile_stats(conn, wad_id);

    // Capture stats snapshot before play. Prefer disk, but fall back to the
    // DB snapshot when legacy/imported progress has not been materialized as a
    // sourceport stats file yet.
    let stats_before = read_session_stats_before(conn, wad_id);

    // Launch sourceport
    let session_start = std::time::SystemTime::now();
    cmd.stdin(std::process::Stdio::null());
    let mut child = cmd.spawn().map_err(|e| {
        crate::Error::FileNotFound(format!("Failed to launch sourceport '{}': {}", port, e))
    })?;

    // Start a session and link it to the active playthrough if one already
    // exists. We deliberately do NOT eagerly create a playthrough here — a
    // new one is only created post-play if this session actually produced
    // level progress. This prevents an unplayed WAD from being marked
    // in-progress just because the user launched-and-exited.
    let session_id = db::with_transaction(conn, |tx| {
        let session_id = db::start_session(tx, wad_id, Some(&port))?;
        if let Some(pt) = db::get_active_playthrough(tx, wad_id)? {
            tx.execute(
                "UPDATE sessions SET playthrough_id = ?1 WHERE id = ?2",
                rusqlite::params![pt.id, session_id],
            )?;
        }
        Ok(session_id)
    })?;

    let start = Instant::now();
    let status = child.wait()?;
    let elapsed = start.elapsed().as_secs() as i64;

    // End session
    db::end_session(conn, session_id, None, status.code())?;

    // For zdoom-family ports, parse the log and write managed stats.txt
    if is_zdoom
        && config::get_auto_stats()
        && let Some(ref data_dir) = wad_data_dir
    {
        stats_watcher::collect_zdoom_stats(data_dir);
    }

    // For helion, consume the global levelstat file into managed stats.txt
    if is_helion
        && config::get_auto_stats()
        && let Some(ref data_dir) = wad_data_dir
    {
        stats_watcher::collect_helion_stats(data_dir, Some(session_start));
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

    // If this session produced actual level progress, reconcile the WAD's
    // status. If a new playthrough is created here, link this session to it.
    if session_made_progress(stats_before.as_deref(), stats_after.as_deref())
        && let Some(pt_id) = ensure_in_progress(conn, wad_id)?
    {
        conn.execute(
            "UPDATE sessions SET playthrough_id = ?1 WHERE id = ?2",
            rusqlite::params![pt_id, session_id],
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

fn read_session_stats_before(conn: &Connection, wad_id: i64) -> Option<String> {
    read_stats_snapshot(wad_id).or_else(|| {
        db::get_wad(conn, wad_id, false)
            .ok()
            .flatten()
            .and_then(|wad| wad.stats_snapshot)
    })
}

/// Whether a play session produced measurable level progress.
///
/// Used to decide whether to auto-create a playthrough (flipping WAD status
/// from `unplayed` → `in-progress`). Delegates to [`wad_stats::compute_stats_delta`]
/// so this predicate stays consistent with the rest of the session-analysis
/// pipeline — a session counts as progress iff at least one map was actually
/// exited this session (stats.txt `total_exits` increased or appeared > 0
/// on a new entry, or a levelstat.txt line exists — every levelstat line is
/// written on exit). A bare byte-level diff on the serialised JSON is not
/// enough: the stats file can be touched without an exit.
fn session_made_progress(stats_before: Option<&str>, stats_after: Option<&str>) -> bool {
    let Some(after_json) = stats_after else {
        return false;
    };
    let Ok(after) = wad_stats::stats_from_json(after_json) else {
        return false;
    };
    let before = stats_before.and_then(|s| wad_stats::stats_from_json(s).ok());
    !wad_stats::compute_stats_delta(before.as_ref(), &after)
        .maps_played
        .is_empty()
}

/// Read stats and store on the WAD record.
fn auto_track_stats(conn: &Connection, wad_id: i64) -> Option<String> {
    let json_str = read_stats_snapshot(wad_id)?;
    let update = db::WadUpdate::new().set_text("stats_snapshot", Some(json_str.clone()));
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

    // Only check WADs that are currently being played
    let wad = match db::get_wad(conn, wad_id, false) {
        Ok(Some(w)) if w.status == db::Status::InProgress => w,
        _ => return AutoCompleteResult::Unknown,
    };

    let Some(analysis) = db::ensure_fresh_analysis(conn, wad_id, wad_path) else {
        return AutoCompleteResult::Unknown;
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

/// Promote a WAD to `in-progress`, starting a playthrough if none is active.
///
/// Returns `Some(pt_id)` when a new playthrough was created, so the caller
/// can attach the current session to it. Returns `None` when a playthrough
/// already existed (status is also patched if it drifted to a non-active
/// state while a playthrough was still open).
fn ensure_in_progress(conn: &Connection, wad_id: i64) -> crate::Result<Option<i64>> {
    match db::get_active_playthrough(conn, wad_id)? {
        None => {
            let pt_id = db::start_playthrough(conn, wad_id)?;
            Ok(Some(pt_id))
        }
        Some(_) => {
            if let Some(w) = db::get_wad(conn, wad_id, false)?
                && w.status != db::Status::InProgress
                && w.status != db::Status::Completed
            {
                let now = chrono::Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE wads SET status = 'in-progress', updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, wad_id],
                )?;
            }
            Ok(None)
        }
    }
}

/// Sync on-disk stats back into DB state.
///
/// Reads the sourceport's current `stats.txt` for this WAD. If it shows
/// progress beyond the DB snapshot, persists the fresh snapshot, promotes
/// the WAD to in-progress (starting a playthrough if needed), and runs
/// auto-completion.
///
/// Called at the start of [`play`] to catch orphaned on-disk state (sessions
/// where caco exited before the sourceport did, or sourceport launches
/// outside caco entirely). Safe to call opportunistically from read paths
/// — it's a no-op when disk and DB already agree.
pub fn reconcile_stats(conn: &Connection, wad_id: i64) -> AutoCompleteResult {
    let wad = match db::get_wad(conn, wad_id, false) {
        Ok(Some(w)) => w,
        _ => return AutoCompleteResult::Unknown,
    };

    let disk_json = match read_stats_snapshot(wad_id) {
        Some(s) => s,
        None => return AutoCompleteResult::Unknown,
    };

    let wad_path = wad.cached_path.as_deref().and_then(|p| {
        let path = PathBuf::from(p);
        if path.exists() { Some(path) } else { None }
    });

    reconcile_stats_with(conn, wad_id, &wad, &disk_json, wad_path.as_deref())
}

/// Inner logic for [`reconcile_stats`] without filesystem dependencies.
///
/// If the on-disk snapshot shows any played map and the WAD's status hasn't
/// caught up (still `unplayed`), the WAD is promoted to `in-progress` and
/// auto-completion is evaluated. The DB snapshot is also resynced when it
/// drifts from disk, regardless of progress, so later calls don't re-fire.
fn reconcile_stats_with(
    conn: &Connection,
    wad_id: i64,
    wad: &db::WadRecord,
    disk_json: &str,
    wad_path: Option<&Path>,
) -> AutoCompleteResult {
    let snapshot_changed = wad.stats_snapshot.as_deref() != Some(disk_json);

    let disk_has_exits = match wad_stats::stats_from_json(disk_json) {
        Ok(s) => s
            .maps
            .iter()
            .any(|m| m.total_exits > 0 || m.time_secs >= 0.0),
        Err(_) => false,
    };

    let needs_promotion = disk_has_exits && wad.status == db::Status::Unplayed;

    // Quick out: nothing to promote and the stats snapshot is already in
    // sync. Still run auto-completion for active WADs so an analysis-version
    // bump can repair a stale required-map set without needing another exit.
    if !snapshot_changed && !needs_promotion {
        if wad.status == db::Status::InProgress
            && let Some(path) = wad_path
        {
            return check_auto_completion(conn, wad_id, path, Some(disk_json));
        }
        return AutoCompleteResult::Unknown;
    }

    if snapshot_changed {
        let update = db::WadUpdate::new().set_text("stats_snapshot", Some(disk_json.to_string()));
        if let Err(e) = db::update_wad(conn, wad_id, &update) {
            tracing::warn!("reconcile: failed to persist stats snapshot for wad {wad_id}: {e}");
            return AutoCompleteResult::Unknown;
        }
    }

    // Promote unplayed → in-progress whenever disk shows the sourceport has
    // actually exited a map. This catches orphan sessions even if the DB
    // snapshot was already resynced by some other path.
    if needs_promotion && let Err(e) = ensure_in_progress(conn, wad_id) {
        tracing::warn!("reconcile: failed to promote wad {wad_id} to in-progress: {e}");
    }

    // check_auto_completion needs the WAD file to cache analysis on first
    // call. Without a cached file we still detect completion if analysis
    // was previously saved, otherwise it returns Unknown.
    match wad_path {
        Some(path) => check_auto_completion(conn, wad_id, path, Some(disk_json)),
        None => AutoCompleteResult::Unknown,
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
    fn test_select_sourceport_default_satisfies_required_family() {
        let prefs = HashMap::new();
        let port = select_sourceport(None, None, Some("dsda"), "nyan-doom", "gzdoom", &prefs, &[]);
        assert_eq!(port, "nyan-doom");
    }

    #[test]
    fn test_select_sourceport_custom_beats_required_family() {
        let prefs = HashMap::new();
        let port = select_sourceport(
            None,
            Some("dsda-doom"),
            Some("zdoom"),
            "nyan-doom",
            "gzdoom",
            &prefs,
            &[],
        );
        assert_eq!(port, "dsda-doom");
    }

    #[test]
    fn test_select_sourceport_cli_beats_everything() {
        let prefs = HashMap::new();
        let port = select_sourceport(
            Some("woof"),
            Some("dsda-doom"),
            Some("zdoom"),
            "nyan-doom",
            "gzdoom",
            &prefs,
            &[],
        );
        assert_eq!(port, "woof");
    }

    #[test]
    fn test_select_sourceport_zdoom_family_uses_configured_zdoom_port() {
        let prefs = HashMap::new();
        let port = select_sourceport(
            None,
            None,
            Some("zdoom"),
            "nyan-doom",
            "uzdoom",
            &prefs,
            &[],
        );
        assert_eq!(port, "uzdoom");
    }

    #[test]
    fn test_select_sourceport_zdoom_port_beats_same_family_default() {
        let prefs = HashMap::new();
        let port = select_sourceport(None, None, Some("zdoom"), "gzdoom", "uzdoom", &prefs, &[]);
        assert_eq!(port, "uzdoom");
    }

    #[test]
    fn test_select_sourceport_uses_family_preference() {
        let prefs = HashMap::from([("dsda".to_string(), "nugget-doom".to_string())]);
        let port = select_sourceport(None, None, Some("dsda"), "gzdoom", "uzdoom", &prefs, &[]);
        assert_eq!(port, "nugget-doom");
    }

    #[test]
    fn test_select_sourceport_preference_beats_same_family_default() {
        let prefs = HashMap::from([("dsda".to_string(), "nugget-doom".to_string())]);
        let port = select_sourceport(None, None, Some("dsda"), "nyan-doom", "uzdoom", &prefs, &[]);
        assert_eq!(port, "nugget-doom");
    }

    #[test]
    fn test_select_sourceport_uses_installed_family_port() {
        let prefs = HashMap::new();
        let installed = vec![("nugget-doom".to_string(), "dsda".to_string())];
        let port = select_sourceport(
            None,
            None,
            Some("dsda"),
            "gzdoom",
            "uzdoom",
            &prefs,
            &installed,
        );
        assert_eq!(port, "nugget-doom");
    }

    #[test]
    fn test_legacy_zdoom_required_does_not_override_blank_family() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();
        let update = db::WadUpdate::new().set_int("zdoom_required", Some(1));
        db::update_wad(&conn, wad_id, &update).unwrap();

        let port = select_sourceport(
            None,
            None,
            None,
            "nyan-doom",
            "uzdoom",
            &HashMap::new(),
            &[],
        );

        assert_eq!(port, "nyan-doom");
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert!(wad.required_sourceport_family.is_none());
    }

    #[test]
    fn test_fresh_zdoom_detection_keeps_blank_family_on_default_sourceport() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("zdoom.wad");
        std::fs::write(
            &wad_path,
            test_wad(&[
                ("MAP01", &[]),
                ("TEXTMAP", b"namespace = \"zdoom\";"),
                ("ENDMAP", &[]),
            ]),
        )
        .unwrap();

        detect_and_persist_zdoom_required_if_missing(&conn, wad_id, None, &wad_path).unwrap();
        let port = select_sourceport(
            None,
            None,
            None,
            "nyan-doom",
            "uzdoom",
            &HashMap::new(),
            &[],
        );

        assert_eq!(port, "nyan-doom");
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.zdoom_required, Some(1));
        assert!(wad.required_sourceport_family.is_none());
    }

    #[test]
    fn test_explicit_family_wins_over_zdoom_required() {
        let port = select_sourceport(
            None,
            None,
            Some("dsda"),
            "gzdoom",
            "uzdoom",
            &HashMap::new(),
            &[],
        );

        assert_eq!(port, "dsda-doom");
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
    fn test_read_session_stats_before_falls_back_to_db_snapshot() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();
        let snapshot = stats_json_one_map("MAP04", 1, 4);
        let update = db::WadUpdate::new().set_text("stats_snapshot", Some(snapshot.clone()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        assert_eq!(
            read_session_stats_before(&conn, wad_id).as_deref(),
            Some(snapshot.as_str())
        );
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
        let update =
            db::WadUpdate::new().set_text("stats_snapshot", Some(r#"{"maps":{}}"#.to_string()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        // Complete the playthrough
        db::complete_playthrough(&conn, pt_id, Some(r#"{"maps":{}}"#), None).unwrap();

        // Verify WAD is completed with a snapshot
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::Completed);
        assert!(wad.stats_snapshot.is_some());

        // Start a new playthrough (simulating --new-playthrough)
        start_new_playthrough(&conn, wad_id).unwrap();

        // Stats snapshot should be cleared
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::InProgress);
        assert!(wad.stats_snapshot.is_none());

        // A new active playthrough should exist
        let active = db::get_active_playthrough(&conn, wad_id).unwrap();
        assert!(active.is_some());
        assert_ne!(active.unwrap().id, pt_id);
    }

    #[test]
    fn test_archive_stats_files_hides_them_from_reader() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("dsda_doom_data/doom2/test");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.path().join("stats.txt"), "1\n0\n").unwrap();
        std::fs::write(nested.join("levelstat.txt"), "").unwrap();

        archive_stats_files(dir.path());

        // The reader must no longer find anything
        assert!(find_all_stats_files(dir.path()).is_empty());
        // The data is preserved under archived names
        assert!(dir.path().join("stats.txt.archived").exists());
        assert!(nested.join("levelstat.txt.archived").exists());

        // A second playthrough archives again without clobbering the first
        std::fs::write(dir.path().join("stats.txt"), "1\n5\n").unwrap();
        archive_stats_files(dir.path());
        assert!(find_all_stats_files(dir.path()).is_empty());
        assert!(dir.path().join("stats.txt.archived2").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("stats.txt.archived")).unwrap(),
            "1\n0\n"
        );
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

    #[test]
    fn test_session_made_progress_no_after() {
        // No stats_after → can't detect progress.
        assert!(!session_made_progress(None, None));
        assert!(!session_made_progress(
            Some(r#"{"format":"stats_txt","maps":[]}"#),
            None
        ));
    }

    #[test]
    fn test_session_made_progress_first_stats_empty() {
        // stats_after exists but contains no completed maps → no progress.
        let after = r#"{"format":"stats_txt","maps":[]}"#;
        assert!(!session_made_progress(None, Some(after)));
    }

    #[test]
    fn test_session_made_progress_first_stats_with_played_map() {
        // stats_after has a map that was actually played (best_skill > 0) → progress.
        let after = r#"{"format":"stats_txt","maps":[{"lump":"MAP01","best_skill":3,"total_exits":1,"time_secs":-1.0}]}"#;
        assert!(session_made_progress(None, Some(after)));
    }

    #[test]
    fn test_session_made_progress_first_stats_levelstat() {
        // levelstat: any completed map entry counts as progress.
        let after = r#"{"format":"levelstat_txt","maps":[{"lump":"MAP01","best_skill":4,"time_secs":32.5}]}"#;
        assert!(session_made_progress(None, Some(after)));
    }

    #[test]
    fn test_session_made_progress_unchanged() {
        // stats_before == stats_after → no progress (no exits_delta).
        let same = r#"{"format":"stats_txt","maps":[{"lump":"MAP01","best_skill":3,"total_exits":1,"time_secs":-1.0}]}"#;
        assert!(!session_made_progress(Some(same), Some(same)));
    }

    #[test]
    fn test_session_made_progress_changed() {
        // total_exits increased between before and after → progress.
        let before = r#"{"format":"stats_txt","maps":[{"lump":"MAP01","best_skill":3,"total_exits":1,"time_secs":-1.0}]}"#;
        let after = r#"{"format":"stats_txt","maps":[{"lump":"MAP01","best_skill":3,"total_exits":2,"time_secs":-1.0}]}"#;
        assert!(session_made_progress(Some(before), Some(after)));
    }

    #[test]
    fn test_session_made_progress_first_stats_entered_not_exited() {
        // Regression: dsda-doom wrote a stats entry on first launch with no
        // exit recorded (total_exits == 0, best_skill == 0). This must NOT
        // count as progress, otherwise the WAD is incorrectly flipped to
        // in-progress.
        let after = r#"{"format":"stats_txt","maps":[{"lump":"MAP01","best_skill":0,"total_exits":0,"time_secs":-1.0}]}"#;
        assert!(!session_made_progress(None, Some(after)));
    }

    #[test]
    fn test_session_made_progress_noise_no_exits() {
        // Regression: before/after differ only in header fields with no exit
        // recorded. Must NOT count as progress.
        let before = r#"{"format":"stats_txt","maps":[{"lump":"MAP01","best_skill":0,"total_exits":0,"time_secs":-1.0}]}"#;
        let after = r#"{"format":"stats_txt","header_total_kills":5,"maps":[{"lump":"MAP01","best_skill":0,"total_exits":0,"time_secs":-1.0}]}"#;
        assert!(!session_made_progress(Some(before), Some(after)));
    }

    #[test]
    fn test_session_made_progress_new_map_played() {
        // A map appears in after that wasn't in before, and it's played → progress.
        let before = r#"{"format":"stats_txt","maps":[]}"#;
        let after = r#"{"format":"stats_txt","maps":[{"lump":"MAP01","best_skill":3,"total_exits":1,"time_secs":-1.0}]}"#;
        assert!(session_made_progress(Some(before), Some(after)));
    }

    #[test]
    fn test_session_made_progress_new_map_unplayed() {
        // A map appears in after but wasn't played (all zeros) → no progress.
        let before = r#"{"format":"stats_txt","maps":[]}"#;
        let after = r#"{"format":"stats_txt","maps":[{"lump":"MAP01","best_skill":0,"total_exits":0,"time_secs":-1.0}]}"#;
        assert!(!session_made_progress(Some(before), Some(after)));
    }

    // -----------------------------------------------------------------------
    // ensure_in_progress / reconcile_stats
    // -----------------------------------------------------------------------

    fn fresh_db_with_wad() -> (rusqlite::Connection, i64) {
        use crate::db::{self, SourceType, init_db, open_memory};
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        let wad_id = db::add_wad(&conn, &db::NewWad::new("Test WAD", SourceType::Local)).unwrap();
        (conn, wad_id)
    }

    fn test_linedefs(specials: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        for &special in specials {
            out.extend_from_slice(&0u16.to_le_bytes()); // v1
            out.extend_from_slice(&0u16.to_le_bytes()); // v2
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&special.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // tag
            out.extend_from_slice(&0u16.to_le_bytes()); // sidedef 1
            out.extend_from_slice(&0u16.to_le_bytes()); // sidedef 2
        }
        out
    }

    fn test_wad(lumps: &[(&str, &[u8])]) -> Vec<u8> {
        let mut wad = Vec::new();
        let mut data = Vec::new();
        let mut entries = Vec::new();
        let mut offset = 12u32;

        for (name, lump_data) in lumps {
            entries.push((name.to_string(), offset, lump_data.len() as u32));
            data.extend_from_slice(lump_data);
            offset += lump_data.len() as u32;
        }

        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&(lumps.len() as i32).to_le_bytes());
        wad.extend_from_slice(&(offset as i32).to_le_bytes());
        wad.extend_from_slice(&data);

        for (name, lump_offset, size) in entries {
            wad.extend_from_slice(&lump_offset.to_le_bytes());
            wad.extend_from_slice(&size.to_le_bytes());
            let mut name_bytes = [0u8; 8];
            for (i, b) in name.bytes().take(8).enumerate() {
                name_bytes[i] = b;
            }
            wad.extend_from_slice(&name_bytes);
        }

        wad
    }

    #[test]
    fn test_ensure_in_progress_creates_playthrough_when_none() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();

        let created = ensure_in_progress(&conn, wad_id).unwrap();
        assert!(created.is_some(), "should create a new playthrough");

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::InProgress);
        assert!(db::get_active_playthrough(&conn, wad_id).unwrap().is_some());
    }

    #[test]
    fn test_ensure_in_progress_no_new_playthrough_when_one_exists() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();
        let pt_id = db::start_playthrough(&conn, wad_id).unwrap();

        let created = ensure_in_progress(&conn, wad_id).unwrap();
        assert!(created.is_none(), "should not create a second playthrough");

        let active = db::get_active_playthrough(&conn, wad_id).unwrap().unwrap();
        assert_eq!(active.id, pt_id);
    }

    #[test]
    fn test_ensure_in_progress_promotes_drifted_status() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();

        // Simulate a drifted state: an active playthrough exists but the
        // wads.status column has been reset to unplayed. This mirrors the
        // recovery path described in play().
        db::start_playthrough(&conn, wad_id).unwrap();
        conn.execute(
            "UPDATE wads SET status = 'unplayed' WHERE id = ?1",
            rusqlite::params![wad_id],
        )
        .unwrap();

        ensure_in_progress(&conn, wad_id).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::InProgress);
    }

    #[test]
    fn test_ensure_in_progress_preserves_completed_status() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();
        let pt_id = db::start_playthrough(&conn, wad_id).unwrap();
        db::complete_playthrough(&conn, pt_id, None, None).unwrap();
        // Re-open a playthrough (simulating a post-completion replay) but
        // leave the wad status at completed. ensure_in_progress must not
        // demote a completed WAD.
        conn.execute(
            "UPDATE playthroughs SET completed_at = NULL WHERE id = ?1",
            rusqlite::params![pt_id],
        )
        .unwrap();

        ensure_in_progress(&conn, wad_id).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::Completed);
    }

    /// Minimal stats.txt snapshot with one map at the given exit count.
    fn stats_json_one_map(lump: &str, exits: i32, skill: i32) -> String {
        format!(
            r#"{{"format":"stats_txt","version":1,"header_total_kills":0,"maps":[{{"lump":"{lump}","kills":0,"total_kills":-1,"items":0,"total_items":-1,"secrets":0,"total_secrets":-1,"episode":1,"map_num":1,"best_skill":{skill},"best_time":-1,"best_max_time":-1,"best_nm_time":-1,"total_exits":{exits},"cumulative_kills":0,"time_secs":-1.0,"total_time_secs":-1.0}}]}}"#
        )
    }

    #[test]
    fn test_reconcile_stats_with_no_op_when_disk_matches_db() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();

        let snapshot = stats_json_one_map("MAP01", 0, 0);
        let update = db::WadUpdate::new().set_text("stats_snapshot", Some(snapshot.clone()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let result = reconcile_stats_with(&conn, wad_id, &wad, &snapshot, None);
        assert_eq!(result, AutoCompleteResult::Unknown);

        // Still unplayed, no playthrough created.
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::Unplayed);
        assert!(db::get_active_playthrough(&conn, wad_id).unwrap().is_none());
    }

    #[test]
    fn test_reconcile_stats_with_synced_snapshot_still_checks_completion() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("single.wad");
        let linedefs = test_linedefs(&[11]);
        std::fs::write(
            &wad_path,
            test_wad(&[("MAP01", &[]), ("LINEDEFS", &linedefs)]),
        )
        .unwrap();

        db::start_playthrough(&conn, wad_id).unwrap();
        let snapshot = stats_json_one_map("MAP01", 1, 4);
        let update = db::WadUpdate::new().set_text("stats_snapshot", Some(snapshot.clone()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let result = reconcile_stats_with(&conn, wad_id, &wad, &snapshot, Some(&wad_path));
        assert_eq!(result, AutoCompleteResult::Completed);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::Completed);
    }

    #[test]
    fn test_reconcile_stats_with_promotes_on_fresh_exit() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();

        // DB has a stale "launched but no exits" snapshot — the exact shape
        // that the orphan-session bug leaves behind.
        let stale = stats_json_one_map("MAP01", 0, 0);
        let update = db::WadUpdate::new().set_text("stats_snapshot", Some(stale.clone()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        // Disk has the post-exit version (skill=4, one exit).
        let fresh = stats_json_one_map("MAP01", 1, 4);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        reconcile_stats_with(&conn, wad_id, &wad, &fresh, None);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::InProgress);
        assert_eq!(wad.stats_snapshot.as_deref(), Some(fresh.as_str()));
        assert!(db::get_active_playthrough(&conn, wad_id).unwrap().is_some());
    }

    #[test]
    fn test_reconcile_stats_with_syncs_snapshot_without_progress() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();

        // DB and disk differ only in a header field — no real exit.
        let before = r#"{"format":"stats_txt","version":1,"header_total_kills":0,"maps":[]}"#;
        let after = r#"{"format":"stats_txt","version":1,"header_total_kills":5,"maps":[]}"#;

        let update = db::WadUpdate::new().set_text("stats_snapshot", Some(before.to_string()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        reconcile_stats_with(&conn, wad_id, &wad, after, None);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        // Snapshot synced (so we don't re-trigger on the same drift)...
        assert_eq!(wad.stats_snapshot.as_deref(), Some(after));
        // ...but status stays unplayed (no real progress).
        assert_eq!(wad.status, db::Status::Unplayed);
        assert!(db::get_active_playthrough(&conn, wad_id).unwrap().is_none());
    }

    #[test]
    fn test_reconcile_stats_with_empty_db_snapshot_and_fresh_exit() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();

        // No prior snapshot at all — first reconciliation picks up fresh exit.
        let fresh = stats_json_one_map("MAP01", 1, 4);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert!(wad.stats_snapshot.is_none());

        reconcile_stats_with(&conn, wad_id, &wad, &fresh, None);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::InProgress);
        assert_eq!(wad.stats_snapshot.as_deref(), Some(fresh.as_str()));
    }

    #[test]
    fn test_reconcile_stats_with_promotes_when_snapshot_already_synced() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();

        // Snapshot was synced by some other path, but status never caught up.
        // This reproduces a production race where reconciling on byte-equal
        // snapshots must still promote status for a WAD with real on-disk
        // exits.
        let synced = stats_json_one_map("MAP01", 1, 4);
        let update = db::WadUpdate::new().set_text("stats_snapshot", Some(synced.clone()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::Unplayed);

        reconcile_stats_with(&conn, wad_id, &wad, &synced, None);

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.status, db::Status::InProgress);
        assert!(db::get_active_playthrough(&conn, wad_id).unwrap().is_some());
    }

    #[test]
    fn test_reconcile_stats_with_no_second_playthrough_when_in_progress() {
        use crate::db;
        let (conn, wad_id) = fresh_db_with_wad();
        let pt_id = db::start_playthrough(&conn, wad_id).unwrap();

        let fresh = stats_json_one_map("MAP01", 1, 4);
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        reconcile_stats_with(&conn, wad_id, &wad, &fresh, None);

        let active = db::get_active_playthrough(&conn, wad_id).unwrap().unwrap();
        assert_eq!(active.id, pt_id, "must not create a second playthrough");
    }
}
