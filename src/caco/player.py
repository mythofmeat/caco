"""Sourceport launcher and playtime tracking."""

import json
import re
import subprocess
from pathlib import Path

from rich.console import Console

from caco import db
from caco.config import (
    get_cache_dir,
    get_cache_auto_clean,
    get_cache_max_age,
    get_cache_max_size,
    get_default_sourceport,
    get_iwad,
    get_sourceport_args,
    get_stats_dir,
)


# =============================================================================
# Stats File Parsing (nyan-doom/dsda-doom)
# =============================================================================

# Common IWAD name mappings
IWAD_NAMES = {
    "doom.wad": "doom",
    "doom2.wad": "doom2",
    "tnt.wad": "tnt",
    "plutonia.wad": "plutonia",
    "heretic.wad": "heretic",
    "hexen.wad": "hexen",
    "strife.wad": "strife",
    "freedoom1.wad": "freedoom1",
    "freedoom2.wad": "freedoom2",
    "freedm.wad": "freedm",
}


def _detect_iwad_name(wad: dict) -> str | None:
    """Detect the IWAD name from WAD metadata."""
    # Check custom_iwad first
    if wad.get("custom_iwad"):
        iwad_path = Path(wad["custom_iwad"])
        iwad_lower = iwad_path.name.lower()
        return IWAD_NAMES.get(iwad_lower, iwad_path.stem.lower())

    # Try to detect from custom_args "-iwad tnt"
    if wad.get("custom_args"):
        try:
            args = json.loads(wad["custom_args"])
            for i, arg in enumerate(args):
                if arg.lower() == "-iwad" and i + 1 < len(args):
                    iwad_arg = Path(args[i + 1])
                    iwad_lower = iwad_arg.name.lower()
                    return IWAD_NAMES.get(iwad_lower, iwad_arg.stem.lower())
        except (json.JSONDecodeError, TypeError):
            pass

    # Default to doom2 (most common)
    return "doom2"


def _get_wad_basename(wad: dict) -> str | None:
    """Get the WAD basename for stats directory."""
    filename = wad.get("filename")
    if not filename:
        return None

    # Strip extension
    p = Path(filename)
    return p.stem.lower()


def get_stats_path(wad: dict, stats_dir: Path | None = None) -> Path | None:
    """
    Build stats.txt path from WAD metadata.

    Path format: {stats_dir}/{iwad}/{wad_basename}/stats.txt

    Returns None if can't determine path.
    """
    if stats_dir is None:
        stats_dir = get_stats_dir()

    iwad_name = _detect_iwad_name(wad)
    wad_basename = _get_wad_basename(wad)

    if not iwad_name or not wad_basename:
        return None

    return stats_dir / iwad_name / wad_basename / "stats.txt"


def parse_stats_file(path: Path) -> list[tuple[str, int]]:
    """
    Parse nyan-doom/dsda-doom stats.txt file.

    Stats.txt format (space-delimited):
    MAP01 1 1 4 9920 -1 -1 1 214 99 14 1 101 16 3
          ^     ^              ^
          |     |              +-- exits (index 7, 0 = never completed)
          |     +-- skill (index 3, 0=unplayed, 1-5 = ITYTD→NM)
          +-- map number (index 1)

    A map is completed if: skill > 0 AND exits > 0

    Returns: list of (map_name, skill) tuples for completed maps.
    """
    if not path.exists():
        return []

    completions = []
    try:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue

                parts = line.split()
                if len(parts) < 8:
                    continue

                map_name = parts[0]
                try:
                    skill = int(parts[3])
                    exits = int(parts[7])
                except (ValueError, IndexError):
                    continue

                # Completed if skill > 0 and exits > 0
                if skill > 0 and exits > 0:
                    completions.append((map_name, skill))

    except (OSError, IOError):
        pass

    return completions


def sync_completions_from_stats(wad: dict, console: Console | None = None) -> int:
    """
    Sync map completions from stats file for a WAD.

    Returns number of new completions added.
    """
    stats_path = get_stats_path(wad)
    if not stats_path or not stats_path.exists():
        return 0

    completions = parse_stats_file(stats_path)
    if not completions:
        return 0

    added = db.sync_map_completions(wad["id"], completions)

    if added > 0 and console:
        console.print(f"[dim]Synced {added} new map completion(s)[/dim]")

    return added


def get_wad_path(wad: dict, console: Console | None = None) -> Path | None:
    """Get the local path to a WAD file, downloading if needed."""
    # If already cached, return cached path
    if wad.get("cached_path"):
        cached = Path(wad["cached_path"])
        if cached.exists():
            return cached

    # Download based on source type
    source_type = wad["source_type"]

    # Resolve idgames ID: explicit idgames_id takes priority, then source_id for idgames sources
    idgames_id = wad.get("idgames_id") or (wad["source_id"] if source_type == "idgames" else None)

    if idgames_id:
        from caco.sources.idgames import IdgamesSource

        cache_dir = get_cache_dir()
        cache_dir.mkdir(parents=True, exist_ok=True)

        with IdgamesSource() as source:
            entry = source.get(int(idgames_id))
            dest = source.download(entry, cache_dir, console=console)

            # Update cached path in database
            db.update_wad(wad["id"], cached_path=str(dest))
            return dest

    elif source_type == "local":
        # For local files, the source_url is the path
        path = Path(wad["source_url"])
        if path.exists():
            return path

    # Other sources not yet implemented
    return None


# =============================================================================
# Cache Auto-Cleanup
# =============================================================================


def auto_clean_cache(console: Console | None = None) -> int:
    """Perform automatic cache cleanup based on config rules.

    Returns the number of files deleted.
    """
    from datetime import datetime, timedelta

    if not get_cache_auto_clean():
        return 0

    cache_dir = get_cache_dir()
    if not cache_dir.exists():
        return 0

    max_size = get_cache_max_size()
    max_age_days = get_cache_max_age()

    if not max_size and not max_age_days:
        return 0  # No limits configured

    cached_wads = db.get_cached_wads()
    if not cached_wads:
        return 0

    # Build list of cache entries with metadata
    cache_entries = []
    for wad in cached_wads:
        # Only consider idgames sources - they can always be re-downloaded
        # Local files are user's originals, URLs may not be re-downloadable
        if wad.get("source_type") != "idgames":
            continue

        path = Path(wad["cached_path"])
        if path.exists():
            stat = path.stat()
            last_played = db.get_last_played(wad["id"])
            cache_entries.append({
                "wad": wad,
                "path": path,
                "size": stat.st_size,
                "mtime": stat.st_mtime,
                "last_played": last_played,
            })

    if not cache_entries:
        return 0

    to_delete = []

    # Rule 1: Remove files older than max_age_days
    if max_age_days > 0:
        cutoff = datetime.now() - timedelta(days=max_age_days)
        cutoff_ts = cutoff.timestamp()

        for entry in cache_entries:
            # Use last_played if available, otherwise mtime
            if entry["last_played"]:
                try:
                    played_dt = datetime.fromisoformat(entry["last_played"])
                    if played_dt < cutoff:
                        to_delete.append(entry)
                except ValueError:
                    pass
            elif entry["mtime"] < cutoff_ts:
                to_delete.append(entry)

    # Rule 2: If over max_size, remove LRU files until under limit
    if max_size > 0:
        total_size = sum(e["size"] for e in cache_entries)
        if total_size > max_size:
            # Sort by last_played (oldest first), then mtime
            # None values sort first (oldest)
            remaining = [e for e in cache_entries if e not in to_delete]
            remaining.sort(key=lambda e: (e["last_played"] or "", e["mtime"]))

            for entry in remaining:
                if total_size <= max_size:
                    break
                to_delete.append(entry)
                total_size -= entry["size"]

    # Delete files
    if to_delete and console:
        console.print(f"[dim]Auto-cleaning {len(to_delete)} cached file(s)...[/dim]")

    deleted = 0
    for entry in to_delete:
        try:
            entry["path"].unlink()
            db.clear_cached_path(entry["wad"]["id"])
            deleted += 1
        except OSError:
            pass

    return deleted


def play(
    wad_id: int,
    sourceport: str | None = None,
    extra_args: list[str] | None = None,
    console: Console | None = None,
) -> int | None:
    """
    Play a WAD with the specified sourceport.

    Returns the play session duration in seconds, or None if cancelled.
    """
    wad = db.get_wad(wad_id)
    if not wad:
        raise ValueError(f"WAD {wad_id} not found")

    # Auto-clean cache before potentially downloading new files
    auto_clean_cache(console=console)

    # Get or download WAD file
    wad_path = get_wad_path(wad, console=console)
    if not wad_path:
        # Build a helpful error message with source URL if available
        error_parts = [f"No WAD file linked for '{wad['title']}'"]

        source_url = wad.get("source_url")
        source_type = wad.get("source_type")

        if source_url:
            if source_type == "doomwiki":
                error_parts.append(f"\nDoom Wiki page: {source_url}")
            elif source_type == "doomworld":
                error_parts.append(f"\nDoomworld thread: {source_url}")
            else:
                error_parts.append(f"\nSource: {source_url}")

        error_parts.append(f"\n\nDownload the WAD file, then link it with:")
        error_parts.append(f"  caco link {wad_id} /path/to/downloaded.wad")

        raise ValueError("".join(error_parts))

    # Determine sourceport (CLI > WAD-specific > global config)
    port = sourceport or wad.get("custom_sourceport") or get_default_sourceport()
    if not port:
        raise ValueError("No sourceport specified and no default configured")

    # Build command
    cmd = [port]

    # Add IWAD (CLI option would be in extra_args, so: WAD-specific > global config)
    iwad = wad.get("custom_iwad") or get_iwad()
    if iwad:
        cmd.extend(["-iwad", iwad])

    # Add default sourceport args from global config
    default_args = get_sourceport_args()
    if default_args:
        cmd.extend(default_args)

    # Add per-WAD custom args
    if wad.get("custom_args"):
        try:
            wad_args = json.loads(wad["custom_args"])
            if isinstance(wad_args, list):
                cmd.extend(wad_args)
        except json.JSONDecodeError:
            pass

    # Add the WAD file
    cmd.extend(["-file", str(wad_path)])

    # Add extra args from command line (highest priority, can override anything)
    if extra_args:
        cmd.extend(extra_args)

    # Start session
    session_id = db.start_session(wad_id, sourceport=port)

    try:
        # Run sourceport (blocking)
        subprocess.run(cmd)
    finally:
        # End session and calculate duration
        db.end_session(session_id)

    # Auto-sync map completions from stats file
    sync_completions_from_stats(wad, console=console)

    # Return duration
    sessions = db.get_sessions(wad_id)
    if sessions:
        return sessions[0].get("duration_seconds")
    return None


def format_duration(seconds: int) -> str:
    """Format duration as human-readable string."""
    if seconds < 60:
        return f"{seconds}s"
    elif seconds < 3600:
        minutes = seconds // 60
        secs = seconds % 60
        return f"{minutes}m {secs}s"
    else:
        hours = seconds // 3600
        minutes = (seconds % 3600) // 60
        return f"{hours}h {minutes}m"
