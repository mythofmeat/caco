"""Database connection management and shared tag helpers."""

import sqlite3
from typing import Any


def get_connection() -> sqlite3.Connection:
    """Get a database connection, creating the database if needed."""
    # Deferred import — ensures test patches on caco.config.get_db_path are picked up
    from caco.config import get_db_path

    db_path = get_db_path()
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys = ON")
    conn.execute("PRAGMA journal_mode = WAL")
    conn.execute("PRAGMA synchronous = NORMAL")
    conn.execute("PRAGMA cache_size = -20000")  # 20 MB
    conn.execute("PRAGMA temp_store = MEMORY")
    return conn


def _fetch_tags(conn: sqlite3.Connection, wad_id: int) -> list[str]:
    """Fetch tags for a single WAD."""
    rows = conn.execute(
        "SELECT tag FROM tags WHERE wad_id = ?", (wad_id,)
    ).fetchall()
    return [r["tag"] for r in rows]


def _attach_tags(conn: sqlite3.Connection, wad: dict) -> dict:
    """Attach tags to a WAD dict in-place and return it."""
    wad["tags"] = _fetch_tags(conn, wad["id"])
    return wad


def _fetch_tags_batch(conn: sqlite3.Connection, wad_ids: list[int]) -> dict[int, list[str]]:
    """Fetch tags for multiple WADs efficiently. Returns {wad_id: [tags]}."""
    if not wad_ids:
        return {}
    placeholders = ",".join("?" * len(wad_ids))
    rows = conn.execute(
        f"SELECT wad_id, tag FROM tags WHERE wad_id IN ({placeholders}) ORDER BY tag",
        wad_ids,
    ).fetchall()
    result: dict[int, list[str]] = {}
    for r in rows:
        result.setdefault(r["wad_id"], []).append(r["tag"])
    return result


# Conservative limit for SQLite's SQLITE_MAX_VARIABLE_NUMBER (default 999)
_SQLITE_MAX_VARS = 900


def _batch_query(
    wad_ids: list[int],
    query_template: str,
    result_column: str = "result",
) -> dict[int, Any]:
    """Generic batch query helper for aggregation queries.

    Args:
        wad_ids: List of WAD IDs to query
        query_template: SQL with {placeholders} format string for IN clause
        result_column: Column name to extract from results

    Returns:
        Dict mapping wad_id to result value
    """
    if not wad_ids:
        return {}

    result = {}
    with get_connection() as conn:
        for i in range(0, len(wad_ids), _SQLITE_MAX_VARS):
            chunk = wad_ids[i:i + _SQLITE_MAX_VARS]
            placeholders = ",".join("?" * len(chunk))
            query = query_template.format(placeholders=placeholders)
            rows = conn.execute(query, chunk).fetchall()
            for row in rows:
                result[row["wad_id"]] = row[result_column]
    return result
