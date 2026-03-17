"""Parse saved API JSON responses for offline import.

When idgames or Doom Wiki APIs are blocked by WAF challenges,
users can visit the API URL in their browser, save the JSON response,
and import from the saved file.

Used by both CLI (import_cmds.py) and GUI (import panes).
"""

from __future__ import annotations

import json
from pathlib import Path


def parse_idgames_json(path: str | Path) -> list:
    """Parse a saved idgames API JSON response into FileEntry objects.

    Handles both single-file (action=get) and search (action=search) responses.
    """
    from caco.idgames.models import FileEntry, Review

    data = json.loads(Path(path).read_text())

    content = data.get("content", {})
    if not content:
        return []

    files = content.get("file", [])
    if isinstance(files, dict):
        files = [files]

    entries = []
    for f in files:
        # Parse reviews if present (same logic as IdgamesClient.get)
        reviews = []
        if "reviews" in f and f["reviews"]:
            review_data = f["reviews"].get("review") if isinstance(f["reviews"], dict) else None
            if review_data:
                if isinstance(review_data, dict):
                    review_data = [review_data]
                reviews = [Review(**r) for r in review_data]
        f["reviews"] = reviews
        entries.append(FileEntry(**f))

    return entries


def parse_doomwiki_json(path: str | Path) -> list:
    """Parse a saved Doom Wiki API JSON response into WikiEntry objects.

    Only returns pages containing a {{Wad}} infobox template.
    """
    from caco.doomwiki.parser import WikitextParser
    from caco.doomwiki.models import WikiEntry

    data = json.loads(Path(path).read_text())
    parser = WikitextParser()

    pages = data.get("query", {}).get("pages", {})
    entries = []
    for page_id_str, page_data in pages.items():
        if page_id_str == "-1" or "missing" in page_data:
            continue
        revisions = page_data.get("revisions", [])
        if not revisions:
            continue
        wikitext = revisions[0].get("*", "")
        title = page_data.get("title", "")
        if not parser.has_wad_template(wikitext):
            continue
        parsed = parser.parse(wikitext, title, int(page_id_str))
        entries.append(WikiEntry(**parsed))

    return entries


def detect_json_source(path: str | Path) -> str | None:
    """Detect whether a JSON file is an idgames or doomwiki API response.

    Returns 'idgames', 'doomwiki', or None if unrecognized.
    """
    try:
        data = json.loads(Path(path).read_text())
    except (json.JSONDecodeError, OSError):
        return None

    if "content" in data and isinstance(data["content"], dict):
        content = data["content"]
        if "file" in content or "status" in content:
            return "idgames"

    if "query" in data and isinstance(data["query"], dict):
        query = data["query"]
        if "pages" in query:
            return "doomwiki"

    return None


def idgames_api_url(query_or_id: str) -> str:
    """Build the idgames API URL the user should visit in their browser."""
    base = "https://www.doomworld.com/idgames/api/api.php"
    try:
        file_id = int(query_or_id)
        return f"{base}?action=get&id={file_id}&out=json"
    except ValueError:
        from urllib.parse import quote
        return f"{base}?action=search&query={quote(query_or_id)}&type=title&out=json"


def doomwiki_api_url(query_or_title: str) -> str:
    """Build the Doom Wiki API URL the user should visit in their browser."""
    from urllib.parse import quote
    base = "https://doomwiki.org/w/api.php"
    return f"{base}?action=query&titles={quote(query_or_title)}&prop=revisions&rvprop=content&format=json"
