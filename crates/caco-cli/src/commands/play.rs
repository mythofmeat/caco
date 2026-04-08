//! `caco play` — play a WAD or IWAD with the configured sourceport.

use std::path::PathBuf;

use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;

use caco_core::config;
use caco_core::db;

fn make_download_progress_bar(message: &str) -> ProgressBar {
    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::with_template(
            "Downloading {msg} [{bar:30}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .unwrap()
        .progress_chars("=> "),
    );
    pb.set_message(message.to_string());
    pb
}

fn progress_callback(pb: &ProgressBar) -> impl Fn(u64, u64) + '_ {
    move |downloaded: u64, total: u64| {
        if pb.length() != Some(total) {
            pb.set_length(total);
        }
        pb.set_position(downloaded);
    }
}
use caco_core::db::models::WadRecord;
use caco_core::player::{self, AutoCompleteResult, PlayOptions, RecordOption, format_duration};
use caco_core::wad_stats;
use caco_sources::idgames::IdgamesClient;
use crate::resolve;

#[derive(Args)]
pub struct PlayArgs {
    /// WAD query or ID + extra args for sourceport
    #[arg(trailing_var_arg = true)]
    query: Vec<String>,

    /// Sourceport to use
    #[arg(short = 'p', long)]
    sourceport: Option<String>,

    /// Auto-select first match
    #[arg(long, short = '1')]
    first: bool,

    /// Play IWAD directly (no WAD)
    #[arg(long)]
    iwad: Option<String>,

    /// Record demo (auto-name or NAME)
    #[arg(short = 'r', long, num_args = 0..=1, default_missing_value = "")]
    record: Option<String>,

    /// Override complevel (int or alias)
    #[arg(short = 'c', long)]
    complevel: Option<String>,

    /// Config profile name
    #[arg(short = 'C', long = "config")]
    config_profile: Option<String>,
}

pub fn run(conn: &Connection, args: &PlayArgs) -> Result<(), String> {
    // IWAD mode
    if let Some(ref iwad_name) = args.iwad {
        return play_iwad(conn, iwad_name, args);
    }

    // WAD mode
    let wad = if args.query.is_empty() {
        // No query: play most recently played
        db::get_most_recently_played(conn)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No WADs have been played yet.".to_string())?
    } else {
        resolve::resolve_one_wad(conn, &args.query, args.first)?
    };

    // Ensure WAD file is available (download from idgames if needed)
    ensure_wad_path(conn, &wad)?;

    // Build play options
    let record = args.record.as_ref().map(|name| {
        if name.is_empty() {
            RecordOption::Auto
        } else {
            RecordOption::Named(name.clone())
        }
    });

    let mut extra_args = Vec::new();

    // Inject complevel override if specified
    if let Some(ref cl_str) = args.complevel {
        let cl = caco_core::complevel::parse_complevel(cl_str)
            .ok_or_else(|| format!("Invalid complevel: {cl_str}"))?;
        extra_args.push("-complevel".to_string());
        extra_args.push(cl.to_string());
    }

    let opts = PlayOptions {
        sourceport: args.sourceport.clone(),
        extra_args,
        record,
        config_profile: args.config_profile.clone(),
    };

    eprintln!("Playing: {} (ID: {})", wad.title, wad.id);

    let result = player::play(conn, wad.id, &opts).map_err(|e| e.to_string())?;

    if let Some(duration) = result.duration {
        eprintln!("Session duration: {}", format_duration(duration));
    }
    if result.crashed() {
        eprintln!(
            "Warning: Sourceport exited with code {}",
            result.exit_code.unwrap_or(-1),
        );
    }

    // Show map progress after play
    if let Ok(Some(refreshed)) = db::get_wad(conn, wad.id, false)
        && let Some(display) = wad_stats::get_progress_display(refreshed.stats_snapshot.as_deref())
    {
        eprintln!("Progress: {display}");
    }

    // Report auto-completion detection
    match result.auto_complete {
        AutoCompleteResult::Completed => {
            eprintln!("All maps completed! Marked '{}' as finished.", wad.title);
        }
        AutoCompleteResult::Incomplete { exited, required } => {
            eprintln!("Maps: {exited}/{required} required maps exited.");
        }
        AutoCompleteResult::Unknown => {}
    }

    Ok(())
}

fn play_iwad(conn: &Connection, iwad_name: &str, args: &PlayArgs) -> Result<(), String> {
    eprintln!("Playing IWAD: {iwad_name}");

    let mut extra_args: Vec<String> = args.query.clone();

    if let Some(ref cl_str) = args.complevel {
        let cl = caco_core::complevel::parse_complevel(cl_str)
            .ok_or_else(|| format!("Invalid complevel: {cl_str}"))?;
        extra_args.push("-complevel".to_string());
        extra_args.push(cl.to_string());
    }

    let result = player::play_iwad(
        conn,
        iwad_name,
        args.sourceport.as_deref(),
        &extra_args,
        args.config_profile.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    if let Some(duration) = result.duration {
        eprintln!("Session duration: {}", format_duration(duration));
    }
    if result.crashed() {
        eprintln!(
            "Warning: Sourceport exited with code {}",
            result.exit_code.unwrap_or(-1),
        );
    }

    Ok(())
}

/// Ensure a WAD file is available locally, downloading from idgames if needed.
fn ensure_wad_path(conn: &Connection, wad: &WadRecord) -> Result<(), String> {
    // Check if cached_path already exists
    if let Some(ref path) = wad.cached_path
        && PathBuf::from(path).exists()
    {
        return Ok(());
    }

    // Try to download from idgames.
    // Import stores the numeric ID in source_id; idgames_id is a later-added alias.
    // Fall back to source_id when source_type is "idgames" and idgames_id is unset.
    let idgames_id_str = wad.idgames_id.as_deref().or_else(|| {
        if wad.source_type == "idgames" {
            wad.source_id.as_deref()
        } else {
            None
        }
    });
    let idgames_id = idgames_id_str.and_then(|id| id.parse::<i64>().ok());

    // If no numeric ID, try direct mirror download via source_url
    if idgames_id.is_none() {
        let source_url = wad.source_url.as_deref().unwrap_or("");
        let filename = wad.filename.as_deref().unwrap_or("");

        if filename.is_empty() || !source_url.contains("/idgames/") {
            return Err(format!(
                "No WAD file available for '{}'. Link a file with: caco modify id:{} --link /path/to/wad",
                wad.title, wad.id
            ));
        }

        let client = IdgamesClient::new();
        let cache_dir = config::get_cache_dir();
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {e}"))?;
        let mirror = config::load_config().download_mirror as usize;

        let pb = make_download_progress_bar(filename);
        let cb = progress_callback(&pb);

        let dest = client
            .download_direct(source_url, filename, &cache_dir, mirror, Some(&cb))
            .map_err(|e| format!("Direct download failed: {e}"))?;

        pb.finish_and_clear();
        eprintln!("Downloaded (via mirror): {filename}");

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(dest.to_string_lossy().to_string()))
            .map_err(|e| format!("Failed to update cached_path: {e}"))?;
        db::update_wad(conn, wad.id, &update)
            .map_err(|e| format!("Failed to update WAD record: {e}"))?;

        return Ok(());
    }

    let idgames_id = idgames_id.unwrap();
    let client = IdgamesClient::new();
    let cache_dir = config::get_cache_dir();
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create cache directory: {e}"))?;
    let cfg = config::load_config();
    let mirror = cfg.download_mirror as usize;

    // Try API first, fall back to direct mirror on WAF block
    let entry = match client.get(Some(idgames_id), None) {
        Ok(e) => Some(e),
        Err(caco_sources::SourceError::WafBlocked { .. }) => {
            eprintln!("API blocked — trying direct mirror download...");
            None
        }
        Err(e) => return Err(format!("Failed to fetch idgames entry: {e}")),
    };

    if let Some(entry) = entry {
        // Normal API-based download
        let pb = make_download_progress_bar(&entry.filename);
        let cb = progress_callback(&pb);

        let dest = client
            .download(&entry, Some(&cache_dir), mirror, Some(&cb))
            .map_err(|e| format!("Download failed: {e}"))?;

        pb.finish_and_clear();
        eprintln!("Downloaded: {}", entry.filename);

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(dest.to_string_lossy().to_string()))
            .map_err(|e| format!("Failed to update cached_path: {e}"))?;
        db::update_wad(conn, wad.id, &update)
            .map_err(|e| format!("Failed to update WAD record: {e}"))?;
    } else {
        // Direct mirror fallback using stored source_url + filename
        let source_url = wad.source_url.as_deref().unwrap_or("");
        let filename = wad.filename.as_deref().unwrap_or("");

        if filename.is_empty() || !source_url.contains("/idgames/") {
            return Err(format!(
                "API blocked and no stored idgames path for '{}'. Download manually and link with: caco modify id:{} --link /path/to/wad",
                wad.title, wad.id
            ));
        }

        let pb = make_download_progress_bar(filename);
        let cb = progress_callback(&pb);

        let dest = client
            .download_direct(source_url, filename, &cache_dir, mirror, Some(&cb))
            .map_err(|e| format!("Direct download failed: {e}"))?;

        pb.finish_and_clear();
        eprintln!("Downloaded (via mirror): {filename}");

        let update = db::WadUpdate::new()
            .set_text("cached_path", Some(dest.to_string_lossy().to_string()))
            .map_err(|e| format!("Failed to update cached_path: {e}"))?;
        db::update_wad(conn, wad.id, &update)
            .map_err(|e| format!("Failed to update WAD record: {e}"))?;
    }

    Ok(())
}
