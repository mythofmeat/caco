"""HTTP client for Doomworld forum."""

import httpx

from caco.doomworld.models import ForumThread
from caco.doomworld.parser import DoomworldParser


class DoomworldError(Exception):
    """Error from the Doomworld forum."""

    pass


class DoomworldClient:
    """Client for fetching Doomworld forum threads.

    Fetches forum thread pages and parses metadata using the DoomworldParser.
    """

    BASE_URL = "https://www.doomworld.com"
    USER_AGENT = "Caco/1.0 (Doom WAD library manager; https://github.com/eshen/caco)"

    def __init__(self, timeout: float = 30.0):
        self._client = httpx.Client(
            timeout=timeout,
            headers={"User-Agent": self.USER_AGENT},
            follow_redirects=True,
        )
        self._parser = DoomworldParser()

    def close(self) -> None:
        """Close the HTTP client."""
        self._client.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def get_thread(self, url: str) -> ForumThread:
        """Fetch and parse a forum thread by URL.

        Args:
            url: Full URL to the Doomworld forum thread

        Returns:
            ForumThread with parsed metadata

        Raises:
            DoomworldError: If the thread cannot be fetched or parsed
        """
        # Validate URL - accept both new (/forum/topic/) and old (/vb/thread/) formats
        if "doomworld.com/forum/topic/" not in url and "doomworld.com/vb/thread/" not in url:
            raise DoomworldError(f"Invalid Doomworld forum URL: {url}")

        try:
            response = self._client.get(url)
            response.raise_for_status()
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                raise DoomworldError(f"Thread not found: {url}") from e
            raise DoomworldError(f"HTTP error fetching thread: {e}") from e
        except httpx.RequestError as e:
            raise DoomworldError(f"Request error fetching thread: {e}") from e

        html_content = response.text
        parsed = self._parser.parse(html_content, url)

        # Validate we got a thread_id
        if not parsed.get("thread_id"):
            raise DoomworldError(f"Could not extract thread ID from URL: {url}")

        return ForumThread(**parsed)

    def get_thread_by_id(self, thread_id: int) -> ForumThread:
        """Fetch a forum thread by its ID.

        Constructs a URL and redirects to the canonical slug URL.

        Args:
            thread_id: Numeric thread ID

        Returns:
            ForumThread with parsed metadata
        """
        url = f"{self.BASE_URL}/forum/topic/{thread_id}/"
        return self.get_thread(url)
