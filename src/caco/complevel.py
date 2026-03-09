"""Shared complevel (compatibility level) names, aliases, and parsing.

Complevel is a dsda-doom concept that tells the engine which Doom engine
version to emulate (e.g., 2=vanilla, 9=Boom, 11=MBF, 21=MBF21).
"""

# Human-readable names for common complevels
COMPLEVEL_NAMES: dict[int, str] = {
    0: "Doom v1.2",
    1: "Doom v1.666",
    2: "Doom v1.9 / Vanilla",
    3: "Ultimate Doom",
    4: "Final Doom",
    9: "Boom",
    11: "MBF",
    21: "MBF21",
}

# Aliases: string name -> complevel int
COMPLEVEL_ALIASES: dict[str, int] = {
    "vanilla": 2,
    "boom": 9,
    "mbf": 11,
    "mbf21": 21,
    "limit-removing": 2,
    "lr": 2,
}


def complevel_name(cl: int | None) -> str:
    """Get human-readable name for a complevel."""
    if cl is None:
        return "Unknown"
    return COMPLEVEL_NAMES.get(cl, f"Complevel {cl}")


# Helion uses string names for complevels via +complevel
HELION_COMPLEVEL_NAMES: dict[int, str] = {
    2: "vanilla",
    9: "boom",
    11: "mbf",
    21: "mbf21",
}


def complevel_to_helion_name(complevel: int) -> str | None:
    """Map a numeric complevel to Helion's +complevel string."""
    return HELION_COMPLEVEL_NAMES.get(complevel)


def parse_complevel(value: str) -> int | None:
    """Parse a complevel from a string — accepts integer or alias name.

    Returns the complevel int, or None if invalid.
    """
    # Try as integer first
    try:
        return int(value)
    except ValueError:
        pass

    # Try as alias
    return COMPLEVEL_ALIASES.get(value.lower())
