"""Save game management — find, backup, restore, and clean save files."""

import re
import zipfile
from datetime import datetime, timezone
from pathlib import Path

from caco.config import _sanitize_dirname, get_backup_dir
from caco.sourceports import ALL_SAVE_EXTENSIONS


def find_save_files(data_dir: Path) -> list[dict]:
    """Find all save files in a WAD data directory.

    Recursively scans for files matching known save extensions (.dsg, .zds).

    Returns list of dicts with keys: path, name, rel_path, size, mtime_iso.
    """
    if not data_dir.is_dir():
        return []

    saves = []
    for path in sorted(data_dir.rglob("*")):
        if path.is_file() and path.suffix.lower() in ALL_SAVE_EXTENSIONS:
            stat = path.stat()
            saves.append({
                "path": path,
                "name": path.name,
                "rel_path": str(path.relative_to(data_dir)),
                "size": stat.st_size,
                "mtime_iso": datetime.fromtimestamp(
                    stat.st_mtime, tz=timezone.utc
                ).isoformat(),
            })
    return saves


def create_backup(wad_id: int, title: str, data_dir: Path) -> Path:
    """Create a zip backup of a WAD's entire data directory.

    Returns the path to the created backup file.
    Raises FileNotFoundError if data_dir doesn't exist.
    """
    if not data_dir.is_dir():
        raise FileNotFoundError(f"Data directory does not exist: {data_dir}")

    backup_dir = get_backup_dir()
    backup_dir.mkdir(parents=True, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    sanitized = _sanitize_dirname(title)
    backup_name = f"{wad_id}_{sanitized}_{timestamp}.zip"
    backup_path = backup_dir / backup_name

    with zipfile.ZipFile(backup_path, "w", zipfile.ZIP_DEFLATED) as zf:
        for path in sorted(data_dir.rglob("*")):
            if path.is_file():
                zf.write(path, path.relative_to(data_dir))

    return backup_path


def restore_backup(backup_path: Path, data_dir: Path) -> int:
    """Restore a backup zip into a WAD's data directory.

    Creates the data directory if it doesn't exist.
    Returns the number of files extracted.
    Raises FileNotFoundError if backup_path doesn't exist.
    """
    if not backup_path.is_file():
        raise FileNotFoundError(f"Backup file not found: {backup_path}")

    data_dir.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(backup_path, "r") as zf:
        members = [m for m in zf.namelist() if not m.endswith("/")]
        zf.extractall(data_dir)
        return len(members)


def list_backups(wad_id: int) -> list[dict]:
    """List existing backups for a specific WAD.

    Returns list of dicts with keys: path, name, size, created_iso.
    Sorted by creation time (newest first).
    """
    backup_dir = get_backup_dir()
    if not backup_dir.is_dir():
        return []

    prefix = f"{wad_id}_"
    backups = []
    for path in backup_dir.iterdir():
        if path.is_file() and path.name.startswith(prefix) and path.suffix == ".zip":
            stat = path.stat()
            backups.append({
                "path": path,
                "name": path.name,
                "size": stat.st_size,
                "created_iso": datetime.fromtimestamp(
                    stat.st_mtime, tz=timezone.utc
                ).isoformat(),
            })

    backups.sort(key=lambda b: b["created_iso"], reverse=True)
    return backups


def list_all_backups() -> list[dict]:
    """List all existing backups across all WADs.

    Returns list of dicts with keys: path, name, wad_id, size, created_iso.
    Sorted by creation time (newest first).
    """
    backup_dir = get_backup_dir()
    if not backup_dir.is_dir():
        return []

    backups = []
    for path in backup_dir.iterdir():
        if path.is_file() and path.suffix == ".zip":
            # Parse wad_id from filename: {wad_id}_{title}_{timestamp}.zip
            match = re.match(r"^(\d+)_", path.name)
            if not match:
                continue
            wad_id = int(match.group(1))
            stat = path.stat()
            backups.append({
                "path": path,
                "name": path.name,
                "wad_id": wad_id,
                "size": stat.st_size,
                "created_iso": datetime.fromtimestamp(
                    stat.st_mtime, tz=timezone.utc
                ).isoformat(),
            })

    backups.sort(key=lambda b: b["created_iso"], reverse=True)
    return backups


def clean_save_files(data_dir: Path) -> list[Path]:
    """Delete save files from a WAD data directory, keeping stats and configs.

    Returns list of deleted file paths.
    """
    saves = find_save_files(data_dir)
    deleted = []
    for save in saves:
        path: Path = save["path"]
        path.unlink()
        deleted.append(path)
    return deleted


def resolve_backup_path(wad_id: int, backup_arg: str | None = None) -> Path | None:
    """Resolve a backup argument to a path.

    If backup_arg is None, returns the most recent backup for the WAD.
    If backup_arg is a filename, looks it up in the backup directory.
    If backup_arg is an absolute path, returns it directly.

    Returns None if no matching backup is found.
    """
    if backup_arg:
        # Absolute path
        candidate = Path(backup_arg)
        if candidate.is_absolute():
            return candidate if candidate.is_file() else None

        # Filename in backup dir
        backup_dir = get_backup_dir()
        candidate = backup_dir / backup_arg
        if candidate.is_file():
            return candidate
        return None

    # No arg — use most recent
    backups = list_backups(wad_id)
    if backups:
        return backups[0]["path"]
    return None
