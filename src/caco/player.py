"""Sourceport launcher and playtime tracking."""

import json
import logging
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from caco import db
from caco.config import (
    find_wad_data_dir,
    get_auto_detect_complevel,
    get_auto_detect_iwad,
    get_auto_stats,
    get_cache_dir,
    get_cache_auto_clean,
    get_cache_max_age,
    get_cache_max_size,
    get_default_sourceport,
    get_iwad,
    get_manage_data_dirs,
    get_profile_path,
    get_sourceport_args,
    get_wad_data_dir,
    resolve_iwad,
    resolve_sourceport,
)

logger = logging.getLogger(__name__)

# Callback for download progress: (downloaded_bytes, total_bytes, filename) -> None
ProgressCallback = Callable[[int, int | None, str], None]


@dataclass
class PlayResult:
    """Result of a play session."""

    duration: int | None
    exit_code: int | None

    @property
    def crashed(self) -> bool:
        """True if the sourceport exited with a non-zero code."""
        return self.exit_code is not None and self.exit_code != 0


def get_wad_path(
    wad: dict,
    progress_callback: ProgressCallback | None = None,
) -> Path | None:
    """Get the local path to a WAD file, downloading if needed."""
    # If already cached, return cached path
    if wad.get("cached_path"):
        cached = Path(wad["cached_path"])
        if cached.exists():
            return cached

        # Cached path is stale — check current cache dir for the same filename
        cache_dir = get_cache_dir()
        relocated = cache_dir / cached.name
        if relocated.exists():
            db.update_wad(wad["id"], cached_path=str(relocated))
            return relocated

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
            dest: Path = source.download(
                entry, cache_dir,
                progress_callback=progress_callback,
            )

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


def auto_clean_cache() -> int:
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
    # Filter to re-downloadable sources first
    eligible = [w for w in cached_wads if w.get("source_type") == "idgames"]
    # Batch-fetch all last_played dates in one query
    wad_ids = [w["id"] for w in eligible]
    last_played_map = db.get_last_played_batch(wad_ids) if wad_ids else {}

    cache_entries: list[dict[str, Any]] = []
    for wad in eligible:
        path = Path(wad["cached_path"])
        if path.exists():
            stat = path.stat()
            cache_entries.append({
                "wad": wad,
                "path": path,
                "size": stat.st_size,
                "mtime": stat.st_mtime,
                "last_played": last_played_map.get(wad["id"]),
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
    if to_delete:
        logger.info("Auto-cleaning %d cached file(s)", len(to_delete))

    deleted = 0
    for entry in to_delete:
        try:
            entry["path"].unlink()
        except FileNotFoundError:
            pass  # Already deleted by another process or user
        except OSError:
            continue  # Permission error or other issue, skip
        db.clear_cached_path(entry["wad"]["id"])
        deleted += 1

    return deleted


def _find_stats_files(directory: Path) -> list[Path]:
    """Search for all stats files in a WAD data directory.

    nyan-doom nests stats as {iwad}/{wad}/stats.txt, so search recursively.
    Multiple files can exist when IWAD or sourceport changes create different
    nested directories. Prefers stats.txt over levelstat.txt.
    """
    result: list[Path] = []
    for name in ("stats.txt", "levelstat.txt"):
        result.extend(directory.rglob(name))
    return result


def _read_stats_snapshot(wad_id: int) -> str | None:
    """Read and parse stats from the WAD's data dir, returning JSON string or None.

    When multiple stats files exist (e.g., from IWAD/sourceport changes),
    merges them keeping the best data per map.

    Silently returns None if data dirs are disabled, no data dir exists,
    no stats file is found, or parsing fails.
    """
    if not get_auto_stats() or not get_manage_data_dirs():
        return None

    try:
        data_dir = find_wad_data_dir(wad_id)
        if not data_dir or not data_dir.is_dir():
            return None

        stats_paths = _find_stats_files(data_dir)
        if not stats_paths:
            return None

        from caco.wad_stats import parse_stats_file, merge_stats, stats_to_json

        parsed = []
        for path in stats_paths:
            try:
                parsed.append(parse_stats_file(path))
            except (OSError, ValueError):
                logger.debug("Skipping unparseable stats file: %s", path)
        if not parsed:
            return None

        wad_stats = merge_stats(parsed)
        return stats_to_json(wad_stats)
    except (OSError, ValueError, KeyError):
        logger.warning("Failed to read stats for WAD %d", wad_id, exc_info=True)
        return None


def _auto_track_stats(wad_id: int, wad: dict) -> str | None:
    """Read stats from the WAD's data dir and store on the WAD record.

    Returns the JSON stats string if successful, None otherwise.
    Silently skips if data dirs are disabled, no data dir exists,
    no stats file is found, or parsing fails.
    """
    json_str = _read_stats_snapshot(wad_id)
    if json_str:
        db.update_wad(wad_id, stats_snapshot=json_str)
        logger.info("Auto-tracked stats for WAD %d", wad_id)
    return json_str


def _get_id24_resource_args(wad: dict, wad_path: Path | None) -> list[str]:
    """Return id24 resource WAD paths to prepend to the -file list.

    - Any WAD with a COMPLVL lump (id24 signal) gets id24res.wad
    - When playing id1.wad specifically, also load id1-res, id1-tex, id1-weap, id1-mus
    """
    file_args: list[str] = []

    # Check for COMPLVL lump directly — this correctly identifies id24 WADs
    # without relying on any DB column (a regular WAD with heuristic-detected
    # complevel should NOT trigger id24 resource loading)
    has_complvl = False
    if wad_path and Path(wad_path).exists():
        from caco.iwad_detect import detect_complvl
        has_complvl = detect_complvl(wad_path) is not None

    if not has_complvl:
        return file_args

    # Load id24res.wad for any id24 WAD
    id24res = db.get_id24("id24res")
    if id24res and Path(id24res["path"]).exists():
        file_args.append(id24res["path"])

    # Check if this is id1.wad (Legacy of Rust) — load its specific resources
    is_id1 = False
    if wad_path:
        stem = Path(wad_path).stem.lower()
        if stem == "id1":
            is_id1 = True

    if is_id1:
        for name in ("id1-res", "id1-tex", "id1-weap", "id1-mus"):
            entry = db.get_id24(name)
            if entry and Path(entry["path"]).exists():
                file_args.append(entry["path"])

    return file_args


def play(
    wad_id: int,
    sourceport: str | None = None,
    extra_args: list[str] | None = None,
    progress_callback: ProgressCallback | None = None,
    process_ref: list | None = None,
    record: str | bool | None = None,
    config_profile: str | None = None,
) -> PlayResult:
    """
    Play a WAD with the specified sourceport.

    Returns a PlayResult with duration and exit code.
    """
    wad = db.get_wad(wad_id)
    if not wad:
        raise ValueError(f"WAD {wad_id} not found")

    # Auto-clean cache before potentially downloading new files
    auto_clean_cache()

    # Get or download WAD file
    wad_path = get_wad_path(wad, progress_callback=progress_callback)
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
        error_parts.append(f"  caco modify id:{wad_id} --link /path/to/downloaded.wad")

        raise ValueError("".join(error_parts))

    # Determine sourceport (CLI > WAD-specific > global config)
    port = sourceport or wad.get("custom_sourceport") or get_default_sourceport()
    if not port:
        raise ValueError("No sourceport specified and no default configured")

    # Build command — validate sourceport exists before launching
    port = resolve_sourceport(port)
    if not shutil.which(port) and not Path(port).is_file():
        raise FileNotFoundError(
            f"Sourceport '{port}' not found on PATH or as a file. "
            "Set a valid sourceport with: caco config set sourceport <name>"
        )
    cmd = [port]

    # Auto-detect IWAD if not explicitly set
    if not wad.get("custom_iwad") and wad_path and get_auto_detect_iwad():
        from caco.iwad_detect import detect_iwad

        detected = detect_iwad(wad_path)
        if detected:
            logger.info("Auto-detected IWAD: %s for WAD %d", detected, wad_id)
            db.update_wad(wad_id, custom_iwad=detected)
            wad["custom_iwad"] = detected

    # Auto-detect complevel if not explicitly set
    if wad.get("complevel") is None and wad_path and get_auto_detect_complevel():
        from caco.complevel_detect import detect_complevel

        detected_cl = detect_complevel(wad_path)
        if detected_cl is not None:
            logger.info("Auto-detected complevel: %d for WAD %d", detected_cl, wad_id)
            db.update_wad(wad_id, complevel=detected_cl)
            wad["complevel"] = detected_cl

    # Add IWAD (CLI option would be in extra_args, so: WAD-specific > global config)
    iwad = wad.get("custom_iwad") or get_iwad()
    if iwad:
        cmd.extend(["-iwad", resolve_iwad(iwad)])

    # Add default sourceport args from global config
    default_args = get_sourceport_args()
    if default_args:
        cmd.extend(default_args)

    # Inject complevel flag if set and not already present in args
    if wad.get("complevel") is not None:
        all_args = cmd + (extra_args or [])
        if "-complevel" not in all_args:
            from caco.sourceports import get_complevel_args

            cl_args = get_complevel_args(port, wad["complevel"])
            if cl_args:
                cmd.extend(cl_args)

    # Add per-WAD custom args
    if wad.get("custom_args"):
        try:
            wad_args = json.loads(wad["custom_args"])
            if isinstance(wad_args, list):
                cmd.extend(wad_args)
        except json.JSONDecodeError:
            pass

    # Inject managed config profile for dsda-family ports
    profile_name = config_profile or wad.get("custom_config") or "default"
    from caco.sourceports import get_config_args

    profile_path = get_profile_path(port, profile_name)
    config_args = get_config_args(port, str(profile_path))
    if config_args:
        # Auto-create profile if it doesn't exist
        profile_path.parent.mkdir(parents=True, exist_ok=True)
        if not profile_path.exists():
            profile_path.touch()
        cmd.extend(config_args)

    # Inject per-WAD data directory args (if enabled and sourceport is recognized)
    if get_manage_data_dirs():
        from caco.sourceports import get_data_dir_args

        wad_data_dir = find_wad_data_dir(wad_id) or get_wad_data_dir(wad_id, wad["title"])
        wad_data_dir.mkdir(parents=True, exist_ok=True)
        iwad_name = wad.get("custom_iwad") or get_iwad() or None
        data_args = get_data_dir_args(port, str(wad_data_dir), iwad=iwad_name, wad_path=str(wad_path))
        if data_args:
            cmd.extend(data_args)

    # Handle demo recording
    demo_path: str | None = None
    if record:
        from caco.demos import generate_demo_name, get_demos_dir

        if get_manage_data_dirs():
            demos_dir = get_demos_dir(wad_data_dir)
        else:
            demos_dir = get_demos_dir(
                find_wad_data_dir(wad_id) or get_wad_data_dir(wad_id, wad["title"])
            )
        demos_dir.mkdir(parents=True, exist_ok=True)

        if isinstance(record, str):
            demo_name = record
        else:
            wad_stem = Path(wad_path).stem if wad_path else wad["title"]
            demo_name = generate_demo_name(wad_stem)

        demo_path = str(demos_dir / demo_name)
        cmd.extend(["-record", demo_path])

    # Build -file list: id24 resources + main WAD + companion WADs
    file_args = _get_id24_resource_args(wad, wad_path)
    companion_file_args = []
    deh_args = []
    companions = db.get_wad_companions(wad["id"], enabled_only=True)
    if companions:
        from caco.sourceports import uses_deh_flag

        deh_extensions = {".deh", ".bex"}
        for comp in companions:
            comp_path = comp.get("path")
            if not comp_path:
                continue
            if Path(comp["filename"]).suffix.lower() in deh_extensions:
                if uses_deh_flag(port):
                    deh_args.extend(["-deh", comp_path])
                else:
                    companion_file_args.append(comp_path)
            else:
                companion_file_args.append(comp_path)

    # Add DEH args before -file
    if deh_args:
        cmd.extend(deh_args)

    # Add the WAD file before any companion WADs on the same -file.
    cmd.extend(["-file"] + file_args + [str(wad_path)] + companion_file_args)

    # Add extra args from command line (highest priority, can override anything)
    if extra_args:
        cmd.extend(extra_args)

    # Launch sourceport process — use Popen so launch failures are caught
    # before we create a session record in the database.
    try:
        proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL)
    except FileNotFoundError:
        raise FileNotFoundError(
            f"Sourceport '{port}' not found. "
            "Check that it's installed and available on your PATH."
        ) from None
    except PermissionError:
        raise PermissionError(
            f"Permission denied running sourceport '{port}'. "
            "Check file permissions."
        ) from None

    # Expose process handle for external cancellation (GUI stop button)
    if process_ref is not None:
        process_ref.append(proc)

    # Capture stats snapshot before play for per-session map tracking
    stats_before = _read_stats_snapshot(wad_id)

    # Only start tracking the session after a successful launch
    session_id = db.start_session(wad_id, sourceport=port)

    try:
        proc.wait()
    finally:
        # End session and calculate duration; always record exit code
        db.end_session(session_id, exit_code=proc.returncode)

    # Auto-track stats from data directory
    stats_after = _auto_track_stats(wad_id, wad)

    # Attach before/after snapshots to the session for per-session map tracking
    if stats_before or stats_after:
        db.update_session_stats(session_id, stats_before, stats_after)

    # Link recorded demo to the session
    if demo_path:
        # Sourceport appends .lmp if it wasn't already there
        lmp_path = demo_path if demo_path.endswith(".lmp") else demo_path + ".lmp"
        if Path(lmp_path).exists():
            db.update_session_demo(session_id, lmp_path)
            logger.info("Recorded demo: %s", lmp_path)
        else:
            logger.warning("Demo file not found after recording: %s", lmp_path)

    # Build result
    sessions = db.get_sessions(wad_id)
    duration = sessions[0].get("duration_seconds") if sessions else None
    return PlayResult(duration=duration, exit_code=proc.returncode)


def play_iwad(
    iwad_name: str,
    sourceport: str | None = None,
    extra_args: list[str] | None = None,
    config_profile: str | None = None,
) -> PlayResult:
    """
    Play an IWAD directly with no PWAD.

    Returns a PlayResult with duration and exit code.
    """
    import time

    # Resolve IWAD
    resolved = resolve_iwad(iwad_name)
    if not Path(resolved).exists():
        raise FileNotFoundError(
            f"IWAD '{iwad_name}' not found. "
            "Register it with: caco import /path/to/iwad.wad"
        )

    # Determine sourceport
    port = sourceport or get_default_sourceport()
    if not port:
        raise ValueError("No sourceport specified and no default configured")

    port = resolve_sourceport(port)
    if not shutil.which(port) and not Path(port).is_file():
        raise FileNotFoundError(
            f"Sourceport '{port}' not found on PATH or as a file. "
            "Set a valid sourceport with: caco config set sourceport <name>"
        )

    # Build command
    cmd = [port, "-iwad", resolved]

    # Add default sourceport args from global config
    default_args = get_sourceport_args()
    if default_args:
        cmd.extend(default_args)

    # Inject managed config profile for dsda-family ports
    profile_name = config_profile or "default"
    from caco.sourceports import get_config_args

    profile_path = get_profile_path(port, profile_name)
    config_args = get_config_args(port, str(profile_path))
    if config_args:
        profile_path.parent.mkdir(parents=True, exist_ok=True)
        if not profile_path.exists():
            profile_path.touch()
        cmd.extend(config_args)

    if extra_args:
        cmd.extend(extra_args)

    # Launch
    try:
        proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL)
    except FileNotFoundError:
        raise FileNotFoundError(
            f"Sourceport '{port}' not found. "
            "Check that it's installed and available on your PATH."
        ) from None
    except PermissionError:
        raise PermissionError(
            f"Permission denied running sourceport '{port}'. "
            "Check file permissions."
        ) from None

    start = time.monotonic()
    proc.wait()
    duration = int(time.monotonic() - start)
    return PlayResult(duration=duration, exit_code=proc.returncode)


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
