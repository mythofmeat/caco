"""IWAD registry: known IWADs, identification, and database CRUD."""

import hashlib
import sqlite3
from pathlib import Path
from typing import Any

from caco.db._connection import get_connection

# =============================================================================
# Known IWAD MD5 checksums -> (short_name, display_title)
# =============================================================================

KNOWN_IWADS: dict[str, tuple[str, str]] = {
    # Ultimate Doom
    "c4fe9fd920207691a9f493668e0a2083": ("doom", "The Ultimate Doom"),
    # Doom (registered, pre-Ultimate)
    "1cd63c5ddff1bf8ce844237f580e9cf3": ("doom", "Doom (Registered)"),
    # Doom shareware
    "f0cefca49926d00903cf57551d901abe": ("doom1", "Doom (Shareware)"),
    # Doom II
    "25e1459ca71d321525f84628f45ca8cd": ("doom2", "Doom II: Hell on Earth"),
    # Doom II 1.666
    "30e3c2d0350b67bfbf47271970b74b2f": ("doom2", "Doom II: Hell on Earth"),
    # Plutonia
    "75c8cf89566741fa9d22447604053bd7": ("plutonia", "The Plutonia Experiment"),
    # Plutonia (Anthology)
    "3493be7e1e2588bc9c8b31eab2587a04": ("plutonia", "The Plutonia Experiment"),
    # TNT
    "4e158d9953c79ccf97bd0663244cc6b6": ("tnt", "TNT: Evilution"),
    # TNT (Anthology)
    "1d39e405bf6ee3df69a8d2646c8d5c49": ("tnt", "TNT: Evilution"),
    # Heretic
    "66d686b1ed6d35ff103f15dbd30e0341": ("heretic", "Heretic"),
    # Heretic shareware
    "ae779722390ec32fa37b0d361f7d82f8": ("heretic1", "Heretic (Shareware)"),
    # Hexen
    "abb033caf81e26f12a2103e1fa25453f": ("hexen", "Hexen"),
    # Hexen: Deathkings
    "78d5898e99e220e4de64edaa0e479593": ("hexdd", "Hexen: Deathkings"),
    # Strife
    "2fed2031a5b03892106e0f117f17901f": ("strife", "Strife"),
    # Chex Quest
    "25485721882b050afa96a56e5758dd52": ("chex", "Chex Quest"),
    # Chex Quest 3
    "bce163d06521f9d15f9686786e64df13": ("chex3", "Chex Quest 3"),
}

# =============================================================================
# Filename fallback for when MD5 doesn't match (modded IWADs, newer releases)
# =============================================================================

KNOWN_IWAD_FILENAMES: dict[str, tuple[str, str]] = {
    "doom2.wad": ("doom2", "Doom II: Hell on Earth"),
    "doom.wad": ("doom", "The Ultimate Doom"),
    "doomu.wad": ("doom", "The Ultimate Doom"),
    "doom1.wad": ("doom1", "Doom (Shareware)"),
    "plutonia.wad": ("plutonia", "The Plutonia Experiment"),
    "tnt.wad": ("tnt", "TNT: Evilution"),
    "heretic.wad": ("heretic", "Heretic"),
    "hexen.wad": ("hexen", "Hexen"),
    "hexdd.wad": ("hexdd", "Hexen: Deathkings"),
    "strife1.wad": ("strife", "Strife"),
    "chex.wad": ("chex", "Chex Quest"),
    "chex3.wad": ("chex3", "Chex Quest 3"),
    "freedoom2.wad": ("freedoom2", "Freedoom: Phase 2"),
    "freedoom1.wad": ("freedoom1", "Freedoom: Phase 1"),
    "hacx.wad": ("hacx", "HacX"),
}

# =============================================================================
# Alias mapping: free-text IWAD strings -> short names
# Used for normalizing wiki/forum IWAD fields to registry names.
# =============================================================================

IWAD_ALIASES: dict[str, str] = {
    # Doom II
    "doom ii": "doom2",
    "doom 2": "doom2",
    "doom2": "doom2",
    "doom ii: hell on earth": "doom2",
    "hell on earth": "doom2",
    # Ultimate Doom
    "the ultimate doom": "doom",
    "ultimate doom": "doom",
    "doom": "doom",
    "doom 1": "doom",
    # Doom shareware
    "doom (shareware)": "doom1",
    "doom shareware": "doom1",
    # Plutonia
    "plutonia": "plutonia",
    "the plutonia experiment": "plutonia",
    "plutonia experiment": "plutonia",
    # TNT
    "tnt": "tnt",
    "tnt: evilution": "tnt",
    "tnt evilution": "tnt",
    "evilution": "tnt",
    # Final Doom (maps to plutonia by convention, but can be either)
    "final doom": "doom2",
    # Heretic
    "heretic": "heretic",
    "heretic (shareware)": "heretic1",
    # Hexen
    "hexen": "hexen",
    "hexen: deathkings": "hexdd",
    "hexen deathkings": "hexdd",
    # Strife
    "strife": "strife",
    # Chex Quest
    "chex quest": "chex",
    "chex quest 3": "chex3",
    # Freedoom
    "freedoom": "freedoom2",
    "freedoom phase 1": "freedoom1",
    "freedoom phase 2": "freedoom2",
    "freedoom: phase 1": "freedoom1",
    "freedoom: phase 2": "freedoom2",
    # HacX
    "hacx": "hacx",
}


# =============================================================================
# Identification helpers
# =============================================================================


def _compute_md5(path: str | Path) -> str:
    """Compute MD5 hex digest of a file."""
    h = hashlib.md5()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(8192), b""):
            h.update(chunk)
    return h.hexdigest()


def identify_iwad(path: str | Path) -> tuple[str, str] | None:
    """Identify an IWAD file by MD5 hash, falling back to filename.

    Returns (short_name, display_title) or None if unrecognized.
    """
    path = Path(path)
    if not path.exists():
        return None

    md5 = _compute_md5(path)
    if md5 in KNOWN_IWADS:
        return KNOWN_IWADS[md5]

    filename = path.name.lower()
    if filename in KNOWN_IWAD_FILENAMES:
        return KNOWN_IWAD_FILENAMES[filename]

    return None


def normalize_iwad_name(text: str) -> str | None:
    """Normalize free text to a known IWAD short name.

    Looks up in IWAD_ALIASES (case-insensitive). Returns the short name
    or None if unrecognized.
    """
    key = text.strip().lower()
    return IWAD_ALIASES.get(key)


# =============================================================================
# Database CRUD
# =============================================================================


def add_iwad(
    name: str,
    path: str,
    *,
    title: str | None = None,
    md5: str | None = None,
) -> int:
    """Register an IWAD in the database.

    Args:
        name: Short name (e.g., "doom2")
        path: Absolute path to the .wad file
        title: Display title (e.g., "Doom II: Hell on Earth")
        md5: MD5 checksum (computed if not provided)

    Returns:
        The new IWAD's database ID.

    Raises:
        sqlite3.IntegrityError: If name is already registered.
    """
    with get_connection() as conn:
        cursor = conn.execute(
            "INSERT INTO iwads (name, path, title, md5) VALUES (?, ?, ?, ?)",
            (name, path, title, md5),
        )
        return cursor.lastrowid  # type: ignore[return-value]


def get_iwad(name: str) -> dict[str, Any] | None:
    """Get a registered IWAD by short name."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT * FROM iwads WHERE name = ?", (name,)
        ).fetchone()
        return dict(row) if row else None


def get_iwad_by_path(path: str) -> dict[str, Any] | None:
    """Get a registered IWAD by file path."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT * FROM iwads WHERE path = ?", (path,)
        ).fetchone()
        return dict(row) if row else None


def get_all_iwads() -> list[dict[str, Any]]:
    """Get all registered IWADs."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT * FROM iwads ORDER BY name"
        ).fetchall()
        return [dict(r) for r in rows]


def remove_iwad(name: str) -> bool:
    """Remove a registered IWAD by short name. Returns True if removed."""
    with get_connection() as conn:
        cursor = conn.execute("DELETE FROM iwads WHERE name = ?", (name,))
        return cursor.rowcount > 0


def resolve_iwad_from_db(name: str) -> str | None:
    """Look up a short name in the IWAD registry and return its path.

    Returns the file path if the name is registered, None otherwise.
    Gracefully returns None if the iwads table doesn't exist yet.
    """
    try:
        iwad = get_iwad(name)
        return iwad["path"] if iwad else None
    except sqlite3.OperationalError:
        # Table may not exist if init_db() hasn't run yet
        return None
