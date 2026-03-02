"""Companion file registry: managed storage with MD5 deduplication and per-WAD linking."""

import sqlite3
from typing import Any

from caco.db._connection import get_connection


# =============================================================================
# Companion file CRUD
# =============================================================================


def add_companion(
    filename: str,
    path: str,
    md5: str,
    size: int,
) -> int:
    """Register a companion file in the database.

    Returns the new companion file's database ID.

    Raises:
        sqlite3.IntegrityError: If md5 is already registered.
    """
    with get_connection() as conn:
        cursor = conn.execute(
            "INSERT INTO companion_files_registry (filename, path, md5, size) VALUES (?, ?, ?, ?)",
            (filename, path, md5, size),
        )
        return cursor.lastrowid  # type: ignore[return-value]


def get_companion(companion_id: int) -> dict[str, Any] | None:
    """Get a companion file by ID."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT * FROM companion_files_registry WHERE id = ?", (companion_id,)
        ).fetchone()
        return dict(row) if row else None


def get_companion_by_md5(md5: str) -> dict[str, Any] | None:
    """Get a companion file by MD5 hash (for dedup checks)."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT * FROM companion_files_registry WHERE md5 = ?", (md5,)
        ).fetchone()
        return dict(row) if row else None


def get_all_companions() -> list[dict[str, Any]]:
    """Get all registered companion files."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT * FROM companion_files_registry ORDER BY filename"
        ).fetchall()
        return [dict(r) for r in rows]


def get_all_companions_with_counts() -> list[dict[str, Any]]:
    """Get all registered companion files with their WAD link counts."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT cf.*, COUNT(wc.wad_id) AS wad_count "
            "FROM companion_files_registry cf "
            "LEFT JOIN wad_companions wc ON wc.companion_id = cf.id "
            "GROUP BY cf.id "
            "ORDER BY cf.filename"
        ).fetchall()
        return [dict(r) for r in rows]


def remove_companion(companion_id: int) -> int:
    """Remove a companion file from the database.

    Returns number of rows removed (0 or 1).
    """
    with get_connection() as conn:
        cursor = conn.execute(
            "DELETE FROM companion_files_registry WHERE id = ?", (companion_id,)
        )
        return cursor.rowcount


def remove_companion_with_path(companion_id: int) -> str | None:
    """Remove a companion file and return its managed path for cleanup.

    Returns the file path if found, None otherwise.
    """
    with get_connection() as conn:
        row = conn.execute(
            "SELECT path FROM companion_files_registry WHERE id = ?", (companion_id,)
        ).fetchone()
        if not row:
            return None
        conn.execute(
            "DELETE FROM companion_files_registry WHERE id = ?", (companion_id,)
        )
        return row["path"]


# =============================================================================
# WAD-companion linking (junction table)
# =============================================================================


def link_companion(
    wad_id: int,
    companion_id: int,
    *,
    enabled: bool = True,
    load_order: int | None = None,
) -> None:
    """Link a companion file to a WAD.

    If load_order is None, appends after the current highest order.
    """
    if load_order is None:
        load_order = get_next_load_order(wad_id)
    with get_connection() as conn:
        conn.execute(
            "INSERT OR IGNORE INTO wad_companions (wad_id, companion_id, enabled, load_order) "
            "VALUES (?, ?, ?, ?)",
            (wad_id, companion_id, 1 if enabled else 0, load_order),
        )


def unlink_companion(wad_id: int, companion_id: int) -> int:
    """Unlink a companion file from a WAD.

    Returns number of rows removed (0 or 1).
    """
    with get_connection() as conn:
        cursor = conn.execute(
            "DELETE FROM wad_companions WHERE wad_id = ? AND companion_id = ?",
            (wad_id, companion_id),
        )
        return cursor.rowcount


def set_companion_enabled(wad_id: int, companion_id: int, enabled: bool) -> None:
    """Enable or disable a companion file for a WAD."""
    with get_connection() as conn:
        conn.execute(
            "UPDATE wad_companions SET enabled = ? WHERE wad_id = ? AND companion_id = ?",
            (1 if enabled else 0, wad_id, companion_id),
        )


def set_companion_load_order(wad_id: int, companion_id: int, load_order: int) -> None:
    """Set the load order for a companion file on a WAD."""
    with get_connection() as conn:
        conn.execute(
            "UPDATE wad_companions SET load_order = ? WHERE wad_id = ? AND companion_id = ?",
            (load_order, wad_id, companion_id),
        )


def get_wad_companions(
    wad_id: int,
    *,
    enabled_only: bool = False,
) -> list[dict[str, Any]]:
    """Get companion files linked to a WAD, ordered by load_order.

    Returns dicts with companion_files_registry columns plus enabled and load_order.
    """
    where = "wc.wad_id = ?"
    if enabled_only:
        where += " AND wc.enabled = 1"
    with get_connection() as conn:
        rows = conn.execute(
            f"SELECT cf.*, wc.enabled, wc.load_order "
            f"FROM wad_companions wc "
            f"JOIN companion_files_registry cf ON cf.id = wc.companion_id "
            f"WHERE {where} "
            f"ORDER BY wc.load_order, cf.filename",
            (wad_id,),
        ).fetchall()
        return [dict(r) for r in rows]


def get_companion_wads(companion_id: int) -> list[dict[str, Any]]:
    """Get WADs that reference a companion file."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT w.id, w.title FROM wads w "
            "JOIN wad_companions wc ON wc.wad_id = w.id "
            "WHERE wc.companion_id = ?",
            (companion_id,),
        ).fetchall()
        return [dict(r) for r in rows]


def get_next_load_order(wad_id: int) -> int:
    """Get the next load_order value for a WAD's companions."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT COALESCE(MAX(load_order), -1) + 1 AS next_order "
            "FROM wad_companions WHERE wad_id = ?",
            (wad_id,),
        ).fetchone()
        return row["next_order"]  # type: ignore[index]


def is_orphan(companion_id: int) -> bool:
    """Check if a companion file has no WAD links."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT COUNT(*) AS cnt FROM wad_companions WHERE companion_id = ?",
            (companion_id,),
        ).fetchone()
        return row["cnt"] == 0  # type: ignore[index]


def would_be_orphan(companion_id: int, wad_id: int) -> bool:
    """Check if unlinking from wad_id would leave the companion with no links."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT COUNT(*) AS cnt FROM wad_companions "
            "WHERE companion_id = ? AND wad_id != ?",
            (companion_id, wad_id),
        ).fetchone()
        return row["cnt"] == 0  # type: ignore[index]


def get_wad_companion_by_filename(
    wad_id: int,
    filename: str,
) -> dict[str, Any] | None:
    """Find a companion linked to a WAD by original filename."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT cf.*, wc.enabled, wc.load_order "
            "FROM wad_companions wc "
            "JOIN companion_files_registry cf ON cf.id = wc.companion_id "
            "WHERE wc.wad_id = ? AND cf.filename = ?",
            (wad_id, filename),
        ).fetchone()
        return dict(row) if row else None
