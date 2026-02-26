"""Shared utilities for caco."""

from __future__ import annotations

import struct
from typing import Any

import httpx


def coerce_str(v: Any) -> str:
    """Coerce None to empty string. Used as a Pydantic field validator."""
    return "" if v is None else v


def format_rating(rating: int | None, max_stars: int = 5) -> str:
    """Render a rating as filled/empty star characters (e.g., '★★★☆☆')."""
    if not rating:
        return ""
    return "\u2605" * rating + "\u2606" * (max_stars - rating)


def format_author_year(author: str | None, year: int | str | None) -> str:
    """Format 'Author (Year)' with graceful fallbacks."""
    parts = []
    if author:
        parts.append(str(author))
    if year:
        parts.append(f"({year})")
    return " ".join(parts) if parts else "Unknown author"


def truncate(text: str | None, max_len: int, suffix: str = "...") -> str:
    """Truncate text to max_len, appending suffix if truncated."""
    if not text:
        return ""
    if len(text) <= max_len:
        return text
    return text[:max_len - len(suffix)] + suffix


def format_size(size_bytes: int) -> str:
    """Format bytes as human-readable size (e.g., '12.3 MB')."""
    value: float = float(size_bytes)
    for unit in ("B", "KB", "MB", "GB"):
        if value < 1024:
            if unit == "B":
                return f"{int(value)} {unit}"
            return f"{value:.1f} {unit}"
        value /= 1024
    return f"{value:.1f} TB"


def parse_wad_directory(wad_data: bytes | memoryview) -> list[tuple[str, int, int]]:
    """Parse WAD header and directory. Returns [(name, offset, size), ...].

    Accepts raw bytes, mmap objects, or any buffer supporting slicing
    and ``len()``.
    """
    if len(wad_data) < 12:
        return []

    magic = wad_data[:4]
    if magic not in (b"IWAD", b"PWAD"):
        return []

    num_lumps = struct.unpack_from("<i", wad_data, 4)[0]
    dir_offset = struct.unpack_from("<i", wad_data, 8)[0]

    entries: list[tuple[str, int, int]] = []
    for i in range(num_lumps):
        entry_offset = dir_offset + i * 16
        if entry_offset + 16 > len(wad_data):
            break

        lump_offset = struct.unpack_from("<i", wad_data, entry_offset)[0]
        lump_size = struct.unpack_from("<i", wad_data, entry_offset + 4)[0]
        name_bytes = wad_data[entry_offset + 8:entry_offset + 16]
        name = bytes(name_bytes).split(b"\x00")[0].decode("ascii", errors="replace").upper()
        entries.append((name, lump_offset, lump_size))

    return entries


def extract_year(date_str: str | None) -> int | None:
    """Extract a 4-digit year from a date string (e.g. '2023-03-01' or '2023-03-01T...')."""
    if not date_str:
        return None
    try:
        return int(date_str[:4])
    except (ValueError, IndexError):
        return None


# =============================================================================
# Base HTTP Client
# =============================================================================


class CacoSourceError(Exception):
    """Base error for all source adapters."""

    pass


class BaseHttpClient:
    """Base HTTP client with shared lifecycle management.

    Subclasses should set class attributes (e.g. BASE_URL, USER_AGENT)
    and implement API-specific methods.
    """

    def __init__(self, timeout: float = 30.0, **client_kwargs):
        self._client = httpx.Client(timeout=timeout, **client_kwargs)

    def close(self) -> None:
        self._client.close()

    def __enter__(self) -> BaseHttpClient:
        return self

    def __exit__(self, *args: object) -> None:
        self.close()
