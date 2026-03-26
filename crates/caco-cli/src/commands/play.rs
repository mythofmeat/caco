//! `caco play` — play a WAD or IWAD with the configured sourceport.

use std::path::PathBuf;

use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;

use caco_core::config;
use caco_core::db;
use caco_core::db::models::WadRecord;
use caco_core::player::{self, PlayOptions, RecordOption, format_duration};
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

    // Try to download from idgames
    let idgames_id = wad.idgames_id.as_deref().and_then(|id| id.parse::<i64>().ok());
    let idgames_id = match idgames_id {
        Some(id) => id,
        None => {
            // Not an idgames WAD — can't auto-download
            return Err(format!(
                "No WAD file available for '{}'. Link a file with: caco modify id:{} --link /path/to/wad",
                wad.title, wad.id
            ));
        }
    };

    let client = IdgamesClient::new();
    let entry = client
        .get(Some(idgames_id), None)
        .map_err(|e| format!("Failed to fetch idgames entry: {e}"))?;

    let cache_dir = config::get_cache_dir();
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create cache directory: {e}"))?;

    let cfg = config::load_config();
    let mirror = cfg.download_mirror as usize;

    // Create progress bar
    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::with_template(
            "Downloading {msg} [{bar:30}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .unwrap()
        .progress_chars("=> "),
    );
    pb.set_message(entry.filename.clone());

    let progress_cb = |downloaded: u64, total: u64| {
        if pb.length() != Some(total) {
            pb.set_length(total);
        }
        pb.set_position(downloaded);
    };

    let dest = client
        .download(&entry, Some(&cache_dir), mirror, Some(&progress_cb))
        .map_err(|e| format!("Download failed: {e}"))?;

    pb.finish_and_clear();
    eprintln!("Downloaded: {}", entry.filename);

    // Update cached_path in DB
    let update = db::WadUpdate::new()
        .set_text("cached_path", Some(dest.to_string_lossy().to_string()))
        .map_err(|e| format!("Failed to update cached_path: {e}"))?;
    db::update_wad(conn, wad.id, &update)
        .map_err(|e| format!("Failed to update WAD record: {e}"))?;

    Ok(())
}
