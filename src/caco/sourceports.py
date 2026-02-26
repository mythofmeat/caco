"""Sourceport family registry — maps executables to CLI flags for data/save redirection."""

from pathlib import Path

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

# Build reverse lookup: executable name -> family dict
_EXECUTABLE_MAP: dict[str, dict] = {}
for _family in SOURCEPORT_FAMILIES.values():
    for _exe in _family["executables"]:
        _EXECUTABLE_MAP[_exe] = _family


def identify_sourceport_family(executable: str) -> dict | None:
    """Identify a sourceport family from an executable path or name.

    Strips the path to match just the basename (e.g., /usr/bin/nyan-doom -> nyan-doom).
    Returns the family dict or None if unrecognized.
    """
    basename = Path(executable).stem
    return _EXECUTABLE_MAP.get(basename)


def get_data_dir_args(executable: str, data_dir: str) -> list[str]:
    """Return CLI args to redirect sourceport data/save dirs.

    Returns e.g. ["-data", dir, "-save", dir] for dsda family,
    ["-savedir", dir] for zdoom family, or [] for unknown sourceports.
    """
    family = identify_sourceport_family(executable)
    if not family:
        return []

    args: list[str] = []
    if "data_arg" in family:
        args.extend([family["data_arg"], data_dir])
    if "save_arg" in family:
        args.extend([family["save_arg"], data_dir])
    return args
