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
// CACO_SYNC_DIR   — override the folder of machines' logs
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
    if let Some(p) = env_path("CACO_CONFIG") {
        return p;
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
    if let Some(p) = env_path("CACO_HOME") {
        return p;
    }
    dirs::data_dir()
        .unwrap_or_else(|| home_dir().join(".local/share"))
        .join("caco")
}

pub fn default_db_path() -> PathBuf {
    if let Some(p) = env_path("CACO_DB_PATH") {
        return p;
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
    if let Some(p) = env_path("CACO_CACHE_HOME") {
        return p;
    }
    dirs::cache_dir()
        .unwrap_or_else(|| home_dir().join(".cache"))
        .join("caco")
}

pub fn default_cache_dir() -> PathBuf {
    if let Some(p) = env_path("CACO_CACHE_DIR") {
        return p;
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
    if let Some(p) = env_path("CACO_DATA_DIR") {
        return p;
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

/// What the directory of backups is called, in the data dir and beside a database.
const BACKUPS_DIR_NAME: &str = "backups";

pub fn backup_dir() -> PathBuf {
    default_data_dir().join(BACKUPS_DIR_NAME)
}

/// Where the database at `db_path` is snapshotted before a migration: beside it,
/// which for a library at its default path is [`backup_dir`].
///
/// Worked out from the database rather than read from config, for the reason
/// `GcPaths` is explicit. Otherwise a test's database in a tempdir snapshots into the
/// developer's real data dir, and a `CACO_DB_PATH` database overwrites the real
/// library's snapshot of the same schema version.
pub fn migration_backup_dir(db_path: &Path) -> PathBuf {
    db_path.with_file_name(BACKUPS_DIR_NAME)
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

/// Where user Cacowards files live, merged over the dataset compiled into the
/// binary: see [`crate::cacowards`].
///
/// On the portable side, because a correction or a private addition cannot be
/// re-derived from anything.
pub fn cacoward_data_dir() -> PathBuf {
    default_data_dir().join("cacowards")
}

/// Where tokens pasted into caco for its mirrors are saved: see
/// [`crate::mirror_tokens`], which says why this is on neither side of the data split.
///
/// `CACO_TOKEN_FILE` overrides it. So does `CACO_HOME`, which is a complete isolation
/// switch, though beside that directory rather than in it: a `CACO_HOME` can be a real
/// library in a git repository as easily as a scratch one.
pub fn mirror_token_file() -> PathBuf {
    if let Some(p) = env_path("CACO_TOKEN_FILE") {
        return p;
    }
    if let Some(home) = env_path("CACO_HOME")
        && let Some(name) = home.file_name()
    {
        let beside = format!("{}.mirror-tokens.toml", name.to_string_lossy());
        return home.with_file_name(beside);
    }
    dirs::preference_dir()
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("caco")
        .join("mirror-tokens.toml")
}

/// A file that belongs to this machine and travels with nothing, beside the mirror tokens
/// for the same reasons. `CACO_HOME` and `CACO_CONFIG` each keep a run away from the real
/// one, by putting it beside themselves rather than in them, since either of those can be
/// a real library that gets synced.
fn machine_local_file(override_var: &str, name: &str) -> PathBuf {
    if let Some(p) = env_path(override_var) {
        return p;
    }
    for var in ["CACO_HOME", "CACO_CONFIG"] {
        if let Some(value) = env_path(var)
            && let Some(base) = value.file_name()
        {
            let beside = format!("{}.{name}", base.to_string_lossy());
            return value.with_file_name(beside);
        }
    }
    dirs::preference_dir()
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("caco")
        .join(name)
}

/// The settings that differ from one machine to the next: see [`MACHINE_KEYS`].
pub fn machine_config_file() -> PathBuf {
    machine_local_file("CACO_MACHINE_CONFIG", "machine.toml")
}

/// Which machine this is: see [`crate::sync::machine`].
pub fn machine_id_file() -> PathBuf {
    machine_local_file("CACO_MACHINE_ID", "machine-id")
}

/// The folder holding each machine's log of what happened on it, for the library at
/// `db_path`: see [`crate::sync`].
///
/// A library anywhere but the configured place has a folder of its own beside it, for the
/// reason its migration backups do. A database opened with `--db-path` to look at
/// something must not write into the real logs, or read them into itself.
pub fn sync_dir_for(db_path: &Path) -> PathBuf {
    let beside = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("sync");
    if db_path != get_db_path() {
        return beside;
    }
    if let Some(p) = env_path("CACO_SYNC_DIR") {
        return p;
    }
    let cfg = load_config();
    if cfg.sync_dir.is_empty() {
        beside
    } else {
        configured_dir(&cfg.sync_dir)
    }
}

/// The local index of the idgames archive, as its own SQLite file: see
/// `caco_sources::idgames::index`.
///
/// Cache side and not in `library.db`, because all of it can be fetched again and a
/// library meant to be copied between machines should not carry tens of megabytes of
/// someone else's catalogue. `CACO_IDGAMES_INDEX` overrides just this, as `CACO_CACHE_DIR`
/// does the WAD cache.
pub fn idgames_index_path() -> PathBuf {
    if let Some(p) = env_path("CACO_IDGAMES_INDEX") {
        return p;
    }
    cache_home().join("idgames-index.db")
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
    /// What Get does with a file it links out of the Downloads folder: `move` or
    /// `copy`. See [`LinkMode`].
    pub link_mode: String,
    /// The folder Get watches. Empty means the system's: see [`downloads_dir`].
    pub downloads_dir: String,
    /// The folder of machines' logs. Empty means beside the database: see [`sync_dir_for`].
    pub sync_dir: String,
    /// Whether to ask before launching a WAD another machine has newer saves for. Worth
    /// turning off for someone who keeps a separate game going on each machine, since
    /// the question would then come up every time they swapped.
    pub warn_newer_saves: bool,
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
    /// How often to ask a mirror for a new listing of the idgames archive. `0` leaves
    /// the local index as it is, so caco asks nothing of anyone at startup, and search
    /// goes by what is already in it and then by the API.
    pub idgames_index_check_days: i64,
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

    /// Stores of WAD files the user controls, tried before anywhere else in the
    /// order they are listed. See [`crate::mirrors`].
    pub mirrors: Vec<MirrorConfig>,

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
            downloads_dir: String::new(),
            sync_dir: String::new(),
            warn_newer_saves: true,
            manage_data_dirs: true,
            auto_stats: true,
            auto_detect_iwad: true,
            auto_detect_complevel: true,
            auto_doomwiki_enrich: true,
            cache_max_size_gb: 0.0,
            cache_max_age_days: 0,
            cache_auto_clean: false,
            sourceport_update_check_days: 1,
            idgames_index_check_days: 1,
            data_dir: default_data_subdir().to_string_lossy().into_owned(),
            iwad_dir: iwad_dir().to_string_lossy().into_owned(),
            profile_dir: String::new(),
            companion_orphan_cleanup: "ask".to_string(),
            zdoom_sourceport: String::new(),
            sourceport_preferences: HashMap::new(),
            executable_args: HashMap::new(),
            mirrors: Vec::new(),
            gui: GuiConfig::default(),
            list: ListConfig::default(),
            iwad_priority: HashMap::new(),
        }
    }
}

/// What becomes of a file caco takes from outside its store directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMode {
    Move,
    Copy,
}

impl LinkMode {
    /// Read the `link_mode` setting. Only an explicit `move` moves, because a typo in
    /// a config file should not make files vanish from where the user left them.
    pub fn parse(value: &str) -> Self {
        if value == "move" {
            Self::Move
        } else {
            Self::Copy
        }
    }
}

/// One `[[mirrors]]` entry: a store of WAD files the user controls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MirrorConfig {
    /// What the library knows it by. The synced index is keyed on this, so
    /// renaming a mirror reads its index again under the new name.
    pub name: String,
    /// Its root over HTTP. A mirror has this or a `path`.
    pub url: Option<String>,
    /// Its root on a filesystem. A mirror has this or a `url`.
    pub path: Option<String>,
    /// The login, for a store that wants a username and password, as WebDAV servers
    /// do. The token is the password then. Without this it goes as a bearer token.
    pub username: Option<String>,
    /// The environment variable holding the token, for those who would rather it was
    /// not saved anywhere. Wins over a saved one.
    pub token_env: Option<String>,
    /// Whether caco may upload to it. Never to a copy of idgames, whatever this says:
    /// see [`MirrorConfig::takes_uploads`].
    pub write: bool,
    /// How its files are arranged: `caco` (the default) or `idgames`. See
    /// [`MirrorLayout`].
    pub layout: Option<String>,
    /// The token saved on this machine, read from [`mirror_token_file`] along with
    /// the config. Never written to the config, which is meant to travel.
    #[serde(skip)]
    pub saved_token: Option<SavedToken>,
}

/// A token, kept out of anything that prints a config.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SavedToken(pub String);

impl std::fmt::Debug for SavedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SavedToken(..)")
    }
}

/// How a mirror's files are arranged, which decides how a file on it is named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorLayout {
    /// `<sha256>/<filename>` with an index: see [`crate::mirrors`]. What caco makes.
    Caco,
    /// `levels/doom2/Ports/megawads/sunlust.zip`: a copy of the idgames archive, made
    /// by something else, which has its path and its name and nothing else to go by.
    Idgames,
}

impl MirrorLayout {
    /// How `layout` spells this in a config file.
    pub const IDGAMES: &str = "idgames";
}

/// Where a mirror's root is, once [`MirrorConfig::location`] has checked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorLocation {
    Url(String),
    Path(PathBuf),
}

impl MirrorConfig {
    /// Where this mirror's root is, or what is wrong with how it says so.
    pub fn location(&self) -> Result<MirrorLocation, String> {
        let nonempty = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        };
        match (nonempty(&self.url), nonempty(&self.path)) {
            (Some(_), Some(_)) => Err("set either a URL or a path, not both".to_string()),
            (None, None) => Err("set a URL or a path".to_string()),
            (Some(url), None) if url.starts_with("http://") || url.starts_with("https://") => {
                Ok(MirrorLocation::Url(url.trim_end_matches('/').to_string()))
            }
            (Some(url), None) => Err(format!("{url} is not an http:// or https:// URL")),
            (None, Some(path)) => {
                // Absolute because caco can be started from anywhere, and a
                // relative path would mean a different mirror each time.
                let path = expand_tilde(&path);
                if path.is_absolute() {
                    Ok(MirrorLocation::Path(path))
                } else {
                    Err(format!("{} is not an absolute path", path.display()))
                }
            }
        }
    }

    /// How this mirror's files are arranged, or what is wrong with how it says so. A
    /// string in the file rather than an enum, because a typo in an enum would fail the
    /// whole config, and with it every setting in the file.
    pub fn layout(&self) -> Result<MirrorLayout, String> {
        match self.layout.as_deref().map(str::trim) {
            None | Some("") | Some("caco") => Ok(MirrorLayout::Caco),
            Some(MirrorLayout::IDGAMES) => Ok(MirrorLayout::Idgames),
            Some(other) => Err(format!(
                "its layout is \"{other}\", which is neither \"caco\" nor \"{}\"",
                MirrorLayout::IDGAMES
            )),
        }
    }

    /// Whether this is a copy of the idgames archive rather than a mirror caco keeps.
    pub fn is_idgames_copy(&self) -> bool {
        self.layout() == Ok(MirrorLayout::Idgames)
    }

    /// Whether caco may put files on this mirror. Not on a copy of idgames, which belongs to
    /// whatever keeps it up to date, and is meant to hold what the archive holds. An index
    /// on it would also make it look like a mirror caco keeps.
    pub fn takes_uploads(&self) -> bool {
        self.write && self.layout() == Ok(MirrorLayout::Caco)
    }

    /// The token or password to present, if there is one on this machine.
    pub fn token(&self) -> Option<String> {
        self.token_env_name()
            .and_then(|name| std::env::var(name).ok())
            .filter(|token| !token.is_empty())
            .or_else(|| self.saved_token.as_ref().map(|token| token.0.clone()))
    }

    /// The username to log in with, if the store wants one.
    pub fn login(&self) -> Option<&str> {
        self.username
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }

    /// The name of the variable the token should be in, if it should be in one.
    pub fn token_env_name(&self) -> Option<&str> {
        self.token_env
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
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
    let mut cfg = read_config_files(&config_file(), &machine_config_file());
    attach_saved_tokens(&mut cfg);
    cfg
}

/// [`read_config_from_disk`] from explicit files, without the machine's tokens.
fn read_config_files(path: &Path, machine_path: &Path) -> Config {
    let mut table = match fs::read_to_string(path) {
        Ok(contents) => match contents.parse::<toml::Table>() {
            Ok(table) => table,
            Err(e) => return invalid_config(path, &e),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => {
            eprintln!("Warning: Failed to load config: {e}");
            eprintln!("Warning: Using default configuration.");
            return Config::default();
        }
    };
    overlay_machine_settings(&mut table, read_machine_settings(machine_path));

    match toml::Value::Table(table).try_into::<Config>() {
        Ok(cfg) => cfg,
        Err(e) => invalid_config(path, &e),
    }
}

fn invalid_config(path: &Path, error: &dyn std::fmt::Display) -> Config {
    eprintln!(
        "Warning: Invalid TOML syntax in {}: {error}",
        path.display()
    );
    eprintln!("Warning: Using default configuration.");
    Config::default()
}

/// What this machine's own file says, if it has one yet. A file that cannot be read is
/// left alone and said to be unusable, rather than taken for an empty one, which saving
/// any setting would then overwrite.
fn read_machine_settings(path: &Path) -> Option<toml::Table> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Warning: Failed to read {}: {e}", path.display());
            }
            return None;
        }
    };
    match contents.parse() {
        Ok(table) => Some(table),
        Err(e) => {
            eprintln!("Warning: Invalid TOML syntax in {}: {e}", path.display());
            None
        }
    }
}

/// The settings that belong to a machine and not to a library.
///
/// Sourceports and IWADs live in different places on Linux and on a Mac, so one
/// `config.toml` in a synced folder has one machine's paths in it and a conflict every
/// time they both change it. These go in [`machine_config_file`], and the rest stays in
/// `config.toml`, which travels with the data dir.
pub const MACHINE_KEYS: &[&str] = &[
    "sourceport",
    "zdoom_sourceport",
    "sourceport_preferences",
    "sourceport_args",
    "executable_args",
    "iwad",
    "iwad_dirs",
    "iwad_dir",
    "cache_dir",
    "db_path",
    "data_dir",
    "profile_dir",
    "downloads_dir",
    "sync_dir",
];

/// Put this machine's settings over what `config.toml` says.
///
/// A machine with no file of its own yet goes by what `config.toml` says, which is what
/// an existing install's first run after this looks like: the settings stay as they were
/// until one is saved, which moves them all across. Once there is a file it is the whole
/// answer, so that a setting cleared here is not brought back by what another machine
/// left in `config.toml` from before.
fn overlay_machine_settings(table: &mut toml::Table, machine: Option<toml::Table>) {
    let Some(machine) = machine else {
        return;
    };
    for key in MACHINE_KEYS {
        table.remove(*key);
        if let Some(value) = machine.get(*key) {
            table.insert((*key).to_string(), value.clone());
        }
    }
}

/// Split non-default settings into what travels and what belongs to this machine.
fn split_machine_settings(mut table: toml::Table) -> (toml::Table, toml::Table) {
    let mut machine = toml::Table::new();
    for key in MACHINE_KEYS {
        if let Some(value) = table.remove(*key) {
            machine.insert((*key).to_string(), value);
        }
    }
    (table, machine)
}

/// Give each mirror the token saved for it on this machine. Only read if there is a
/// mirror, so that a config with none touches nothing outside the data dir.
fn attach_saved_tokens(config: &mut Config) {
    if config.mirrors.is_empty() {
        return;
    }
    let tokens = crate::mirror_tokens::TokenFile::from_config().read();
    for mirror in &mut config.mirrors {
        mirror.saved_token = tokens.get(&mirror.name).cloned().map(SavedToken);
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
///   defaults live on `Config` itself, not echoed back.
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

/// A config's non-default settings.
fn minimal_table(config: &Config) -> crate::Result<toml::Table> {
    let current: toml::Table = toml::Value::try_from(config)?
        .as_table()
        .cloned()
        .unwrap_or_default();
    let default: toml::Table = toml::Value::try_from(Config::default())?
        .as_table()
        .cloned()
        .unwrap_or_default();

    Ok(strip_defaults(current, &default))
}

/// Serialize a config down to just its non-default settings.
#[cfg(test)]
fn minimal_toml(config: &Config) -> crate::Result<String> {
    Ok(toml::to_string_pretty(&minimal_table(config)?)?)
}

/// Save configuration to disk: what travels to `config.toml`, and what belongs to this
/// machine to its own file.
///
/// The machine's file first, so that dying in between leaves a setting in both places,
/// which reads the same, rather than in neither.
pub fn save_config(config: &Config) -> crate::Result<()> {
    write_config_files(config, &config_file(), &machine_config_file())
}

/// [`save_config`] to explicit files.
fn write_config_files(config: &Config, path: &Path, machine_path: &Path) -> crate::Result<()> {
    let (portable, machine) = split_machine_settings(minimal_table(config)?);

    if let Some(parent) = machine_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(machine_path, toml::to_string_pretty(&machine)?)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml::to_string_pretty(&portable)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Derived path helpers
// ---------------------------------------------------------------------------

/// Get the database file path. Env var `CACO_DB_PATH` takes precedence over config.
pub fn get_db_path() -> PathBuf {
    if let Some(p) = env_path("CACO_DB_PATH") {
        return p;
    }
    let cfg = load_config();
    let p = &cfg.db_path;
    if p.is_empty() {
        default_db_path()
    } else {
        configured_dir(p)
    }
}

/// Get the WAD cache directory. Env var `CACO_CACHE_DIR` takes precedence over config.
pub fn get_cache_dir() -> PathBuf {
    if let Some(p) = env_path("CACO_CACHE_DIR") {
        return p;
    }
    let cfg = load_config();
    let p = &cfg.cache_dir;
    if p.is_empty() {
        default_cache_dir()
    } else {
        configured_dir(p)
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
        configured_dir(p)
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
        .map(|d| configured_dir(d))
        .collect()
}

/// Get the base directory for per-WAD data directories.
/// Env var `CACO_DATA_DIR` takes precedence over config.
pub fn get_data_dir() -> PathBuf {
    if let Some(p) = env_path("CACO_DATA_DIR") {
        return p;
    }
    let cfg = load_config();
    let p = &cfg.data_dir;
    if p.is_empty() {
        default_data_subdir()
    } else {
        configured_dir(p)
    }
}

/// Get the sourceport config profiles directory.
pub fn get_profile_dir() -> PathBuf {
    let cfg = load_config();
    let p = &cfg.profile_dir;
    if p.is_empty() {
        profile_dir()
    } else {
        configured_dir(p)
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

/// Where the system's browsers save files unless told otherwise.
///
/// `dirs` answers from `XDG_DOWNLOAD_DIR` on Linux, which a desktop without
/// xdg-user-dirs does not set, and the folder is there all the same.
pub fn default_downloads_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| home_dir().join("Downloads"))
}

/// The folder Get watches for a browser's download. Settable because a sandboxed
/// browser, or one told to, can save somewhere else.
pub fn downloads_dir() -> PathBuf {
    downloads_dir_from(&load_config())
}

fn downloads_dir_from(config: &Config) -> PathBuf {
    let set = config.downloads_dir.trim();
    if set.is_empty() {
        default_downloads_dir()
    } else {
        configured_dir(set)
    }
}

/// Get the configured link mode: what Get does with a file that lands in Downloads.
pub fn get_link_mode() -> LinkMode {
    LinkMode::parse(&load_config().link_mode)
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

/// Whether to ask before launching a WAD another machine has newer saves for.
pub fn get_warn_newer_saves() -> bool {
    load_config().warn_newer_saves
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

/// Whether `key` could name a WAD's data folder.
///
/// A rowid means something different on every machine, so these go by a key that travels
/// with the row: the rowid a WAD had when this was introduced, or else its uid. Only those
/// shapes, because what is read from a folder name decides which folders count as a WAD's
/// at all, and what comes from a log goes into a path. No `_`, which is what ends one.
pub fn is_data_key(key: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if digits(key) {
        return true;
    }
    if key.len() == 32
        && key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return true;
    }
    key.split_once('-').is_some_and(|(source, id)| {
        matches!(source, "idgames" | "doomwiki" | "doomworld") && digits(id)
    })
}

/// The data key at the front of a `{key}_{title}` name, if it has one.
pub fn data_key_of(name: &str) -> Option<&str> {
    let (key, _) = name.split_once('_')?;
    is_data_key(key).then_some(key)
}

/// Return the per-WAD data directory path.
///
/// Format: `{data_dir}/{key}_{sanitized_title}/`
pub fn get_wad_data_dir(key: &str, title: &str) -> PathBuf {
    get_data_dir().join(format!("{key}_{}", sanitize_dirname(title)))
}

/// Find an existing per-WAD data directory by key.
///
/// Handles title renames, by matching `{key}_*`. Two WADs can share a key if each machine
/// gave its own WAD the same rowid before there were uids. Each has its own folder, under
/// its own title, so that is looked for first.
pub fn find_wad_data_dir(key: &str, title: &str) -> Option<PathBuf> {
    find_data_dir_in(&get_data_dir(), key, title)
}

/// [`find_wad_data_dir`] under an explicit root. Blank `title` for any folder with that key.
pub fn find_data_dir_in(root: &Path, key: &str, title: &str) -> Option<PathBuf> {
    let prefix = format!("{key}_");
    let mut candidates: Vec<PathBuf> = fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect();
    // The same answer every time, since `read_dir` comes in no particular order.
    candidates.sort();
    choose_data_dir(&candidates, title).cloned()
}

/// Which of the folders with one key belongs to the WAD with this title: the one named for
/// it if there is one, or else the first. Sorted, by the caller, so that it stays the first.
pub fn choose_data_dir<'a>(dirs: &'a [PathBuf], title: &str) -> Option<&'a PathBuf> {
    let exact = sanitize_dirname(title);
    dirs.iter()
        .find(|dir| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.split_once('_'))
                .is_some_and(|(_, rest)| rest == exact)
        })
        .or_else(|| dirs.first())
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
/// A path from the environment, made absolute against the directory caco was started in,
/// which is what it meant there. Every directory caco works in is absolute, because a
/// sourceport is run from a WAD's data folder (`player::build_launch`), where a relative
/// path handed on to it would mean somewhere else.
fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|p| anchored(PathBuf::from(p)))
}

/// A directory from `config.toml`, absolute for the reason [`env_path`]'s are.
fn configured_dir(p: &str) -> PathBuf {
    anchored(expand_tilde(p))
}

/// `path` against caco's own working directory, or as it was if there is none to be had.
fn anchored(path: PathBuf) -> PathBuf {
    std::path::absolute(&path).unwrap_or(path)
}

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
    fn a_library_at_its_default_path_snapshots_into_the_backup_dir() {
        assert_eq!(migration_backup_dir(&default_db_path()), backup_dir());
    }

    #[test]
    fn a_database_anywhere_else_snapshots_beside_itself() {
        assert_eq!(
            migration_backup_dir(Path::new("/mnt/games/caco.db")),
            Path::new("/mnt/games/backups")
        );
    }

    #[test]
    fn test_directories_from_the_environment_and_the_config_are_absolute() {
        // A variable no other test reads, since tests share the process's environment.
        const VAR: &str = "CACO_TEST_ENV_PATH_IS_ABSOLUTE";
        let here = std::env::current_dir().unwrap();

        // SAFETY: test-only env mutation of a variable nothing else reads.
        unsafe { std::env::set_var(VAR, "scratch/library") };
        assert_eq!(env_path(VAR), Some(here.join("scratch/library")));
        unsafe { std::env::set_var(VAR, "/elsewhere/library") };
        assert_eq!(env_path(VAR), Some(PathBuf::from("/elsewhere/library")));
        unsafe { std::env::remove_var(VAR) };
        assert_eq!(env_path(VAR), None);

        assert_eq!(configured_dir("wads"), here.join("wads"));
        assert_eq!(configured_dir("~/wads"), home_dir().join("wads"));
        assert_eq!(configured_dir("/srv/wads"), PathBuf::from("/srv/wads"));
    }

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
        let dir = get_wad_data_dir("42", "Scythe 2");
        let name = dir.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "42_scythe-2");
    }

    #[test]
    fn test_what_can_name_a_wads_data_folder() {
        for good in [
            "42",
            "idgames-18000",
            "doomwiki-7",
            "doomworld-151858",
            "0123456789abcdef0123456789abcdef",
        ] {
            assert!(is_data_key(good), "{good}");
        }
        for bad in [
            "",
            "-",
            "../../etc",
            "idgames-",
            "idgames-12a",
            "other-12",
            "my",
            "stuff-1",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef",
            "42_scythe",
        ] {
            assert!(!is_data_key(bad), "{bad}");
        }
    }

    /// What is not a WAD's folder is not one to clean up.
    #[test]
    fn test_only_a_folder_named_for_a_key_has_one() {
        assert_eq!(data_key_of("42_scythe-2"), Some("42"));
        assert_eq!(data_key_of("7_a_b_c"), Some("7"));
        assert_eq!(data_key_of("idgames-5_x"), Some("idgames-5"));
        assert_eq!(
            data_key_of("0123456789abcdef0123456789abcdef_x"),
            Some("0123456789abcdef0123456789abcdef")
        );
        for none in ["noprefix", "_leading", "12a_mixed", "my_stuff", "a-1_x"] {
            assert_eq!(data_key_of(none), None, "{none}");
        }
    }

    #[test]
    fn test_a_data_dir_is_found_through_a_title_change() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("7_old-title")).unwrap();
        fs::create_dir_all(dir.path().join("77_other")).unwrap();

        let found = find_data_dir_in(dir.path(), "7", "New Title").unwrap();
        assert_eq!(found.file_name().unwrap(), "7_old-title");
        assert!(find_data_dir_in(dir.path(), "8", "x").is_none());
    }

    /// Two machines each gave a different WAD the same rowid, before there were uids.
    #[test]
    fn test_two_wads_sharing_a_key_each_get_their_own_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("117_ancient-aliens")).unwrap();
        fs::create_dir_all(dir.path().join("117_sunlust")).unwrap();

        let find = |title| {
            find_data_dir_in(dir.path(), "117", title)
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(find("Sunlust"), "117_sunlust");
        assert_eq!(find("Ancient Aliens"), "117_ancient-aliens");
    }

    #[test]
    fn test_a_database_opened_elsewhere_has_its_own_logs() {
        assert_eq!(
            sync_dir_for(Path::new("/mnt/games/test.db")),
            Path::new("/mnt/games/sync")
        );
    }

    /// What an existing install's first save after this does, on disk: moves them across,
    /// with nothing changing as far as anything reading the settings can tell.
    #[test]
    fn test_saving_moves_a_machines_settings_out_of_the_file_that_travels() {
        let dir = tempfile::tempdir().unwrap();
        let (portable, machine) = (
            dir.path().join("config.toml"),
            dir.path().join("m/machine.toml"),
        );
        fs::write(
            &portable,
            "sourceport = \"nyan-doom\"\ncache_dir = \"/fast\"\nlink_mode = \"copy\"\n",
        )
        .unwrap();

        let before = read_config_files(&portable, &machine);
        assert_eq!(before.sourceport, "nyan-doom");
        assert!(!machine.exists(), "reading settings writes nothing");

        write_config_files(&before, &portable, &machine).unwrap();

        let travels = fs::read_to_string(&portable).unwrap();
        assert!(travels.contains("link_mode"));
        assert!(!travels.contains("sourceport"), "{travels}");
        assert!(!travels.contains("cache_dir"), "{travels}");
        let stays = fs::read_to_string(&machine).unwrap();
        assert!(
            stays.contains("sourceport") && stays.contains("cache_dir"),
            "{stays}"
        );

        let after = read_config_files(&portable, &machine);
        assert_eq!(after.sourceport, "nyan-doom");
        assert_eq!(after.cache_dir, "/fast");
        assert_eq!(after.link_mode, "copy");
    }

    /// The conflict this exists to end: a config another machine wrote, arriving by sync,
    /// says nothing about what this one has set up.
    #[test]
    fn test_another_machines_settings_arriving_change_nothing_here() {
        let dir = tempfile::tempdir().unwrap();
        let (portable, machine) = (
            dir.path().join("config.toml"),
            dir.path().join("machine.toml"),
        );
        let mine = Config {
            sourceport: "/usr/bin/nyan-doom".to_string(),
            ..Config::default()
        };
        write_config_files(&mine, &portable, &machine).unwrap();

        fs::write(
            &portable,
            "sourceport = \"/Applications/Nyan.app\"\nlink_mode = \"copy\"\n",
        )
        .unwrap();

        let read = read_config_files(&portable, &machine);
        assert_eq!(read.sourceport, "/usr/bin/nyan-doom");
        assert_eq!(read.link_mode, "copy", "and the rest of it still travels");
    }

    /// Damaged is not empty, or saving a setting would write over what could be fixed by hand.
    #[test]
    fn test_a_damaged_machine_file_is_ignored_rather_than_taken_for_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (portable, machine) = (
            dir.path().join("config.toml"),
            dir.path().join("machine.toml"),
        );
        fs::write(&portable, "sourceport = \"from-the-config\"\n").unwrap();
        fs::write(&machine, "this is [not toml").unwrap();
        assert_eq!(
            read_config_files(&portable, &machine).sourceport,
            "from-the-config"
        );
    }

    #[test]
    fn test_settings_split_into_what_travels_and_what_stays() {
        let cfg = Config {
            sourceport: "nyan-doom".to_string(),
            cache_dir: "/fast/cache".to_string(),
            sync_dir: "~/Sync/caco".to_string(),
            link_mode: "copy".to_string(),
            auto_stats: false,
            ..Config::default()
        };
        let (portable, machine) = split_machine_settings(minimal_table(&cfg).unwrap());

        assert_eq!(portable.get("link_mode").unwrap().as_str(), Some("copy"));
        assert_eq!(portable.get("auto_stats").unwrap().as_bool(), Some(false));
        assert_eq!(portable.len(), 2);

        assert_eq!(
            machine.get("sourceport").unwrap().as_str(),
            Some("nyan-doom")
        );
        assert_eq!(
            machine.get("cache_dir").unwrap().as_str(),
            Some("/fast/cache")
        );
        assert_eq!(
            machine.get("sync_dir").unwrap().as_str(),
            Some("~/Sync/caco")
        );
        assert_eq!(machine.len(), 3);
    }

    #[test]
    fn test_every_machine_key_is_a_real_setting() {
        let toml::Value::Table(table) = toml::Value::try_from(Config::default()).unwrap() else {
            panic!("a config is a table");
        };
        for key in MACHINE_KEYS {
            assert!(table.contains_key(*key), "{key}");
        }
    }

    fn table(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    /// An existing install's first run: nothing has changed until something is saved.
    #[test]
    fn test_a_machine_with_no_file_goes_by_what_the_config_says() {
        let mut settings = table(r#"sourceport = "dsda-doom""#);
        overlay_machine_settings(&mut settings, None);
        assert_eq!(
            settings.get("sourceport").unwrap().as_str(),
            Some("dsda-doom")
        );
    }

    #[test]
    fn test_a_machines_own_file_wins_over_what_the_config_says() {
        let mut settings = table(r#"sourceport = "from-another-machine""#);
        overlay_machine_settings(&mut settings, Some(table(r#"sourceport = "mine""#)));
        assert_eq!(settings.get("sourceport").unwrap().as_str(), Some("mine"));
    }

    /// A setting cleared here stays cleared, whatever is left in the file that travels.
    #[test]
    fn test_a_machines_file_is_the_whole_answer_once_it_exists() {
        let mut settings = table(r#"sourceport = "from-another-machine""#);
        overlay_machine_settings(&mut settings, Some(toml::Table::new()));
        assert!(settings.get("sourceport").is_none());
    }

    /// And nothing that belongs to a library can be shadowed by a stray key in it.
    #[test]
    fn test_a_machines_file_only_decides_what_is_a_machines() {
        let mut settings = table(r#"link_mode = "copy""#);
        overlay_machine_settings(&mut settings, Some(table(r#"link_mode = "move""#)));
        assert_eq!(settings.get("link_mode").unwrap().as_str(), Some("copy"));
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

    // -- mirrors --

    #[test]
    fn test_a_mirror_is_caco_s_layout_unless_it_says_otherwise() {
        let with = |layout: Option<&str>| MirrorConfig {
            layout: layout.map(str::to_string),
            ..Default::default()
        };
        assert_eq!(with(None).layout(), Ok(MirrorLayout::Caco));
        assert_eq!(with(Some("")).layout(), Ok(MirrorLayout::Caco));
        assert_eq!(with(Some("caco")).layout(), Ok(MirrorLayout::Caco));
        assert_eq!(with(Some("idgames")).layout(), Ok(MirrorLayout::Idgames));
        assert_eq!(with(Some(" idgames ")).layout(), Ok(MirrorLayout::Idgames));
        assert!(with(Some("idgames")).is_idgames_copy());
        assert!(!with(None).is_idgames_copy());
    }

    #[test]
    fn test_a_layout_with_a_typo_says_so_and_does_not_sink_the_config() {
        let config: Config = toml::from_str(
            r#"
            sourceport = "dsda-doom"

            [[mirrors]]
            name = "disk"
            path = "/mnt/idgames"
            layout = "idgmaes"
            "#,
        )
        .unwrap();
        assert_eq!(config.sourceport, "dsda-doom");
        let problem = config.mirrors[0].layout().unwrap_err();
        assert!(problem.contains("idgmaes"), "{problem}");
        assert!(!config.mirrors[0].is_idgames_copy());
    }

    #[test]
    fn test_a_copy_of_idgames_takes_no_uploads_whatever_it_says() {
        let copy = |write| MirrorConfig {
            layout: Some("idgames".to_string()),
            write,
            ..Default::default()
        };
        assert!(!copy(true).takes_uploads());
        assert!(!copy(false).takes_uploads());

        let kept = |write| MirrorConfig {
            write,
            ..Default::default()
        };
        assert!(kept(true).takes_uploads());
        assert!(!kept(false).takes_uploads());

        // Whatever it is, it is not known to be something to write to.
        let typo = MirrorConfig {
            layout: Some("idgmaes".to_string()),
            write: true,
            ..Default::default()
        };
        assert!(!typo.takes_uploads());
    }

    #[test]
    fn test_a_layout_reaches_the_file_only_when_it_says_something() {
        let config = Config {
            mirrors: vec![
                MirrorConfig {
                    name: "kept".to_string(),
                    path: Some("/a".to_string()),
                    ..Default::default()
                },
                MirrorConfig {
                    name: "disk".to_string(),
                    path: Some("/b".to_string()),
                    layout: Some("idgames".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let written = minimal_toml(&config).unwrap();
        assert_eq!(written.matches("layout").count(), 1, "{written}");
        let back: Config = toml::from_str(&written).unwrap();
        assert_eq!(back.mirrors, config.mirrors);
    }

    #[test]
    fn test_the_index_of_idgames_is_on_the_cache_side_of_the_split() {
        assert!(idgames_index_path().starts_with(cache_home()));
        assert!(!idgames_index_path().starts_with(default_data_dir()));
    }

    fn mirror(url: Option<&str>, path: Option<&str>) -> MirrorConfig {
        MirrorConfig {
            name: "home".to_string(),
            url: url.map(str::to_string),
            path: path.map(str::to_string),
            ..Default::default()
        }
    }

    /// The issue's example, parsed as written.
    #[test]
    fn test_config_mirrors_parse_as_the_issue_spells_them() {
        let toml_str = r#"
[[mirrors]]
name      = "home"
url       = "https://files.example.lan/doom"
token_env = "CACO_MIRROR_TOKEN"
write     = true

[[mirrors]]
name = "nas"
path = "/mnt/nas/doom-mirror"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.mirrors.len(), 2);
        assert_eq!(cfg.mirrors[0].name, "home");
        assert_eq!(cfg.mirrors[0].token_env_name(), Some("CACO_MIRROR_TOKEN"));
        assert!(cfg.mirrors[0].write);
        assert_eq!(
            cfg.mirrors[0].location(),
            Ok(MirrorLocation::Url(
                "https://files.example.lan/doom".to_string()
            ))
        );
        assert!(
            !cfg.mirrors[1].write,
            "a mirror is read-only unless it says"
        );
        assert_eq!(
            cfg.mirrors[1].location(),
            Ok(MirrorLocation::Path(PathBuf::from("/mnt/nas/doom-mirror")))
        );
    }

    #[test]
    fn test_minimal_toml_round_trips_mirrors() {
        let cfg = Config {
            mirrors: vec![
                MirrorConfig {
                    token_env: Some("TOKEN".to_string()),
                    write: true,
                    ..mirror(Some("https://example.lan/doom"), None)
                },
                MirrorConfig {
                    name: "nas".to_string(),
                    ..mirror(None, Some("/mnt/nas"))
                },
            ],
            ..Default::default()
        };

        let rendered = minimal_toml(&cfg).unwrap();
        assert!(rendered.contains("[[mirrors]]"), "{rendered}");
        let parsed: Config = toml::from_str(&rendered).unwrap();
        assert_eq!(parsed.mirrors, cfg.mirrors);
    }

    #[test]
    fn test_a_mirrors_url_loses_its_trailing_slash() {
        assert_eq!(
            mirror(Some(" https://example.lan/doom/ "), None).location(),
            Ok(MirrorLocation::Url("https://example.lan/doom".to_string()))
        );
    }

    #[test]
    fn test_a_mirrors_path_expands_a_tilde() {
        assert_eq!(
            mirror(None, Some("~/doom-mirror")).location(),
            Ok(MirrorLocation::Path(home_dir().join("doom-mirror")))
        );
    }

    #[test]
    fn test_a_mirror_has_one_location_and_a_usable_one() {
        for (bad, says) in [
            (mirror(None, None), "set a URL or a path"),
            (mirror(Some(" "), Some("")), "set a URL or a path"),
            (mirror(Some("https://x"), Some("/y")), "not both"),
            (mirror(Some("ftp://x/doom"), None), "not an http"),
            (mirror(None, Some("relative/doom")), "not an absolute path"),
        ] {
            let problem = bad.location().unwrap_err();
            assert!(problem.contains(says), "{problem}");
        }
    }

    #[test]
    fn test_a_saved_token_never_reaches_the_config_file() {
        let cfg = Config {
            mirrors: vec![MirrorConfig {
                saved_token: Some(SavedToken("sekrit".to_string())),
                ..mirror(Some("https://example.lan/doom"), None)
            }],
            ..Default::default()
        };

        let rendered = minimal_toml(&cfg).unwrap();

        assert!(rendered.contains("[[mirrors]]"), "{rendered}");
        assert!(!rendered.contains("sekrit"), "{rendered}");
        assert!(!format!("{cfg:?}").contains("sekrit"));
    }

    #[test]
    fn test_mirror_tokens_stay_out_of_the_data_dir() {
        assert!(!mirror_token_file().starts_with(default_data_dir()));
        assert!(!mirror_token_file().starts_with(cache_home()));
    }

    #[test]
    fn test_a_mirrors_login_is_a_username_if_it_has_one() {
        let login = |username: Option<&str>| MirrorConfig {
            username: username.map(str::to_string),
            ..Default::default()
        };
        assert_eq!(login(Some(" doom ")).login(), Some("doom"));
        assert_eq!(login(Some("  ")).login(), None);
        assert_eq!(login(None).login(), None);
    }

    #[test]
    fn test_a_mirrors_token_comes_from_its_environment_variable() {
        const VAR: &str = "CACO_TEST_MIRROR_TOKEN_READ";
        let configured = MirrorConfig {
            token_env: Some(VAR.to_string()),
            ..Default::default()
        };
        assert_eq!(configured.token(), None);

        // SAFETY: test-only env mutation, on a variable only this test reads.
        unsafe { std::env::set_var(VAR, "sekrit") };
        assert_eq!(configured.token().as_deref(), Some("sekrit"));
        let both = MirrorConfig {
            saved_token: Some(SavedToken("saved".to_string())),
            ..configured.clone()
        };
        assert_eq!(both.token().as_deref(), Some("sekrit"), "the variable wins");
        unsafe { std::env::set_var(VAR, "") };
        assert_eq!(configured.token(), None, "an empty token is no token");
        assert_eq!(both.token().as_deref(), Some("saved"));
        unsafe { std::env::remove_var(VAR) };

        let blank = MirrorConfig {
            token_env: Some("  ".to_string()),
            ..Default::default()
        };
        assert_eq!(blank.token_env_name(), None);
        assert_eq!(MirrorConfig::default().token_env_name(), None);
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
        let dir = get_wad_data_dir("1", "Scythe 2: Electric Boogaloo!");
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
    fn test_the_downloads_folder_is_the_systems_unless_set() {
        let with = |value: &str| Config {
            downloads_dir: value.to_string(),
            ..Config::default()
        };
        assert_eq!(downloads_dir_from(&with("")), default_downloads_dir());
        assert_eq!(downloads_dir_from(&with("   ")), default_downloads_dir());
        assert_eq!(
            downloads_dir_from(&with("/mnt/flatpak/downloads")),
            PathBuf::from("/mnt/flatpak/downloads")
        );
        assert_eq!(downloads_dir_from(&with("~/dl")), home_dir().join("dl"));
    }

    #[test]
    fn test_only_an_explicit_move_moves() {
        assert_eq!(LinkMode::parse("move"), LinkMode::Move);
        assert_eq!(LinkMode::parse("copy"), LinkMode::Copy);
        assert_eq!(LinkMode::parse("mvoe"), LinkMode::Copy);
        assert_eq!(LinkMode::parse(""), LinkMode::Copy);
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
