"""Helion sourceport stats watcher — polls levelstat.txt during play sessions."""

import json
import logging
import os
import platform
import threading
import zipfile
from pathlib import Path

from caco.stats_watcher import StatsWatcher, register_watcher
from caco.wad_stats import (
    WadStats,
    format_stats,
    merge_stats,
    parse_stats_text,
)

logger = logging.getLogger(__name__)


def _get_helion_config_dir() -> Path:
    """Platform-dependent Helion config directory."""
    if platform.system() == "Windows":
        return Path.home() / "Saved Games" / "Helion"
    # Linux/macOS: respect XDG_CONFIG_HOME
    xdg = os.environ.get("XDG_CONFIG_HOME")
    if xdg:
        return Path(xdg) / "Helion"
    return Path.home() / ".config" / "Helion"


def _find_latest_save(save_dir: Path) -> Path | None:
    """Find most recently modified .hsg file in a directory."""
    saves = [p for p in save_dir.glob("*.hsg") if p.is_file()]
    if not saves:
        return None
    return max(saves, key=lambda p: p.stat().st_mtime)


def _read_helion_save_skill(save_path: Path) -> int | None:
    """Extract Skill from a Helion .hsg save archive.

    Helion saves are ZIP files containing world.json with a Skill field.
    Helion's Skill is 0-indexed (0=ITYTD..4=NM) but MapStats.best_skill
    is 1-indexed, so we add 1.
    """
    try:
        with zipfile.ZipFile(save_path, "r") as zf:
            data = json.loads(zf.read("world.json"))
            skill = data.get("Skill")
            if isinstance(skill, int):
                return skill + 1  # Convert 0-indexed to 1-indexed
    except (zipfile.BadZipFile, json.JSONDecodeError, KeyError, OSError):
        logger.debug("Failed to read skill from save: %s", save_path)
    return None


class HelionWatcher(StatsWatcher):
    """Watches Helion's levelstat.txt for changes during a play session."""

    def __init__(
        self,
        wad_data_dir: Path,
        helion_config_dir: Path | None = None,
    ):
        self._wad_data_dir = wad_data_dir
        self._config_dir = helion_config_dir or _get_helion_config_dir()
        self._levelstat_path = self._config_dir / "levelstat.txt"
        self._stop_event = threading.Event()
        self._last_mtime: float = 0.0
        self._accumulated: list[WadStats] = []
        self._poll_interval: float = 1.0

    def extra_args(self) -> list[str]:
        """Helion requires -levelstat to write levelstat.txt."""
        return ["-levelstat"]

    def start(self) -> None:
        """Poll levelstat.txt mtime every ~1 second until stopped."""
        if self._levelstat_path.exists():
            self._last_mtime = self._levelstat_path.stat().st_mtime

        while not self._stop_event.wait(self._poll_interval):
            self._check_for_changes()

    def stop(self) -> None:
        """Signal stop, do final read, enrich skill from save."""
        self._stop_event.set()
        self._check_for_changes()
        self._enrich_skill()

    def collect(self) -> str | None:
        """Return accumulated stats as levelstat.txt-format string."""
        if not self._accumulated:
            return None
        merged = merge_stats(self._accumulated)
        return format_stats(merged)

    def _check_for_changes(self) -> None:
        """Check if levelstat.txt has been updated and parse new content."""
        try:
            mtime = self._levelstat_path.stat().st_mtime
        except FileNotFoundError:
            return
        try:
            if mtime <= self._last_mtime:
                return
            self._last_mtime = mtime

            text = self._levelstat_path.read_text()
            if not text.strip():
                return

            parsed = parse_stats_text(text)
            self._accumulated.append(parsed)
        except (OSError, ValueError):
            logger.debug("Failed to read levelstat.txt", exc_info=True)

    def _enrich_skill(self) -> None:
        """Patch accumulated stats with skill from most recent save file."""
        if not self._accumulated:
            return

        save = _find_latest_save(self._config_dir)
        if not save:
            return

        skill = _read_helion_save_skill(save)
        if skill is None:
            return

        for stats in self._accumulated:
            for m in stats.maps:
                m.best_skill = skill


register_watcher("helion", HelionWatcher)
