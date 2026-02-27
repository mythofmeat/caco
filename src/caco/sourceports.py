"""Sourceport family registry — maps executables to CLI flags for data/save redirection."""

from pathlib import Path

# Save file extensions by sourceport family
SAVE_EXTENSIONS: dict[str, set[str]] = {
    "dsda": {".dsg"},
    "zdoom": {".zds"},
    "chocolate": {".dsg"},
    "woof": {".dsg"},
    "eternity": {".dsg"},
}
ALL_SAVE_EXTENSIONS: frozenset[str] = frozenset(
    ext for exts in SAVE_EXTENSIONS.values() for ext in exts
)

SOURCEPORT_FAMILIES: dict[str, dict] = {
    "dsda": {
        "executables": [
            "dsda-doom",
            "nyan-doom",
            "nugget-doom",
            "prboom+",
            "prboom-plus",
            "glboom+",
            "glboom-plus",
        ],
        "data_arg": "-data",
        "save_arg": "-save",
    },
    "zdoom": {
        "executables": ["gzdoom", "lzdoom", "vkdoom", "qzdoom", "zdoom"],
        "save_arg": "-savedir",
    },
    "chocolate": {
        "executables": ["chocolate-doom", "crispy-doom"],
        "save_arg": "-savedir",
    },
    "woof": {
        "executables": ["woof"],
        "data_arg": "-data",
        "save_arg": "-save",
    },
    "eternity": {
        "executables": ["eternity"],
        "save_arg": "-savedir",
    },
}

# Build reverse lookups: executable name -> family dict / family name
_EXECUTABLE_MAP: dict[str, dict] = {}
_EXECUTABLE_FAMILY_NAME: dict[str, str] = {}
for _name, _family in SOURCEPORT_FAMILIES.items():
    for _exe in _family["executables"]:
        _EXECUTABLE_MAP[_exe] = _family
        _EXECUTABLE_FAMILY_NAME[_exe] = _name


def detect_sourceports() -> list[tuple[str, str, str]]:
    """Detect sourceports installed on the system.

    Iterates all known executables in SOURCEPORT_FAMILIES and checks
    each with shutil.which().

    Returns a list of (executable_name, full_path, family_name) for found ports.
    """
    import shutil

    found: list[tuple[str, str, str]] = []
    for family_name, family in SOURCEPORT_FAMILIES.items():
        for exe in family["executables"]:
            path = shutil.which(exe)
            if path:
                found.append((exe, path, family_name))
    return found


def identify_sourceport_family(executable: str) -> dict | None:
    """Identify a sourceport family from an executable path or name.

    Strips the path to match just the basename (e.g., /usr/bin/nyan-doom -> nyan-doom).
    Returns the family dict or None if unrecognized.
    """
    basename = Path(executable).stem
    return _EXECUTABLE_MAP.get(basename)


def get_dsda_save_dir(executable: str, data_dir: str, iwad: str, wad_path: str) -> str:
    """Compute the nested save directory for dsda-family sourceports.

    dsda-family ports nest data as {exe}_data/{iwad}/{wad_stem}/stats.txt,
    but saves go to the root of -save by default. This returns the nested
    path so saves end up alongside stats.

    Returns path like: {data_dir}/{exe}_data/{iwad}/{wad_stem}/
    """
    exe_stem = Path(executable).stem.replace("-", "_") + "_data"
    wad_stem = Path(wad_path).stem.lower()
    save_dir = Path(data_dir) / exe_stem / iwad / wad_stem
    save_dir.mkdir(parents=True, exist_ok=True)
    return str(save_dir)


def uses_deh_flag(executable: str) -> bool:
    """Return True if this sourceport uses -deh for DEH/BEX files.

    ZDoom-family ports load DEH via -file; all others use -deh.
    Returns True (use -deh) for unknown sourceports as the safe default.
    """
    basename = Path(executable).stem
    family_name = _EXECUTABLE_FAMILY_NAME.get(basename)
    return family_name != "zdoom"


def get_data_dir_args(
    executable: str,
    data_dir: str,
    *,
    iwad: str | None = None,
    wad_path: str | None = None,
) -> list[str]:
    """Return CLI args to redirect sourceport data/save dirs.

    Returns e.g. ["-data", dir, "-save", dir] for dsda family,
    ["-savedir", dir] for zdoom family, or [] for unknown sourceports.

    For dsda-family ports, if iwad and wad_path are provided, -save points
    to the nested directory where stats live so saves end up alongside them.
    """
    family = identify_sourceport_family(executable)
    if not family:
        return []

    args: list[str] = []
    if "data_arg" in family:
        args.extend([family["data_arg"], data_dir])
    if "save_arg" in family:
        # For dsda family, use nested save dir if we have enough info
        basename = Path(executable).stem
        family_name = _EXECUTABLE_FAMILY_NAME.get(basename)
        if family_name == "dsda" and iwad and wad_path:
            save_dir = get_dsda_save_dir(executable, data_dir, iwad, wad_path)
        else:
            save_dir = data_dir
        args.extend([family["save_arg"], save_dir])
    return args
