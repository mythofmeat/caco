"""Doom Wiki API client package.

Provides access to the Doom Wiki (doomwiki.org) for fetching WAD metadata.
Uses the MediaWiki API to search and retrieve page content, then parses
the {{Wad}} infobox template to extract structured metadata.
"""

from caco.doomwiki.client import DoomwikiClient, DoomwikiError
from caco.doomwiki.models import SearchResult, WikiEntry

__all__ = [
    "DoomwikiClient",
    "DoomwikiError",
    "SearchResult",
    "WikiEntry",
]
