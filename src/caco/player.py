"""Sourceport launcher and playtime tracking."""

import subprocess
from pathlib import Path

from rich.console import Console

from caco import db
from caco.config import get_cache_dir, get_default_sourceport, get_iwad, get_sourceport_args


def get_wad_path(wad: dict, console: Console | None = None) -> Path | None:
    """Get the local path to a WAD file, downloading if needed."""
    # If already cached, return cached path
    if wad.get("cached_path"):
        cached = Path(wad["cached_path"])
        if cached.exists():
            return cached

    # Download based on source type
    source_type = wad["source_type"]

    if source_type == "idgames":
        from caco.sources.idgames import IdgamesSource

        cache_dir = get_cache_dir()
        cache_dir.mkdir(parents=True, exist_ok=True)

        with IdgamesSource() as source:
            entry = source.get(int(wad["source_id"]))
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

    # Get or download WAD file
    wad_path = get_wad_path(wad, console=console)
    if not wad_path:
        raise ValueError(f"Could not get WAD file for {wad['title']}")

    # Determine sourceport
    port = sourceport or get_default_sourceport()
    if not port:
        raise ValueError("No sourceport specified and no default configured")

    # Build command
    cmd = [port]

    # Add IWAD if configured
    iwad = get_iwad()
    if iwad:
        cmd.extend(["-iwad", iwad])

    # Add default sourceport args
    default_args = get_sourceport_args()
    if default_args:
        cmd.extend(default_args)

    # Add the WAD file
    cmd.extend(["-file", str(wad_path)])

    # Add extra args from command line
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
