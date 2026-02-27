"""Parser and formatter for sourceport per-map statistics files.

Supports two formats:
- nyan-doom/dsda-doom stats.txt (binary-ish, 15 fields per map)
- dsda-doom levelstat.txt (human-readable, from -levelstat flag)

Stats are stored as JSON in the wad_completions.stats_snapshot column.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Any

# Doom runs at 35 tics per second
TICS_PER_SECOND = 35

SKILL_NAMES = {
    0: "-",
    1: "ITYTD",
    2: "HNTR",
    3: "HMP",
    4: "UV",
    5: "NM",
}


@dataclass
class MapStats:
    """Per-map statistics entry (superset of both formats)."""

    lump: str  # Map name (e.g., MAP01, E1M1)

    # Common to both formats
    kills: int = 0  # Best kills achieved
    total_kills: int = -1  # Max killable in map (-1 = unknown)
    items: int = 0  # Best items achieved
    total_items: int = -1  # Max items in map (-1 = unknown)
    secrets: int = 0  # Best secrets achieved
    total_secrets: int = -1  # Max secrets in map (-1 = unknown)

    # stats.txt specific
    episode: int = 0
    map_num: int = 0
    best_skill: int = 0  # Highest skill completed (0=unplayed)
    best_time: int = -1  # Best completion time in tics (-1=never)
    best_max_time: int = -1  # Best 100% time in tics (-1=never)
    best_nm_time: int = -1  # Best nightmare time in tics (-1=never)
    total_exits: int = 0  # Times completed
    cumulative_kills: int = 0  # Total kills across all plays

    # levelstat.txt specific
    time_secs: float = -1.0  # Level time in seconds (-1=not available)
    total_time_secs: float = -1.0  # Cumulative time in seconds

    @property
    def played(self) -> bool:
        """Whether this map was actually played."""
        return self.best_skill > 0 or self.time_secs >= 0


@dataclass
class WadStats:
    """Complete WAD statistics from a stats file."""

    format: str  # "stats_txt" or "levelstat_txt"
    maps: list[MapStats] = field(default_factory=list)

    # stats.txt header fields
    version: int = 1
    header_total_kills: int = 0

    @property
    def played_maps(self) -> list[MapStats]:
        """Return only maps that were actually played."""
        return [m for m in self.maps if m.played]

    @property
    def total_time_display(self) -> str:
        """Human-readable total time across all played maps."""
        if self.format == "stats_txt":
            total_tics = sum(
                m.best_time for m in self.maps if m.best_time > 0
            )
            return format_time_tics(total_tics) if total_tics > 0 else "-"
        else:
            played = self.played_maps
            if played and played[-1].total_time_secs >= 0:
                return format_time_secs(played[-1].total_time_secs)
            return "-"


def format_time_tics(tics: int) -> str:
    """Convert tics (35/sec) to human-readable M:SS or H:MM:SS."""
    if tics < 0:
        return "-"
    total_secs = tics / TICS_PER_SECOND
    return _format_seconds(total_secs)


def format_time_secs(secs: float) -> str:
    """Convert seconds to human-readable M:SS.CC."""
    if secs < 0:
        return "-"
    mins = int(secs) // 60
    remaining = secs - (mins * 60)
    if mins >= 60:
        hours = mins // 60
        mins = mins % 60
        return f"{hours}:{mins:02d}:{remaining:05.2f}"
    return f"{mins}:{remaining:05.2f}"


def _format_seconds(secs: float) -> str:
    """Format seconds as M:SS or H:MM:SS."""
    total = int(secs)
    mins = total // 60
    s = total % 60
    if mins >= 60:
        hours = mins // 60
        mins = mins % 60
        return f"{hours}:{mins:02d}:{s:02d}"
    return f"{mins}:{s:02d}"


def skill_name(skill: int) -> str:
    """Get display name for a skill level."""
    return SKILL_NAMES.get(skill, str(skill))


# --- Parsing ---


def parse_stats_file(path: str | Path) -> WadStats:
    """Parse a stats file, auto-detecting format."""
    text = Path(path).read_text()
    return parse_stats_text(text)


def parse_stats_text(text: str) -> WadStats:
    """Parse stats from text, auto-detecting format."""
    lines = [line for line in text.splitlines() if line.strip()]
    if not lines:
        raise ValueError("Empty stats file")

    # Auto-detect format by examining the third line (first map line)
    # stats.txt: "MAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3"
    # levelstat.txt: "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5"
    # Also check: stats.txt has a numeric-only first line (version)
    if _is_stats_txt(lines):
        return _parse_stats_txt(lines)
    elif _is_levelstat_txt(lines):
        return _parse_levelstat_txt(lines)
    else:
        raise ValueError("Unrecognized stats file format")


_STATS_TXT_MAP_RE = re.compile(
    r"^(\S+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)"
    r"\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)"
    r"\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)$"
)

_LEVELSTAT_MAP_RE = re.compile(
    r"^(\S+)\s+-\s+"  # map name
    r"(\d+):(\d+(?:\.\d+)?)"  # time M:SS.CC
    r"\s+\((\d+):(\d+(?:\.\d+)?)\)"  # total time (M:SS.CC)
    r"\s+K:\s*(\d+)/(\d+)"  # kills
    r"\s+I:\s*(\d+)/(\d+)"  # items
    r"\s+S:\s*(\d+)/(\d+)"  # secrets
)


def _is_stats_txt(lines: list[str]) -> bool:
    """Check if this looks like a nyan-doom/dsda-doom stats.txt."""
    if len(lines) < 3:
        return False
    # First two lines should be integers (version, total_kills)
    try:
        int(lines[0].strip())
        int(lines[1].strip())
    except ValueError:
        return False
    # Third line should match the 15-field map format
    return bool(_STATS_TXT_MAP_RE.match(lines[2].strip()))


def _is_levelstat_txt(lines: list[str]) -> bool:
    """Check if this looks like a dsda-doom levelstat.txt."""
    # First line should match the levelstat map format
    return bool(_LEVELSTAT_MAP_RE.match(lines[0].strip()))


def _parse_stats_txt(lines: list[str]) -> WadStats:
    """Parse nyan-doom/dsda-doom stats.txt format."""
    version = int(lines[0].strip())
    total_kills = int(lines[1].strip())

    maps: list[MapStats] = []
    for line in lines[2:]:
        line = line.strip()
        if not line:
            continue
        m = _STATS_TXT_MAP_RE.match(line)
        if not m:
            continue
        g = m.groups()
        maps.append(
            MapStats(
                lump=g[0],
                episode=int(g[1]),
                map_num=int(g[2]),
                best_skill=int(g[3]),
                best_time=int(g[4]),
                best_max_time=int(g[5]),
                best_nm_time=int(g[6]),
                total_exits=int(g[7]),
                cumulative_kills=int(g[8]),
                kills=int(g[9]),
                items=int(g[10]),
                secrets=int(g[11]),
                total_kills=int(g[12]),
                total_items=int(g[13]),
                total_secrets=int(g[14]),
            )
        )

    return WadStats(
        format="stats_txt",
        maps=maps,
        version=version,
        header_total_kills=total_kills,
    )


def _parse_levelstat_txt(lines: list[str]) -> WadStats:
    """Parse dsda-doom levelstat.txt format."""
    maps: list[MapStats] = []
    for line in lines:
        line = line.strip()
        if not line:
            continue
        m = _LEVELSTAT_MAP_RE.match(line)
        if not m:
            continue
        g = m.groups()
        time_secs = int(g[1]) * 60 + float(g[2])
        total_time_secs = int(g[3]) * 60 + float(g[4])
        maps.append(
            MapStats(
                lump=g[0],
                time_secs=time_secs,
                total_time_secs=total_time_secs,
                kills=int(g[5]),
                total_kills=int(g[6]),
                items=int(g[7]),
                total_items=int(g[8]),
                secrets=int(g[9]),
                total_secrets=int(g[10]),
                best_skill=4,  # levelstat doesn't record skill; mark as played
            )
        )

    return WadStats(format="levelstat_txt", maps=maps)


# --- Formatting / Export ---


def format_stats(stats: WadStats) -> str:
    """Export WadStats back to original text format."""
    if stats.format == "stats_txt":
        return _format_stats_txt(stats)
    else:
        return _format_levelstat_txt(stats)


def _format_stats_txt(stats: WadStats) -> str:
    """Export to nyan-doom/dsda-doom stats.txt format."""
    lines = [str(stats.version), str(stats.header_total_kills)]
    for m in stats.maps:
        lines.append(
            f"{m.lump} {m.episode} {m.map_num} {m.best_skill} "
            f"{m.best_time} {m.best_max_time} {m.best_nm_time} "
            f"{m.total_exits} {m.cumulative_kills} "
            f"{m.kills} {m.items} {m.secrets} "
            f"{m.total_kills} {m.total_items} {m.total_secrets}"
        )
    return "\n".join(lines) + "\n"


def _format_levelstat_txt(stats: WadStats) -> str:
    """Export to dsda-doom levelstat.txt format."""
    lines = []
    for m in stats.maps:
        time_str = _secs_to_levelstat(m.time_secs)
        total_str = _secs_to_levelstat(m.total_time_secs)
        lines.append(
            f"{m.lump} - {time_str} ({total_str})  "
            f"K: {m.kills}/{m.total_kills}  "
            f"I: {m.items}/{m.total_items}  "
            f"S: {m.secrets}/{m.total_secrets}"
        )
    return "\n".join(lines) + "\n"


def _secs_to_levelstat(secs: float) -> str:
    """Convert seconds to levelstat time format M:SS.CC."""
    if secs < 0:
        return "0:00.00"
    mins = int(secs) // 60
    remaining = secs - (mins * 60)
    return f"{mins}:{remaining:05.2f}"


# --- JSON serialization ---


def stats_to_json(stats: WadStats) -> str:
    """Serialize WadStats to JSON for DB storage."""
    data: dict[str, Any] = {
        "format": stats.format,
        "version": stats.version,
        "header_total_kills": stats.header_total_kills,
        "maps": [asdict(m) for m in stats.maps],
    }
    return json.dumps(data, separators=(",", ":"))


def compute_stats_delta(
    before: WadStats | None,
    after: WadStats,
) -> dict[str, Any]:
    """Compute which maps were played in a session by diffing before/after snapshots.

    Args:
        before: Stats snapshot taken before the session (None for first play).
        after: Stats snapshot taken after the session.

    Returns:
        Dict with:
        - maps_played: list of map lump names that were played this session
        - deltas: list of per-map delta dicts with detailed changes

    For stats.txt (persistent/cumulative): a map is "played" if total_exits
    increased or the map is new. For levelstat.txt (rewritten each run): all
    maps in `after` are this session's maps.
    """
    if after.format == "levelstat_txt":
        # levelstat.txt is rewritten each run — all maps are this session's
        maps_played = [m.lump for m in after.maps]
        deltas = []
        for m in after.maps:
            deltas.append({
                "lump": m.lump,
                "new_map": True,
                "time_secs": m.time_secs,
                "kills": m.kills,
                "total_kills": m.total_kills,
                "items": m.items,
                "total_items": m.total_items,
                "secrets": m.secrets,
                "total_secrets": m.total_secrets,
            })
        return {"maps_played": maps_played, "deltas": deltas}

    # stats.txt: diff field-by-field
    before_map: dict[str, MapStats] = {}
    if before:
        for m in before.maps:
            before_map[m.lump] = m

    maps_played = []
    deltas = []
    for m in after.maps:
        prev = before_map.get(m.lump)
        if prev is None:
            # New map not in before snapshot
            if m.played:
                maps_played.append(m.lump)
                deltas.append({
                    "lump": m.lump,
                    "new_map": True,
                    "exits_delta": m.total_exits,
                    "kills_delta": m.kills,
                    "items_delta": m.items,
                    "secrets_delta": m.secrets,
                    "best_time_before": -1,
                    "best_time_after": m.best_time,
                    "time_improved": m.best_time > 0,
                })
        else:
            exits_delta = m.total_exits - prev.total_exits
            if exits_delta > 0:
                maps_played.append(m.lump)
                deltas.append({
                    "lump": m.lump,
                    "new_map": False,
                    "exits_delta": exits_delta,
                    "kills_delta": m.kills - prev.kills,
                    "items_delta": m.items - prev.items,
                    "secrets_delta": m.secrets - prev.secrets,
                    "best_time_before": prev.best_time,
                    "best_time_after": m.best_time,
                    "time_improved": (
                        m.best_time > 0
                        and (prev.best_time < 0 or m.best_time < prev.best_time)
                    ),
                })

    return {"maps_played": maps_played, "deltas": deltas}


def stats_from_json(json_str: str) -> WadStats:
    """Deserialize WadStats from JSON."""
    data = json.loads(json_str)
    maps = [MapStats(**m) for m in data.get("maps", [])]
    return WadStats(
        format=data["format"],
        maps=maps,
        version=data.get("version", 1),
        header_total_kills=data.get("header_total_kills", 0),
    )
