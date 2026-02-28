"""Auto-detect required IWAD family from WAD file contents.

Inspects a PWAD's PNAMES lump and map lump names to determine which
IWAD (base game) it requires. Detection is unambiguous: TNT-only and
Plutonia-only patch sets are disjoint.

Priority order:
1. PNAMES analysis — patches unique to TNT or Plutonia (strongest signal)
2. Map name format — ExMy (doom) vs MAPxx (doom2)
3. None if no signal found
"""

import logging
import re
import struct
import zipfile
from pathlib import Path

from caco.utils import parse_wad_directory

logger = logging.getLogger(__name__)

# Maximum size for a WAD inside a ZIP (256 MB)
_MAX_ZIP_ENTRY_SIZE = 256 * 1024 * 1024

# Map lump name patterns
_DOOM1_MAP_RE = re.compile(r"^E[1-9]M[0-9]$")
_DOOM2_MAP_RE = re.compile(r"^MAP[0-9][0-9]$")

# fmt: off
# 197 patch names present in TNT.WAD but not in DOOM2.WAD
TNT_ONLY_PATCHES: frozenset[str] = frozenset({
    "ALTAQUA", "ASPHALT", "BCRAT16", "BCRAT32", "BIGMURAL", "BIGWALL",
    "BLOD128A", "BLOD128B", "BLOD64A", "BLOD64B", "BLUTNT", "BRNOPEN",
    "BTNTCRAT", "BUL64A", "BUL64B", "BUL64C", "BUL64D", "CARLLF1",
    "CARLLF2", "CARLRT1", "CARLRT2", "CAVERN1", "CAVERN4", "CAVERN5",
    "CAVERN6", "CAVERN7", "CLWDVS3", "CRLWDH6", "CRLWDH6B", "CRLWDL12",
    "CRLWDL6", "CRLWDL6B", "CRLWDL6C", "CRLWDL6D", "CRLWDL6E", "CRLWDS6",
    "CRLWDT3", "CRWDH6", "CRWDH6B", "CRWDL12", "CRWDL6B", "CRWDL6C",
    "CRWDL6D", "CRWDS6", "CRWDT3", "CRWDVS3", "CYAN", "DISASTER",
    "DOBIGTVA", "DOBIGTVB", "DOBIGTVC", "DOBIGTVD", "DOBLIP1A", "DOBLIP2A",
    "DOBLIP3A", "DOBLIP4A", "DOBWIRE", "DOBWIRE2", "DOEDAY", "DOEHELL",
    "DOENITE", "DOGLDIR", "DOGLPANL", "DOGRID", "DOGRMSC", "DOGRNMEN",
    "DOKGRIR", "DOKODO1B", "DOKODO2B", "DONDAY", "DONHELL", "DONNITE",
    "DOPUNK4", "DORED", "DOSDAY", "DOSHA1", "DOSHB1", "DOSHC1", "DOSHD1",
    "DOSHE1", "DOSHELL", "DOSHF1", "DOSLVR11", "DOSLVR12", "DOSLVR13",
    "DOSLVR14", "DOSLVR21", "DOSLVR22", "DOSLVR23", "DOSLVR24", "DOSNITE",
    "DOSPI1B", "DOSPI2B", "DOSPI3B", "DOSPI4B", "DOSW1", "DOSW1C", "DOSW2",
    "DOSW2C", "DOSW3", "DOSW3C", "DOSW4", "DOSW4C", "DOSWX1", "DOSWX1C",
    "DOSWX2", "DOSWX2C", "DOSWX3", "DOSWX3C", "DOSWX4", "DOSWX4C",
    "DOTNTDR", "DOTV1B", "DOTV2B", "DOTV3B", "DOTV4B", "DOWDAY", "DOWEBL",
    "DOWEBR", "DOWHELL", "DOWINDOW", "DOWNITE", "DRFRONT", "DRSIDE1",
    "DRSIDE2", "DRTOPFR", "DRTOPSID", "EGGREENI", "EGREDI", "FENCE4",
    "FENCE5", "GRNLIT1", "GRNOPEN", "LONGWALL", "MTNT2", "MURAL1", "MURAL2",
    "PBLAK", "PCWINL", "PILLAR", "PIVY3", "PL_01", "PL_05", "PL_10",
    "PL_18", "PL_19", "PL_20", "PL_25", "PL_31", "PL_A", "PL_C", "PL_N",
    "PL_T", "PL_U", "PREEL1", "PREEL2", "PREEL3", "PREEL4", "PREEL5",
    "PREEL6", "PREEL7", "PSTON2", "REDLITE1", "REDLITE2", "REDOPEN",
    "REDTNT2", "ROMERO1", "SAW1", "SAW1SD", "SAW2", "SAW2SD", "SAW3",
    "SAW3SD", "SAW4", "SAW4SD", "SAW5", "SAW5SD", "SAW6", "SAW6SD",
    "SKIRTING", "SMCRATG", "SMFILLER", "STONEW1", "STONEW5", "STWALL",
    "TFOGF0", "TFOGI0", "TYUNDER1", "TYWFALL1", "TYWFALL2", "TYWFALL3",
    "TYWFALL4", "TYWHEEL1", "YELLITE1", "YELLITE2", "YELLITE3", "YELTNT",
})

# 78 patch names present in Plutonia but not in DOOM2.WAD or TNT.WAD
PLUTONIA_ONLY_PATCHES: frozenset[str] = frozenset({
    "AROCK2", "AROCK3", "AROCK4", "AROCK5", "BOSFA0", "BRBRICK",
    "BRBRICK2", "BRICK", "BRICK1", "BRICK2", "BROCK2", "BROWN1", "BROWN2",
    "BROWN3", "BROWN5", "CAMO1", "CAMO4", "CAMO5", "CONCRETE", "DARKROCK",
    "DIRBRI1", "DIRBRI2", "FIREBLU1", "FIREBLU2", "GRATE", "HELL6_1",
    "HELL8_3", "MARBLE1", "MC1", "MC10", "MC11", "MC12", "MC13", "MC14",
    "MC15", "MC16", "MC17", "MC18", "MC19", "MC2", "MC20", "MC3", "MC4",
    "MC5", "MC6", "MC7", "MC8", "MOSROK2", "MOSSBRIK", "MOSSROCK", "MOULD",
    "MUD", "MYWOOD", "NATROCK", "POISON", "RAILING", "REDROCK", "ROCK",
    "SKY1", "SKY2A", "SKY2B", "SKY2C", "SKY2D", "SKY3A", "SKY3B",
    "SW1SKULL", "SW2SKULL", "TILE", "VINES1", "W109_1", "W109_2", "W110_1",
    "WFALL1", "WFALL2", "WFALL3", "WFALL4", "WOOD", "YELLOW",
})
# fmt: on


def _load_wad_data(wad_path: str | Path) -> bytes | None:
    """Load WAD data from a file, handling ZIP-wrapped WADs.

    Returns the raw WAD bytes, or None if the file can't be read.
    """
    path = Path(wad_path)
    if not path.exists():
        return None

    wad_data: bytes | None = None

    # Handle ZIP-wrapped WADs
    if path.suffix.lower() == ".zip" or path.suffix.lower() not in (".wad", ".pk3", ".pk7"):
        try:
            with zipfile.ZipFile(path) as zf:
                for info in zf.infolist():
                    if info.filename.lower().endswith(".wad"):
                        if info.file_size > _MAX_ZIP_ENTRY_SIZE:
                            break
                        wad_data = zf.read(info)
                        break
        except (zipfile.BadZipFile, KeyError):
            pass

    if wad_data is None:
        try:
            wad_data = path.read_bytes()
        except OSError:
            return None

    return wad_data


def detect_iwad(wad_path: str | Path) -> str | None:
    """Detect the required IWAD family for a PWAD file.

    Returns an IWAD family name ("doom", "doom2", "tnt", "plutonia")
    or None if detection is inconclusive.
    """
    wad_data = _load_wad_data(wad_path)
    if wad_data is None:
        return None

    try:
        directory = parse_wad_directory(wad_data)
    except Exception:
        return None

    if not directory:
        return None

    lump_names = _get_lump_names(directory)

    # Priority 1: PNAMES analysis (strongest signal for TNT/Plutonia vs Doom 2)
    pnames = _parse_pnames(wad_data, directory)
    if pnames:
        result = _detect_from_pnames(pnames, lump_names)
        if result:
            return result

    # Priority 2: Map name format (Doom 1 vs Doom 2 family)
    return _detect_from_maps(lump_names)


def detect_complvl(wad_path: str | Path) -> int | None:
    """Detect complevel from a WAD file's COMPLVL lump (id24 only).

    The id24 COMPLVL lump is exactly 1 byte — the byte value IS the
    compatibility level. Text-based COMPLVL lumps (e.g. "mbf21") are
    NOT id24 signals and are ignored here.

    Returns the complevel as an integer, or None if no id24 COMPLVL lump found.
    """
    wad_data = _load_wad_data(wad_path)
    if wad_data is None:
        return None

    try:
        directory = parse_wad_directory(wad_data)
    except Exception:
        return None

    if not directory:
        return None

    for name, offset, size in directory:
        if name == "COMPLVL" and size == 1:
            try:
                return wad_data[offset]
            except IndexError:
                return None

    return None


def _parse_pnames(wad_data: bytes, directory: list[tuple[str, int, int]]) -> set[str] | None:
    """Extract patch names from the PNAMES lump."""
    for name, offset, size in directory:
        if name == "PNAMES" and size >= 4:
            try:
                count = struct.unpack_from("<i", wad_data, offset)[0]
                if count < 0 or 4 + count * 8 > size:
                    return None
                patches = set()
                for i in range(count):
                    pname_offset = offset + 4 + i * 8
                    pname = wad_data[pname_offset:pname_offset + 8]
                    pname = pname.split(b"\x00")[0].decode("ascii", errors="replace").upper()
                    patches.add(pname)
                return patches
            except (struct.error, IndexError):
                return None
    return None


def _get_lump_names(directory: list[tuple[str, int, int]]) -> set[str]:
    """Get all lump names from the WAD directory."""
    return {name for name, _, _ in directory}


def _detect_from_pnames(pnames: set[str], lump_names: set[str]) -> str | None:
    """Check if PNAMES references TNT-only or Plutonia-only patches.

    Only counts patches that are NOT also present as lumps within the PWAD
    itself — a self-contained WAD that includes the patches doesn't need
    the specific IWAD.
    """
    # Patches referenced in PNAMES but not provided as lumps in the WAD
    needed_tnt = pnames & TNT_ONLY_PATCHES - lump_names
    needed_plutonia = pnames & PLUTONIA_ONLY_PATCHES - lump_names

    if needed_tnt and not needed_plutonia:
        return "tnt"
    if needed_plutonia and not needed_tnt:
        return "plutonia"

    return None


def _detect_from_maps(lump_names: set[str]) -> str | None:
    """Detect IWAD family from map lump naming convention."""
    has_doom1 = any(_DOOM1_MAP_RE.match(name) for name in lump_names)
    has_doom2 = any(_DOOM2_MAP_RE.match(name) for name in lump_names)

    if has_doom1 and not has_doom2:
        return "doom"
    if has_doom2 and not has_doom1:
        return "doom2"

    return None
