"""id24 WAD registry: known id24 content, identification, and database CRUD.

id24 WADs are multi-file content from the 2024 Doom re-release (Legacy of Rust,
resource WADs, modder packs).  Unlike IWADs, each id24 file is a distinct entity
with at most one registered copy — no family/variant model needed.
"""

import sqlite3
from pathlib import Path
from typing import Any

from caco.db._connection import get_connection
from caco.utils import compute_md5

# =============================================================================
# Known id24 MD5 checksums -> (name, version, title)
# =============================================================================

KNOWN_ID24_WADS: dict[str, tuple[str, str, str]] = {
    # id1.wad — Legacy of Rust
    "713c5a3c1734b1d55b2813a3dd0136d9": ("id1", "update2", "Legacy of Rust"),
    "681bcea18c1286e8b9986c335034bdd1": ("id1", "initial", "Legacy of Rust"),
    # id24res.wad — id24 resource WAD
    "4f0651accebc007b853943ac12aa95b8": ("id24res", "all", "id24 Resource WAD"),
    # id1-res.wad — Legacy of Rust resources
    "f8fbab472230bfa090d6a9234d65fae6": ("id1-res", "update2", "Legacy of Rust Resources"),
    "b6b2370ae8733aaf1377b0ef12351572": ("id1-res", "initial", "Legacy of Rust Resources"),
    # id1-tex.wad — Legacy of Rust textures
    "187bfe543f8328b379e46957976e800d": ("id1-tex", "update2", "Legacy of Rust Textures"),
    # id1-weap.wad — Legacy of Rust weapons
    "85d25c8c3d06a05a1283ae4afe749c9f": ("id1-weap", "update2", "Legacy of Rust Weapons"),
    "b50da800b17db51fa06b5191becad82d": ("id1-weap", "initial", "Legacy of Rust Weapons"),
    # id1-mus.wad — Legacy of Rust music
    "436c83dd83a47f8dd251ba15108e9459": ("id1-mus", "update2", "Legacy of Rust Music"),
    # iddm1.wad — id Deathmatch 1
    "5670fd8fe8eb6910ec28f9e27969d84f": ("iddm1", "initial", "id Deathmatch 1"),
}

# =============================================================================
# Filename fallback for unrecognized MD5s
# =============================================================================

KNOWN_ID24_FILENAMES: dict[str, tuple[str, str, str]] = {
    "id1.wad": ("id1", "unknown", "Legacy of Rust"),
    "id24res.wad": ("id24res", "unknown", "id24 Resource WAD"),
    "id1-res.wad": ("id1-res", "unknown", "Legacy of Rust Resources"),
    "id1-tex.wad": ("id1-tex", "unknown", "Legacy of Rust Textures"),
    "id1-weap.wad": ("id1-weap", "unknown", "Legacy of Rust Weapons"),
    "id1-mus.wad": ("id1-mus", "unknown", "Legacy of Rust Music"),
    "iddm1.wad": ("iddm1", "unknown", "id Deathmatch 1"),
}


# =============================================================================
# Identification helpers
# =============================================================================


def identify_id24(path: str | Path) -> tuple[str, str, str] | None:
    """Identify an id24 WAD file by MD5 hash, falling back to filename.

    Returns (name, version, display_title) or None if unrecognized.
    """
    path = Path(path)
    if not path.exists():
        return None

    md5 = compute_md5(path)
    if md5 in KNOWN_ID24_WADS:
        return KNOWN_ID24_WADS[md5]

    filename = path.name.lower()
    if filename in KNOWN_ID24_FILENAMES:
        return KNOWN_ID24_FILENAMES[filename]

    return None


# =============================================================================
# Database CRUD
# =============================================================================


def add_id24(
    name: str,
    path: str,
    *,
    version: str | None = None,
    title: str | None = None,
    md5: str | None = None,
) -> int:
    """Register an id24 WAD in the database.

    Args:
        name: id24 WAD name (e.g., "id1", "id24res")
        path: Absolute path to the .wad file
        version: Version identifier (e.g., "update2", "initial")
        title: Display title (e.g., "Legacy of Rust")
        md5: MD5 checksum (computed if not provided)

    Returns:
        The new id24 WAD's database ID.

    Raises:
        sqlite3.IntegrityError: If name is already registered.
    """
    with get_connection() as conn:
        cursor = conn.execute(
            "INSERT INTO id24_wads (name, version, title, path, md5) VALUES (?, ?, ?, ?, ?)",
            (name, version, title, path, md5),
        )
        return cursor.lastrowid  # type: ignore[return-value]


def get_id24(name: str) -> dict[str, Any] | None:
    """Get a registered id24 WAD by name."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT * FROM id24_wads WHERE name = ?", (name,)
        ).fetchone()
        return dict(row) if row else None


def get_all_id24() -> list[dict[str, Any]]:
    """Get all registered id24 WADs, ordered by name."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT * FROM id24_wads ORDER BY name"
        ).fetchall()
        return [dict(r) for r in rows]


def get_id24_by_path(path: str) -> dict[str, Any] | None:
    """Get a registered id24 WAD by file path."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT * FROM id24_wads WHERE path = ?", (path,)
        ).fetchone()
        return dict(row) if row else None


def remove_id24(name: str) -> int:
    """Remove a registered id24 WAD by name.

    Returns:
        Number of rows removed (0 or 1).
    """
    with get_connection() as conn:
        cursor = conn.execute(
            "DELETE FROM id24_wads WHERE name = ?", (name,)
        )
        return cursor.rowcount


def remove_id24_with_paths(name: str) -> list[str]:
    """Remove a registered id24 WAD and return the path of the removed entry.

    Returns:
        List of file paths from removed rows (0 or 1 entries).
    """
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT path FROM id24_wads WHERE name = ?", (name,)
        ).fetchall()
        conn.execute(
            "DELETE FROM id24_wads WHERE name = ?", (name,)
        )
        return [r["path"] for r in rows]
