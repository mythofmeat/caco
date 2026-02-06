"""Shared utilities for caco."""

import httpx


def coerce_str(v):
    """Coerce None to empty string. Used as a Pydantic field validator."""
    return "" if v is None else v


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

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()
