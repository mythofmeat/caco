"""Doom Wiki API client using MediaWiki API."""

from caco.doomwiki.models import SearchResult, WikiEntry
from caco.doomwiki.parser import WikitextParser
from caco.utils import BaseHttpClient, CacoSourceError


class DoomwikiError(CacoSourceError):
    """Error from the Doom Wiki API."""

    pass


class DoomwikiClient(BaseHttpClient):
    """Client for the Doom Wiki MediaWiki API.

    Uses the MediaWiki API at https://doomwiki.org/w/api.php to search
    and retrieve wiki page content.
    """

    API_URL = "https://doomwiki.org/w/api.php"
    USER_AGENT = "Caco/1.0 (Doom WAD library manager; https://github.com/eshen/caco)"

    def __init__(self, timeout: float = 30.0):
        super().__init__(
            timeout=timeout,
            headers={"User-Agent": self.USER_AGENT},
        )
        self._parser = WikitextParser()

    def _request(self, **params) -> dict:
        """Make a request to the MediaWiki API."""
        params["format"] = "json"

        response = self._client.get(self.API_URL, params=params)
        response.raise_for_status()

        data = response.json()

        if "error" in data:
            raise DoomwikiError(data["error"].get("info", "Unknown error"))

        return data

    def search(self, query: str, limit: int = 20) -> list[SearchResult]:
        """
        Search the wiki for pages matching the query.

        Args:
            query: Search query string
            limit: Maximum number of results (default 20)

        Returns:
            List of SearchResult objects
        """
        data = self._request(
            action="query",
            list="search",
            srsearch=query,
            srlimit=limit,
            srprop="snippet",
        )

        results = []
        for item in data.get("query", {}).get("search", []):
            results.append(SearchResult(
                pageid=item.get("pageid", 0),
                title=item.get("title", ""),
                snippet=item.get("snippet", ""),
            ))

        return results

    def get_page_content(self, title: str) -> tuple[int, str] | None:
        """
        Get the raw wikitext content of a page by title.

        Args:
            title: Page title

        Returns:
            Tuple of (page_id, wikitext) or None if page doesn't exist
        """
        data = self._request(
            action="query",
            titles=title,
            prop="revisions",
            rvprop="content",
        )

        pages = data.get("query", {}).get("pages", {})
        for page_id, page_data in pages.items():
            if page_id == "-1":
                return None  # Page doesn't exist

            revisions = page_data.get("revisions", [])
            if revisions:
                # Content is directly in "*" key (older MediaWiki format)
                content = revisions[0].get("*", "")
                return int(page_id), content

        return None

    def get_page_content_by_id(self, page_id: int) -> tuple[str, str] | None:
        """
        Get the raw wikitext content of a page by ID.

        Args:
            page_id: MediaWiki page ID

        Returns:
            Tuple of (title, wikitext) or None if page doesn't exist
        """
        data = self._request(
            action="query",
            pageids=page_id,
            prop="revisions",
            rvprop="content",
        )

        pages = data.get("query", {}).get("pages", {})
        page_data = pages.get(str(page_id), {})

        if "missing" in page_data:
            return None

        title = page_data.get("title", "")
        revisions = page_data.get("revisions", [])
        if revisions:
            # Content is directly in "*" key (older MediaWiki format)
            content = revisions[0].get("*", "")
            return title, content

        return None

    def get_entry(self, title: str) -> WikiEntry | None:
        """
        Get parsed WAD entry for a wiki page by title.

        Args:
            title: Page title

        Returns:
            WikiEntry with parsed metadata, or None if page doesn't exist
        """
        result = self.get_page_content(title)
        if result is None:
            return None

        page_id, wikitext = result
        parsed = self._parser.parse(wikitext, title, page_id)
        return WikiEntry(**parsed)

    def get_entry_by_id(self, page_id: int) -> WikiEntry | None:
        """
        Get parsed WAD entry for a wiki page by ID.

        Args:
            page_id: MediaWiki page ID

        Returns:
            WikiEntry with parsed metadata, or None if page doesn't exist
        """
        result = self.get_page_content_by_id(page_id)
        if result is None:
            return None

        title, wikitext = result
        parsed = self._parser.parse(wikitext, title, page_id)
        return WikiEntry(**parsed)

    def get_pages_batch(self, titles: list[str]) -> dict[str, tuple[int, str]]:
        """Fetch multiple page contents in a single API request.

        Uses MediaWiki pipe-separated titles API (max 50 per request).

        Args:
            titles: List of page titles to fetch

        Returns:
            Dict mapping title -> (page_id, wikitext) for pages that exist
        """
        if not titles:
            return {}

        results: dict[str, tuple[int, str]] = {}

        # MediaWiki API supports up to 50 titles per request
        for i in range(0, len(titles), 50):
            batch = titles[i:i + 50]
            data = self._request(
                action="query",
                titles="|".join(batch),
                prop="revisions",
                rvprop="content",
            )

            for page_id_str, page_data in data.get("query", {}).get("pages", {}).items():
                if page_id_str == "-1" or "missing" in page_data:
                    continue
                revisions = page_data.get("revisions", [])
                if revisions:
                    title = page_data.get("title", "")
                    content = revisions[0].get("*", "")
                    results[title] = (int(page_id_str), content)

        return results

    def search_wads(self, query: str, limit: int = 20) -> list[WikiEntry]:
        """
        Search for WAD pages and return parsed entries.

        Only returns pages that contain a {{Wad}} infobox template.
        Uses batch page fetch to minimize API requests.

        Args:
            query: Search query string
            limit: Maximum number of results to search (may return fewer)

        Returns:
            List of WikiEntry objects for pages with WAD infoboxes
        """
        search_results = self.search(query, limit=limit)
        if not search_results:
            return []

        # Batch-fetch all page contents in one API call
        titles = [r.title for r in search_results]
        pages = self.get_pages_batch(titles)

        entries = []
        for result in search_results:
            content_result = pages.get(result.title)
            if content_result is None:
                continue

            page_id, wikitext = content_result

            # Only include pages with {{Wad}} template
            if not self._parser.has_wad_template(wikitext):
                continue

            parsed = self._parser.parse(wikitext, result.title, page_id)
            entries.append(WikiEntry(**parsed))

        return entries
