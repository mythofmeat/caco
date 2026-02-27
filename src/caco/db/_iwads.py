"""IWAD registry: known IWADs, identification, and database CRUD.

IWADs are organized by **family** (doom, doom2, plutonia, tnt, ...) with
multiple **variants** per family (v1.9, bfg, enhanced, kex, ...).  Resolution
uses a configurable priority list to pick the preferred variant.
"""

import sqlite3
from pathlib import Path
from typing import Any

from caco.db._connection import get_connection
from caco.utils import compute_md5

# =============================================================================
# Known IWAD MD5 checksums -> (family, variant, display_title)
# =============================================================================

KNOWN_IWADS: dict[str, tuple[str, str, str]] = {
    # ── doom family ──────────────────────────────────────────────────────
    "1cd63c5ddff1bf8ce844237f580e9cf3": ("doom", "v1.9", "Doom (Registered)"),
    "c4fe9fd920207691a9f493668e0a2083": ("doom", "v1.9ud", "The Ultimate Doom"),
    "fb35c4a5a9fd49ec29ab6e900572c524": ("doom", "bfg", "The Ultimate Doom (BFG Edition)"),
    "8517c4e8f0eef90b82852667d345eb86": ("doom", "enhanced", "The Ultimate Doom (Enhanced)"),
    "4461d4511386518e784c647e3128e7bc": ("doom", "kex", "The Ultimate Doom (KEX)"),
    "3b37188f6337f15718b617c16e6e7a9c": ("doom", "kex", "The Ultimate Doom (KEX)"),
    # ── doom1 (shareware) ───────────────────────────────────────────────
    "f0cefca49926d00903cf57551d901abe": ("doom1", "v1.0", "Doom (Shareware)"),
    # ── doom2 family ─────────────────────────────────────────────────────
    "25e1459ca71d321525f84628f45ca8cd": ("doom2", "v1.9", "Doom II: Hell on Earth"),
    "c3bea40570c23e511a7ed3ebcd9865f7": ("doom2", "bfg", "Doom II: Hell on Earth (BFG Edition)"),
    "8ab6d0527a29efdc1ef200e5687b5cae": ("doom2", "enhanced", "Doom II: Hell on Earth (Enhanced)"),
    "9aa3cbf65b961d0bdac98ec403b832e1": ("doom2", "kex", "Doom II: Hell on Earth (KEX)"),
    "64a4c88a871da67492aaa2020a068cd8": ("doom2", "kex", "Doom II: Hell on Earth (KEX)"),
    # ── plutonia family ──────────────────────────────────────────────────
    "75c8cf89566741fa9d22447604053bd7": ("plutonia", "v1.9", "The Plutonia Experiment"),
    "3493be7e1e2588bc9c8b31eab2587a04": ("plutonia", "v1.9alt", "The Plutonia Experiment"),
    "0b381ff7bae93bde6496f9547463619d": ("plutonia", "unity", "The Plutonia Experiment (Unity)"),
    "ae76c20366ff685d3bb9fab11b148b84": ("plutonia", "unity", "The Plutonia Experiment (Unity)"),
    "24037397056e919961005e08611623f4": ("plutonia", "kex", "The Plutonia Experiment (KEX)"),
    "e47cf6d82a0ccedf8c1c16a284bb5937": ("plutonia", "kex", "The Plutonia Experiment (KEX)"),
    # ── tnt family ───────────────────────────────────────────────────────
    "4e158d9953c79ccf97bd0663244cc6b6": ("tnt", "v1.9", "TNT: Evilution"),
    "1d39e405bf6ee3df69a8d2646c8d5c49": ("tnt", "v1.9alt", "TNT: Evilution"),
    "a6685de59ddf2c07f45deeec95296d98": ("tnt", "unity", "TNT: Evilution (Unity)"),
    "f5528f6fd55cf9629141d79eda169630": ("tnt", "unity", "TNT: Evilution (Unity)"),
    "8974e3117ed4a1839c752d5e11ab1b7b": ("tnt", "kex", "TNT: Evilution (KEX)"),
    "ad7885c17a6b9b79b09d7a7634dd7e2c": ("tnt", "kex", "TNT: Evilution (KEX)"),
    # ── other families ───────────────────────────────────────────────────
    "66d686b1ed6d35ff103f15dbd30e0341": ("heretic", "v1.3", "Heretic"),
    "ae779722390ec32fa37b0d361f7d82f8": ("heretic1", "v1.0", "Heretic (Shareware)"),
    "abb033caf81e26f12a2103e1fa25453f": ("hexen", "v1.1", "Hexen"),
    "78d5898e99e220e4de64edaa0e479593": ("hexdd", "v1.0", "Hexen: Deathkings"),
    "2fed2031a5b03892106e0f117f17901f": ("strife", "v1.2", "Strife"),
    "25485721882b050afa96a56e5758dd52": ("chex", "v1.0", "Chex Quest"),
    "bce163d06521f9d15f9686786e64df13": ("chex3", "v1.0", "Chex Quest 3"),
}

# =============================================================================
# Filename fallback for when MD5 doesn't match (modded IWADs, newer releases)
# Variant is "unknown" since MD5 didn't identify the specific version.
# =============================================================================

KNOWN_IWAD_FILENAMES: dict[str, tuple[str, str, str]] = {
    "doom2.wad": ("doom2", "unknown", "Doom II: Hell on Earth"),
    "doom.wad": ("doom", "unknown", "The Ultimate Doom"),
    "doomu.wad": ("doom", "unknown", "The Ultimate Doom"),
    "doom1.wad": ("doom1", "unknown", "Doom (Shareware)"),
    "plutonia.wad": ("plutonia", "unknown", "The Plutonia Experiment"),
    "tnt.wad": ("tnt", "unknown", "TNT: Evilution"),
    "heretic.wad": ("heretic", "unknown", "Heretic"),
    "hexen.wad": ("hexen", "unknown", "Hexen"),
    "hexdd.wad": ("hexdd", "unknown", "Hexen: Deathkings"),
    "strife1.wad": ("strife", "unknown", "Strife"),
    "chex.wad": ("chex", "unknown", "Chex Quest"),
    "chex3.wad": ("chex3", "unknown", "Chex Quest 3"),
    "freedoom2.wad": ("freedoom2", "unknown", "Freedoom: Phase 2"),
    "freedoom1.wad": ("freedoom1", "unknown", "Freedoom: Phase 1"),
    "hacx.wad": ("hacx", "unknown", "HacX"),
}

# =============================================================================
# Alias mapping: free-text IWAD strings -> family names
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
    # Final Doom (maps to doom2 by convention)
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
# Variant priority: preferred variant order per family.
# First match in the registered set wins.
# =============================================================================

DEFAULT_IWAD_PRIORITY: dict[str, list[str]] = {
    "doom": ["v1.9ud", "v1.9", "bfg", "enhanced", "kex"],
    "doom1": ["v1.0"],
    "doom2": ["v1.9", "bfg", "enhanced", "kex"],
    "plutonia": ["v1.9", "v1.9alt", "unity", "kex"],
    "tnt": ["v1.9", "v1.9alt", "unity", "kex"],
    # single-variant families
    "freedoom1": ["latest"],
    "freedoom2": ["latest"],
    "heretic": ["v1.3"],
    "heretic1": ["v1.0"],
    "hexen": ["v1.1"],
    "hexdd": ["v1.0"],
    "strife": ["v1.2"],
    "chex": ["v1.0"],
    "chex3": ["v1.0"],
}

# =============================================================================
# Cross-family fallbacks (freedoom as last resort)
# =============================================================================

FAMILY_FALLBACKS: dict[str, list[str]] = {
    "doom": ["freedoom1"],
    "doom2": ["freedoom2"],
    "plutonia": ["freedoom2"],
    "tnt": ["freedoom2"],
}


# =============================================================================
# Identification helpers
# =============================================================================


def identify_iwad(path: str | Path) -> tuple[str, str, str] | None:
    """Identify an IWAD file by MD5 hash, falling back to filename.

    Returns (family, variant, display_title) or None if unrecognized.
    """
    path = Path(path)
    if not path.exists():
        return None

    md5 = compute_md5(path)
    if md5 in KNOWN_IWADS:
        return KNOWN_IWADS[md5]

    filename = path.name.lower()
    if filename in KNOWN_IWAD_FILENAMES:
        return KNOWN_IWAD_FILENAMES[filename]

    return None


def normalize_iwad_name(text: str) -> str | None:
    """Normalize free text to a known IWAD family name.

    Looks up in IWAD_ALIASES (case-insensitive). Returns the family name
    or None if unrecognized.
    """
    key = text.strip().lower()
    return IWAD_ALIASES.get(key)


# =============================================================================
# Priority resolution
# =============================================================================


def get_iwad_priority(family: str) -> list[str]:
    """Get variant priority list for a family.

    Checks config ``[iwad_priority]`` section first, then falls back to
    ``DEFAULT_IWAD_PRIORITY``.  Returns an empty list for unknown families.
    """
    from caco.config import load_config

    config = load_config()
    user_priority = config.get("iwad_priority")
    if isinstance(user_priority, dict) and family in user_priority:
        val = user_priority[family]
        if isinstance(val, list):
            return val

    return DEFAULT_IWAD_PRIORITY.get(family, [])


# =============================================================================
# Database CRUD
# =============================================================================


def add_iwad(
    family: str,
    variant: str,
    path: str,
    *,
    title: str | None = None,
    md5: str | None = None,
) -> int:
    """Register an IWAD variant in the database.

    Args:
        family: IWAD family (e.g., "doom2")
        variant: Variant identifier (e.g., "v1.9", "bfg")
        path: Absolute path to the .wad file
        title: Display title (e.g., "Doom II: Hell on Earth")
        md5: MD5 checksum (computed if not provided)

    Returns:
        The new IWAD's database ID.

    Raises:
        sqlite3.IntegrityError: If (family, variant) is already registered.
    """
    with get_connection() as conn:
        cursor = conn.execute(
            "INSERT INTO iwads (family, variant, path, title, md5) VALUES (?, ?, ?, ?, ?)",
            (family, variant, path, title, md5),
        )
        return cursor.lastrowid  # type: ignore[return-value]


def get_iwad(family: str) -> dict[str, Any] | None:
    """Get the preferred variant of an IWAD family.

    Walks the priority list (config override → ``DEFAULT_IWAD_PRIORITY``)
    and returns the first registered variant.  If no priority-listed variant
    is found, falls back to any registered variant of that family, then
    tries cross-family fallbacks (e.g., freedoom).
    """
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT * FROM iwads WHERE family = ?", (family,)
        ).fetchall()

    if not rows:
        # Try cross-family fallback
        for fallback_family in FAMILY_FALLBACKS.get(family, []):
            result = get_iwad(fallback_family)
            if result:
                return result
        return None

    variants = {r["variant"]: dict(r) for r in rows}

    # Walk priority list
    for v in get_iwad_priority(family):
        if v in variants:
            return variants[v]

    # Fallback: "unknown" variant (filename-detected) or first registered
    if "unknown" in variants:
        return variants["unknown"]
    return dict(rows[0])


def get_iwad_variant(family: str, variant: str) -> dict[str, Any] | None:
    """Get a specific IWAD variant."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT * FROM iwads WHERE family = ? AND variant = ?",
            (family, variant),
        ).fetchone()
        return dict(row) if row else None


def get_family_iwads(family: str) -> list[dict[str, Any]]:
    """Get all variants of an IWAD family, sorted by priority."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT * FROM iwads WHERE family = ?", (family,)
        ).fetchall()

    if not rows:
        return []

    # Sort by priority order
    priority = get_iwad_priority(family)
    priority_map = {v: i for i, v in enumerate(priority)}

    def sort_key(r: dict) -> int:
        return priority_map.get(r["variant"], len(priority))

    result = [dict(r) for r in rows]
    result.sort(key=sort_key)
    return result


def get_iwad_by_path(path: str) -> dict[str, Any] | None:
    """Get a registered IWAD by file path."""
    with get_connection() as conn:
        row = conn.execute(
            "SELECT * FROM iwads WHERE path = ?", (path,)
        ).fetchone()
        return dict(row) if row else None


def get_all_iwads() -> list[dict[str, Any]]:
    """Get all registered IWADs, ordered by family then variant."""
    with get_connection() as conn:
        rows = conn.execute(
            "SELECT * FROM iwads ORDER BY family, variant"
        ).fetchall()
        return [dict(r) for r in rows]


def remove_iwad(family: str, variant: str | None = None) -> int:
    """Remove registered IWAD(s).

    Args:
        family: IWAD family to remove.
        variant: If given, removes only that variant. If None, removes all
                 variants of the family.

    Returns:
        Number of rows removed.
    """
    with get_connection() as conn:
        if variant:
            cursor = conn.execute(
                "DELETE FROM iwads WHERE family = ? AND variant = ?",
                (family, variant),
            )
        else:
            cursor = conn.execute(
                "DELETE FROM iwads WHERE family = ?", (family,)
            )
        return cursor.rowcount


def managed_iwad_filename(family: str, variant: str) -> str:
    """Return the canonical path for a managed IWAD: {variant}/{family}.wad.

    This gives sourceports the canonical filename (e.g., ``tnt.wad``) while
    keeping variants separated in subdirectories.
    """
    return f"{variant}/{family}.wad"


def remove_iwad_with_paths(family: str, variant: str | None = None) -> list[str]:
    """Remove registered IWAD(s) and return the paths of removed entries.

    This avoids TOCTOU races vs. a separate fetch-then-delete pattern.

    Args:
        family: IWAD family to remove.
        variant: If given, removes only that variant. If None, removes all
                 variants of the family.

    Returns:
        List of file paths from the removed rows.
    """
    with get_connection() as conn:
        if variant:
            rows = conn.execute(
                "SELECT path FROM iwads WHERE family = ? AND variant = ?",
                (family, variant),
            ).fetchall()
            conn.execute(
                "DELETE FROM iwads WHERE family = ? AND variant = ?",
                (family, variant),
            )
        else:
            rows = conn.execute(
                "SELECT path FROM iwads WHERE family = ?", (family,)
            ).fetchall()
            conn.execute(
                "DELETE FROM iwads WHERE family = ?", (family,)
            )
        return [r["path"] for r in rows]


def resolve_iwad_from_db(name: str) -> str | None:
    """Look up a family name in the IWAD registry and return its path.

    Returns the file path of the preferred variant if the family is
    registered, None otherwise.
    Gracefully returns None if the iwads table doesn't exist yet.
    """
    try:
        iwad = get_iwad(name)
        return iwad["path"] if iwad else None
    except sqlite3.OperationalError:
        # Table may not exist if init_db() hasn't run yet
        return None
