"""Auto-detect complevel (compatibility level) from WAD file contents.

Conservative heuristics — returns None when ambiguous. Inspects lumps like
UMAPINFO and DEHACKED to infer the minimum required complevel.

Detection hierarchy:
1. UMAPINFO lump present -> MBF21 (21)
2. DEHACKED with MBF codepointers -> MBF (11)
3. DEHACKED without MBF features -> None (ambiguous — could be vanilla or Boom)
4. ExMy maps only, no DEHACKED/UMAPINFO -> vanilla (2)
5. MAPxx maps without special lumps -> None (could be vanilla doom2 or Boom)
"""

import logging
import re
import zipfile
from pathlib import Path

from caco.utils import parse_wad_directory

logger = logging.getLogger(__name__)

# MBF-specific DeHackEd codepointers (indicate MBF or higher)
MBF_CODEPOINTERS = frozenset({
    "A_MUSHROOM",
    "A_SPAWN",
    "A_TURN",
    "A_FACE",
    "A_SCRATCH",
    "A_PLAYSSOUND",
    "A_RANDOMJUMP",
    "A_LINEEFFECT",
    "A_DIE",
    "A_DETONATE",
    "A_HEALCHASE",
    "A_SEEKERMISSILE",
    "A_FINDTRACER",
    "A_CLEARTARGET",
    "A_JUMPIFHEALTHBELOW",
    "A_JUMPIFFLAGSSET",
    "A_ADDFLAGS",
    "A_REMOVEFLAGS",
})

# MBF21 codepointers
MBF21_CODEPOINTERS = frozenset({
    "A_SEEKERMISSILE",
    "A_FINDTRACER",
    "A_CLEARTARGET",
    "A_JUMPIFHEALTHBELOW",
    "A_JUMPIFFLAGSSET",
    "A_ADDFLAGS",
    "A_REMOVEFLAGS",
    "A_WEAPONPROJECTILE",
    "A_WEAPONBULLETATTACK",
    "A_WEAPONMELEEATTACK",
    "A_WEAPONSOUND",
    "A_WEAPONJUMP",
    "A_CONSUMEAMMO",
    "A_CHECKAMMO",
    "A_REFIRETO",
    "A_GUNFLASHTO",
    "A_WEAPONALERT",
    "A_NOISEALERT",
    "A_HEALCHASE",
    "A_SPAWNOBJECT",
    "A_MONSTERPROJECTILE",
    "A_MONSTERMELEEATTACK",
    "A_MONSTERBULLETATTACK",
    "A_RADIUSDAMAGE",
})


def detect_complevel(wad_path: str | Path) -> int | None:
    """Detect complevel from WAD file contents.

    Returns complevel int if confidently detected, or None if ambiguous.
    """
    wad_path = Path(wad_path)
    if not wad_path.exists():
        return None

    try:
        wad_data = _read_wad_data(wad_path)
        if wad_data is None:
            return None

        directory = parse_wad_directory(wad_data)
        lump_names = {name for name, _off, _sz in directory}

        # Check for UMAPINFO -> MBF21
        if "UMAPINFO" in lump_names:
            logger.info("Detected UMAPINFO lump -> complevel 21 (MBF21)")
            return 21

        # Check DEHACKED lump for MBF codepointers
        if "DEHACKED" in lump_names:
            deh_text = _read_lump_text(wad_data, directory, "DEHACKED")
            if deh_text is not None:
                cl = _detect_from_dehacked(deh_text)
                if cl is not None:
                    return cl
                # DEHACKED present but no MBF pointers — ambiguous
                return None

        # No DEHACKED, no UMAPINFO — check map lump names
        has_exmy = any(re.match(r"^E\dM\d$", name) for name in lump_names)
        has_mapxx = any(re.match(r"^MAP\d\d$", name) for name in lump_names)

        if has_exmy and not has_mapxx:
            # ExMy-only maps without special lumps -> vanilla Doom
            logger.info("ExMy maps only, no DEHACKED/UMAPINFO -> complevel 2 (Vanilla)")
            return 2

        # MAPxx or mixed — ambiguous (could be vanilla doom2 or boom)
        return None

    except Exception as e:
        logger.debug("Failed to detect complevel from %s: %s", wad_path, e)
        return None


def _read_wad_data(wad_path: Path) -> bytes | None:
    """Read WAD data, handling ZIP-wrapped WADs."""
    if wad_path.suffix.lower() == ".zip":
        try:
            with zipfile.ZipFile(wad_path) as zf:
                for name in zf.namelist():
                    if name.lower().endswith(".wad"):
                        return zf.read(name)
        except (zipfile.BadZipFile, KeyError):
            return None
        return None

    data = wad_path.read_bytes()
    if len(data) < 12:
        return None

    magic = data[:4]
    if magic in (b"IWAD", b"PWAD"):
        return data
    return None


def _read_lump_text(
    wad_data: bytes,
    directory: list[tuple[str, int, int]],
    lump_name: str,
) -> str | None:
    """Read a text lump from WAD data."""
    for name, offset, size in directory:
        if name == lump_name and size > 0:
            try:
                return wad_data[offset:offset + size].decode("ascii", errors="replace")
            except Exception:
                return None
    return None


def _detect_from_dehacked(deh_text: str) -> int | None:
    """Detect complevel from DEHACKED lump contents.

    Checks for MBF21 codepointers first, then MBF codepointers.
    Returns None if no MBF features found (ambiguous).
    """
    upper = deh_text.upper()

    # Check for MBF21 codepointers
    for cp in MBF21_CODEPOINTERS:
        if cp in upper:
            logger.info("Detected MBF21 codepointer %s -> complevel 21", cp)
            return 21

    # Check for MBF codepointers
    for cp in MBF_CODEPOINTERS:
        if cp in upper:
            logger.info("Detected MBF codepointer %s -> complevel 11", cp)
            return 11

    # DEHACKED present but no MBF-specific features — ambiguous
    return None
