"""Integration tests for external service reachability.

Run with: uv run pytest tests/test_reachability.py -v
Skip in CI: these are marked @pytest.mark.network
"""

import pytest
import httpx

pytestmark = pytest.mark.network


# -- idgames API --

class TestIdgamesReachability:
    """Test that the idgames API on doomworld.com is reachable."""

    API_URL = "https://www.doomworld.com/idgames/api/api.php"

    def test_api_ping(self):
        """idgames API responds to ping action."""
        from caco.idgames import IdgamesClient

        with IdgamesClient() as client:
            status = client.ping()
            assert status == "true"

    def test_api_search(self):
        """idgames search returns results for a known WAD."""
        from caco.idgames import IdgamesClient

        with IdgamesClient() as client:
            results = client.search("scythe", type="title")
            assert len(results) > 0
            assert any("scythe" in r.title.lower() for r in results)

    def test_download_mirrors(self):
        """At least one idgames download mirror is reachable."""
        from caco.idgames.client import MIRRORS

        reachable = []
        for mirror in MIRRORS:
            try:
                r = httpx.head(mirror, timeout=10, follow_redirects=True)
                if r.status_code < 400:
                    reachable.append(mirror)
            except httpx.HTTPError:
                continue

        assert reachable, f"No idgames mirrors reachable (tested {len(MIRRORS)})"


# -- Doom Wiki --

class TestDoomwikiReachability:
    """Test that the Doom Wiki MediaWiki API is reachable."""

    def test_api_search(self):
        """Doom Wiki search returns results."""
        from caco.doomwiki import DoomwikiClient

        with DoomwikiClient() as client:
            results = client.search("Eviternity", limit=5)
            assert len(results) > 0

    def test_api_get_page(self):
        """Doom Wiki can retrieve page content for a known WAD."""
        from caco.doomwiki import DoomwikiClient

        with DoomwikiClient() as client:
            result = client.get_page_content("Eviternity")
            assert result is not None
            page_id, wikitext = result
            assert page_id > 0
            assert "{{wad" in wikitext.lower() or "{{Wad" in wikitext

    def test_api_get_entry(self):
        """Doom Wiki parses a WAD entry with expected fields."""
        from caco.doomwiki import DoomwikiClient

        with DoomwikiClient() as client:
            entry = client.get_entry("Eviternity")
            assert entry is not None
            assert entry.name or entry.title
            assert entry.author


# -- Doomworld forums --

class TestDoomworldReachability:
    """Test that Doomworld forums are reachable."""

    KNOWN_THREAD = "https://www.doomworld.com/forum/topic/132598-myolden/"

    def test_forum_thread_fetch(self):
        """Can fetch a known Doomworld forum thread page."""
        r = httpx.get(
            self.KNOWN_THREAD,
            timeout=15,
            follow_redirects=True,
            headers={"User-Agent": "Caco/1.0 (Doom WAD library manager)"},
        )
        # Accept 200 (success) — 403 means Cloudflare challenge
        assert r.status_code == 200, (
            f"Doomworld forums returned {r.status_code}"
            + (" (Cloudflare challenge)" if r.headers.get("cf-mitigated") == "challenge" else "")
        )
