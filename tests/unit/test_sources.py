"""Tests for source adapters using respx to mock HTTP calls."""

import httpx
import respx

from caco.idgames.models import FileEntry
from caco.doomwiki.models import WikiEntry
from caco.doomworld.models import ForumThread


# =============================================================================
# IdgamesSource tests
# =============================================================================


class TestIdgamesSource:
    """Tests for IdgamesSource adapter."""

    @respx.mock
    def test_search(self):
        respx.get("https://www.doomworld.com/idgames/api/api.php").mock(
            return_value=httpx.Response(200, json={
                "content": {
                    "file": {
                        "id": 12345,
                        "title": "Scythe",
                        "dir": "levels/doom2/Ports/megawads/",
                        "filename": "scythe.zip",
                        "author": "Erik Alm",
                        "date": "2003-08-23",
                        "description": "A megawad",
                    }
                }
            })
        )

        from caco.sources.idgames import IdgamesSource
        with IdgamesSource() as src:
            results = src.search("scythe")

        assert len(results) == 1
        assert isinstance(results[0], FileEntry)
        assert results[0].title == "Scythe"
        assert results[0].author == "Erik Alm"

    @respx.mock
    def test_search_multiple_results(self):
        respx.get("https://www.doomworld.com/idgames/api/api.php").mock(
            return_value=httpx.Response(200, json={
                "content": {
                    "file": [
                        {"id": 1, "title": "Scythe", "author": "Erik Alm"},
                        {"id": 2, "title": "Scythe 2", "author": "Erik Alm"},
                    ]
                }
            })
        )

        from caco.sources.idgames import IdgamesSource
        with IdgamesSource() as src:
            results = src.search("scythe")

        assert len(results) == 2

    @respx.mock
    def test_get(self):
        respx.get("https://www.doomworld.com/idgames/api/api.php").mock(
            return_value=httpx.Response(200, json={
                "content": {
                    "id": 99,
                    "title": "Eviternity",
                    "author": "Dragonfly",
                    "filename": "eviternity.zip",
                    "dir": "levels/doom2/Ports/megawads/",
                    "date": "2018-12-10",
                }
            })
        )

        from caco.sources.idgames import IdgamesSource
        with IdgamesSource() as src:
            entry = src.get(99)

        assert isinstance(entry, FileEntry)
        assert entry.title == "Eviternity"
        assert entry.id == 99

    @respx.mock
    def test_import_wad(self, tmp_db):
        """Test that import_wad inserts into the database."""
        entry = FileEntry(
            id=42,
            title="Ancient Aliens",
            author="skillsaw",
            date="2016-05-16",
            description="32-level megawad",
            dir="levels/doom2/Ports/megawads/",
            filename="aaliens.zip",
            url="https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/aaliens",
        )

        from caco.sources.idgames import IdgamesSource
        from caco import db

        with IdgamesSource() as src:
            wad_id = src.import_wad(entry, tags=["megawad", "cacoward"])

        assert wad_id > 0

        wad = db.get_wad(wad_id)
        assert wad is not None
        assert wad["title"] == "Ancient Aliens"
        assert wad["author"] == "skillsaw"
        assert wad["source_type"] == "idgames"
        assert wad["source_id"] == "42"
        assert wad["filename"] == "aaliens.zip"
        assert "megawad" in wad["tags"]
        assert "cacoward" in wad["tags"]
        assert wad["year"] == 2016

    @respx.mock
    def test_search_error(self):
        """Test that API errors are raised properly."""
        respx.get("https://www.doomworld.com/idgames/api/api.php").mock(
            return_value=httpx.Response(200, json={
                "error": {"message": "No results found"}
            })
        )

        from caco.sources.idgames import IdgamesSource
        from caco.idgames.client import IdgamesError

        with IdgamesSource() as src:
            try:
                src.search("nonexistent_query_xyz")
                assert False, "Expected IdgamesError"
            except IdgamesError as e:
                assert "No results found" in str(e)

    @respx.mock
    def test_search_empty(self):
        """Test search with no results returns empty list."""
        # First call: title search returns empty
        # Second call: filename search also returns empty
        respx.get("https://www.doomworld.com/idgames/api/api.php").mock(
            return_value=httpx.Response(200, json={"content": {}})
        )

        from caco.sources.idgames import IdgamesSource
        with IdgamesSource() as src:
            results = src.search("nonexistent")

        assert results == []


# =============================================================================
# DoomwikiSource tests
# =============================================================================


class TestDoomwikiSource:
    """Tests for DoomwikiSource adapter."""

    WIKITEXT_WITH_WAD = """{{Wad
|name = Eviternity
|author = [[Dragonfly]] et al.
|iwad = Doom II
|year = 2018
|port = Limit-removing
|link = {{idgames|levels/doom2/Ports/megawads/eviternity.zip}}
}}

'''Eviternity''' is a 32-level megawad for Doom II."""

    @respx.mock
    def test_search(self):
        # Mock search API
        respx.get("https://doomwiki.org/w/api.php").mock(
            side_effect=[
                # First call: search
                httpx.Response(200, json={
                    "query": {
                        "search": [
                            {"pageid": 100, "title": "Eviternity", "snippet": "A megawad"},
                        ]
                    }
                }),
                # Second call: batch page fetch
                httpx.Response(200, json={
                    "query": {
                        "pages": {
                            "100": {
                                "title": "Eviternity",
                                "revisions": [{"*": self.WIKITEXT_WITH_WAD}],
                            }
                        }
                    }
                }),
            ]
        )

        from caco.sources.doomwiki import DoomwikiSource
        with DoomwikiSource() as src:
            results = src.search("eviternity")

        assert len(results) == 1
        assert isinstance(results[0], WikiEntry)
        assert results[0].name == "Eviternity"

    @respx.mock
    def test_get(self):
        respx.get("https://doomwiki.org/w/api.php").mock(
            return_value=httpx.Response(200, json={
                "query": {
                    "pages": {
                        "100": {
                            "title": "Eviternity",
                            "revisions": [{"*": self.WIKITEXT_WITH_WAD}],
                        }
                    }
                }
            })
        )

        from caco.sources.doomwiki import DoomwikiSource
        with DoomwikiSource() as src:
            entry = src.get("Eviternity")

        assert entry is not None
        assert isinstance(entry, WikiEntry)
        assert entry.page_id == 100
        assert entry.name == "Eviternity"
        assert entry.year == 2018

    @respx.mock
    def test_get_by_id(self):
        respx.get("https://doomwiki.org/w/api.php").mock(
            return_value=httpx.Response(200, json={
                "query": {
                    "pages": {
                        "200": {
                            "title": "Sunlust",
                            "revisions": [{"*": """{{Wad
|name = Sunlust
|author = [[Ribbiks]] & [[Dannebubinga]]
|year = 2015
|iwad = Doom II
}}
'''Sunlust''' is a megawad."""}],
                        }
                    }
                }
            })
        )

        from caco.sources.doomwiki import DoomwikiSource
        with DoomwikiSource() as src:
            entry = src.get_by_id(200)

        assert entry is not None
        assert entry.name == "Sunlust"
        assert entry.year == 2015

    @respx.mock
    def test_import_wad(self, tmp_db):
        entry = WikiEntry(
            page_id=100,
            title="Eviternity",
            name="Eviternity",
            author="Dragonfly",
            year=2018,
            wiki_url="https://doomwiki.org/wiki/Eviternity",
            description="A 32-level megawad",
        )

        from caco.sources.doomwiki import DoomwikiSource
        from caco import db

        with DoomwikiSource() as src:
            wad_id = src.import_wad(entry, tags=["megawad"])

        assert wad_id > 0

        wad = db.get_wad(wad_id)
        assert wad is not None
        assert wad["title"] == "Eviternity"
        assert wad["author"] == "Dragonfly"
        assert wad["source_type"] == "doomwiki"
        assert wad["source_id"] == "100"
        assert wad["source_url"] == "https://doomwiki.org/wiki/Eviternity"
        assert "megawad" in wad["tags"]

    @respx.mock
    def test_get_missing_page(self):
        respx.get("https://doomwiki.org/w/api.php").mock(
            return_value=httpx.Response(200, json={
                "query": {
                    "pages": {
                        "-1": {"title": "Nonexistent", "missing": ""}
                    }
                }
            })
        )

        from caco.sources.doomwiki import DoomwikiSource
        with DoomwikiSource() as src:
            entry = src.get("Nonexistent")

        assert entry is None


# =============================================================================
# DoomworldSource tests
# =============================================================================


FORUM_HTML = """<!DOCTYPE html>
<html>
<head>
<title>MyWad - Doomworld</title>
<script type="application/ld+json">
{
    "@type": "DiscussionForumPosting",
    "headline": "MyWad - A cool WAD",
    "author": {"name": "DoomMapper"},
    "datePublished": "2024-03-15T12:00:00Z"
}
</script>
</head>
<body>
<article>
<div class="ipsType_richText ipsContained" data-ipslazyload>
<p>Check out my new WAD! Download here: https://example.com/mywad.zip</p>
</div>
</article>
</body>
</html>
"""


class TestDoomworldSource:
    """Tests for DoomworldSource adapter."""

    @respx.mock
    def test_get(self):
        respx.get("https://www.doomworld.com/forum/topic/12345-mywad/").mock(
            return_value=httpx.Response(200, text=FORUM_HTML)
        )

        from caco.sources.doomworld import DoomworldSource
        with DoomworldSource() as src:
            thread = src.get("https://www.doomworld.com/forum/topic/12345-mywad/")

        assert thread is not None
        assert isinstance(thread, ForumThread)
        assert thread.thread_id == 12345
        assert thread.title == "MyWad - A cool WAD"
        assert thread.author == "DoomMapper"

    @respx.mock
    def test_get_by_id(self):
        respx.get("https://www.doomworld.com/forum/topic/99999/").mock(
            return_value=httpx.Response(200, text=FORUM_HTML)
        )

        from caco.sources.doomworld import DoomworldSource
        with DoomworldSource() as src:
            thread = src.get_by_id(99999)

        assert thread is not None
        assert isinstance(thread, ForumThread)

    @respx.mock
    def test_import_wad(self, tmp_db):
        thread = ForumThread(
            thread_id=12345,
            title="MyWad - A cool WAD",
            author="DoomMapper",
            posted_date="2024-03-15",
            first_post_text="Check out my new WAD!",
            thread_url="https://www.doomworld.com/forum/topic/12345-mywad/",
        )

        from caco.sources.doomworld import DoomworldSource
        from caco import db

        with DoomworldSource() as src:
            wad_id = src.import_wad(
                thread,
                tags=["newrelease"],
                title="MyWad",
                author="DoomMapper",
                year=2024,
                version="v1.0",
            )

        assert wad_id > 0

        wad = db.get_wad(wad_id)
        assert wad is not None
        assert wad["title"] == "MyWad"
        assert wad["author"] == "DoomMapper"
        assert wad["year"] == 2024
        assert wad["source_type"] == "doomworld"
        assert wad["source_id"] == "12345"
        assert wad["version"] == "v1.0"
        assert "newrelease" in wad["tags"]

    @respx.mock
    def test_get_bad_url(self):
        """Test that a non-Doomworld URL raises DoomworldError.

        DoomworldError is a business-level error (invalid URL) that propagates
        through the source adapter, unlike transport errors (httpx.HTTPError)
        which are caught and return None.
        """
        import pytest
        from caco.doomworld.client import DoomworldError
        from caco.sources.doomworld import DoomworldSource

        with DoomworldSource() as src:
            with pytest.raises(DoomworldError, match="Invalid Doomworld forum URL"):
                src.get("https://example.com/not-a-forum")

    @respx.mock
    def test_get_network_error(self):
        """Test that a network-level error raises DoomworldError.

        DoomworldClient.get_thread() catches httpx.RequestError and wraps it
        in DoomworldError, which propagates through the source adapter since
        DoomworldSource.get() only catches httpx.HTTPError/ValueError/KeyError.
        """
        respx.get("https://www.doomworld.com/forum/topic/99999-gone/").mock(
            side_effect=httpx.ConnectError("Connection refused")
        )

        import pytest
        from caco.doomworld.client import DoomworldError
        from caco.sources.doomworld import DoomworldSource

        with DoomworldSource() as src:
            with pytest.raises(DoomworldError, match="Request error"):
                src.get("https://www.doomworld.com/forum/topic/99999-gone/")

    @respx.mock
    def test_import_with_defaults(self, tmp_db):
        """Test import_wad uses thread metadata when no overrides provided."""
        thread = ForumThread(
            thread_id=555,
            title="Thread Title WAD",
            author="ThreadAuthor",
            posted_date="2023-06-01",
            first_post_text="A description of the WAD",
            thread_url="https://www.doomworld.com/forum/topic/555-wad/",
        )

        from caco.sources.doomworld import DoomworldSource
        from caco import db

        with DoomworldSource() as src:
            wad_id = src.import_wad(thread)

        wad = db.get_wad(wad_id)
        assert wad["title"] == "Thread Title WAD"
        assert wad["author"] == "ThreadAuthor"
        assert wad["year"] == 2023
