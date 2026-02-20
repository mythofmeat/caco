"""Doom Wiki source adapter."""

from caco.doomwiki import DoomwikiClient, WikiEntry
from caco.db import SourceType, add_wad
from caco.sources.base import BaseSource


class DoomwikiSource(BaseSource):
    """Adapter for importing WADs from Doom Wiki.

    The Doom Wiki (doomwiki.org) contains structured metadata about WADs
    in {{Wad}} infobox templates. This adapter searches the wiki and
    extracts this metadata for import into the local library.

    Note: Doom Wiki doesn't host WAD files directly, but the 'link' field
    often contains idgames URLs which could be used for downloading.
    """

    def __init__(self):
        self.client = DoomwikiClient()

    def search(self, query: str, limit: int = 20) -> list[WikiEntry]:
        """Search Doom Wiki for WAD pages.

        Only returns pages that contain a {{Wad}} infobox template.

        Args:
            query: Search query string
            limit: Maximum number of results

        Returns:
            List of WikiEntry objects with parsed metadata
        """
        return self.client.search_wads(query, limit=limit)

    def get(self, title: str) -> WikiEntry | None:
        """Get a specific wiki page by title.

        Args:
            title: Wiki page title

        Returns:
            WikiEntry with parsed metadata, or None if not found
        """
        return self.client.get_entry(title)

    def get_by_id(self, page_id: int) -> WikiEntry | None:
        """Get a specific wiki page by ID.

        Args:
            page_id: MediaWiki page ID

        Returns:
            WikiEntry with parsed metadata, or None if not found
        """
        return self.client.get_entry_by_id(page_id)

    def import_wad(
        self,
        entry: WikiEntry,
        tags: list[str] | None = None,
    ) -> int:
        """Import a WAD from Doom Wiki into the local database.

        Args:
            entry: WikiEntry with metadata
            tags: Optional list of tags to add

        Returns:
            The new WAD's database ID
        """
        return add_wad(
            title=entry.display_name,
            author=entry.author or None,
            year=entry.year,
            description=entry.description or None,
            source_type=SourceType.DOOMWIKI,
            source_id=str(entry.page_id),
            source_url=entry.wiki_url,
            tags=tags,
        )
