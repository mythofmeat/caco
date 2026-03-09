"""Configuration management for caco."""

import shutil
import sys
import tomllib
from pathlib import Path
from typing import Any

CONFIG_DIR = Path.home() / ".config" / "caco"
CONFIG_FILE = CONFIG_DIR / "config.toml"
DB_DIR = Path.home() / ".local" / "share" / "caco"
DEFAULT_DB_PATH = DB_DIR / "library.db"
CACHE_DIR = DB_DIR / "wads"
IWAD_DIR = DB_DIR / "iwads"
ID24_DIR = DB_DIR / "id24"
DATA_DIR = DB_DIR / "data"
COMPANION_DIR = DB_DIR / "companions"
BACKUP_DIR = DB_DIR / "backups"
SOURCEPORT_DIR = DB_DIR / "sourceports"

DEFAULT_CONFIG = {
    "sourceport": "",
    "cache_dir": str(CACHE_DIR),
    "db_path": str(DEFAULT_DB_PATH),
    "iwad": "",
    "iwad_dirs": [],
    "sourceport_args": [],
    "download_mirror": 0,
    "link_mode": "move",
    "manage_data_dirs": True,
    "auto_stats": True,
    "auto_detect_iwad": True,
    "auto_detect_complevel": True,
    "cache_max_size_gb": 0,
    "cache_max_age_days": 0,
    "cache_auto_clean": False,
    "companion_orphan_cleanup": "ask",
}

# Default list configuration
DEFAULT_LIST_CONFIG = {
    "format": ["id", "title", "author", "status", "beaten", "playtime", "last_played"],
    "sort": None,  # None means use default (status priority)
    "default_status": [],  # Empty means all statuses
}


_config_cache: dict[str, Any] | None = None
_ensuring_config = False  # Recursion guard for ensure_config_keys


def load_config() -> dict[str, Any]:
    """Load configuration from file, creating defaults if needed.

    Results are cached; call save_config() to invalidate.
    Returns a shallow copy so callers can safely mutate top-level keys.
    """
    global _config_cache
    if _config_cache is not None:
        return _config_cache.copy()

    config = DEFAULT_CONFIG.copy()

    if CONFIG_FILE.exists():
        try:
            with open(CONFIG_FILE, "rb") as f:
                user_config = tomllib.load(f)
                config.update(user_config)
        except tomllib.TOMLDecodeError as e:
            print(f"Warning: Invalid TOML syntax in {CONFIG_FILE}: {e}", file=sys.stderr)
            print("Warning: Using default configuration.", file=sys.stderr)
        except PermissionError:
            print(f"Warning: Permission denied reading {CONFIG_FILE}", file=sys.stderr)
            print("Warning: Using default configuration.", file=sys.stderr)
        except Exception as e:
            print(f"Warning: Failed to load config: {e}", file=sys.stderr)
            print("Warning: Using default configuration.", file=sys.stderr)

    _config_cache = config

    # Auto-update config file with any missing keys
    ensure_config_keys()

    return config.copy()


def ensure_config_keys() -> None:
    """Ensure the config file on disk has all known keys.

    Compares the existing config file against DEFAULT_CONFIG and section
    defaults (tui, gui, list). Adds missing keys with their default values.
    Only runs if the config file already exists.  Writes only if changes
    were made.
    """
    global _ensuring_config
    if _ensuring_config or not CONFIG_FILE.exists():
        return

    _ensuring_config = True
    try:
        with open(CONFIG_FILE, "rb") as f:
            on_disk = tomllib.load(f)
    except Exception:
        return  # Can't read — skip silently
    finally:
        _ensuring_config = False

    changed = False

    # Check top-level scalar keys
    for key, default in DEFAULT_CONFIG.items():
        if key not in on_disk:
            on_disk[key] = default
            changed = True

    # Check section defaults
    section_defaults: dict[str, dict[str, Any]] = {
        "tui": DEFAULT_TUI_CONFIG,
        "gui": DEFAULT_GUI_CONFIG,
        "list": DEFAULT_LIST_CONFIG,
    }
    for section_name, defaults in section_defaults.items():
        if section_name in on_disk and isinstance(on_disk[section_name], dict):
            for key, default in defaults.items():
                if key not in on_disk[section_name]:
                    on_disk[section_name][key] = default
                    changed = True
        # Don't create missing sections — only backfill keys in existing ones

    if changed:
        _ensuring_config = True
        try:
            save_config(on_disk)
        finally:
            _ensuring_config = False


def save_config(config: dict[str, Any]) -> None:
    """Save configuration to file. Invalidates the load_config cache."""
    global _config_cache
    _config_cache = None
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)

    lines = []
    sections = []  # Nested dicts emitted after top-level keys (valid TOML order)
    for key, value in config.items():
        if isinstance(value, dict):
            sections.append((key, value))
        elif isinstance(value, str):
            lines.append(f'{key} = "{value}"')
        elif isinstance(value, bool):
            lines.append(f"{key} = {str(value).lower()}")
        elif isinstance(value, (int, float)):
            lines.append(f"{key} = {value}")
        elif isinstance(value, list):
            items = ", ".join(f'"{v}"' for v in value)
            lines.append(f"{key} = [{items}]")

    # Emit nested sections (e.g., [tui], [gui], [list])
    for section_name, section_dict in sections:
        lines.append(f"\n[{section_name}]")
        for sub_key, sub_value in section_dict.items():
            if isinstance(sub_value, str):
                lines.append(f'{sub_key} = "{sub_value}"')
            elif isinstance(sub_value, bool):
                lines.append(f"{sub_key} = {str(sub_value).lower()}")
            elif isinstance(sub_value, (int, float)):
                lines.append(f"{sub_key} = {sub_value}")
            elif isinstance(sub_value, list):
                items = ", ".join(f'"{v}"' for v in sub_value)
                lines.append(f"{sub_key} = [{items}]")

    CONFIG_FILE.write_text("\n".join(lines) + "\n")


def get_default_sourceport() -> str | None:
    """Get the configured default sourceport."""
    config = load_config()
    port = config.get("sourceport", "")
    return port if port else None


def set_default_sourceport(sourceport: str) -> None:
    """Set the default sourceport."""
    config = load_config()
    config["sourceport"] = sourceport
    save_config(config)


def resolve_sourceport(name: str) -> str:
    """Resolve a sourceport name to a full path.

    If name is already an absolute path, return as-is.
    Otherwise, use shutil.which() to find it on PATH.
    Falls back to the original name if not found.
    """
    if Path(name).is_absolute():
        return name
    return shutil.which(name) or name


def get_db_path() -> Path:
    """Get the database file path."""
    config = load_config()
    return Path(config.get("db_path", str(DEFAULT_DB_PATH))).expanduser()


def get_cache_dir() -> Path:
    """Get the WAD cache directory."""
    config = load_config()
    return Path(config.get("cache_dir", str(CACHE_DIR))).expanduser()


def get_iwad_dir() -> Path:
    """Get the managed IWAD directory."""
    config = load_config()
    return Path(config.get("iwad_dir", str(IWAD_DIR))).expanduser()


def get_id24_dir() -> Path:
    """Get the managed id24 WAD directory."""
    return ID24_DIR


def get_companion_dir() -> Path:
    """Get the managed companion files directory."""
    return COMPANION_DIR


def get_companion_orphan_cleanup() -> str:
    """Get orphan cleanup policy: 'delete', 'keep', or 'ask'."""
    config = load_config()
    value = config.get("companion_orphan_cleanup", "ask")
    return value if value in ("delete", "keep", "ask") else "ask"


def set_cache_dir(path: str) -> None:
    """Set the WAD cache directory."""
    config = load_config()
    config["cache_dir"] = path
    save_config(config)


def get_iwad() -> str | None:
    """Get the configured default IWAD path."""
    config = load_config()
    iwad = config.get("iwad", "")
    return iwad if iwad else None


def set_iwad(path: str) -> None:
    """Set the default IWAD path."""
    config = load_config()
    config["iwad"] = path
    save_config(config)


def get_iwad_dirs() -> list[Path]:
    """Get configured IWAD directories, with tilde expansion."""
    config = load_config()
    dirs = config.get("iwad_dirs", [])
    if not isinstance(dirs, list):
        return []
    return [Path(d).expanduser() for d in dirs if d]


def resolve_iwad(name: str) -> str:
    """Resolve an IWAD name to a full path.

    Resolution order:
    1. If name is an existing absolute path, return as-is.
    2. Check the IWAD registry (iwads table) for a matching short name.
    3. Search each iwad_dirs entry for name and name.wad.
    4. If not found, return the original name unchanged.
    """
    path = Path(name).expanduser()
    if path.is_absolute() and path.exists():
        return str(path)

    # Check IWAD registry
    from caco.db._iwads import resolve_iwad_from_db

    db_path = resolve_iwad_from_db(name)
    if db_path:
        return db_path

    for iwad_dir in get_iwad_dirs():
        if not iwad_dir.is_dir():
            continue
        # Try exact name, then with .wad extension
        for candidate in (iwad_dir / name, iwad_dir / f"{name}.wad"):
            if candidate.exists():
                return str(candidate)

    return name


def get_sourceport_dir() -> Path:
    """Get the sourceport config profiles directory."""
    config = load_config()
    custom = config.get("sourceport_dir", "")
    return Path(custom).expanduser() if custom else SOURCEPORT_DIR


def get_profile_path(sourceport: str, profile: str) -> Path:
    """Return the path to a sourceport config profile file.

    Path: {sourceport_dir}/{basename}/{profile}.{ext}
    Extension is .ini for Helion, .cfg for all others.
    """
    from caco.sourceports import get_profile_ext

    basename = Path(sourceport).stem
    ext = get_profile_ext(sourceport)
    return get_sourceport_dir() / basename / f"{profile}{ext}"


def list_profiles(sourceport: str | None = None) -> dict[str, list[str]]:
    """Scan the sourceport config directory for profiles.

    Args:
        sourceport: If given, only list profiles for this sourceport basename.
                    Otherwise, list all sourceports and their profiles.

    Returns:
        Dict mapping sourceport basename to sorted list of profile names.
    """
    sp_dir = get_sourceport_dir()
    if not sp_dir.is_dir():
        return {}

    result: dict[str, list[str]] = {}

    config_globs = ("*.cfg", "*.ini")

    if sourceport:
        basename = Path(sourceport).stem
        port_dir = sp_dir / basename
        if port_dir.is_dir():
            profiles = sorted({p.stem for g in config_globs for p in port_dir.glob(g)})
            if profiles:
                result[basename] = profiles
    else:
        for entry in sorted(sp_dir.iterdir()):
            if entry.is_dir():
                profiles = sorted({p.stem for g in config_globs for p in entry.glob(g)})
                if profiles:
                    result[entry.name] = profiles

    return result


def get_sourceport_args() -> list[str]:
    """Get the default sourceport arguments."""
    config = load_config()
    args = config.get("sourceport_args", [])
    return args if isinstance(args, list) else []


def set_sourceport_args(args: list[str]) -> None:
    """Set the default sourceport arguments."""
    config = load_config()
    config["sourceport_args"] = args
    save_config(config)


def get_link_mode() -> str:
    """Get the link mode (copy or move)."""
    config = load_config()
    mode = config.get("link_mode", "move")
    return mode if mode in ("copy", "move") else "move"


def get_download_mirror() -> int:
    """Get the preferred download mirror index."""
    config = load_config()
    return int(config.get("download_mirror", 0))


def _merge_section_config(section_name: str, defaults: dict[str, Any]) -> dict[str, Any]:
    """Merge a [section] from user config over defaults. Only known keys are merged."""
    config = load_config()
    result = defaults.copy()
    section = config.get(section_name)
    if isinstance(section, dict):
        for key in defaults:
            if key in section:
                result[key] = section[key]
    return result


def get_list_config() -> dict[str, Any]:
    """Get list display configuration, merging defaults with user config."""
    return _merge_section_config("list", DEFAULT_LIST_CONFIG)


# =============================================================================
# Cache Configuration
# =============================================================================


def get_cache_max_size() -> int:
    """Get max cache size in bytes. 0 = unlimited."""
    config = load_config()
    gb = config.get("cache_max_size_gb", 0)
    return int(float(gb) * 1024 * 1024 * 1024) if gb > 0 else 0


def get_cache_max_age() -> int:
    """Get max cache age in days. 0 = never expire."""
    config = load_config()
    return int(config.get("cache_max_age_days", 0))


def get_cache_auto_clean() -> bool:
    """Whether to auto-clean cache before play."""
    config = load_config()
    return bool(config.get("cache_auto_clean", False))


# =============================================================================
# TUI Configuration
# =============================================================================

DEFAULT_TUI_CONFIG = {
    "default_tab": "all",
    "default_sort": "id",
    "default_sort_desc": False,
}


def get_tui_config() -> dict[str, Any]:
    """Get TUI configuration, merging defaults with user config."""
    return _merge_section_config("tui", DEFAULT_TUI_CONFIG)


# =============================================================================
# GUI Configuration
# =============================================================================

DEFAULT_GUI_CONFIG = {
    "default_tab": "all",
    "default_sort": "id",
    "default_sort_desc": False,
    "default_view": "list",
    "window_width": 1200,
    "window_height": 800,
    "detail_panel_width": 300,
    "show_detail_panel": True,
    "thumbnail_size": 160,
}


def get_gui_config() -> dict[str, Any]:
    """Get GUI configuration, merging defaults with user config."""
    return _merge_section_config("gui", DEFAULT_GUI_CONFIG)


# =============================================================================
# Per-WAD Data Directories
# =============================================================================


def get_auto_detect_iwad() -> bool:
    """Whether to auto-detect IWAD from WAD file contents on first play."""
    config = load_config()
    return bool(config.get("auto_detect_iwad", True))


def get_auto_detect_complevel() -> bool:
    """Whether to auto-detect complevel from WAD file contents on first play."""
    config = load_config()
    return bool(config.get("auto_detect_complevel", True))


def get_auto_stats() -> bool:
    """Whether to auto-track stats after play sessions."""
    config = load_config()
    return bool(config.get("auto_stats", True))


def get_auto_doomwiki_enrich() -> bool:
    """Whether to auto-enrich imports with Doom Wiki metadata."""
    config = load_config()
    return bool(config.get("auto_doomwiki_enrich", True))


def get_data_dir() -> Path:
    """Get the base directory for per-WAD data directories."""
    config = load_config()
    return Path(config.get("data_dir", str(DATA_DIR))).expanduser()


def get_backup_dir() -> Path:
    """Get the directory for WAD data backups."""
    return BACKUP_DIR


def get_manage_data_dirs() -> bool:
    """Whether to manage per-WAD data directories (inject -data/-save args)."""
    config = load_config()
    return bool(config.get("manage_data_dirs", True))


def _sanitize_dirname(title: str) -> str:
    """Sanitize a WAD title for use as a directory name.

    Lowercase, replace non-alphanumeric with hyphens, strip leading/trailing
    hyphens, collapse runs, and truncate to 64 chars.
    """
    import re

    name = title.lower()
    name = re.sub(r"[^a-z0-9]+", "-", name)
    name = name.strip("-")
    # Collapse any remaining runs of hyphens
    name = re.sub(r"-{2,}", "-", name)
    return name[:64]


def get_wad_data_dir(wad_id: int, title: str) -> Path:
    """Return the per-WAD data directory path.

    Format: {data_dir}/{id}_{sanitized_title}/
    """
    return get_data_dir() / f"{wad_id}_{_sanitize_dirname(title)}"


def find_wad_data_dir(wad_id: int) -> Path | None:
    """Find an existing per-WAD data directory by ID prefix.

    Handles title renames — matches {id}_* pattern.
    Returns the path if found, None otherwise.
    """
    base = get_data_dir()
    if not base.is_dir():
        return None
    prefix = f"{wad_id}_"
    for entry in base.iterdir():
        if entry.is_dir() and entry.name.startswith(prefix):
            return entry
    return None
