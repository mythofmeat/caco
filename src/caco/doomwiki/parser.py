"""Wikitext parser for Doom Wiki pages."""

import re
from typing import Any


class WikitextParser:
    """Parser for extracting structured data from Doom Wiki wikitext."""

    # Pattern to match {{ig|file=...}} idgames template
    IDGAMES_PATTERN = re.compile(r"\{\{ig\s*\|\s*file\s*=\s*([^}|]+)", re.IGNORECASE)

    # Pattern to remove wiki markup
    LINK_PATTERN = re.compile(r"\[\[(?:[^|\]]*\|)?([^\]]+)\]\]")  # [[link|text]] or [[text]]
    TEMPLATE_PATTERN = re.compile(r"\{\{[^{}]*\}\}")  # {{template}} (non-nested only)
    HTML_TAG_PATTERN = re.compile(r"<[^>]+>")  # <tag>
    REF_PATTERN = re.compile(r"<ref[^>]*>.*?</ref>", re.DOTALL | re.IGNORECASE)  # <ref>...</ref>

    def parse(self, wikitext: str, page_title: str, page_id: int) -> dict[str, Any]:
        """
        Parse wikitext to extract WAD metadata.

        Args:
            wikitext: Raw wikitext content
            page_title: Title of the wiki page
            page_id: MediaWiki page ID

        Returns:
            Dictionary with parsed fields matching WikiEntry model
        """
        result = {
            "page_id": page_id,
            "title": page_title,
            "name": "",
            "author": "",
            "year": None,
            "iwad": "",
            "port": "",
            "link": "",
            "description": "",
            "wiki_url": f"https://doomwiki.org/wiki/{page_title.replace(' ', '_')}",
        }

        # Extract {{wad}} template content using brace matching
        template_content = self._extract_wad_template(wikitext)
        if template_content:
            params = self._parse_template_params(template_content)

            # Map template params to our fields
            result["name"] = self._clean_value(params.get("name", params.get("title", "")))
            result["author"] = self._clean_value(
                params.get("author", params.get("authors", ""))
            )
            result["iwad"] = self._clean_value(
                params.get("iwad", params.get("iwad2", ""))
            )
            result["port"] = self._clean_value(
                params.get("port", params.get("port2", ""))
            )

            # Parse year from various possible fields
            year_str = params.get("year", "")
            if year_str:
                result["year"] = self._parse_year(year_str)

            # Parse link - handle {{ig|file=...}} template
            link = params.get("link", "")
            result["link"] = self._parse_link(link)

        # Extract first paragraph as description
        result["description"] = self._extract_first_paragraph(wikitext)

        return result

    def _extract_wad_template(self, wikitext: str) -> str | None:
        """Extract the content of the {{wad}} template using brace matching.

        Returns the content between {{wad| and the matching }}, or None if not found.
        """
        # Find {{wad (case insensitive)
        lower = wikitext.lower()
        start = lower.find("{{wad")
        if start == -1:
            return None

        # Find the first | after {{wad
        pipe_pos = wikitext.find("|", start)
        if pipe_pos == -1:
            return None

        # Now count braces to find the matching }}
        content_start = pipe_pos + 1
        brace_count = 2  # We're inside {{ already
        i = content_start

        while i < len(wikitext) and brace_count > 0:
            if wikitext[i:i+2] == "{{":
                brace_count += 2
                i += 2
            elif wikitext[i:i+2] == "}}":
                brace_count -= 2
                if brace_count == 0:
                    return wikitext[content_start:i]
                i += 2
            else:
                i += 1

        return None

    def _parse_template_params(self, template_content: str) -> dict[str, str]:
        """Parse template parameters from the content inside {{Wad|...}}."""
        params = {}

        # Split by | but handle nested templates
        current_param = ""
        nesting = 0
        parts = []

        for char in template_content:
            if char == "{":
                nesting += 1
                current_param += char
            elif char == "}":
                nesting -= 1
                current_param += char
            elif char == "|" and nesting == 0:
                if current_param.strip():
                    parts.append(current_param.strip())
                current_param = ""
            else:
                current_param += char

        if current_param.strip():
            parts.append(current_param.strip())

        # Parse each part as name=value
        for part in parts:
            if "=" in part:
                name, _, value = part.partition("=")
                params[name.strip().lower()] = value.strip()

        return params

    def _clean_value(self, value: str) -> str:
        """Remove wiki markup from a value."""
        if not value:
            return ""

        # Remove <ref>...</ref> tags first
        value = self.REF_PATTERN.sub("", value)

        # Remove HTML tags
        value = self.HTML_TAG_PATTERN.sub("", value)

        # Convert [[link|text]] to text, [[text]] to text
        value = self.LINK_PATTERN.sub(r"\1", value)

        # Remove remaining templates (but preserve some text)
        value = self.TEMPLATE_PATTERN.sub("", value)

        # Remove bold/italic wiki markup
        value = value.replace("'''", "").replace("''", "")

        # Clean up whitespace
        value = " ".join(value.split())

        return value.strip()

    def _parse_year(self, year_str: str) -> int | None:
        """Extract a year from a string."""
        # Clean the value first
        cleaned = self._clean_value(year_str)

        # Try to find a 4-digit year
        match = re.search(r"\b(19|20)\d{2}\b", cleaned)
        if match:
            try:
                return int(match.group(0))
            except ValueError:
                pass
        return None

    def _parse_link(self, link: str) -> str:
        """Parse the link field, converting idgames templates to URLs."""
        if not link:
            return ""

        # Check for {{ig|file=...}} template
        ig_match = self.IDGAMES_PATTERN.search(link)
        if ig_match:
            idgames_path = ig_match.group(1).strip()
            return f"https://www.doomworld.com/idgames/{idgames_path}"

        # Check for direct URL
        if link.startswith(("http://", "https://")):
            return link

        # Check for plain [[link]] or external link
        cleaned = self._clean_value(link)
        if cleaned.startswith(("http://", "https://")):
            return cleaned

        return ""

    def _extract_first_paragraph(self, wikitext: str) -> str:
        """Extract the first paragraph of content from wikitext."""
        # Remove templates at the start
        text = wikitext

        # Find content after any infobox templates
        # Skip past {{Wad}}, {{Navbox}}, etc. at the start
        lines = []
        in_template = 0
        started = False

        for line in text.split("\n"):
            # Track template nesting
            in_template += line.count("{{") - line.count("}}")

            # Skip lines that are part of templates
            if in_template > 0:
                continue

            # Skip empty lines before content starts
            stripped = line.strip()
            if not stripped:
                if started:
                    break  # End of first paragraph
                continue

            # Skip headers, categories, and special lines
            if stripped.startswith(("=", "[[Category:", "__", "{|", "|")):
                continue

            # Skip lines that look like template remnants
            if stripped.startswith("}}"):
                continue

            started = True
            lines.append(stripped)

            # Limit to reasonable length
            if len(" ".join(lines)) > 500:
                break

        paragraph = " ".join(lines)

        # Clean wiki markup from the paragraph
        paragraph = self._clean_value(paragraph)

        # Truncate if too long
        if len(paragraph) > 500:
            paragraph = paragraph[:497] + "..."

        return paragraph

    def has_wad_template(self, wikitext: str) -> bool:
        """Check if the wikitext contains a {{wad}} template (case insensitive)."""
        return "{{wad" in wikitext.lower()
