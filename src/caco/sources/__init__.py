"""Data sources for WAD metadata."""

from caco.sources.base import BaseSource
from caco.sources.idgames import IdgamesSource
from caco.sources.doomwiki import DoomwikiSource
from caco.sources.doomworld import DoomworldSource

__all__ = ["BaseSource", "IdgamesSource", "DoomwikiSource", "DoomworldSource"]
