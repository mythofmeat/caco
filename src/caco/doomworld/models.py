"""Pydantic models for Doomworld forum data."""

from pydantic import BaseModel, field_validator

from caco.utils import coerce_str as _coerce_str


class ForumThread(BaseModel):
    """Parsed data from a Doomworld forum thread.

    This model represents the structured metadata extracted from a forum thread,
    primarily from JSON-LD structured data and HTML content.
    """

    thread_id: int  # Extracted from URL: /forum/topic/{id}-{slug}/
    title: str  # Thread title
    author: str = ""  # OP username
    posted_date: str = ""  # ISO date string
    first_post_html: str = ""  # HTML content of first post
    first_post_text: str = ""  # Plain text of first post (stripped HTML)
    thread_url: str = ""  # Full URL to the thread

    # Phase 2: Enhanced metadata extracted from post content
    download_links: list[str] = []  # URLs to download files
    complevel: int | None = None  # Compatibility level (e.g., 9 for Boom)
    iwad: str | None = None  # Required IWAD (e.g., "doom2", "plutonia")
    sourceport: str | None = None  # Required sourceport (e.g., "gzdoom", "dsda-doom")

    @field_validator(
        "title", "author", "posted_date", "first_post_html", "first_post_text", "thread_url",
        mode="before"
    )
    @classmethod
    def coerce_str(cls, v):
        return _coerce_str(v)

    @field_validator("download_links", mode="before")
    @classmethod
    def coerce_list(cls, v):
        return v if v is not None else []

    @property
    def display_name(self) -> str:
        """Return the best available name for display."""
        return self.title

    @property
    def has_technical_info(self) -> bool:
        """Check if any technical metadata was extracted."""
        return bool(self.download_links or self.complevel is not None
                    or self.iwad or self.sourceport)
