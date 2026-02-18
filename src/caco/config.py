"""Configuration management for caco."""

import shutil
import tomllib
from pathlib import Path
from typing import Any

CONFIG_DIR = Path.home() / ".config" / "caco"
CONFIG_FILE = CONFIG_DIR / "config.toml"
CACHE_DIR = Path.home() / ".cache" / "caco" / "wads"
DB_DIR = Path.home() / ".local" / "share" / "caco"
DEFAULT_DB_PATH = DB_DIR / "library.db"

DEFAULT_CONFIG = {
    "sourceport": "",
    "cache_dir": str(CACHE_DIR),
    "db_path": str(DEFAULT_DB_PATH),
    "iwad": "",
    "iwad_dirs": [],
    "sourceport_args": [],
    "download_mirror": 0,
}

# Default list configuration
DEFAULT_LIST_CONFIG = {
    "format": ["id", "title", "author", "status", "beaten", "playtime", "last_played"],
    "sort": None,  # None means use default (status priority)
    "default_status": [],  # Empty means all statuses
}


def load_config() -> dict[str, Any]:
    """Load configuration from file, creating defaults if needed."""
    config = DEFAULT_CONFIG.copy()

    if CONFIG_FILE.exists():
        try:
            with open(CONFIG_FILE, "rb") as f:
                user_config = tomllib.load(f)
                config.update(user_config)
        except tomllib.TOMLDecodeError as e:
            import sys
            print(f"Warning: Invalid TOML syntax in {CONFIG_FILE}: {e}", file=sys.stderr)
            print("Warning: Using default configuration.", file=sys.stderr)
        except PermissionError:
            import sys
            print(f"Warning: Permission denied reading {CONFIG_FILE}", file=sys.stderr)
            print("Warning: Using default configuration.", file=sys.stderr)
        except Exception as e:
            import sys
            print(f"Warning: Failed to load config: {e}", file=sys.stderr)
            print("Warning: Using default configuration.", file=sys.stderr)

    return config


def save_config(config: dict[str, Any]) -> None:
    """Save configuration to file."""
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)

    lines = []
    for key, value in config.items():
        if isinstance(value, str):
            lines.append(f'{key} = "{value}"')
        elif isinstance(value, bool):
            lines.append(f"{key} = {str(value).lower()}")
        elif isinstance(value, (int, float)):
            lines.append(f"{key} = {value}")
        elif isinstance(value, list):
            items = ", ".join(f'"{v}"' for v in value)
            lines.append(f"{key} = [{items}]")

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
    """Resolve an IWAD name to a full path using iwad_dirs.

    Resolution order:
    1. If name is an existing absolute path, return as-is.
    2. Search each iwad_dirs entry for name and name.wad.
    3. If not found, return the original name unchanged.
    """
    path = Path(name).expanduser()
    if path.is_absolute() and path.exists():
        return str(path)

    for iwad_dir in get_iwad_dirs():
        if not iwad_dir.is_dir():
            continue
        # Try exact name, then with .wad extension
        for candidate in (iwad_dir / name, iwad_dir / f"{name}.wad"):
            if candidate.exists():
                return str(candidate)

    return name


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


def get_download_mirror() -> int:
    """Get the preferred download mirror index."""
    config = load_config()
    return int(config.get("download_mirror", 0))


def get_list_config() -> dict[str, Any]:
    """Get list display configuration, merging defaults with user config."""
    config = load_config()
    list_config = DEFAULT_LIST_CONFIG.copy()

    # Merge user's [list] section if present
    if "list" in config and isinstance(config["list"], dict):
        user_list = config["list"]
        if "format" in user_list:
            list_config["format"] = user_list["format"]
        if "sort" in user_list:
            list_config["sort"] = user_list["sort"]
        if "default_status" in user_list:
            list_config["default_status"] = user_list["default_status"]

    return list_config


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
    config = load_config()
    tui_config = DEFAULT_TUI_CONFIG.copy()

    if "tui" in config and isinstance(config["tui"], dict):
        user_tui = config["tui"]
        if "default_tab" in user_tui:
            tui_config["default_tab"] = user_tui["default_tab"]
        if "default_sort" in user_tui:
            tui_config["default_sort"] = user_tui["default_sort"]
        if "default_sort_desc" in user_tui:
            tui_config["default_sort_desc"] = user_tui["default_sort_desc"]

    return tui_config


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
    config = load_config()
    gui_config = DEFAULT_GUI_CONFIG.copy()

    if "gui" in config and isinstance(config["gui"], dict):
        user_gui = config["gui"]
        for key in DEFAULT_GUI_CONFIG:
            if key in user_gui:
                gui_config[key] = user_gui[key]

    return gui_config
