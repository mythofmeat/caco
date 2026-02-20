"""Doomworld forum source adapter."""

import logging

import httpx

from caco.doomworld import DoomworldClient, ForumThread
from caco.db import SourceType, add_wad
from caco.sources.base import BaseSource
from caco.utils import extract_year

logger = logging.getLogger(__name__)


class DoomworldSource(BaseSource):
    """Adapter for importing WADs from Doomworld forum threads.

    The Doomworld forums (doomworld.com/forum) host many WAD release threads,
    particularly in the "WADs & Mods" section. This adapter fetches thread
    metadata and imports it into the local library.

    Note: The forum thread URL is stored as source_url, and download links
    from the post content may need to be followed manually or extracted
    with `extract_download_links()`.
    """

    def __init__(self):
        self.client = DoomworldClient()

    def get(self, url: str) -> ForumThread | None:
        """Get a forum thread by URL.

        Args:
            url: Full URL to the Doomworld forum thread

        Returns:
            ForumThread with parsed metadata, or None if not found
        """
        try:
            thread: ForumThread | None = self.client.get_thread(url)
            return thread
        except (httpx.HTTPError, ValueError, KeyError) as e:
            logger.debug("Failed to fetch thread %s: %s", url, e)
            return None

    def get_by_id(self, thread_id: int) -> ForumThread | None:
        """Get a forum thread by ID.

        Args:
            thread_id: Numeric thread ID from the URL

        Returns:
            ForumThread with parsed metadata, or None if not found
        """
        try:
            thread: ForumThread | None = self.client.get_thread_by_id(thread_id)
            return thread
        except (httpx.HTTPError, ValueError, KeyError) as e:
            logger.debug("Failed to fetch thread %d: %s", thread_id, e)
            return None

    def import_wad(
        self,
        thread: ForumThread,
        tags: list[str] | None = None,
        title: str | None = None,
        author: str | None = None,
        year: int | None = None,
        version: str | None = None,
    ) -> int:
        """Import a WAD from a Doomworld forum thread into the local database.

        Args:
            thread: ForumThread with metadata
            tags: Optional list of tags to add
            title: Override title (defaults to thread title)
            author: Override author (defaults to thread author/OP)
            year: Override year (extracted from posted_date if not provided)
            version: Version string (e.g., 'v1.0', 'RC2') if known

        Returns:
            The new WAD's database ID
        """
        # Use provided values or fall back to thread data
        final_title = title or thread.title
        final_author = author or thread.author or None

        final_year = year if year is not None else extract_year(thread.posted_date)

        # Use first post text as description, truncated if too long
        description = thread.first_post_text
        if len(description) > 2000:
            description = description[:1997] + "..."

        return add_wad(
            title=final_title,
            author=final_author,
            year=final_year,
            description=description or None,
            source_type=SourceType.DOOMWORLD,
            source_id=str(thread.thread_id),
            source_url=thread.thread_url,
            tags=tags,
            version=version,
        )
