"""Tests for Pydantic model validation."""

import pytest

from caco.idgames.models import FileEntry, Review
from caco.doomwiki.models import WikiEntry, SearchResult
from caco.doomworld.models import ForumThread


class TestFileEntry:
    def test_minimal(self):
        entry = FileEntry(id=123)
        assert entry.id == 123
        assert entry.title == ""
        assert entry.author == ""

    def test_coerce_none_to_str(self):
        entry = FileEntry(id=1, title=None, author=None)
        assert entry.title == ""
        assert entry.author == ""

    def test_coerce_none_rating(self):
        entry = FileEntry(id=1, rating=None)
        assert entry.rating == 0.0

    def test_full_entry(self):
        entry = FileEntry(
            id=19509,
            title="Scythe 2",
            author="Erik Alm",
            filename="scythe2.zip",
            date="2005-06-17",
            rating=4.5,
        )
        assert entry.title == "Scythe 2"
        assert entry.rating == 4.5


class TestReview:
    def test_coerce_none(self):
        review = Review(text=None, username=None)
        assert review.text == ""
        assert review.username == ""


class TestWikiEntry:
    def test_minimal(self):
        entry = WikiEntry(page_id=100, title="Scythe")
        assert entry.page_id == 100
        assert entry.display_name == "Scythe"

    def test_display_name_prefers_name(self):
        entry = WikiEntry(page_id=100, title="Page Title", name="WAD Name")
        assert entry.display_name == "WAD Name"

    def test_coerce_none(self):
        entry = WikiEntry(page_id=1, title="T", author=None, iwad=None)
        assert entry.author == ""
        assert entry.iwad == ""


class TestSearchResult:
    def test_from_alias(self):
        result = SearchResult(pageid=42, title="Test")
        assert result.page_id == 42


class TestForumThread:
    def test_minimal(self):
        thread = ForumThread(thread_id=12345, title="My WAD")
        assert thread.thread_id == 12345
        assert thread.display_name == "My WAD"

    def test_coerce_none_fields(self):
        thread = ForumThread(thread_id=1, title="T", author=None, posted_date=None)
        assert thread.author == ""
        assert thread.posted_date == ""

    def test_coerce_none_download_links(self):
        thread = ForumThread(thread_id=1, title="T", download_links=None)
        assert thread.download_links == []

    def test_has_technical_info_false(self):
        thread = ForumThread(thread_id=1, title="T")
        assert thread.has_technical_info is False

    def test_has_technical_info_true(self):
        thread = ForumThread(thread_id=1, title="T", iwad="doom2")
        assert thread.has_technical_info is True
