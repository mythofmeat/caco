"""Doomworld forum source adapter."""

from caco.doomworld import DoomworldClient, ForumThread
from caco.db import SourceType, add_wad


class DoomworldSource:
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

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.client.close()

    def get(self, url: str) -> ForumThread | None:
        """Get a forum thread by URL.

        Args:
            url: Full URL to the Doomworld forum thread

        Returns:
            ForumThread with parsed metadata, or None if not found
        """
        try:
            return self.client.get_thread(url)
        except Exception:
            return None

    def get_by_id(self, thread_id: int) -> ForumThread | None:
        """Get a forum thread by ID.

        Args:
            thread_id: Numeric thread ID from the URL

        Returns:
            ForumThread with parsed metadata, or None if not found
        """
        try:
            return self.client.get_thread_by_id(thread_id)
        except Exception:
            return None

    def import_wad(
        self,
        thread: ForumThread,
        tags: list[str] | None = None,
        title: str | None = None,
        author: str | None = None,
        year: int | None = None,
    ) -> int:
        """Import a WAD from a Doomworld forum thread into the local database.

        Args:
            thread: ForumThread with metadata
            tags: Optional list of tags to add
            title: Override title (defaults to thread title)
            author: Override author (defaults to thread author/OP)
            year: Override year (extracted from posted_date if not provided)

        Returns:
            The new WAD's database ID
        """
        # Use provided values or fall back to thread data
        final_title = title or thread.title
        final_author = author or thread.author or None

        # Try to extract year from posted_date if not provided
        final_year = year
        if final_year is None and thread.posted_date:
            # posted_date is ISO format like "2023-03-01T..."
            try:
                final_year = int(thread.posted_date[:4])
            except (ValueError, IndexError):
                pass

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
        )
