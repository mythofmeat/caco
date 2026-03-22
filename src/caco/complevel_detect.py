"""Auto-detect complevel (compatibility level) from WAD file contents.

Inspects lumps like COMPLVL, UMAPINFO, DEHACKED, ANIMATED/SWITCHES, and
LINEDEFS to infer the minimum required complevel.

Detection hierarchy:
1. COMPLVL lump (id24 signal) -> byte value directly
2. UMAPINFO lump present -> MBF21 (21)
3. DEHACKED with MBF21 features -> MBF21 (21)
4. DEHACKED with MBF codepointers -> MBF (11)
5. ANIMATED or SWITCHES lumps -> Boom (9)
6. Boom-range linedef types (> 141) in LINEDEFS -> Boom (9)
7. Has map lumps (ExMy or MAPxx) without advanced features -> vanilla (2)
8. No map lumps -> None (resource WAD, can't determine)
"""

import logging
import re
from pathlib import Path

from caco.iwad_detect import _load_wad_data
from caco.utils import parse_wad_directory

logger = logging.getLogger(__name__)

# Highest vanilla Doom linedef type
_MAX_VANILLA_LINEDEF_TYPE = 141

# MBF-specific DeHackEd codepointers (indicate MBF, complevel 11)
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
    "A_SEEKTRACER",
    "A_FINDTRACER",
    "A_CLEARTARGET",
    "A_CLEARTRACER",
})

# MBF21-exclusive codepointers (not in original MBF)
MBF21_CODEPOINTERS = frozenset({
    "A_SPAWNOBJECT",
    "A_MONSTERPROJECTILE",
    "A_MONSTERMELEEATTACK",
    "A_MONSTERBULLETATTACK",
    "A_RADIUSDAMAGE",
    "A_NOISEALERT",
    "A_JUMPIFHEALTHBELOW",
    "A_JUMPIFFLAGSSET",
    "A_JUMPIFTARGETINLOS",
    "A_JUMPIFTARGETCLOSER",
    "A_JUMPIFTRACERINLOS",
    "A_JUMPIFTRACERCLOSER",
    "A_ADDFLAGS",
    "A_REMOVEFLAGS",
    "A_WEAPONPROJECTILE",
    "A_WEAPONBULLETATTACK",
    "A_WEAPONMELEEATTACK",
    "A_WEAPONSOUND",
    "A_WEAPONJUMP",
    "A_WEAPONALERT",
    "A_CONSUMEAMMO",
    "A_CHECKAMMO",
    "A_REFIRETO",
    "A_GUNFLASHTO",
})


def detect_complevel(wad_path: str | Path) -> int | None:
    """Detect complevel from WAD file contents.

    Returns complevel int if confidently detected, or None if no map lumps.
    Checks COMPLVL lump first (id24 signal), then falls back to heuristics.
    """
    wad_path = Path(wad_path)
    if not wad_path.exists():
        return None

    try:
        wad_data = _load_wad_data(wad_path)
        if wad_data is None:
            return None

        directory = parse_wad_directory(wad_data)
        lump_names = {name for name, _off, _sz in directory}

        # Check for COMPLVL lump — id24 uses a single byte, but some WADs
        # use a text string (e.g. "mbf21", "vanilla")
        for name, offset, size in directory:
            if name == "COMPLVL" and size >= 1:
                cl = _parse_complvl_lump(wad_data, offset, size)
                if cl is not None:
                    logger.info("Detected COMPLVL lump -> complevel %d", cl)
                    return cl

        # Check for UMAPINFO -> MBF21
        if "UMAPINFO" in lump_names:
            logger.info("Detected UMAPINFO lump -> complevel 21 (MBF21)")
            return 21

        # Check DEHACKED lump for MBF/MBF21 features
        if "DEHACKED" in lump_names:
            deh_text = _read_lump_text(wad_data, directory, "DEHACKED")
            if deh_text is not None:
                cl = _detect_from_dehacked(deh_text)
                if cl is not None:
                    return cl
                # DEHACKED present but no MBF features — fall through to
                # Boom checks (vanilla DEHACKED is compatible with all levels)

        # Check for Boom-specific lumps
        if lump_names & {"ANIMATED", "SWITCHES"}:
            logger.info("Detected Boom lump (ANIMATED/SWITCHES) -> complevel 9")
            return 9

        # Check LINEDEFS for Boom-range linedef types (> 141)
        if _has_boom_linedefs(wad_data, directory):
            logger.info("Detected Boom linedef types -> complevel 9")
            return 9

        # Check for map lumps
        has_exmy = any(re.match(r"^E\dM\d$", name) for name in lump_names)
        has_mapxx = any(re.match(r"^MAP\d\d$", name) for name in lump_names)

        if has_exmy or has_mapxx:
            # Maps present, no advanced features -> vanilla
            logger.info("No advanced features detected -> complevel 2 (Vanilla)")
            return 2

        # No map lumps — resource WAD, can't determine complevel
        return None

    except Exception as e:
        logger.debug("Failed to detect complevel from %s: %s", wad_path, e)
        return None


def _parse_complvl_lump(wad_data: bytes, offset: int, size: int) -> int | None:
    """Parse a COMPLVL lump, handling both id24 (1-byte) and text formats.

    id24 spec: single byte where the byte value IS the complevel.
    Some WADs use a text string instead (e.g. "mbf21", "vanilla").
    """
    try:
        raw = wad_data[offset:offset + size]
    except IndexError:
        return None

    if size == 1:
        # id24 format: single byte = complevel number
        return raw[0]

    # Text format: try to decode as a complevel name/number
    try:
        text = raw.rstrip(b"\x00").decode("ascii", errors="replace").strip()
    except Exception:
        return None

    if not text:
        return None

    from caco.complevel import parse_complevel
    return parse_complevel(text)


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

    Checks Doom version header, MBF21 fields/codepointers, then MBF
    codepointers. Returns None if no MBF features found (vanilla DEHACKED).
    """
    upper = deh_text.upper()

    # Check Doom version field — 2021 is the definitive MBF21 signal
    version_match = re.search(r"DOOM\s+VERSION\s*=\s*(\d+)", upper)
    if version_match:
        version = int(version_match.group(1))
        if version == 2021:
            logger.info("DEHACKED Doom version 2021 -> complevel 21 (MBF21)")
            return 21

    # Check for MBF21-specific DEHACKED fields (thing/weapon/frame flags)
    if re.search(r"MBF21\s+BITS", upper):
        logger.info("Detected MBF21 Bits field -> complevel 21")
        return 21

    # Check for MBF21-exclusive codepointers
    for cp in MBF21_CODEPOINTERS:
        if cp in upper:
            logger.info("Detected MBF21 codepointer %s -> complevel 21", cp)
            return 21

    # Check for MBF codepointers
    for cp in MBF_CODEPOINTERS:
        if cp in upper:
            logger.info("Detected MBF codepointer %s -> complevel 11", cp)
            return 11

    # DEHACKED present but no MBF-specific features — vanilla-compatible
    return None


def _has_boom_linedefs(
    wad_data: bytes,
    directory: list[tuple[str, int, int]],
) -> bool:
    """Check LINEDEFS lumps for Boom-range linedef types (> 141).

    Scans each LINEDEFS lump (14 bytes per entry in Doom format) and checks
    the special/type field for values above the vanilla Doom maximum.
    """
    for name, offset, size in directory:
        if name != "LINEDEFS" or size == 0:
            continue
        # Doom format: 14 bytes per linedef; skip if size doesn't align
        if size % 14 != 0:
            continue
        num_linedefs = size // 14
        for i in range(num_linedefs):
            # special field is at offset 6 within each 14-byte entry
            ld_offset = offset + i * 14 + 6
            special = int.from_bytes(
                wad_data[ld_offset:ld_offset + 2], "little"
            )
            if special > _MAX_VANILLA_LINEDEF_TYPE:
                return True
    return False
