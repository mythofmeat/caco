"""Sourceport launcher and playtime tracking."""

import json
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
    resolve_iwad,
    resolve_sourceport,
)


def get_wad_path(
    wad: dict,
    console: Console | None = None,
    progress_callback: object = None,
) -> Path | None:
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
            dest = source.download(
                entry, cache_dir, console=console,
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
        except FileNotFoundError:
            pass  # Already deleted by another process or user
        except OSError:
            continue  # Permission error or other issue, skip
        db.clear_cached_path(entry["wad"]["id"])
        deleted += 1

    return deleted


def play(
    wad_id: int,
    sourceport: str | None = None,
    extra_args: list[str] | None = None,
    console: Console | None = None,
    progress_callback: object = None,
    process_ref: list | None = None,
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
    wad_path = get_wad_path(wad, console=console, progress_callback=progress_callback)
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
    port = resolve_sourceport(port)
    cmd = [port]

    # Add IWAD (CLI option would be in extra_args, so: WAD-specific > global config)
    iwad = wad.get("custom_iwad") or get_iwad()
    if iwad:
        cmd.extend(["-iwad", resolve_iwad(iwad)])

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

    # Only start tracking the session after a successful launch
    session_id = db.start_session(wad_id, sourceport=port)

    try:
        proc.wait()
    finally:
        # End session and calculate duration
        db.end_session(session_id)

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
