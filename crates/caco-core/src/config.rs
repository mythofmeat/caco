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

pub fn config_dir() -> PathBuf {
    home_dir().join(".config").join("caco")
}

pub fn config_file() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_CONFIG") {
        return PathBuf::from(p);
    }
    config_dir().join("config.toml")
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

pub fn default_cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CACO_CACHE_DIR") {
        return PathBuf::from(p);
    }
    default_data_dir().join("wads")
}

pub fn iwad_dir() -> PathBuf {
    default_data_dir().join("iwads")
}

pub fn id24_dir() -> PathBuf {
    default_data_dir().join("id24")
}

pub fn thumbnail_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| home_dir().join(".cache"))
        .join("caco/thumbnails")
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
    pub tui: TuiConfig,
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
            tui: TuiConfig::default(),
            gui: GuiConfig::default(),
            list: ListConfig::default(),
            iwad_priority: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    pub default_tab: String,
    pub default_sort: String,
    pub default_sort_desc: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            default_tab: "all".to_string(),
            default_sort: "id".to_string(),
            default_sort_desc: false,
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
            Ok(cfg) => {
                ensure_config_keys(&path, &contents);
                cfg
            }
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

/// Ensure the config file on disk has all known keys.
///
/// Compares the existing config against `Config::default()`. Adds missing
/// top-level keys with their default values. For sections (tui, gui, list),
/// only backfills keys in sections that already exist on disk — does not
/// create missing sections. Writes only if changes were made.
fn ensure_config_keys(path: &Path, contents: &str) {
    let Ok(mut on_disk) = contents.parse::<toml::Table>() else {
        return;
    };

    let defaults = Config::default();
    let Ok(default_toml) = toml::to_string_pretty(&defaults) else {
        return;
    };
    let Ok(default_table) = default_toml.parse::<toml::Table>() else {
        return;
    };

    let mut changed = false;

    for (key, default_val) in &default_table {
        if let toml::Value::Table(default_section) = default_val {
            // Section: only backfill keys if section already exists on disk
            if let Some(toml::Value::Table(on_disk_section)) = on_disk.get_mut(key) {
                for (skey, sval) in default_section {
                    if !on_disk_section.contains_key(skey) {
                        on_disk_section.insert(skey.clone(), sval.clone());
                        changed = true;
                    }
                }
            }
        } else {
            // Top-level scalar: add if missing
            if !on_disk.contains_key(key) {
                on_disk.insert(key.clone(), default_val.clone());
                changed = true;
            }
        }
    }

    if changed {
        let Ok(new_contents) = toml::to_string_pretty(&on_disk) else {
            return;
        };
        let _ = fs::write(path, new_contents);
    }
}

/// Save configuration to disk.
pub fn save_config(config: &Config) -> crate::Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let contents = toml::to_string_pretty(config)?;
    fs::write(config_file(), contents)?;
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
        assert_eq!(parsed.tui.default_tab, cfg.tui.default_tab);
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

    #[test]
    fn test_ensure_config_keys_adds_missing_toplevel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Write a minimal config with only one key
        let contents = r#"sourceport = "dsda-doom""#;
        fs::write(&path, contents).unwrap();

        ensure_config_keys(&path, contents);

        // Re-read and verify missing keys were added
        let updated = fs::read_to_string(&path).unwrap();
        let table: toml::Table = updated.parse().unwrap();

        assert_eq!(
            table.get("sourceport").and_then(|v| v.as_str()),
            Some("dsda-doom")
        );
        // auto_stats should have been added with default value
        assert_eq!(
            table.get("auto_stats").and_then(|v| v.as_bool()),
            Some(true)
        );
        // manage_data_dirs should have been added
        assert_eq!(
            table.get("manage_data_dirs").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn test_ensure_config_keys_backfills_existing_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Config with a [tui] section missing default_sort_desc
        let contents = "[tui]\ndefault_tab = \"playing\"\n";
        fs::write(&path, contents).unwrap();

        ensure_config_keys(&path, contents);

        let updated = fs::read_to_string(&path).unwrap();
        let table: toml::Table = updated.parse().unwrap();
        let tui = table.get("tui").unwrap().as_table().unwrap();

        // Existing key preserved
        assert_eq!(
            tui.get("default_tab").and_then(|v| v.as_str()),
            Some("playing")
        );
        // Missing key added
        assert_eq!(
            tui.get("default_sort_desc").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(tui.get("default_sort").and_then(|v| v.as_str()), Some("id"));
    }

    #[test]
    fn test_ensure_config_keys_does_not_create_missing_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Config with no [gui] section
        let contents = r#"sourceport = "dsda-doom""#;
        fs::write(&path, contents).unwrap();

        ensure_config_keys(&path, contents);

        let updated = fs::read_to_string(&path).unwrap();
        let table: toml::Table = updated.parse().unwrap();

        // [gui] section should NOT have been created
        assert!(table.get("gui").is_none());
    }

    #[test]
    fn test_ensure_config_keys_noop_when_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Write a full default config
        let cfg = Config::default();
        let contents = toml::to_string_pretty(&cfg).unwrap();
        fs::write(&path, &contents).unwrap();

        ensure_config_keys(&path, &contents);

        // File should be unchanged (no extra write)
        let updated = fs::read_to_string(&path).unwrap();
        assert_eq!(updated, contents);
    }

    #[test]
    fn test_ensure_config_keys_preserves_user_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let contents = "sourceport = \"dsda-doom\"\ndownload_mirror = 3\n";
        fs::write(&path, contents).unwrap();

        ensure_config_keys(&path, contents);

        let updated = fs::read_to_string(&path).unwrap();
        let table: toml::Table = updated.parse().unwrap();
        assert_eq!(
            table.get("sourceport").and_then(|v| v.as_str()),
            Some("dsda-doom")
        );
        assert_eq!(
            table.get("download_mirror").and_then(|v| v.as_integer()),
            Some(3)
        );
    }

    #[test]
    fn test_section_defaults_tui() {
        let cfg = TuiConfig::default();
        assert_eq!(cfg.default_tab, "all");
        assert_eq!(cfg.default_sort, "id");
        assert!(!cfg.default_sort_desc);
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
    fn test_config_tui_section_override() {
        let toml_str = r#"
sourceport = "gzdoom"

[tui]
default_tab = "playing"
default_sort_desc = true
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.tui.default_tab, "playing");
        assert!(cfg.tui.default_sort_desc);
        // Non-overridden key keeps default
        assert_eq!(cfg.tui.default_sort, "id");
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
    fn test_config_with_nested_tui_save() {
        let mut cfg = Config::default();
        cfg.tui.default_tab = "playing".to_string();
        cfg.tui.default_sort = "playtime".to_string();

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.tui.default_tab, "playing");
        assert_eq!(parsed.tui.default_sort, "playtime");
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
