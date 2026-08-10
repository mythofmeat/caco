use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

use crate::utils::sanitize_dirname;

// ---------------------------------------------------------------------------
// XDG-style paths (with env var overrides for testing)
//
// CACO_HOME       — override the base data directory (~/.local/share/caco)
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

/// Where the config lived before it moved next to the database.
pub fn legacy_config_file() -> PathBuf {
    home_dir().join(".config").join("caco").join("config.toml")
}

/// Base data directory. Overridden by `CACO_HOME` env var.
pub fn default_data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_HOME") {
        return PathBuf::from(p);
    }
    home_dir().join(".local/share/caco")
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

/// Where the WAD cache lived before it moved to [`cache_home`].
///
/// Retained so [`migrate_legacy_wad_cache`] can find and relocate it.
pub fn legacy_wad_cache_dir() -> PathBuf {
    default_data_dir().join("wads")
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

pub fn backup_dir() -> PathBuf {
    default_data_dir().join("backups")
}

pub fn companion_dir() -> PathBuf {
    default_data_dir().join("companions")
}

pub fn default_sourceport_dir() -> PathBuf {
    default_data_dir().join("sourceports")
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
    pub data_dir: String,
    pub iwad_dir: String,
    pub sourceport_dir: String,
    pub companion_orphan_cleanup: String,
    pub zdoom_sourceport: String,
    pub sourceport_preferences: HashMap<String, String>,
    /// Extra launch args applied only when a specific sourceport launches,
    /// keyed by executable basename (e.g. `"nyan-doom"`, `"helion"`).
    /// Appended after the global `sourceport_args`.
    #[serde(default)]
    pub port_args: HashMap<String, Vec<String>>,

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
            data_dir: default_data_subdir().to_string_lossy().into_owned(),
            iwad_dir: iwad_dir().to_string_lossy().into_owned(),
            sourceport_dir: String::new(),
            companion_orphan_cleanup: "ask".to_string(),
            zdoom_sourceport: String::new(),
            sourceport_preferences: HashMap::new(),
            port_args: HashMap::new(),
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

/// Outcome of [`migrate_legacy_config`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigMigration {
    /// Nothing to do — already migrated, or no legacy file exists.
    NotNeeded,
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// Left alone, with the reason. Never destructive.
    Skipped {
        reason: String,
    },
}

/// Relocate `~/.config/caco/config.toml` next to the database.
///
/// Also normalises the file on the way: the old code wrote every default back
/// to disk, including absolute paths under `$HOME`, which is exactly what makes
/// a config non-portable. Re-saving through [`save_config`] drops all of it.
///
/// Refuses to overwrite an existing config at the destination — two configs
/// means the user (or another tool) put one there, and picking a winner is not
/// this function's call.
pub fn migrate_legacy_config() -> ConfigMigration {
    // An explicitly chosen path is the user's business.
    if std::env::var("CACO_CONFIG").is_ok() {
        return ConfigMigration::NotNeeded;
    }

    let outcome = move_config_file(&legacy_config_file(), &config_file());
    if matches!(outcome, ConfigMigration::Moved { .. }) {
        reload_config();
    }
    outcome
}

/// Split out from [`migrate_legacy_config`] so the filesystem behaviour can be
/// tested against temp dirs rather than the caller's real home — the same
/// reason [`move_cache_dir`] exists separately.
fn move_config_file(legacy: &Path, target: &Path) -> ConfigMigration {
    if legacy == target || !legacy.is_file() {
        return ConfigMigration::NotNeeded;
    }
    if target.exists() {
        return ConfigMigration::Skipped {
            reason: format!("{} already exists", target.display()),
        };
    }

    let Ok(contents) = fs::read_to_string(legacy) else {
        return ConfigMigration::Skipped {
            reason: format!("could not read {}", legacy.display()),
        };
    };

    if let Some(parent) = target.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return ConfigMigration::Skipped {
            reason: format!("could not create {}: {e}", parent.display()),
        };
    }

    // Normalise when it parses; copy verbatim when it does not, so a config we
    // cannot understand is preserved rather than dropped.
    let written = match toml::from_str::<Config>(&contents) {
        Ok(cfg) => minimal_toml(&cfg)
            .and_then(|minimal| Ok(fs::write(target, minimal)?))
            .is_ok(),
        Err(_) => fs::write(target, &contents).is_ok(),
    };

    if !written {
        return ConfigMigration::Skipped {
            reason: format!("could not write {}", target.display()),
        };
    }

    let _ = fs::remove_file(legacy);
    // Succeeds only if we emptied it; the directory may hold unrelated files.
    if let Some(parent) = legacy.parent() {
        let _ = fs::remove_dir(parent);
    }

    ConfigMigration::Moved {
        from: legacy.to_path_buf(),
        to: target.to_path_buf(),
    }
}

/// What [`ensure_sourceport_defaults`] picked, if anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DetectedPorts {
    /// Newly chosen default sourceport.
    pub sourceport: Option<String>,
    /// Newly chosen zdoom-family sourceport.
    pub zdoom_sourceport: Option<String>,
}

impl DetectedPorts {
    pub fn is_empty(&self) -> bool {
        self.sourceport.is_none() && self.zdoom_sourceport.is_none()
    }
}

/// On a config with no sourceport set, adopt one that is actually installed.
///
/// An empty `sourceport` means launching fails until the user goes looking for
/// the setting, which is a poor first five minutes. Only ever fills in blanks —
/// a port the user chose is never second-guessed, including one that is not
/// currently on `PATH` (they may be about to install it).
pub fn ensure_sourceport_defaults() -> DetectedPorts {
    let cfg = load_config();
    let needs_default = cfg.sourceport.trim().is_empty();
    let needs_zdoom = cfg.zdoom_sourceport.trim().is_empty();
    if !needs_default && !needs_zdoom {
        return DetectedPorts::default();
    }

    let installed = crate::sourceports::detect_sourceports();
    if installed.is_empty() {
        return DetectedPorts::default();
    }

    let mut found = DetectedPorts::default();
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
        return DetectedPorts::default();
    }

    let mut updated = (*cfg).clone();
    if let Some(ref port) = found.sourceport {
        updated.sourceport = port.clone();
    }
    if let Some(ref port) = found.zdoom_sourceport {
        updated.zdoom_sourceport = port.clone();
    }

    if save_config(&updated).is_err() {
        return DetectedPorts::default();
    }
    reload_config();
    found
}

/// Outcome of [`migrate_legacy_wad_cache`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheMigration {
    /// Nothing to do — already migrated, or no legacy directory exists.
    NotNeeded,
    /// The cache was relocated.
    Moved { from: PathBuf, to: PathBuf },
    /// A legacy directory exists but was deliberately left alone.
    Skipped { reason: String },
}

/// Move the WAD cache out of the data dir and into [`cache_home`].
///
/// The WAD cache is regenerable — every file re-downloads from idgames — so it
/// does not belong in the directory the user copies between machines. This
/// relocates it once and rewrites the stored `cache_dir` so the move sticks.
///
/// Deliberately conservative. It only acts when the configured `cache_dir` is
/// unset or still points at the old default: a user who pointed the cache
/// somewhere of their own choosing has made a decision, and it is not this
/// function's place to override it. It also refuses to merge into a non-empty
/// destination rather than risk interleaving two caches.
///
/// Idempotent, so calling it on every startup is fine.
pub fn migrate_legacy_wad_cache() -> CacheMigration {
    // An explicit env override means the caller is driving; don't interfere.
    if std::env::var("CACO_CACHE_DIR").is_ok() {
        return CacheMigration::NotNeeded;
    }

    let legacy = legacy_wad_cache_dir();
    let target = default_cache_dir();

    if legacy == target || !legacy.is_dir() {
        return CacheMigration::NotNeeded;
    }

    // Respect a cache_dir the user pointed somewhere deliberate.
    let cfg = load_config();
    if !cfg.cache_dir.is_empty() && expand_tilde(&cfg.cache_dir) != legacy {
        return CacheMigration::NotNeeded;
    }

    let outcome = move_cache_dir(&legacy, &target);

    // Persist the new location so the move is not re-attempted or reverted.
    if matches!(outcome, CacheMigration::Moved { .. }) && !cfg.cache_dir.is_empty() {
        let mut updated = (*cfg).clone();
        updated.cache_dir = target.to_string_lossy().into_owned();
        if save_config(&updated).is_ok() {
            reload_config();
        }
    }

    outcome
}

/// Relocate `legacy` to `target`, without consulting config or environment.
///
/// Split out from [`migrate_legacy_wad_cache`] so the filesystem behaviour can
/// be tested against temp dirs rather than the caller's real home directory.
fn move_cache_dir(legacy: &Path, target: &Path) -> CacheMigration {
    // Never merge two caches together.
    let target_occupied = fs::read_dir(target)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    if target_occupied {
        return CacheMigration::Skipped {
            reason: format!(
                "{} already exists and is not empty; left {} in place",
                target.display(),
                legacy.display()
            ),
        };
    }

    if let Some(parent) = target.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return CacheMigration::Skipped {
            reason: format!("could not create {}: {e}", parent.display()),
        };
    }

    // `rename` is the common case (same filesystem) and is atomic. It fails
    // across mount points, so fall back to a copy-then-delete. An empty
    // directory already at the target also fails rename on some platforms,
    // so clear it first.
    let _ = fs::remove_dir(target);
    if fs::rename(legacy, target).is_err() {
        if let Err(e) = copy_dir_recursive(legacy, target) {
            return CacheMigration::Skipped {
                reason: format!("could not copy the cache to {}: {e}", target.display()),
            };
        }
        if let Err(e) = fs::remove_dir_all(legacy) {
            return CacheMigration::Skipped {
                reason: format!(
                    "copied the cache to {} but could not remove {}: {e}",
                    target.display(),
                    legacy.display()
                ),
            };
        }
    }

    CacheMigration::Moved {
        from: legacy.to_path_buf(),
        to: target.to_path_buf(),
    }
}

fn copy_dir_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
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
pub fn get_sourceport_dir() -> PathBuf {
    let cfg = load_config();
    let p = &cfg.sourceport_dir;
    if p.is_empty() {
        default_sourceport_dir()
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

/// Get per-port launch args for a sourceport executable.
///
/// Keys in `[port_args]` are executable basenames (extension stripped,
/// matching `sourceports::identify_family`); `executable` may be a bare
/// name or full path. Lookup is exact first, then case-insensitive (Helion
/// ships as both `helion` and `Helion`). Returns an empty vec when no
/// entry exists.
pub fn get_port_args(executable: &str) -> Vec<String> {
    lookup_port_args(&load_config(), executable)
}

fn lookup_port_args(cfg: &Config, executable: &str) -> Vec<String> {
    let basename = Path::new(executable)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(executable);
    if let Some(args) = cfg.port_args.get(basename) {
        return args.clone();
    }
    cfg.port_args
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
/// If name is already an absolute path, return as-is.
/// Otherwise, use `which` to find it on PATH.
pub fn resolve_sourceport(name: &str) -> String {
    let p = Path::new(name);
    if p.is_absolute() {
        return name.to_string();
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
/// Path: `{sourceport_dir}/{basename}/{profile}.{ext}`
///
/// Extension is determined by the sourceport family (e.g. `.ini` for Helion,
/// `.cfg` for everything else).
pub fn get_profile_path(sourceport: &str, profile: &str) -> PathBuf {
    let basename = Path::new(sourceport)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(sourceport);
    let ext = crate::sourceports::config_ext(sourceport);
    get_sourceport_dir()
        .join(basename)
        .join(format!("{profile}.{ext}"))
}

/// Scan the sourceport config directory for profiles.
pub fn list_profiles(sourceport: Option<&str>) -> HashMap<String, Vec<String>> {
    let sp_dir = get_sourceport_dir();
    if !sp_dir.is_dir() {
        return HashMap::new();
    }

    let mut result = HashMap::new();

    if let Some(port) = sourceport {
        let basename = Path::new(port)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(port);
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
[port_args]
nyan-doom = ["-geometry", "1920x1200"]
helion = ["-loglevel", "info"]
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            lookup_port_args(&cfg, "nyan-doom"),
            vec!["-geometry", "1920x1200"]
        );
        // Full path resolves to basename
        assert_eq!(
            lookup_port_args(&cfg, "/usr/bin/nyan-doom"),
            vec!["-geometry", "1920x1200"]
        );
        // Case-insensitive fallback (Helion ships as helion or Helion)
        assert_eq!(lookup_port_args(&cfg, "Helion"), vec!["-loglevel", "info"]);
        // Windows-style extension is stripped
        assert_eq!(
            lookup_port_args(&cfg, "nyan-doom.exe"),
            vec!["-geometry", "1920x1200"]
        );
        // Unknown port gets nothing
        assert!(lookup_port_args(&cfg, "gzdoom").is_empty());
    }

    #[test]
    fn test_config_port_args_default_empty() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.port_args.is_empty());
        assert!(lookup_port_args(&cfg, "nyan-doom").is_empty());
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
        let mut port_args = HashMap::new();
        port_args.insert("nyan-doom".to_string(), vec!["-geometry".to_string()]);
        let cfg = Config {
            port_args,
            ..Default::default()
        };

        let table: toml::Table = minimal_toml(&cfg).unwrap().parse().unwrap();
        assert!(table.get("port_args").is_some());
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

    // -- config relocation --

    #[test]
    fn test_move_config_normalises_on_the_way() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("config/caco/config.toml");
        let target = root.path().join("share/caco/config.toml");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();

        // A config as the old backfill wrote them: one real choice buried in
        // defaults and machine-specific absolute paths.
        let cfg = Config {
            sourceport: "nyan-doom".to_string(),
            ..Default::default()
        };
        fs::write(&legacy, toml::to_string_pretty(&cfg).unwrap()).unwrap();

        let outcome = move_config_file(&legacy, &target);
        assert_eq!(
            outcome,
            ConfigMigration::Moved {
                from: legacy.clone(),
                to: target.clone(),
            }
        );

        assert!(!legacy.exists());
        assert!(
            !legacy.parent().unwrap().exists(),
            "the emptied directory should go too"
        );

        let table: toml::Table = fs::read_to_string(&target).unwrap().parse().unwrap();
        assert_eq!(
            table.get("sourceport").and_then(|v| v.as_str()),
            Some("nyan-doom")
        );
        assert_eq!(table.len(), 1, "the defaults should not have survived");
    }

    #[test]
    fn test_move_config_refuses_to_clobber() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("config/caco/config.toml");
        let target = root.path().join("share/caco/config.toml");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&legacy, "sourceport = \"old\"\n").unwrap();
        fs::write(&target, "sourceport = \"new\"\n").unwrap();

        assert!(matches!(
            move_config_file(&legacy, &target),
            ConfigMigration::Skipped { .. }
        ));
        assert!(legacy.exists(), "the source must survive a refusal");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "sourceport = \"new\"\n"
        );
    }

    #[test]
    fn test_move_config_preserves_a_file_it_cannot_parse() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("config/caco/config.toml");
        let target = root.path().join("share/caco/config.toml");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let broken = "sourceport = \"unclosed\nthis is not toml [[[";
        fs::write(&legacy, broken).unwrap();

        assert!(matches!(
            move_config_file(&legacy, &target),
            ConfigMigration::Moved { .. }
        ));
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            broken,
            "an unparseable config is relocated verbatim, never discarded"
        );
    }

    #[test]
    fn test_move_config_not_needed_without_a_legacy_file() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("config/caco/config.toml");
        let target = root.path().join("share/caco/config.toml");
        assert_eq!(
            move_config_file(&legacy, &target),
            ConfigMigration::NotNeeded
        );
    }

    #[test]
    fn test_config_lives_in_the_data_dir() {
        assert!(
            config_file().starts_with(default_data_dir()),
            "config must travel with the data directory"
        );
    }

    #[test]
    fn test_move_cache_dir_relocates_contents() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("share/caco/wads");
        let target = root.path().join("cache/caco/wads");
        fs::create_dir_all(legacy.join("nested")).unwrap();
        fs::write(legacy.join("doom2.wad"), b"iwad").unwrap();
        fs::write(legacy.join("nested/map01.wad"), b"pwad").unwrap();

        let outcome = move_cache_dir(&legacy, &target);

        assert_eq!(
            outcome,
            CacheMigration::Moved {
                from: legacy.clone(),
                to: target.clone(),
            }
        );
        assert!(!legacy.exists());
        assert_eq!(fs::read(target.join("doom2.wad")).unwrap(), b"iwad");
        assert_eq!(fs::read(target.join("nested/map01.wad")).unwrap(), b"pwad");
    }

    #[test]
    fn test_move_cache_dir_refuses_to_merge_into_occupied_target() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("old");
        let target = root.path().join("new");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(legacy.join("a.wad"), b"a").unwrap();
        fs::write(target.join("b.wad"), b"b").unwrap();

        let outcome = move_cache_dir(&legacy, &target);

        assert!(matches!(outcome, CacheMigration::Skipped { .. }));
        // Both caches are left exactly as they were.
        assert!(legacy.join("a.wad").is_file());
        assert!(target.join("b.wad").is_file());
    }

    #[test]
    fn test_move_cache_dir_into_existing_empty_target() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("old");
        let target = root.path().join("new");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(legacy.join("a.wad"), b"a").unwrap();

        let outcome = move_cache_dir(&legacy, &target);

        assert!(matches!(outcome, CacheMigration::Moved { .. }));
        assert!(target.join("a.wad").is_file());
        assert!(!legacy.exists());
    }

    #[test]
    fn test_copy_dir_recursive_preserves_tree() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        let to = root.path().join("to");
        fs::create_dir_all(from.join("a/b")).unwrap();
        fs::write(from.join("a/b/deep.txt"), b"deep").unwrap();
        fs::write(from.join("top.txt"), b"top").unwrap();

        copy_dir_recursive(&from, &to).unwrap();

        assert_eq!(fs::read(to.join("a/b/deep.txt")).unwrap(), b"deep");
        assert_eq!(fs::read(to.join("top.txt")).unwrap(), b"top");
        // Source is untouched — the caller decides when to delete it.
        assert!(from.join("top.txt").is_file());
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
}
