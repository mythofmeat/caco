"""Shared IWAD and id24 WAD registration helpers.

Used by CLI, TUI, and GUI to register (identify + copy + DB insert)
IWAD and id24 WAD files from local paths.
"""

import shutil
from pathlib import Path

from caco import db
from caco.config import get_iwad_dir, get_id24_dir
from caco.utils import compute_md5


def register_iwad(path: Path) -> tuple[str, str, str] | None:
    """Identify and register an IWAD file.

    Identifies the file by MD5/filename, copies to the managed IWAD
    directory, and adds to the DB.

    Returns (family, variant, title) on success, or None if the file
    is not a recognized IWAD or is already registered.
    """
    info = db.identify_iwad(path)
    if not info:
        return None

    family, variant, title = info

    existing = db.get_iwad_variant(family, variant)
    if existing:
        return None

    iwad_dir = get_iwad_dir()
    managed_rel = db.managed_iwad_filename(family, variant)
    dest = iwad_dir / managed_rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(path), str(dest))

    md5 = compute_md5(path)
    db.add_iwad(family, variant, str(dest), title=title, md5=md5)
    return (family, variant, title)


def register_id24(path: Path) -> tuple[str, str, str] | None:
    """Identify and register an id24 WAD file.

    Identifies the file by MD5/filename, copies to the managed id24
    directory, and adds to the DB.

    Returns (name, version, title) on success, or None if the file
    is not a recognized id24 WAD or is already registered.
    """
    info = db.identify_id24(path)
    if not info:
        return None

    name, version, title = info

    existing = db.get_id24(name)
    if existing:
        return None

    id24_dir = get_id24_dir()
    id24_dir.mkdir(parents=True, exist_ok=True)
    dest = id24_dir / f"{name}.wad"
    shutil.copy2(str(path), str(dest))

    md5 = compute_md5(path)
    db.add_id24(name, str(dest), version=version, title=title, md5=md5)
    return (name, version, title)
