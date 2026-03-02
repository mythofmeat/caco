"""Data models, enums, and constants for the WAD database."""

from dataclasses import dataclass, field
from enum import Enum
from types import MappingProxyType
from typing import TypedDict


class Status(str, Enum):
    """Play status for a WAD."""
    TO_PLAY = "to-play"
    BACKLOG = "backlog"
    PLAYING = "playing"
    FINISHED = "finished"
    ABANDONED = "abandoned"
    AWAITING_UPDATE = "awaiting-update"


class SourceType(str, Enum):
    """Where the WAD can be obtained from."""
    IDGAMES = "idgames"
    DOOMWIKI = "doomwiki"
    DOOMWORLD = "doomworld"
    URL = "url"
    LOCAL = "local"


class WadRecord(TypedDict, total=False):
    """Typed dictionary representing a WAD row with attached tags.

    All fields except 'id', 'title', and 'source_type' are optional
    (total=False). Functions returning WAD dicts should use this type
    for better editor support and static analysis.
    """
    id: int
    title: str
    author: str | None
    year: int | None
    description: str | None
    status: str
    rating: int | None
    notes: str | None
    source_type: str
    source_id: str | None
    source_url: str | None
    idgames_id: str | None
    filename: str | None
    cached_path: str | None
    custom_iwad: str | None
    custom_sourceport: str | None
    custom_args: str | None
    companion_files: str | None
    custom_config: str | None
    version: str | None
    complevel: int | None
    stats_snapshot: str | None
    deleted_at: str | None
    created_at: str
    updated_at: str
    tags: list[str]


# =============================================================================
# Query Parser Data Structures
# =============================================================================


@dataclass
class QueryTerm:
    """A single query term (field:value or free text)."""
    field: str | None  # None for free-text search
    value: str
    negated: bool = False

    def __repr__(self) -> str:
        neg = "-" if self.negated else ""
        if self.field:
            return f"{neg}{self.field}:{self.value}"
        return f"{neg}{self.value}"


@dataclass
class AndGroup:
    """A group of terms joined by AND (implicit)."""
    terms: list[QueryTerm] = field(default_factory=list)


@dataclass
class ParsedQuery:
    """Complete parsed query with OR groups.

    Structure: (term1 AND term2) OR (term3 AND term4)
    Each AndGroup is OR-ed together.
    """
    or_groups: list[AndGroup] = field(default_factory=list)

    def is_empty(self) -> bool:
        return not self.or_groups or all(not g.terms for g in self.or_groups)


# Status shortcuts for query parsing (moved from cli.py)
STATUS_SHORTCUTS: MappingProxyType[str, str] = MappingProxyType({
    "t": "to-play", "toplay": "to-play", "tp": "to-play",
    "b": "backlog", "back": "backlog",
    "p": "playing", "play": "playing",
    "f": "finished", "fin": "finished", "done": "finished",
    "a": "abandoned", "drop": "abandoned", "dropped": "abandoned",
    "w": "awaiting-update", "waiting": "awaiting-update", "wip": "awaiting-update",
    "au": "awaiting-update", "await": "awaiting-update",
})

# Canonical status metadata — single source of truth for display names and colors.
# Keys: (display_name, hex_color, rich_color, css_class)
STATUS_METADATA: MappingProxyType[str, tuple[str, str, str, str]] = MappingProxyType({
    "to-play":         ("To Play",         "#3366cc", "dodger_blue1", "status-to-play"),
    "backlog":         ("Backlog",          "#cccc33", "yellow",       "status-backlog"),
    "playing":         ("Playing",          "#33cc33", "green1",       "status-playing"),
    "finished":        ("Finished",         "#808080", "grey50",       "status-finished"),
    "abandoned":       ("Abandoned",        "#cc3333", "red",          "status-abandoned"),
    "awaiting-update": ("Awaiting Update",  "#cc33cc", "magenta",      "status-awaiting-update"),
})

# OR separator for query syntax (space-comma-space)
OR_SEPARATOR = " , "


# Fields allowed in update_wad() — guards against SQL column-name injection
ALLOWED_UPDATE_FIELDS = frozenset({
    "title", "author", "year", "description", "status", "rating", "notes",
    "source_url", "filename", "cached_path", "custom_iwad",
    "custom_sourceport", "custom_args",
    "custom_config", "version", "complevel", "idgames_id",
    "deleted_at", "stats_snapshot",
})
