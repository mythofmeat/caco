"""HTML and JSON-LD parser for Doomworld forum threads.

Doomworld uses Invision Community forum software which includes JSON-LD
structured data in the page. This makes extraction much more reliable than
pure HTML parsing.
"""

import html
import json
import re
from typing import Any

from caco.complevel import COMPLEVEL_NAMES, complevel_name  # noqa: F401


# =============================================================================
# Complevel Detection
# =============================================================================

# Mapping of complevel keywords to normalized complevel values
# Based on DSDA-Doom/PrBoom+ complevel system
COMPLEVEL_PATTERNS = {
    # Explicit complevel mentions
    r'\bcomplevel\s*[-:]?\s*(\d{1,2})\b': lambda m: int(m.group(1)),
    r'\bcl\s*[-:]?\s*(\d{1,2})\b': lambda m: int(m.group(1)),
    r'\b-complevel\s+(\d{1,2})\b': lambda m: int(m.group(1)),

    # Named compatibility levels
    r'\bvanilla\s*(?:doom|compatible|compat)?\b': lambda m: 2,  # Doom 2 vanilla
    r'\bdoom\s*2?\s*vanilla\b': lambda m: 2,
    r'\bchocolate\s*doom\b': lambda m: 2,
    r'\blimit[- ]?removing\b': lambda m: 2,  # Often means vanilla-ish
    r'\bboom\s*(?:compatible|compat)?\b': lambda m: 9,
    r'\bmbf\s*(?:compatible|compat)?\b': lambda m: 11,
    r'\bmbf21\b': lambda m: 21,
    r'\bdsda[- ]?doom\b': lambda m: 21,  # Usually implies MBF21 support
}

# Backward-compat alias — prefer caco.complevel.COMPLEVEL_ALIASES for new code
COMPLEVEL_SHORTCUTS: dict[str, int] = {
    "vanilla": 2,
    "boom": 9,
    "mbf": 11,
    "mbf21": 21,
}


def extract_complevel(text: str) -> int | None:
    """Extract complevel from post text.

    Looks for patterns like:
    - "complevel 9", "cl21", "-complevel 11"
    - "vanilla compatible", "boom compatible", "MBF21"
    - "limit-removing"

    Returns:
        Complevel as integer, or None if not found
    """
    text_lower = text.lower()

    for pattern, extractor in COMPLEVEL_PATTERNS.items():
        match = re.search(pattern, text_lower, re.IGNORECASE)
        if match:
            return extractor(match)

    return None


# =============================================================================
# IWAD Detection
# =============================================================================

# IWAD patterns - maps regex to normalized IWAD name
IWAD_PATTERNS = [
    # Doom II variants (check first as "Doom 2" is more specific than "Doom")
    (r'\bdoom\s*(?:ii|2)\b', "doom2"),
    (r'\bfor\s+doom\s*(?:ii|2)\b', "doom2"),
    (r'\brequires?\s+doom\s*(?:ii|2)\b', "doom2"),
    (r'\bdoom2\.wad\b', "doom2"),

    # Final Doom
    (r'\btnt\.wad\b', "tnt"),
    (r'\btnt[:\s]+evilution\b', "tnt"),
    (r'\bevilution\b', "tnt"),
    (r'\bpluton(?:ia)?\.wad\b', "plutonia"),
    (r'\bpluton(?:ia)?\s*(?:experiment)?\b', "plutonia"),
    (r'\bfinal\s*doom\b', "finaldoom"),

    # Ultimate Doom / Doom 1
    (r'\bultimate\s*doom\b', "doom"),
    (r'\bdoom\.wad\b', "doom"),
    (r'\bdoom\s*1\b', "doom"),
    (r'\bfor\s+doom\s*1?\b', "doom"),  # "for Doom" without 2
    (r'\brequires?\s+doom(?!\s*(?:ii|2))\b', "doom"),

    # Heretic
    (r'\bheretic\b', "heretic"),

    # Hexen
    (r'\bhexen\b', "hexen"),

    # Strife
    (r'\bstrife\b', "strife"),

    # Chex Quest
    (r'\bchex\s*quest\b', "chex"),

    # FreeDoom
    (r'\bfreedoom\b', "freedoom"),
]

# Display names for IWADs
IWAD_DISPLAY_NAMES = {
    "doom": "Ultimate Doom",
    "doom2": "Doom II",
    "tnt": "TNT: Evilution",
    "plutonia": "Plutonia Experiment",
    "finaldoom": "Final Doom",
    "heretic": "Heretic",
    "hexen": "Hexen",
    "strife": "Strife",
    "chex": "Chex Quest",
    "freedoom": "FreeDoom",
}


def extract_iwad(text: str) -> str | None:
    """Extract IWAD requirement from post text.

    Looks for patterns like:
    - "requires Doom 2", "for Ultimate Doom"
    - "doom2.wad", "plutonia.wad"
    - "Heretic", "Hexen"

    Returns:
        Normalized IWAD name (doom, doom2, tnt, plutonia, heretic, etc.)
        or None if not found
    """
    text_lower = text.lower()

    for pattern, iwad in IWAD_PATTERNS:
        if re.search(pattern, text_lower, re.IGNORECASE):
            return iwad

    return None


def iwad_display_name(iwad: str | None) -> str:
    """Get human-readable display name for an IWAD."""
    if iwad is None:
        return "Unknown"
    return IWAD_DISPLAY_NAMES.get(iwad, iwad.title())


# =============================================================================
# Sourceport Detection
# =============================================================================

# Sourceport patterns - maps regex to normalized port name
SOURCEPORT_PATTERNS = [
    # GZDoom family
    (r'\bgzdoom\b', "gzdoom"),
    (r'\blzdoom\b', "lzdoom"),
    (r'\bvkdoom\b', "vkdoom"),
    (r'\bqzdoom\b', "qzdoom"),
    (r'\bzdoom\b', "zdoom"),  # After GZDoom variants

    # DSDA-Doom / PrBoom family
    (r'\bdsda[- ]?doom\b', "dsda-doom"),
    (r'\bprboom\+?\b', "prboom+"),
    (r'\bglboom\+?\b', "glboom+"),
    (r'\bumapinfo\b', "dsda-doom"),  # UMAPINFO implies DSDA/PrBoom+

    # Eternity
    (r'\beternity\s*(?:engine)?\b', "eternity"),

    # Crispy Doom
    (r'\bcrispy\s*doom\b', "crispy-doom"),

    # Chocolate Doom
    (r'\bchocolate\s*doom\b', "chocolate-doom"),

    # Woof!
    (r'\bwoof!?\b', "woof"),

    # Nugget Doom
    (r'\bnugget\s*doom\b', "nugget-doom"),

    # EDGE
    (r'\bedge(?:-classic)?\b', "edge"),

    # Doomsday
    (r'\bdoomsday\b', "doomsday"),

    # Zandronum (multiplayer)
    (r'\bzandronum\b', "zandronum"),

    # Odamex (multiplayer)
    (r'\bodamex\b', "odamex"),

    # 3DGE
    (r'\b3dge\b', "3dge"),

    # Limit-removing (generic)
    (r'\blimit[- ]?removing\b', "limit-removing"),
]

# Display names for sourceports
SOURCEPORT_DISPLAY_NAMES = {
    "gzdoom": "GZDoom",
    "lzdoom": "LZDoom",
    "vkdoom": "VKDoom",
    "qzdoom": "QZDoom",
    "zdoom": "ZDoom",
    "dsda-doom": "DSDA-Doom",
    "prboom+": "PrBoom+",
    "glboom+": "GLBoom+",
    "eternity": "Eternity Engine",
    "crispy-doom": "Crispy Doom",
    "chocolate-doom": "Chocolate Doom",
    "woof": "Woof!",
    "nugget-doom": "Nugget Doom",
    "edge": "EDGE",
    "doomsday": "Doomsday",
    "zandronum": "Zandronum",
    "odamex": "Odamex",
    "3dge": "3DGE",
    "limit-removing": "Limit-removing",
}


def extract_sourceport(text: str) -> str | None:
    """Extract sourceport requirement from post text.

    Looks for patterns like:
    - "GZDoom required", "tested in DSDA-Doom"
    - "for Crispy Doom", "Eternity Engine"

    Returns:
        Normalized sourceport name or None if not found
    """
    text_lower = text.lower()

    for pattern, port in SOURCEPORT_PATTERNS:
        if re.search(pattern, text_lower, re.IGNORECASE):
            return port

    return None


def sourceport_display_name(port: str | None) -> str:
    """Get human-readable display name for a sourceport."""
    if port is None:
        return "Unknown"
    return SOURCEPORT_DISPLAY_NAMES.get(port, port.title())


# =============================================================================
# Download Link Extraction
# =============================================================================

# File hosting services and direct download patterns
DOWNLOAD_PATTERNS = [
    # Direct file downloads
    r'https?://[^\s<>"\')\]]+\.(?:zip|wad|pk3|pk7|7z|rar|tar\.gz)',

    # Dropbox
    r'https?://(?:www\.)?dropbox\.com/[^\s<>"\')\]]+',
    r'https?://dl\.dropbox(?:usercontent)?\.com/[^\s<>"\')\]]+',

    # Google Drive
    r'https?://drive\.google\.com/[^\s<>"\')\]]+',

    # Mediafire
    r'https?://(?:www\.)?mediafire\.com/[^\s<>"\')\]]+',

    # Mega
    r'https?://mega\.(?:nz|co\.nz)/[^\s<>"\')\]]+',

    # GitHub releases
    r'https?://github\.com/[^\s<>"\')\]]+/releases/[^\s<>"\')\]]+',
    r'https?://github\.com/[^\s<>"\')\]]+\.(?:zip|wad|pk3|pk7)',

    # itch.io
    r'https?://[^\s<>"\')\]]+\.itch\.io/[^\s<>"\')\]]+',

    # ModDB
    r'https?://(?:www\.)?moddb\.com/[^\s<>"\')\]]+/downloads/[^\s<>"\')\]]+',

    # Doomworld idgames
    r'https?://(?:www\.)?doomworld\.com/idgames/[^\s<>"\')\]]+',

    # idgames mirror
    r'https?://[^\s<>"\')\]]*idgames[^\s<>"\')\]]*\.(?:zip|wad)',

    # GameBanana
    r'https?://(?:www\.)?gamebanana\.com/[^\s<>"\')\]]+',

    # OneDrive
    r'https?://(?:1drv\.ms|onedrive\.live\.com)/[^\s<>"\')\]]+',

    # Catbox
    r'https?://files\.catbox\.moe/[^\s<>"\')\]]+',

    # Litterbox (temp catbox)
    r'https?://litter\.catbox\.moe/[^\s<>"\')\]]+',
]


def extract_download_links(text: str) -> list[str]:
    """Extract potential download URLs from post text.

    Finds URLs pointing to common file hosting services and direct file links.

    Args:
        text: Post content (HTML or plain text)

    Returns:
        List of unique download URLs, preserving order of first occurrence
    """
    # Also extract from href attributes in HTML
    href_pattern = r'href=["\']([^"\']+)["\']'
    hrefs = re.findall(href_pattern, text, re.IGNORECASE)

    all_urls = []

    # Check hrefs first (more reliable than text matching)
    for href in hrefs:
        for pattern in DOWNLOAD_PATTERNS:
            if re.match(pattern, href, re.IGNORECASE):
                all_urls.append(href)
                break

    # Then check plain text
    for pattern in DOWNLOAD_PATTERNS:
        matches = re.findall(pattern, text, re.IGNORECASE)
        all_urls.extend(matches)

    # Deduplicate while preserving order
    seen = set()
    unique = []
    for url in all_urls:
        # Clean up URL (remove trailing punctuation that might have been captured)
        url = url.rstrip('.,;:!?')
        url_lower = url.lower()
        if url_lower not in seen:
            seen.add(url_lower)
            unique.append(url)

    return unique


# =============================================================================
# Main Parser Class
# =============================================================================


class DoomworldParser:
    """Parser for extracting metadata from Doomworld forum thread pages.

    Uses a multi-strategy approach:
    1. JSON-LD structured data (preferred, most reliable)
    2. HTML meta tags and content (fallback)
    3. Regex-based extraction for technical requirements
    """

    def parse(self, html_content: str, url: str) -> dict[str, Any]:
        """Parse a Doomworld forum thread page.

        Args:
            html_content: Full HTML content of the page
            url: Original URL (used to extract thread_id)

        Returns:
            Dict with keys: thread_id, title, author, posted_date,
            first_post_html, first_post_text, thread_url,
            download_links, complevel, iwad, sourceport
        """
        result = {
            "thread_id": self._extract_thread_id(url),
            "title": "",
            "author": "",
            "posted_date": "",
            "first_post_html": "",
            "first_post_text": "",
            "thread_url": url,
            "download_links": [],
            "complevel": None,
            "iwad": None,
            "sourceport": None,
        }

        # Try JSON-LD first (most reliable)
        json_ld = self._extract_json_ld(html_content)
        if json_ld:
            result["title"] = json_ld.get("headline", "")
            author_data = json_ld.get("author", {})
            if isinstance(author_data, dict):
                result["author"] = author_data.get("name", "")
            elif isinstance(author_data, str):
                result["author"] = author_data
            result["posted_date"] = json_ld.get("dateCreated", "") or json_ld.get("datePublished", "")

        # Fallback to HTML title if needed
        if not result["title"]:
            result["title"] = self._extract_html_title(html_content)

        # Extract first post content
        first_post_html = self._extract_first_post(html_content)
        result["first_post_html"] = first_post_html
        result["first_post_text"] = self._html_to_text(first_post_html)

        # Extract technical metadata from first post
        combined_text = first_post_html + " " + str(result["first_post_text"])
        result["download_links"] = extract_download_links(combined_text)
        result["complevel"] = extract_complevel(combined_text)
        result["iwad"] = extract_iwad(combined_text)
        result["sourceport"] = extract_sourceport(combined_text)

        return result

    def _extract_thread_id(self, url: str) -> int:
        """Extract thread ID from Doomworld forum URL.

        URL formats:
        - https://www.doomworld.com/forum/topic/134292-myhousewad/
        - https://www.doomworld.com/forum/topic/134292-myhousewad/?page=5
        - https://www.doomworld.com/forum/topic/134292-some-title-here
        - https://www.doomworld.com/vb/thread/153124 (old vBulletin format)
        """
        # Match /forum/topic/{id} or /vb/thread/{id} pattern
        match = re.search(r'/(?:forum/topic|vb/thread)/(\d+)', url)
        if match:
            return int(match.group(1))
        return 0

    def _extract_json_ld(self, html_content: str) -> dict[str, Any] | None:
        """Extract JSON-LD structured data from HTML.

        Looks for DiscussionForumPosting or other relevant types.
        """
        # Find all JSON-LD script blocks
        pattern = r'<script[^>]*type=["\']application/ld\+json["\'][^>]*>(.*?)</script>'
        matches = re.findall(pattern, html_content, re.DOTALL | re.IGNORECASE)

        for match in matches:
            try:
                data: Any = json.loads(match.strip())

                # Handle @graph format (array of entities)
                if isinstance(data, dict) and "@graph" in data:
                    for item in data["@graph"]:
                        if item.get("@type") == "DiscussionForumPosting":
                            result: dict[str, Any] = item
                            return result
                # Direct DiscussionForumPosting
                elif isinstance(data, dict):
                    if data.get("@type") == "DiscussionForumPosting":
                        return data
                # Array of items
                elif isinstance(data, list):
                    for item in data:
                        if isinstance(item, dict) and item.get("@type") == "DiscussionForumPosting":
                            return item

            except json.JSONDecodeError:
                continue

        return None

    def _extract_html_title(self, html_content: str) -> str:
        """Extract title from HTML <title> tag, cleaning up suffix.

        Doomworld typically formats as: "Thread Title - WADs & Mods - Doomworld"
        """
        match = re.search(r'<title[^>]*>(.*?)</title>', html_content, re.DOTALL | re.IGNORECASE)
        if match:
            title = match.group(1).strip()
            # Remove common suffixes
            for suffix in [" - Doomworld", " - WADs & Mods", " - Everything Else"]:
                if title.endswith(suffix):
                    title = title[:-len(suffix)]
            # Decode HTML entities
            title = html.unescape(title)
            return title.strip()
        return ""

    def _extract_first_post(self, html_content: str) -> str:
        """Extract HTML content of the first post.

        Invision Community uses data-role="commentContent" for post bodies.
        """
        # Try data-role attribute first (Invision Community 4.x)
        match = re.search(
            r'<div[^>]*data-role=["\']commentContent["\'][^>]*>(.*?)</div>\s*(?:<div[^>]*class=["\'][^"\']*ipsSigned|</article)',
            html_content,
            re.DOTALL | re.IGNORECASE,
        )
        if match:
            return match.group(1).strip()

        # Fallback: Look for ipsType_richText inside first article
        match = re.search(
            r'<article[^>]*>.*?<div[^>]*class=["\'][^"\']*ipsType_richText[^"\']*["\'][^>]*>(.*?)</div>',
            html_content,
            re.DOTALL | re.IGNORECASE,
        )
        if match:
            return match.group(1).strip()

        return ""

    def _html_to_text(self, html_content: str) -> str:
        """Convert HTML to plain text.

        Simple conversion that preserves paragraph breaks.
        """
        if not html_content:
            return ""

        text = html_content

        # Replace block elements with newlines
        text = re.sub(r'<br\s*/?\s*>', '\n', text, flags=re.IGNORECASE)
        text = re.sub(r'</p>', '\n\n', text, flags=re.IGNORECASE)
        text = re.sub(r'</div>', '\n', text, flags=re.IGNORECASE)
        text = re.sub(r'</li>', '\n', text, flags=re.IGNORECASE)

        # Remove all other tags
        text = re.sub(r'<[^>]+>', '', text)

        # Decode HTML entities
        text = html.unescape(text)

        # Normalize whitespace
        text = re.sub(r'\n\s*\n', '\n\n', text)
        text = re.sub(r' +', ' ', text)

        return text.strip()
