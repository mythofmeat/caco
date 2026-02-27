"""WAD CRUD operations and tag management."""

import sqlite3
from datetime import datetime
from enum import Enum
from typing import Any

from caco.db._models import ALLOWED_UPDATE_FIELDS, SourceType, Status
from caco.db._connection import get_connection, _attach_tags


def add_wad(
    title: str,
    source_type: SourceType,
    *,
    author: str | None = None,
    year: int | None = None,
    description: str | None = None,
    source_id: str | None = None,
    source_url: str | None = None,
    filename: str | None = None,
    cached_path: str | None = None,
    status: Status = Status.BACKLOG,
    tags: list[str] | None = None,
    version: str | None = None,
) -> int:
    """Add a WAD to the library. Returns the new WAD ID."""
    with get_connection() as conn:
        cursor = conn.execute(
            """
            INSERT INTO wads (title, author, year, description, source_type,
                              source_id, source_url, filename, cached_path, status, version)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (title, author, year, description, source_type.value,
             source_id, source_url, filename, cached_path, status.value, version),
        )
        wad_id = cursor.lastrowid
        if not wad_id or wad_id <= 0:
            raise RuntimeError(f"Failed to get valid WAD ID after insert (got {wad_id})")

        if tags:
            for tag in tags:
                conn.execute(
                    "INSERT OR IGNORE INTO tags (wad_id, tag) VALUES (?, ?)",
                    (wad_id, tag.lower()),
                )

        return wad_id


def get_wad(wad_id: int, include_deleted: bool = False) -> dict[str, Any] | None:
    """Get a WAD by ID.

    Args:
        wad_id: WAD ID to fetch
        include_deleted: If True, also return deleted WADs
    """
    with get_connection() as conn:
        if include_deleted:
            row = conn.execute("SELECT * FROM wads WHERE id = ?", (wad_id,)).fetchone()
        else:
            row = conn.execute(
                "SELECT * FROM wads WHERE id = ? AND deleted_at IS NULL",
                (wad_id,)
            ).fetchone()
        if row:
            return _attach_tags(conn, dict(row))
        return None


def update_wad(wad_id: int, **fields) -> bool:
    """Update a WAD's fields. Returns True if updated.

    If status is set to 'finished', automatically records a completion.
    Only fields in ALLOWED_UPDATE_FIELDS may be updated.
    """
    if not fields:
        return False

    # Validate field names against whitelist (prevents SQL column injection)
    invalid = set(fields.keys()) - ALLOWED_UPDATE_FIELDS
    if invalid:
        raise ValueError(f"Cannot update field(s): {', '.join(sorted(invalid))}")

    # Check if setting status to finished (before enum conversion)
    recording_completion = False
    status_value = fields.get("status")
    if status_value:
        if isinstance(status_value, Status):
            recording_completion = status_value == Status.FINISHED
        else:
            recording_completion = status_value == Status.FINISHED.value

    # Build clean copy with enums converted to values
    clean_fields = {}
    for key, value in fields.items():
        clean_fields[key] = value.value if isinstance(value, Enum) else value
    clean_fields["updated_at"] = datetime.now().isoformat()

    set_clause = ", ".join(f"{k} = ?" for k in clean_fields.keys())

    with get_connection() as conn:
        cursor = conn.execute(
            f"UPDATE wads SET {set_clause} WHERE id = ?",
            list(clean_fields.values()) + [wad_id],
        )
        updated = cursor.rowcount > 0

        # Record completion atomically if status was set to 'finished'
        if updated and recording_completion:
            # Fetch current stats snapshot to archive with completion
            row = conn.execute(
                "SELECT stats_snapshot FROM wads WHERE id = ?", (wad_id,)
            ).fetchone()
            snapshot = row["stats_snapshot"] if row else None
            conn.execute(
                "INSERT INTO wad_completions (wad_id, stats_snapshot) VALUES (?, ?)",
                (wad_id, snapshot),
            )

    return updated


def delete_wad(wad_id: int, purge: bool = False) -> bool:
    """Delete a WAD (soft delete by default).

    Args:
        wad_id: WAD ID to delete
        purge: If True, permanently delete. If False (default), soft delete.

    Returns True if deleted/trashed.
    """
    with get_connection() as conn:
        if purge:
            cursor = conn.execute("DELETE FROM wads WHERE id = ?", (wad_id,))
        else:
            cursor = conn.execute(
                "UPDATE wads SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL",
                (datetime.now().isoformat(), wad_id),
            )
        return cursor.rowcount > 0


def restore_wad(wad_id: int) -> bool:
    """Restore a soft-deleted WAD. Returns True if restored."""
    with get_connection() as conn:
        cursor = conn.execute(
            "UPDATE wads SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL",
            (wad_id,),
        )
        return cursor.rowcount > 0


def purge_all_deleted() -> int:
    """Permanently delete all soft-deleted WADs. Returns count of purged WADs."""
    with get_connection() as conn:
        cursor = conn.execute("DELETE FROM wads WHERE deleted_at IS NOT NULL")
        return cursor.rowcount


def add_tag(wad_id: int, tag: str) -> bool:
    """Add a tag to a WAD. Returns True if added."""
    with get_connection() as conn:
        try:
            conn.execute(
                "INSERT INTO tags (wad_id, tag) VALUES (?, ?)",
                (wad_id, tag.lower()),
            )
            return True
        except sqlite3.IntegrityError:
            return False


def remove_tag(wad_id: int, tag: str) -> bool:
    """Remove a tag from a WAD. Returns True if removed."""
    with get_connection() as conn:
        cursor = conn.execute(
            "DELETE FROM tags WHERE wad_id = ? AND tag = ?",
            (wad_id, tag.lower()),
        )
        return cursor.rowcount > 0


def remove_all_tags(wad_id: int) -> int:
    """Remove all tags from a WAD. Returns count of tags removed."""
    with get_connection() as conn:
        cursor = conn.execute(
            "DELETE FROM tags WHERE wad_id = ?",
            (wad_id,),
        )
        return cursor.rowcount


def remove_tags_by_pattern(wad_id: int, glob_pattern: str) -> int:
    """Remove tags matching a glob pattern from a WAD. Returns count removed."""
    from caco.db._query import _glob_to_like, _is_glob_pattern

    with get_connection() as conn:
        if _is_glob_pattern(glob_pattern):
            like_pattern = _glob_to_like(glob_pattern)
            cursor = conn.execute(
                r"DELETE FROM tags WHERE wad_id = ? AND tag LIKE ? ESCAPE '\'",
                (wad_id, like_pattern),
            )
        else:
            cursor = conn.execute(
                "DELETE FROM tags WHERE wad_id = ? AND tag = ?",
                (wad_id, glob_pattern.lower()),
            )
        return cursor.rowcount


def get_all_tags() -> list[str]:
    """Get all unique tags."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT DISTINCT tag FROM tags ORDER BY tag"
        ).fetchall()
        return [row["tag"] for row in rows]


def get_tag_counts() -> list[tuple[str, int]]:
    """Get all tags with their WAD counts (excluding deleted WADs)."""
    with get_connection() as conn:
        rows = conn.execute(
            """
            SELECT t.tag, COUNT(*) as count
            FROM tags t
            JOIN wads w ON w.id = t.wad_id
            WHERE w.deleted_at IS NULL
            GROUP BY t.tag
            ORDER BY t.tag
            """
        ).fetchall()
        return [(row["tag"], row["count"]) for row in rows]
