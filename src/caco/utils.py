"""Shared utilities for caco."""

from __future__ import annotations

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
