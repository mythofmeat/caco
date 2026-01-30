"""Configuration management for caco."""

import tomllib
from pathlib import Path
from typing import Any

CONFIG_DIR = Path.home() / ".config" / "caco"
CONFIG_FILE = CONFIG_DIR / "config.toml"
CACHE_DIR = Path.home() / ".cache" / "caco" / "wads"

STATS_DIR = Path.home() / ".local" / "share" / "nyan-doom" / "nyan_doom_data"

DEFAULT_CONFIG = {
    "sourceport": "",
    "cache_dir": str(CACHE_DIR),
    "iwad": "",
    "sourceport_args": [],
    "download_mirror": 0,
    "stats_dir": str(STATS_DIR),
}

# Default list configuration
DEFAULT_LIST_CONFIG = {
    "format": ["id", "title", "author", "status", "maps", "beaten", "playtime", "last_played"],
    "sort": None,  # None means use default (status priority)
    "default_status": [],  # Empty means all statuses
    "colors": {
        "to-play": "blue",
        "backlog": "yellow",
        "playing": "green",
        "finished": "dim",
        "abandoned": "red",
    },
}


def load_config() -> dict[str, Any]:
    """Load configuration from file, creating defaults if needed."""
    config = DEFAULT_CONFIG.copy()

    if CONFIG_FILE.exists():
        try:
            with open(CONFIG_FILE, "rb") as f:
                user_config = tomllib.load(f)
                config.update(user_config)
        except Exception:
            pass  # Use defaults on error

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


def get_cache_dir() -> Path:
    """Get the WAD cache directory."""
    config = load_config()
    return Path(config.get("cache_dir", str(CACHE_DIR)))


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


def get_stats_dir() -> Path:
    """Get the stats directory (nyan-doom/dsda-doom stats.txt location)."""
    config = load_config()
    return Path(config.get("stats_dir", str(STATS_DIR))).expanduser()


def set_stats_dir(path: str) -> None:
    """Set the stats directory."""
    config = load_config()
    config["stats_dir"] = path
    save_config(config)


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
        if "colors" in user_list and isinstance(user_list["colors"], dict):
            list_config["colors"].update(user_list["colors"])

    return list_config
