"""Doom Wiki image scraping via MediaWiki API.

For WADs without local files (e.g., doomwiki imports), tries to fetch
a title screen image from the WAD's Doom Wiki page.

Two entry points:
- fetch_wiki_image(wiki_url): Fetch from a known Doom Wiki page URL
- search_wiki_image(title): Search Doom Wiki by WAD title, then fetch image
"""

import httpx

API_URL = "https://doomwiki.org/w/api.php"
_TIMEOUT = 15.0
# MediaWiki blocks requests without a descriptive User-Agent (returns 403).
# Must match the pattern used by caco's existing doomwiki client.
_HEADERS = {"User-Agent": "Caco/1.0 (Doom WAD library manager)"}

# Shared httpx client (thread-safe for reads) — avoids creating a new client per thumbnail
_shared_client: httpx.Client | None = None


def _get_client() -> httpx.Client:
    """Get (or lazily create) the shared httpx client."""
    global _shared_client
    if _shared_client is None:
        _shared_client = httpx.Client(timeout=_TIMEOUT, headers=_HEADERS)
    return _shared_client


def fetch_wiki_image(wiki_url: str) -> bytes | None:
    """Try to fetch a title screen image from a Doom Wiki page URL.

    Args:
        wiki_url: Full URL to the Doom Wiki page (e.g., https://doomwiki.org/wiki/Eviternity)

    Returns:
        Image bytes if found, None otherwise.
    """
    if "/wiki/" not in wiki_url:
        return None

    page_title = wiki_url.split("/wiki/")[-1]
    return _fetch_image_for_page(page_title)


def search_wiki_image(title: str) -> bytes | None:
    """Search the Doom Wiki for a WAD by title and fetch its page image.

    Uses MediaWiki's opensearch API to find a matching page, then
    extracts an image from it. Works for any WAD regardless of source.

    Args:
        title: WAD title to search for (e.g., "Eviternity", "Scythe 2")

    Returns:
        Image bytes if found, None otherwise.
    """
    if not title:
        return None

    try:
        client = _get_client()
        # Search for the WAD page by title
        resp = client.get(API_URL, params={
            "action": "opensearch",
            "search": title,
            "limit": "5",
            "namespace": "0",
            "format": "json",
        })
        resp.raise_for_status()
        data = resp.json()

        # opensearch returns [search_term, [titles], [descriptions], [urls]]
        if len(data) < 2 or not data[1]:
            return None

        # Try each result — prefer exact or close matches
        for page_title in data[1]:
            result = _fetch_image_for_page(page_title, client=client)
            if result:
                return result

    except Exception:
        pass

    return None


def _fetch_image_for_page(page_title: str, client: httpx.Client | None = None) -> bytes | None:
    """Core logic: fetch a title screen image from a Doom Wiki page.

    Args:
        page_title: MediaWiki page title (URL-decoded or encoded both work)
        client: Optional existing httpx.Client to reuse (defaults to shared client)

    Returns:
        Image bytes if found, None otherwise.
    """
    try:
        if client is None:
            client = _get_client()

        # Step 1: Get images on the page
        resp = client.get(API_URL, params={
            "action": "query",
            "titles": page_title,
            "prop": "images",
            "imlimit": "50",
            "format": "json",
        })
        resp.raise_for_status()
        data = resp.json()

        pages = data.get("query", {}).get("pages", {})
        if not pages:
            return None

        page = next(iter(pages.values()))

        # Skip missing pages (search result didn't match a real page)
        if page.get("missing") is not None:
            return None

        images = page.get("images", [])

        # Step 2: Filter for title screen images
        title_images = []
        for img in images:
            name = img["title"].lower()
            if any(k in name for k in ("titlepic", "title screen", "title.png", "title.jpg")):
                title_images.append(img["title"])

        # Fallback: try the first .png/.jpg image
        if not title_images:
            for img in images:
                name = img["title"].lower()
                if name.endswith((".png", ".jpg", ".jpeg", ".gif")):
                    title_images.append(img["title"])
                    break

        if not title_images:
            return None

        # Step 3: Get direct URL for the first title image
        image_title = title_images[0]
        resp = client.get(API_URL, params={
            "action": "query",
            "titles": image_title,
            "prop": "imageinfo",
            "iiprop": "url",
            "format": "json",
        })
        resp.raise_for_status()
        data = resp.json()

        pages = data.get("query", {}).get("pages", {})
        if not pages:
            return None

        page = next(iter(pages.values()))
        imageinfo = page.get("imageinfo", [])
        if not imageinfo:
            return None

        image_url = imageinfo[0].get("url")
        if not image_url:
            return None

        # Step 4: Download the image
        resp = client.get(image_url)
        resp.raise_for_status()

        if resp.headers.get("content-type", "").startswith("image/"):
            return resp.content

    except Exception:
        pass

    return None
