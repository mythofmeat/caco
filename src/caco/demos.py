"""Demo file management — find, clean, and name demo recordings."""

import re
from datetime import datetime, timezone
from pathlib import Path

DEMO_EXTENSION = ".lmp"


def get_demos_dir(data_dir: Path) -> Path:
    """Return the demos subdirectory within a WAD's data directory."""
    return data_dir / "demos"


def find_demo_files(data_dir: Path) -> list[dict]:
    """Find all demo files in a WAD's demos directory.

    Returns list of dicts with keys: path, name, rel_path, size, mtime_iso.
    """
    demos_dir = get_demos_dir(data_dir)
    if not demos_dir.is_dir():
        return []

    demos = []
    for path in sorted(demos_dir.iterdir()):
        if path.is_file() and path.suffix.lower() == DEMO_EXTENSION:
            stat = path.stat()
            demos.append({
                "path": path,
                "name": path.name,
                "rel_path": str(path.relative_to(data_dir)),
                "size": stat.st_size,
                "mtime_iso": datetime.fromtimestamp(
                    stat.st_mtime, tz=timezone.utc
                ).isoformat(),
            })
    return demos


def clean_demo_files(data_dir: Path) -> list[Path]:
    """Delete demo files from a WAD's demos directory.

    Returns list of deleted file paths.
    """
    demos = find_demo_files(data_dir)
    deleted = []
    for demo in demos:
        path: Path = demo["path"]
        path.unlink()
        deleted.append(path)
    return deleted


def generate_demo_name(wad_stem: str) -> str:
    """Generate a timestamped demo filename (without extension).

    Sourceports append .lmp automatically when recording, so this returns
    the base name only.
    """
    sanitized = re.sub(r"[^a-zA-Z0-9]", "-", wad_stem).strip("-").lower()
    sanitized = re.sub(r"-+", "-", sanitized)
    if not sanitized:
        sanitized = "demo"
    # Truncate to keep filenames reasonable
    sanitized = sanitized[:48]
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    return f"{sanitized}_{timestamp}"
