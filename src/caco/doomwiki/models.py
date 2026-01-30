"""Pydantic models for Doomwiki data."""

from pydantic import BaseModel, Field, field_validator


def _coerce_str(v):
    """Coerce None to empty string."""
    return "" if v is None else v


class SearchResult(BaseModel):
    """A search result from MediaWiki search API."""

    page_id: int = Field(alias="pageid")
    title: str = ""
    snippet: str = ""

    @field_validator("title", "snippet", mode="before")
    @classmethod
    def coerce_str(cls, v):
        return _coerce_str(v)


class WikiEntry(BaseModel):
    """Parsed WAD data from a Doom Wiki page.

    This model represents the structured metadata extracted from a wiki page,
    primarily from the {{Wad}} infobox template.
    """

    page_id: int
    title: str  # Wiki page title
    name: str = ""  # Name from infobox (may differ from page title)
    author: str = ""
    year: int | None = None
    iwad: str = ""  # Required IWAD (e.g., "Doom II", "Ultimate Doom")
    port: str = ""  # Required source port (e.g., "Limit-removing", "GZDoom")
    link: str = ""  # Download URL (often idgames)
    description: str = ""  # First paragraph of wiki page
    wiki_url: str = ""  # URL to the wiki page

    @field_validator("title", "name", "author", "iwad", "port", "link", "description", "wiki_url", mode="before")
    @classmethod
    def coerce_str(cls, v):
        return _coerce_str(v)

    @property
    def display_name(self) -> str:
        """Return the best available name for display."""
        return self.name or self.title
