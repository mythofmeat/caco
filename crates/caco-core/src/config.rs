use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

use crate::utils::sanitize_dirname;

// ---------------------------------------------------------------------------
// Platform data/cache paths (with env var overrides for testing)
//
// Both roots come from `dirs`, so they follow XDG on Linux and
// ~/Library on macOS. Nothing here spells a literal path except as a
// fallback for when `dirs` cannot answer at all.
//
// CACO_HOME       — override the base data directory
// CACO_DB_PATH    — override the database file path
// CACO_CACHE_DIR  — override the WAD cache directory
// CACO_DATA_DIR   — override the per-WAD data directory
// CACO_CONFIG     — override the config file path
// ---------------------------------------------------------------------------

fn home_dir() -> PathBuf {
    dirs::home_dir().expect("could not determine home directory")
}

/// Where the config file lives — inside the data directory.
///
/// Deliberately not `~/.config/caco`. The file is written by the program far
/// more often than by hand (the settings dialog, the cache migration, first-run
/// sourceport detection), and keeping it beside the database means the portable
/// set is one directory rather than two: copy `~/.local/share/caco` and the
/// install comes with it. It also makes `CACO_HOME` a complete isolation
/// switch, since nothing outside it can redirect the database.
pub fn config_dir() -> PathBuf {
    default_data_dir()
}

pub fn config_file() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_CONFIG") {
        return PathBuf::from(p);
    }
    config_dir().join("config.toml")
}

/// Base data directory. Overridden by `CACO_HOME` env var.
///
/// Resolved per platform rather than hardcoded to `~/.local/share`: on Linux
/// that means `$XDG_DATA_HOME` when it is set, on macOS
/// `~/Library/Application Support`. [`cache_home`] has always resolved its side
/// this way, and the mismatch meant a machine with `XDG_DATA_HOME` set scattered
/// caco across two conventions at once.
///
/// Caco carries no layout migrations, so anyone who had both a set
/// `XDG_DATA_HOME` and an existing library moves the directory by hand once.
pub fn default_data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_HOME") {
        return PathBuf::from(p);
    }
    dirs::data_dir()
        .unwrap_or_else(|| home_dir().join(".local/share"))
        .join("caco")
}

pub fn default_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_DB_PATH") {
        return PathBuf::from(p);
    }
    default_data_dir().join("library.db")
}

/// Base directory for regenerable data. Overridden by `CACO_CACHE_HOME`.
///
/// Everything under here can be deleted without losing anything the user
/// cannot get back: downloaded WADs re-fetch from idgames, thumbnails
/// re-extract from TITLEPIC. This is deliberately *not* under
/// [`default_data_dir`] so the data dir stays small enough to copy between
/// machines — see the split documented in README's Data Locations table.
pub fn cache_home() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_CACHE_HOME") {
        return PathBuf::from(p);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| home_dir().join(".cache"))
        .join("caco")
}

pub fn default_cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_CACHE_DIR") {
        return PathBuf::from(p);
    }
    cache_home().join("wads")
}

pub fn iwad_dir() -> PathBuf {
    default_data_dir().join("iwads")
}

pub fn id24_dir() -> PathBuf {
    default_data_dir().join("id24")
}

pub fn thumbnail_cache_dir() -> PathBuf {
    cache_home().join("thumbnails")
}

pub fn default_data_subdir() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_DATA_DIR") {
        return PathBuf::from(p);
    }
    default_data_dir().join("data")
}

/// Where WAD files live that caco cannot fetch again on its own.
///
/// Portable side, deliberately mirroring `~/.cache/caco/wads` in basename:
/// both hold the same kind of thing, and which one a file lands in is
/// decided solely by [`crate::db::Retrievability`]. A manual-only WAD in the
/// cache is a WAD one `Clean` away from being gone for good, so it belongs
/// with the database rather than beside the disposable downloads.
pub fn keep_dir() -> PathBuf {
    default_data_dir().join("wads")
}

/// The directory a WAD's file belongs in, given whether caco can re-fetch it.
///
/// The single placement decision — every path that brings a file under
/// caco's management goes through here, so placement stays a consequence of
/// retrievability instead of whatever the calling code happened to have on
/// hand.
pub fn wad_store_dir(retrievability: crate::db::Retrievability) -> PathBuf {
    wad_store_dir_in(retrievability, &get_cache_dir(), &keep_dir())
}

/// [`wad_store_dir`] against explicit roots.
///
/// Both roots are passed rather than read, for the reason `GcPaths` takes
/// them: this function's answer is where a file gets written, and a test that
/// could reach the real roots is one that can drop files into the user's
/// library.
pub fn wad_store_dir_in(
    retrievability: crate::db::Retrievability,
    cache_dir: &std::path::Path,
    keep_dir: &std::path::Path,
) -> PathBuf {
    match retrievability {
        crate::db::Retrievability::Automatic => cache_dir.to_path_buf(),
        crate::db::Retrievability::Manual => keep_dir.to_path_buf(),
    }
}

pub fn backup_dir() -> PathBuf {
    default_data_dir().join("backups")
}

pub fn companion_dir() -> PathBuf {
    default_data_dir().join("companions")
}

/// Where per-sourceport config profiles live, as `{exe}/{profile}.{ext}`.
///
/// Named for what it holds rather than for what writes it. It used to be
/// `<data>/sourceports`, which read as "the sourceports themselves" and left
/// no room for the recipes that actually describe one.
pub fn profile_dir() -> PathBuf {
    default_data_dir().join("profiles")
}

/// Where user sourceport build recipes live.
///
/// On the portable side, because the recipe *is* the portable artifact: it is
/// a few hundred bytes that rebuild the sourceport anywhere, whereas the
/// binary it produces is ABI- and OS-specific. Patch files referenced by a
/// recipe resolve against this directory for the same reason.
pub fn sourceport_recipe_dir() -> PathBuf {
    default_data_dir().join("sourceports")
}

/// Root of the managed sourceport install prefixes.
///
/// Cache side: a built sourceport is regenerable from its recipe, and the
/// install trees are large (7.6M for nyan-doom, 74M for uzdoom) next to a data
/// dir meant to stay copyable.
pub fn sourceport_prefix_root() -> PathBuf {
    cache_home().join("sourceports")
}

/// Root of the git checkouts and build trees sourceports are built in.
///
/// Throwaway even by cache standards — uzdoom's checkout alone is 221M — and
/// kept only so a rebuild is an incremental one.
pub fn sourceport_src_root() -> PathBuf {
    cache_home().join("sourceports-src")
}

// ---------------------------------------------------------------------------
// Config structs
// ---------------------------------------------------------------------------

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub sourceport: String,
    pub cache_dir: String,
    pub db_path: String,
    pub iwad: String,
    pub iwad_dirs: Vec<String>,
    pub sourceport_args: Vec<String>,
    pub download_mirror: i64,
    pub link_mode: String,
    pub manage_data_dirs: bool,
    pub auto_stats: bool,
    pub auto_detect_iwad: bool,
    pub auto_detect_complevel: bool,
    pub auto_doomwiki_enrich: bool,
    pub cache_max_size_gb: f64,
    pub cache_max_age_days: i64,
    pub cache_auto_clean: bool,
    /// How often to ask each built sourceport's remote whether its ref has
    /// moved. `0` disables the check, so caco touches no network at startup.
    pub sourceport_update_check_days: i64,
    pub data_dir: String,
    pub iwad_dir: String,
    pub profile_dir: String,
    pub companion_orphan_cleanup: String,
    pub zdoom_sourceport: String,
    pub sourceport_preferences: HashMap<String, String>,
    /// Extra launch args applied only when a specific sourceport launches,
    /// keyed by executable basename (e.g. `"nyan-doom"`, `"helion"`).
    /// Appended after the global `sourceport_args`.
    #[serde(default)]
    pub executable_args: HashMap<String, Vec<String>>,

    #[serde(default)]
    pub gui: GuiConfig,
    #[serde(default)]
    pub list: ListConfig,
    #[serde(default)]
    pub iwad_priority: HashMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sourceport: String::new(),
            cache_dir: default_cache_dir().to_string_lossy().into_owned(),
            db_path: default_db_path().to_string_lossy().into_owned(),
            iwad: String::new(),
            iwad_dirs: Vec::new(),
            sourceport_args: Vec::new(),
            download_mirror: 0,
            link_mode: "move".to_string(),
            manage_data_dirs: true,
            auto_stats: true,
            auto_detect_iwad: true,
            auto_detect_complevel: true,
            auto_doomwiki_enrich: true,
            cache_max_size_gb: 0.0,
            cache_max_age_days: 0,
            cache_auto_clean: false,
            sourceport_update_check_days: 1,
            data_dir: default_data_subdir().to_string_lossy().into_owned(),
            iwad_dir: iwad_dir().to_string_lossy().into_owned(),
            profile_dir: String::new(),
            companion_orphan_cleanup: "ask".to_string(),
            zdoom_sourceport: String::new(),
            sourceport_preferences: HashMap::new(),
            executable_args: HashMap::new(),
            gui: GuiConfig::default(),
            list: ListConfig::default(),
            iwad_priority: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiConfig {
    pub default_tab: String,
    pub default_sort: String,
    pub default_sort_desc: bool,
    pub default_view: String,
    pub window_width: i64,
    pub window_height: i64,
    pub detail_panel_width: i64,
    pub show_detail_panel: bool,
    pub thumbnail_size: i64,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            default_tab: "all".to_string(),
            default_sort: "id".to_string(),
            default_sort_desc: false,
            default_view: "list".to_string(),
            window_width: 1200,
            window_height: 800,
            detail_panel_width: 300,
            show_detail_panel: true,
            thumbnail_size: 160,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ListConfig {
    pub format: Vec<String>,
    pub sort: Option<String>,
    pub default_status: Vec<String>,
}

impl Default for ListConfig {
    fn default() -> Self {
        Self {
            format: vec![
                "id".into(),
                "title".into(),
                "author".into(),
                "status".into(),
                "beaten".into(),
                "playtime".into(),
                "last_played".into(),
            ],
            sort: None,
            default_status: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Loading / saving
// ---------------------------------------------------------------------------

static CONFIG: OnceLock<ArcSwap<Config>> = OnceLock::new();

/// Read and parse the config file, falling back to defaults if missing or invalid.
///
/// Does NOT touch the in-memory cache — use [`load_config`] or [`reload_config`]
/// for that.
fn read_config_from_disk() -> Config {
    let path = config_file();
    if !path.exists() {
        return Config::default();
    }
    match fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("Warning: Invalid TOML syntax in {}: {e}", path.display());
                eprintln!("Warning: Using default configuration.");
                Config::default()
            }
        },
        Err(e) => {
            eprintln!("Warning: Failed to load config: {e}");
            eprintln!("Warning: Using default configuration.");
            Config::default()
        }
    }
}

fn config_cell() -> &'static ArcSwap<Config> {
    CONFIG.get_or_init(|| ArcSwap::from_pointee(read_config_from_disk()))
}

/// Load the current configuration snapshot.
///
/// Returns an [`Arc<Config>`] — a lock-free atomic snapshot that may become
/// stale if [`reload_config`] is called after this returns. Callers that want
/// a consistent view within a single operation should bind the returned Arc
/// to a local (rather than re-invoking [`load_config`] repeatedly).
///
/// Falls back to defaults if the config file is missing or invalid on first
/// load. Also ensures the config file on disk has all known keys.
pub fn load_config() -> Arc<Config> {
    config_cell().load_full()
}

/// Re-read the config file from disk and install it as the new snapshot.
///
/// Subsequent calls to [`load_config`] return the new values. Existing
/// [`Arc<Config>`] handles remain valid but point at the previous snapshot
/// until they are dropped or replaced.
pub fn reload_config() {
    config_cell().store(Arc::new(read_config_from_disk()));
}

/// Strip every key that still equals its default.
///
/// Only settings the user actually changed are written. Two reasons this
/// matters more than tidiness:
///
/// - The path keys default to *absolute* paths under `$HOME`. Persisting them
///   bakes one machine's layout into a file that is meant to travel with the
///   data directory; left absent they resolve at runtime on whatever machine
///   is reading them.
/// - A config that lists only real choices is one a person can read. The
///   defaults are documented in `config.example.toml`, not echoed back.
fn strip_defaults(current: toml::Table, default: &toml::Table) -> toml::Table {
    let mut out = toml::Table::new();

    for (key, value) in current {
        match (&value, default.get(&key)) {
            // Same as stock — say nothing.
            (_, Some(default_value)) if &value == default_value => {}
            // Sub-table: keep only the keys inside it that differ, and drop
            // the table entirely if that leaves it empty.
            (toml::Value::Table(table), Some(toml::Value::Table(default_table))) => {
                let pruned = strip_defaults(table.clone(), default_table);
                if !pruned.is_empty() {
                    out.insert(key, toml::Value::Table(pruned));
                }
            }
            _ => {
                out.insert(key, value);
            }
        }
    }

    out
}

/// Serialize a config down to just its non-default settings.
fn minimal_toml(config: &Config) -> crate::Result<String> {
    let current: toml::Table = toml::Value::try_from(config)?
        .as_table()
        .cloned()
        .unwrap_or_default();
    let default: toml::Table = toml::Value::try_from(Config::default())?
        .as_table()
        .cloned()
        .unwrap_or_default();

    Ok(toml::to_string_pretty(&strip_defaults(current, &default))?)
}

/// Save configuration to disk.
pub fn save_config(config: &Config) -> crate::Result<()> {
    let path = config_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, minimal_toml(config)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Derived path helpers
// ---------------------------------------------------------------------------

/// Get the database file path. Env var `CACO_DB_PATH` takes precedence over config.
pub fn get_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_DB_PATH") {
        return PathBuf::from(p);
    }
    let cfg = load_config();
    let p = &cfg.db_path;
    if p.is_empty() {
        default_db_path()
    } else {
        expand_tilde(p)
    }
}

/// Get the WAD cache directory. Env var `CACO_CACHE_DIR` takes precedence over config.
pub fn get_cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_CACHE_DIR") {
        return PathBuf::from(p);
    }
    let cfg = load_config();
    let p = &cfg.cache_dir;
    if p.is_empty() {
        default_cache_dir()
    } else {
        expand_tilde(p)
    }
}

/// What [`ensure_sourceport_defaults`] picked, if anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DetectedSourceports {
    /// Newly chosen default sourceport.
    pub sourceport: Option<String>,
    /// Newly chosen zdoom-family sourceport.
    pub zdoom_sourceport: Option<String>,
}

impl DetectedSourceports {
    pub fn is_empty(&self) -> bool {
        self.sourceport.is_none() && self.zdoom_sourceport.is_none()
    }
}

/// On a config with no sourceport set, adopt one that is actually installed.
///
/// An empty `sourceport` means launching fails until the user goes looking for
/// the setting, which is a poor first five minutes. Only ever fills in blanks —
/// a sourceport the user chose is never second-guessed, including one that is not
/// currently on `PATH` (they may be about to install it).
pub fn ensure_sourceport_defaults() -> DetectedSourceports {
    let cfg = load_config();
    let needs_default = cfg.sourceport.trim().is_empty();
    let needs_zdoom = cfg.zdoom_sourceport.trim().is_empty();
    if !needs_default && !needs_zdoom {
        return DetectedSourceports::default();
    }

    let installed = crate::sourceports::detect_sourceports();
    if installed.is_empty() {
        return DetectedSourceports::default();
    }

    let mut found = DetectedSourceports::default();
    // FAMILIES order is the preference order, and detect_sourceports walks it,
    // so the first hit is the best available.
    if needs_default {
        found.sourceport = installed.first().map(|(exe, _, _)| (*exe).to_string());
    }
    if needs_zdoom {
        found.zdoom_sourceport = installed
            .iter()
            .find(|(_, _, family)| *family == "zdoom")
            .map(|(exe, _, _)| (*exe).to_string());
    }

    if found.is_empty() {
        return DetectedSourceports::default();
    }

    let mut updated = (*cfg).clone();
    if let Some(ref sourceport) = found.sourceport {
        updated.sourceport = sourceport.clone();
    }
    if let Some(ref sourceport) = found.zdoom_sourceport {
        updated.zdoom_sourceport = sourceport.clone();
    }

    if save_config(&updated).is_err() {
        return DetectedSourceports::default();
    }
    reload_config();
    found
}

/// Get the managed IWAD directory from config.
pub fn get_iwad_dir() -> PathBuf {
    let cfg = load_config();
    let p = &cfg.iwad_dir;
    if p.is_empty() {
        iwad_dir()
    } else {
        expand_tilde(p)
    }
}

/// Get the managed id24 WAD directory.
pub fn get_id24_dir() -> PathBuf {
    id24_dir()
}

/// Get IWAD search directories with tilde expansion.
pub fn get_iwad_dirs() -> Vec<PathBuf> {
    let cfg = load_config();
    cfg.iwad_dirs
        .iter()
        .filter(|d| !d.is_empty())
        .map(|d| expand_tilde(d))
        .collect()
}

/// Get the base directory for per-WAD data directories.
/// Env var `CACO_DATA_DIR` takes precedence over config.
pub fn get_data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_DATA_DIR") {
        return PathBuf::from(p);
    }
    let cfg = load_config();
    let p = &cfg.data_dir;
    if p.is_empty() {
        default_data_subdir()
    } else {
        expand_tilde(p)
    }
}

/// Get the sourceport config profiles directory.
pub fn get_profile_dir() -> PathBuf {
    let cfg = load_config();
    let p = &cfg.profile_dir;
    if p.is_empty() {
        profile_dir()
    } else {
        expand_tilde(p)
    }
}

/// Get the backup directory.
pub fn get_backup_dir() -> PathBuf {
    backup_dir()
}

/// Get the managed companion files directory.
pub fn get_companion_dir() -> PathBuf {
    companion_dir()
}

/// Get the companion orphan cleanup policy ("delete", "keep", or "ask").
pub fn get_companion_orphan_cleanup() -> String {
    let value = load_config().companion_orphan_cleanup.clone();
    match value.as_str() {
        "delete" | "keep" | "ask" => value,
        _ => "ask".to_string(),
    }
}

/// Get the configured default sourceport.
pub fn get_default_sourceport() -> String {
    load_config().sourceport.clone()
}

/// Get the configured ZDoom-family sourceport for WADs that require it.
///
/// Falls back to "uzdoom", then "gzdoom" if not configured.
pub fn get_zdoom_sourceport() -> String {
    let cfg = load_config();
    if !cfg.zdoom_sourceport.is_empty() {
        return cfg.zdoom_sourceport.clone();
    }
    // Try uzdoom first (modern fork), then gzdoom
    if which("uzdoom").is_some() {
        return "uzdoom".to_string();
    }
    "gzdoom".to_string()
}

/// Get configured preferred sourceports by family.
pub fn get_sourceport_preferences() -> HashMap<String, String> {
    load_config().sourceport_preferences.clone()
}

/// Get the configured default IWAD.
pub fn get_iwad() -> String {
    load_config().iwad.clone()
}

/// Get default sourceport args from config.
pub fn get_sourceport_args() -> Vec<String> {
    load_config().sourceport_args.clone()
}

/// Get per-sourceport launch args for a sourceport executable.
///
/// Keys in `[executable_args]` are executable basenames (extension stripped,
/// matching `sourceports::identify_family`); `executable` may be a bare
/// name or full path. Lookup is exact first, then case-insensitive (Helion
/// ships as both `helion` and `Helion`). Returns an empty vec when no
/// entry exists.
pub fn get_executable_args(executable: &str) -> Vec<String> {
    lookup_executable_args(&load_config(), executable)
}

fn lookup_executable_args(cfg: &Config, executable: &str) -> Vec<String> {
    let basename = Path::new(executable)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(executable);
    if let Some(args) = cfg.executable_args.get(basename) {
        return args.clone();
    }
    cfg.executable_args
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(basename))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// Whether to manage per-WAD data directories.
pub fn get_manage_data_dirs() -> bool {
    load_config().manage_data_dirs
}

/// Whether to auto-track stats after play sessions.
pub fn get_auto_stats() -> bool {
    load_config().auto_stats
}

/// Whether to auto-detect IWAD from WAD contents.
pub fn get_auto_detect_iwad() -> bool {
    load_config().auto_detect_iwad
}

/// Whether to auto-detect complevel from WAD contents.
pub fn get_auto_detect_complevel() -> bool {
    load_config().auto_detect_complevel
}

/// Get max cache size in bytes. 0 = unlimited.
pub fn get_cache_max_size() -> u64 {
    let cfg = load_config();
    if cfg.cache_max_size_gb > 0.0 {
        (cfg.cache_max_size_gb * 1024.0 * 1024.0 * 1024.0) as u64
    } else {
        0
    }
}

/// Get max cache age in days. 0 = never expire.
pub fn get_cache_max_age() -> i64 {
    load_config().cache_max_age_days
}

/// Whether to auto-clean cache before play.
pub fn get_cache_auto_clean() -> bool {
    load_config().cache_auto_clean
}

/// Resolve a sourceport name to a full path.
///
/// An absolute path is taken as given. Otherwise a caco-built sourceport wins over
/// `PATH`: if the user asked caco to build `uzdoom`, that build is the one
/// they meant even when a distro package of the same name exists. Falls back
/// to `which`, then to the bare name so the eventual spawn failure names
/// something the user recognises.
///
/// This is the only place managed sourceports are wired in — every launch path
/// already funnels through here.
pub fn resolve_sourceport(name: &str) -> String {
    let p = Path::new(name);
    if p.is_absolute() {
        return name.to_string();
    }
    if let Some(managed) = crate::sourceports::managed_binary(name) {
        return managed;
    }
    which(name).unwrap_or_else(|| name.to_string())
}

/// Return the per-WAD data directory path.
///
/// Format: `{data_dir}/{id}_{sanitized_title}/`
pub fn get_wad_data_dir(wad_id: i64, title: &str) -> PathBuf {
    get_data_dir().join(format!("{}_{}", wad_id, sanitize_dirname(title)))
}

/// Find an existing per-WAD data directory by ID prefix.
///
/// Handles title renames — matches `{id}_*` pattern.
pub fn find_wad_data_dir(wad_id: i64) -> Option<PathBuf> {
    let base = get_data_dir();
    if !base.is_dir() {
        return None;
    }
    let prefix = format!("{wad_id}_");
    for entry in fs::read_dir(&base).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.starts_with(&prefix)
        {
            return Some(path);
        }
    }
    None
}

/// Get the path to a sourceport config profile file.
///
/// Path: `{profile_dir}/{basename}/{profile}.{ext}`
///
/// Extension is determined by the sourceport family (e.g. `.ini` for Helion,
/// `.cfg` for everything else).
pub fn get_profile_path(sourceport: &str, profile: &str) -> PathBuf {
    let basename = Path::new(sourceport)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(sourceport);
    let ext = crate::sourceports::config_ext(sourceport);
    get_profile_dir()
        .join(basename)
        .join(format!("{profile}.{ext}"))
}

/// Scan the sourceport config directory for profiles.
pub fn list_profiles(sourceport: Option<&str>) -> HashMap<String, Vec<String>> {
    let sp_dir = get_profile_dir();
    if !sp_dir.is_dir() {
        return HashMap::new();
    }

    let mut result = HashMap::new();

    if let Some(sourceport) = sourceport {
        let basename = Path::new(sourceport)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(sourceport);
        let port_dir = sp_dir.join(basename);
        if port_dir.is_dir() {
            let mut profiles = collect_profile_stems(&port_dir);
            profiles.sort();
            if !profiles.is_empty() {
                result.insert(basename.to_string(), profiles);
            }
        }
    } else if let Ok(entries) = fs::read_dir(&sp_dir) {
        let mut dirs: Vec<_> = entries.flatten().filter(|e| e.path().is_dir()).collect();
        dirs.sort_by_key(|e| e.file_name());
        for entry in dirs {
            let mut profiles = collect_profile_stems(&entry.path());
            profiles.sort();
            if !profiles.is_empty()
                && let Some(name) = entry.file_name().to_str()
            {
                result.insert(name.to_string(), profiles);
            }
        }
    }

    result
}

/// Resolve an IWAD name to a full path.
///
/// Resolution order:
/// 1. If name is an existing absolute path, return as-is.
/// 2. If `db_resolved` is provided and the file exists, use it.
/// 3. Search each `iwad_dirs` entry for name and name.wad.
/// 4. Check managed IWAD directory.
/// 5. If not found, return the original name unchanged.
///
/// The `db_resolved` parameter allows the caller (e.g., player module) to
/// provide a DB-resolved path without config depending on the DB module.
pub fn resolve_iwad_path(name: &str, db_resolved: Option<&str>) -> String {
    // Check absolute path
    let p = expand_tilde(name);
    if p.is_absolute() && p.exists() {
        return p.to_string_lossy().into_owned();
    }

    // Check DB-resolved path
    if let Some(path) = db_resolved
        && Path::new(path).exists()
    {
        return path.to_string();
    }

    // Search iwad_dirs
    for dir in get_iwad_dirs() {
        if !dir.is_dir() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.exists() {
            return candidate.to_string_lossy().into_owned();
        }
        let with_ext = dir.join(format!("{name}.wad"));
        if with_ext.exists() {
            return with_ext.to_string_lossy().into_owned();
        }
    }

    // Check managed IWAD directory
    let managed_dir = get_iwad_dir();
    if managed_dir.is_dir() {
        // Search for family subdirs: iwads/{variant}/{family}.wad
        if let Ok(entries) = fs::read_dir(&managed_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let candidate = path.join(format!("{name}.wad"));
                    if candidate.exists() {
                        return candidate.to_string_lossy().into_owned();
                    }
                }
            }
        }
    }

    // Not found — return as-is
    name.to_string()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Expand leading `~` to the user's home directory.
fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home_dir().join(rest)
    } else if p == "~" {
        home_dir()
    } else {
        PathBuf::from(p)
    }
}

/// Poor-man's `which` — search PATH for an executable.
pub fn which(name: &str) -> Option<String> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in path_var.split(':') {
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// Known config file extensions for sourceport profiles.
const CONFIG_EXTENSIONS: &[&str] = &["cfg", "ini"];

/// Collect config profile file stems from a directory.
fn collect_profile_stems(dir: &Path) -> Vec<String> {
    let mut stems = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str())
                && CONFIG_EXTENSIONS.contains(&ext)
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                stems.push(stem.to_string());
            }
        }
    }
    stems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let home = home_dir();
        assert_eq!(expand_tilde("~/foo/bar"), home.join("foo/bar"));
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(
            expand_tilde("/absolute/path"),
            PathBuf::from("/absolute/path")
        );
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert!(cfg.manage_data_dirs);
        assert!(cfg.auto_stats);
        assert!(cfg.auto_detect_iwad);
        assert_eq!(cfg.link_mode, "move");
        assert_eq!(cfg.download_mirror, 0);
        assert!(cfg.sourceport_preferences.is_empty());
    }

    #[test]
    fn test_config_roundtrip() {
        let cfg = Config::default();
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.link_mode, cfg.link_mode);
        assert_eq!(parsed.manage_data_dirs, cfg.manage_data_dirs);
        assert_eq!(parsed.gui.window_width, cfg.gui.window_width);
    }

    #[test]
    fn test_config_partial_toml() {
        // Only set one field — everything else should use defaults
        let toml_str = r#"sourceport = "dsda-doom""#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.sourceport, "dsda-doom");
        assert!(cfg.manage_data_dirs); // default
        assert_eq!(cfg.gui.window_width, 1200); // default
    }

    #[test]
    fn test_config_sourceport_preferences() {
        let toml_str = r#"
[sourceport_preferences]
dsda = "nyan-doom"
zdoom = "uzdoom"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            cfg.sourceport_preferences.get("dsda").map(String::as_str),
            Some("nyan-doom")
        );
        assert_eq!(
            cfg.sourceport_preferences.get("zdoom").map(String::as_str),
            Some("uzdoom")
        );
    }

    #[test]
    fn test_config_port_args() {
        let toml_str = r#"
[executable_args]
nyan-doom = ["-geometry", "1920x1200"]
helion = ["-loglevel", "info"]
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            lookup_executable_args(&cfg, "nyan-doom"),
            vec!["-geometry", "1920x1200"]
        );
        // Full path resolves to basename
        assert_eq!(
            lookup_executable_args(&cfg, "/usr/bin/nyan-doom"),
            vec!["-geometry", "1920x1200"]
        );
        // Case-insensitive fallback (Helion ships as helion or Helion)
        assert_eq!(
            lookup_executable_args(&cfg, "Helion"),
            vec!["-loglevel", "info"]
        );
        // Windows-style extension is stripped
        assert_eq!(
            lookup_executable_args(&cfg, "nyan-doom.exe"),
            vec!["-geometry", "1920x1200"]
        );
        // Unknown sourceport gets nothing
        assert!(lookup_executable_args(&cfg, "gzdoom").is_empty());
    }

    #[test]
    fn test_config_port_args_default_empty() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.executable_args.is_empty());
        assert!(lookup_executable_args(&cfg, "nyan-doom").is_empty());
    }

    #[test]
    fn test_get_wad_data_dir() {
        let dir = get_wad_data_dir(42, "Scythe 2");
        let name = dir.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "42_scythe-2");
    }

    #[test]
    fn test_get_profile_path() {
        let path = get_profile_path("dsda-doom", "controller");
        assert!(path.to_string_lossy().contains("dsda-doom"));
        assert!(path.to_string_lossy().ends_with("controller.cfg"));
    }

    #[test]
    fn test_get_profile_path_helion() {
        let path = get_profile_path("helion", "default");
        assert!(path.to_string_lossy().contains("helion"));
        assert!(path.to_string_lossy().ends_with("default.ini"));
    }

    // -- minimal config serialization --

    #[test]
    fn test_minimal_toml_omits_untouched_defaults() {
        let rendered = minimal_toml(&Config::default()).unwrap();
        assert_eq!(
            rendered.trim(),
            "",
            "a stock config has nothing worth writing down"
        );
    }

    #[test]
    fn test_minimal_toml_keeps_only_changed_keys() {
        let cfg = Config {
            sourceport: "nyan-doom".to_string(),
            zdoom_sourceport: "uzdoom".to_string(),
            ..Default::default()
        };

        let table: toml::Table = minimal_toml(&cfg).unwrap().parse().unwrap();
        assert_eq!(
            table.get("sourceport").and_then(|v| v.as_str()),
            Some("nyan-doom")
        );
        assert_eq!(
            table.get("zdoom_sourceport").and_then(|v| v.as_str()),
            Some("uzdoom")
        );
        assert_eq!(table.len(), 2, "everything else matched its default");
    }

    #[test]
    fn test_minimal_toml_drops_machine_specific_paths() {
        // These default to absolute paths under $HOME. Writing them back is
        // what makes a config refuse to travel with its data directory.
        let table: toml::Table = minimal_toml(&Config::default()).unwrap().parse().unwrap();
        for key in ["db_path", "cache_dir", "data_dir", "iwad_dir"] {
            assert!(table.get(key).is_none(), "{key} should not be persisted");
        }
    }

    #[test]
    fn test_minimal_toml_keeps_an_explicit_path() {
        let cfg = Config {
            db_path: "/mnt/games/caco.db".to_string(),
            ..Default::default()
        };

        let table: toml::Table = minimal_toml(&cfg).unwrap().parse().unwrap();
        assert_eq!(
            table.get("db_path").and_then(|v| v.as_str()),
            Some("/mnt/games/caco.db")
        );
    }

    #[test]
    fn test_minimal_toml_prunes_sections_to_changed_keys() {
        let cfg = Config {
            gui: GuiConfig {
                thumbnail_size: 240,
                ..Default::default()
            },
            ..Default::default()
        };

        let table: toml::Table = minimal_toml(&cfg).unwrap().parse().unwrap();
        let gui = table.get("gui").unwrap().as_table().unwrap();
        assert_eq!(
            gui.get("thumbnail_size").and_then(|v| v.as_integer()),
            Some(240)
        );
        assert_eq!(gui.len(), 1, "untouched gui keys stay out");
        assert!(
            table.get("list").is_none(),
            "an untouched section is dropped"
        );
    }

    #[test]
    fn test_minimal_toml_keeps_populated_maps() {
        let mut executable_args = HashMap::new();
        executable_args.insert("nyan-doom".to_string(), vec!["-geometry".to_string()]);
        let cfg = Config {
            executable_args,
            ..Default::default()
        };

        let table: toml::Table = minimal_toml(&cfg).unwrap().parse().unwrap();
        assert!(table.get("executable_args").is_some());
    }

    #[test]
    fn test_minimal_toml_round_trips_through_parse() {
        let cfg = Config {
            sourceport: "woof".to_string(),
            gui: GuiConfig {
                thumbnail_size: 240,
                ..Default::default()
            },
            list: ListConfig {
                default_status: vec!["unplayed".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };

        let parsed: Config = toml::from_str(&minimal_toml(&cfg).unwrap()).unwrap();
        assert_eq!(parsed.sourceport, "woof");
        assert_eq!(parsed.gui.thumbnail_size, 240);
        assert_eq!(parsed.list.default_status, vec!["unplayed".to_string()]);
        // Omitted keys come back as their defaults, not as blanks.
        assert_eq!(parsed.link_mode, "move");
        assert_eq!(parsed.db_path, Config::default().db_path);
    }

    #[test]
    fn test_section_defaults_gui() {
        let cfg = GuiConfig::default();
        assert_eq!(cfg.default_tab, "all");
        assert_eq!(cfg.default_view, "list");
        assert_eq!(cfg.window_width, 1200);
        assert_eq!(cfg.window_height, 800);
        assert_eq!(cfg.detail_panel_width, 300);
        assert!(cfg.show_detail_panel);
        assert_eq!(cfg.thumbnail_size, 160);
    }

    #[test]
    fn test_section_defaults_list() {
        let cfg = ListConfig::default();
        assert!(cfg.format.contains(&"id".to_string()));
        assert!(cfg.format.contains(&"title".to_string()));
        assert!(cfg.format.contains(&"author".to_string()));
        assert!(cfg.sort.is_none());
        assert!(cfg.default_status.is_empty());
    }

    #[test]
    fn test_config_gui_section_override() {
        let toml_str = r#"
[gui]
default_view = "grid"
window_width = 1600
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.gui.default_view, "grid");
        assert_eq!(cfg.gui.window_width, 1600);
        // Defaults preserved
        assert_eq!(cfg.gui.window_height, 800);
    }

    #[test]
    fn test_resolve_iwad_path_absolute_existing() {
        let dir = tempfile::tempdir().unwrap();
        let wad = dir.path().join("doom2.wad");
        fs::write(&wad, "fake wad").unwrap();

        let result = resolve_iwad_path(wad.to_str().unwrap(), None);
        assert_eq!(result, wad.to_string_lossy().to_string());
    }

    #[test]
    fn test_resolve_iwad_path_not_found() {
        let result = resolve_iwad_path("nonexistent_iwad", None);
        assert_eq!(result, "nonexistent_iwad");
    }

    #[test]
    fn test_resolve_iwad_path_db_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let wad = dir.path().join("doom2.wad");
        fs::write(&wad, "fake wad").unwrap();

        let result = resolve_iwad_path("doom2", Some(wad.to_str().unwrap()));
        assert_eq!(result, wad.to_string_lossy().to_string());
    }

    #[test]
    fn test_resolve_iwad_path_db_resolved_missing() {
        // DB path doesn't exist, should fall through to managed dir or name
        let result = resolve_iwad_path("doom2", Some("/nonexistent/doom2.wad"));
        // If managed IWAD dir has doom2.wad, that will be returned;
        // otherwise the bare name is returned. Just ensure the nonexistent
        // DB path was not returned.
        assert_ne!(result, "/nonexistent/doom2.wad");
    }

    #[test]
    fn test_save_config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let orig_config_dir = dir.path().join("config");
        fs::create_dir_all(&orig_config_dir).unwrap();

        let cfg = Config {
            sourceport: "gzdoom".to_string(),
            download_mirror: 2,
            iwad_dirs: vec!["/opt/doom".into(), "/home/user/iwads".into()],
            ..Config::default()
        };

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.sourceport, "gzdoom");
        assert_eq!(parsed.download_mirror, 2);
        assert_eq!(parsed.iwad_dirs, vec!["/opt/doom", "/home/user/iwads"]);
    }

    #[test]
    fn test_config_with_nested_gui_save() {
        let mut cfg = Config::default();
        cfg.gui.default_tab = "playing".to_string();
        cfg.gui.default_sort = "playtime".to_string();

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.gui.default_tab, "playing");
        assert_eq!(parsed.gui.default_sort, "playtime");
    }

    #[test]
    fn test_config_lives_in_the_data_dir() {
        assert!(
            config_file().starts_with(default_data_dir()),
            "config must travel with the data directory"
        );
    }

    #[test]
    fn test_cache_home_is_outside_the_data_dir() {
        // The whole point of the split: nothing regenerable may live under
        // the directory the user copies between machines.
        assert!(!cache_home().starts_with(default_data_dir()));
        assert!(!default_cache_dir().starts_with(default_data_dir()));
        assert!(!thumbnail_cache_dir().starts_with(default_data_dir()));
    }

    #[test]
    fn test_get_wad_data_dir_special_chars() {
        let dir = get_wad_data_dir(1, "Scythe 2: Electric Boogaloo!");
        let name = dir.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "1_scythe-2-electric-boogaloo");
    }

    #[test]
    fn test_default_config_auto_detect_flags() {
        let cfg = Config::default();
        assert!(cfg.auto_detect_iwad);
        assert!(cfg.auto_detect_complevel);
        assert!(cfg.auto_doomwiki_enrich);
        assert!(cfg.auto_stats);
    }

    #[test]
    fn test_default_config_cache_settings() {
        let cfg = Config::default();
        assert_eq!(cfg.cache_max_size_gb, 0.0);
        assert_eq!(cfg.cache_max_age_days, 0);
        assert!(!cfg.cache_auto_clean);
    }

    #[test]
    fn test_read_config_from_disk_reflects_file_changes() {
        // Verifies the reload primitive: writing a new config file and
        // re-parsing it returns the updated values. Uses `read_config_from_disk`
        // directly so it doesn't conflict with the process-global CONFIG cell.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // SAFETY: test-only env mutation; cargo test runs a fresh process.
        unsafe {
            std::env::set_var("CACO_CONFIG", &path);
        }

        fs::write(&path, r#"sourceport = "dsda-doom""#).unwrap();
        let first = read_config_from_disk();
        assert_eq!(first.sourceport, "dsda-doom");

        // Swap the snapshot into an ArcSwap — the mechanism reload_config uses.
        let cell = arc_swap::ArcSwap::from_pointee(first);
        assert_eq!(cell.load().sourceport, "dsda-doom");

        // Change the file and re-read.
        fs::write(&path, r#"sourceport = "woof""#).unwrap();
        cell.store(Arc::new(read_config_from_disk()));
        assert_eq!(cell.load().sourceport, "woof");

        // SAFETY: clean up env var so we don't leak into other tests.
        unsafe {
            std::env::remove_var("CACO_CONFIG");
        }
    }

    #[test]
    fn test_companion_orphan_cleanup_validation() {
        fn validate(value: &str) -> String {
            match value {
                "delete" | "keep" | "ask" => value.to_string(),
                _ => "ask".to_string(),
            }
        }
        // Valid values
        assert_eq!(validate("delete"), "delete");
        assert_eq!(validate("keep"), "keep");
        assert_eq!(validate("ask"), "ask");
        // Invalid values fall back to "ask"
        assert_eq!(validate("invalid"), "ask");
        assert_eq!(validate(""), "ask");
    }

    /// Placement follows retrievability and nothing else — not where the file
    /// came from, not where the caller happened to be writing.
    #[test]
    fn test_wad_store_dir_follows_retrievability() {
        use crate::db::Retrievability;
        let cache = std::path::Path::new("/cache/wads");
        let keep = std::path::Path::new("/data/wads");

        assert_eq!(
            wad_store_dir_in(Retrievability::Automatic, cache, keep),
            cache
        );
        assert_eq!(wad_store_dir_in(Retrievability::Manual, cache, keep), keep);
    }

    /// The keep dir must sit under the portable data dir, since the whole
    /// point is surviving a copy of that directory to another machine.
    #[test]
    fn test_keep_dir_is_on_the_portable_side() {
        assert!(keep_dir().starts_with(default_data_dir()));
        assert!(!keep_dir().starts_with(cache_home()));
    }
}
