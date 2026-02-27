"""Centralized import service — single source of truth for duplicate checking and WAD import.

Replaces ~15 duplicate-check-and-import blocks across CLI, TUI, and GUI with one call site.
Each UI layer calls ImportService methods and interprets the ImportResult.
"""

from __future__ import annotations

import logging
import re
import unicodedata
from dataclasses import dataclass
from pathlib import Path

from caco import db
from caco.db import SourceType

logger = logging.getLogger(__name__)


@dataclass
class ImportResult:
    """Result of an import attempt.

    Callers check `is_duplicate` first. If True and `force` was not set,
    the import was skipped — `duplicate_id` and `duplicate_title` describe the existing entry.
    Otherwise `wad_id` contains the newly created WAD ID.
    """
    wad_id: int | None = None
    is_duplicate: bool = False
    duplicate_id: int | None = None
    duplicate_title: str | None = None
    error: str | None = None

    @property
    def ok(self) -> bool:
        return self.wad_id is not None and self.error is None


def normalize_tags(tags: str | list | tuple | None) -> list[str] | None:
    """Normalize tags from any input format to a clean list.

    Accepts comma-separated string, list, tuple, or None.
    Strips whitespace, lowercases, and removes empty entries.
    """
    if tags is None:
        return None
    if isinstance(tags, str):
        parts = [t.strip().lower() for t in tags.split(",") if t.strip()]
        return parts if parts else None
    items = [str(t).strip().lower() for t in tags if str(t).strip()]
    return items if items else None


def _normalize_title(title: str) -> str:
    """Normalize a title for fuzzy comparison.

    Lowercase, strip accents/diacritics, remove punctuation, collapse whitespace.
    """
    title = title.lower()
    # Decompose unicode and strip combining marks (accents)
    title = unicodedata.normalize("NFD", title)
    title = "".join(c for c in title if unicodedata.category(c) != "Mn")
    # Remove punctuation (keep alphanumeric and spaces)
    title = re.sub(r"[^a-z0-9\s]", "", title)
    # Collapse whitespace
    title = re.sub(r"\s+", " ", title).strip()
    return title


def _titles_match(a: str, b: str) -> bool:
    """Check if two titles match after normalization."""
    return _normalize_title(a) == _normalize_title(b)


class ImportService:
    """Handles duplicate checking and WAD import for all source types.

    Usage:
        svc = ImportService()
        result = svc.import_idgames(entry, tags=["cacoward"])
        if result.is_duplicate:
            # Show duplicate warning using result.duplicate_title/duplicate_id
        elif result.ok:
            # Success — result.wad_id is the new WAD ID
        else:
            # Error — result.error has the message
    """

    @staticmethod
    def _auto_link_iwad(wad_id: int, iwad_text: str) -> None:
        """Auto-set custom_iwad on a WAD if the IWAD name is registered.

        Called after Doom Wiki imports when the entry has an iwad field.
        """
        from caco.db._iwads import normalize_iwad_name, get_iwad

        short_name = normalize_iwad_name(iwad_text)
        if not short_name:
            return

        # Only set if the IWAD is registered in the database
        if not get_iwad(short_name):
            return

        # Only set if the WAD doesn't already have a custom_iwad
        wad = db.get_wad(wad_id)
        if wad and not wad.get("custom_iwad"):
            db.update_wad(wad_id, custom_iwad=short_name)

    def _auto_enrich_doomwiki(self, wad_id: int, title: str) -> None:
        """Auto-enrich a WAD with Doom Wiki metadata after import.

        Searches Doom Wiki for a matching title and backfills missing fields.
        Never overwrites existing author/year/custom_iwad.
        Appends wiki description to existing description with separator.
        Silently ignores all errors.
        """
        try:
            from caco.config import get_auto_doomwiki_enrich

            if not get_auto_doomwiki_enrich():
                return

            from caco.doomwiki import DoomwikiClient

            client = DoomwikiClient()
            results = client.search_wads(title, limit=5)
            if not results:
                return

            # Find first result with matching title
            entry = None
            for r in results:
                if _titles_match(title, r.display_name):
                    entry = r
                    break
            if entry is None:
                return

            wad = db.get_wad(wad_id)
            if not wad:
                return

            updates: dict = {}

            # Fill missing fields (never overwrite)
            if not wad.get("author") and entry.author:
                updates["author"] = entry.author
            if not wad.get("year") and entry.year:
                updates["year"] = entry.year

            # Append wiki description
            if entry.description:
                existing = wad.get("description") or ""
                separator = "\n\n---\nFrom Doom Wiki:\n"
                if existing:
                    updates["description"] = existing + separator + entry.description
                else:
                    updates["description"] = entry.description

            if updates:
                db.update_wad(wad_id, **updates)

            # Auto-link IWAD if wiki entry has one
            if entry.iwad:
                self._auto_link_iwad(wad_id, entry.iwad)

        except Exception:
            logger.debug("Auto-enrich from Doom Wiki failed for WAD %d (%s)", wad_id, title, exc_info=True)

    def import_idgames(
        self,
        entry,  # idgames.FileEntry
        *,
        tags: list[str] | None = None,
        force: bool = False,
    ) -> ImportResult:
        """Import from idgames archive.

        Duplicate detection: source_id + filename + author.
        """
        existing = db.find_duplicate(
            source_type=SourceType.IDGAMES,
            source_id=str(entry.id),
            filename=entry.filename,
            author=entry.author,
        )
        if existing and not force:
            return ImportResult(
                is_duplicate=True,
                duplicate_id=existing["id"],
                duplicate_title=existing["title"],
            )

        try:
            from caco.sources.idgames import IdgamesSource
            with IdgamesSource() as source:
                wad_id = source.import_wad(entry, tags=tags)
        except Exception as e:
            return ImportResult(error=str(e))

        self._auto_enrich_doomwiki(wad_id, entry.title)
        return ImportResult(wad_id=wad_id)

    def import_doomwiki(
        self,
        entry,  # doomwiki.WikiEntry
        *,
        tags: list[str] | None = None,
        force: bool = False,
    ) -> ImportResult:
        """Import from Doom Wiki.

        Duplicate detection: source_id (page_id).
        After import, auto-links to a registered IWAD if the entry has an iwad field.
        """
        existing = db.find_duplicate(
            source_type=SourceType.DOOMWIKI,
            source_id=str(entry.page_id),
        )
        if existing and not force:
            return ImportResult(
                is_duplicate=True,
                duplicate_id=existing["id"],
                duplicate_title=existing["title"],
            )

        try:
            from caco.sources.doomwiki import DoomwikiSource
            with DoomwikiSource() as source:
                wad_id = source.import_wad(entry, tags=tags)

            # Auto-link to registered IWAD if entry has an iwad field
            if wad_id and getattr(entry, "iwad", ""):
                self._auto_link_iwad(wad_id, entry.iwad)

            return ImportResult(wad_id=wad_id)
        except Exception as e:
            return ImportResult(error=str(e))

    def import_doomworld(
        self,
        thread,  # doomworld.ForumThread
        *,
        tags: list[str] | None = None,
        title: str | None = None,
        author: str | None = None,
        year: int | None = None,
        version: str | None = None,
        force: bool = False,
    ) -> ImportResult:
        """Import from Doomworld forum thread.

        Duplicate detection: source_id (thread_id).
        """
        existing = db.find_duplicate(
            source_type=SourceType.DOOMWORLD,
            source_id=str(thread.thread_id),
        )
        if existing and not force:
            return ImportResult(
                is_duplicate=True,
                duplicate_id=existing["id"],
                duplicate_title=existing["title"],
            )

        try:
            from caco.sources.doomworld import DoomworldSource
            with DoomworldSource() as source:
                wad_id = source.import_wad(
                    thread, tags=tags, title=title, author=author,
                    year=year, version=version,
                )
        except Exception as e:
            return ImportResult(error=str(e))

        wad_title = title or thread.title
        self._auto_enrich_doomwiki(wad_id, wad_title)
        return ImportResult(wad_id=wad_id)

    def import_url(
        self,
        title: str,
        url: str,
        *,
        author: str | None = None,
        year: int | None = None,
        description: str | None = None,
        tags: list[str] | None = None,
        force: bool = False,
    ) -> ImportResult:
        """Import from a direct URL.

        Duplicate detection: source_url.
        """
        existing = db.find_duplicate(
            source_type=SourceType.URL,
            source_url=url,
        )
        if existing and not force:
            return ImportResult(
                is_duplicate=True,
                duplicate_id=existing["id"],
                duplicate_title=existing["title"],
            )

        try:
            wad_id = db.add_wad(
                title=title,
                source_type=SourceType.URL,
                source_url=url,
                author=author,
                year=year,
                description=description,
                tags=tags,
            )
        except Exception as e:
            return ImportResult(error=str(e))

        self._auto_enrich_doomwiki(wad_id, title)
        return ImportResult(wad_id=wad_id)

    def import_local(
        self,
        title: str,
        path: str | Path,
        *,
        author: str | None = None,
        year: int | None = None,
        description: str | None = None,
        tags: list[str] | None = None,
        force: bool = False,
    ) -> ImportResult:
        """Import a local file.

        Duplicate detection: source_url (the resolved file path).
        """
        resolved = Path(path).expanduser().resolve()
        source_url = str(resolved)

        existing = db.find_duplicate(
            source_type=SourceType.LOCAL,
            source_url=source_url,
        )
        if existing and not force:
            return ImportResult(
                is_duplicate=True,
                duplicate_id=existing["id"],
                duplicate_title=existing["title"],
            )

        filename = resolved.name if resolved.suffix else None
        cached_path = str(resolved) if resolved.exists() else None

        try:
            wad_id = db.add_wad(
                title=title,
                source_type=SourceType.LOCAL,
                source_url=source_url,
                filename=filename,
                cached_path=cached_path,
                author=author,
                year=year,
                description=description,
                tags=tags,
            )
        except Exception as e:
            return ImportResult(error=str(e))

        self._auto_enrich_doomwiki(wad_id, title)
        return ImportResult(wad_id=wad_id)
