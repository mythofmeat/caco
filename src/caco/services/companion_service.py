"""Companion file registration and lifecycle management.

Handles MD5-based deduplication, managed storage, and orphan cleanup policy.
Used by CLI, TUI, and GUI.
"""

import shutil
from pathlib import Path

from caco import db
from caco.config import get_companion_dir, get_companion_orphan_cleanup, get_link_mode
from caco.utils import compute_md5


def register_companion(
    file_path: str | Path,
    wad_id: int,
) -> tuple[int, str]:
    """Register a companion file and link it to a WAD.

    Computes MD5, checks for dedup, copies/moves to managed dir, and links.

    Args:
        file_path: Path to the file to register.
        wad_id: WAD to link the companion to.

    Returns:
        (companion_id, filename) tuple.

    Raises:
        FileNotFoundError: If file_path doesn't exist.
    """
    file_path = Path(file_path).resolve()
    if not file_path.exists():
        raise FileNotFoundError(f"File not found: {file_path}")

    filename = file_path.name
    md5 = compute_md5(file_path)
    size = file_path.stat().st_size

    # Check for existing companion with same MD5 (dedup)
    existing = db.get_companion_by_md5(md5)
    if existing:
        companion_id = existing["id"]
    else:
        # Copy/move to managed dir
        companion_dir = get_companion_dir()
        companion_dir.mkdir(parents=True, exist_ok=True)

        managed_name = f"{md5[:12]}_{filename}"
        managed_path = companion_dir / managed_name

        if not managed_path.exists():
            link_mode = get_link_mode()
            if link_mode == "move":
                shutil.move(str(file_path), str(managed_path))
            else:
                shutil.copy2(str(file_path), str(managed_path))

        companion_id = db.add_companion(filename, str(managed_path), md5, size)

    # Link to WAD (INSERT OR IGNORE handles already-linked case)
    db.link_companion(wad_id, companion_id)

    return companion_id, filename


def unregister_companion(
    wad_id: int,
    companion_id: int,
    *,
    orphan_policy: str | None = None,
) -> bool:
    """Unlink a companion from a WAD, applying orphan policy if it becomes orphaned.

    Args:
        wad_id: WAD to unlink from.
        companion_id: Companion to unlink.
        orphan_policy: Override policy ('delete', 'keep', 'ask'). If None, reads from config.

    Returns:
        True if the companion was deleted (orphan + delete policy), False otherwise.
    """
    removed = db.unlink_companion(wad_id, companion_id)
    if not removed:
        return False

    if not db.is_orphan(companion_id):
        return False

    # Companion is now orphaned
    if orphan_policy is None:
        orphan_policy = get_companion_orphan_cleanup()

    if orphan_policy == "delete":
        managed_path = db.remove_companion_with_path(companion_id)
        if managed_path:
            p = Path(managed_path)
            if p.exists():
                p.unlink()
        return True

    # "keep" or "ask" (caller handles "ask" at UI level)
    return False
