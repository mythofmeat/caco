"""Tests for wikitext and Doomworld parsers."""

import pytest

from caco.doomwiki.parser import WikitextParser
from caco.doomworld.parser import (
    extract_complevel,
    extract_iwad,
    extract_sourceport,
    extract_download_links,
    DoomworldParser,
)


# =============================================================================
# WikitextParser
# =============================================================================


class TestWikitextParserExtract:
    """Test _extract_wad_template with various wikitext shapes."""

    def setup_method(self):
        self.parser = WikitextParser()

    def test_simple_template(self):
        wikitext = "{{Wad|name=Eviternity|author=Dragonfly|year=2018}}"
        result = self.parser._extract_wad_template(wikitext)
        assert result is not None
        assert "name=Eviternity" in result

    def test_no_template(self):
        wikitext = "This page has no wad template."
        assert self.parser._extract_wad_template(wikitext) is None

    def test_nested_templates(self):
        wikitext = "{{Wad|name=Test|link={{ig|file=levels/doom2/a-c/test.zip}}}}"
        result = self.parser._extract_wad_template(wikitext)
        assert result is not None
        assert "{{ig|file=" in result

    def test_case_insensitive(self):
        wikitext = "{{wad|name=Lower Case}}"
        result = self.parser._extract_wad_template(wikitext)
        assert result is not None
        assert "name=Lower Case" in result

    def test_has_wad_template(self):
        assert self.parser.has_wad_template("{{wad|name=foo}}")
        assert self.parser.has_wad_template("Text before {{Wad|name=bar}} text after")
        assert not self.parser.has_wad_template("No template here")


class TestWikitextParserClean:
    """Test _clean_value wiki markup removal."""

    def setup_method(self):
        self.parser = WikitextParser()

    def test_plain_text(self):
        assert self.parser._clean_value("Hello World") == "Hello World"

    def test_empty(self):
        assert self.parser._clean_value("") == ""

    def test_wiki_link(self):
        assert self.parser._clean_value("[[Doom II]]") == "Doom II"

    def test_piped_link(self):
        assert self.parser._clean_value("[[Doom (game)|Doom]]") == "Doom"

    def test_bold_italic(self):
        assert self.parser._clean_value("'''bold''' and ''italic''") == "bold and italic"

    def test_ref_removal(self):
        text = "Author name<ref>Citation needed</ref>"
        assert self.parser._clean_value(text) == "Author name"

    def test_html_tags(self):
        # _clean_value normalizes whitespace after tag removal
        assert self.parser._clean_value("Text <br/> more") == "Text more"

    def test_template_removal(self):
        assert self.parser._clean_value("Text {{nowrap|inline}} end") == "Text end"


class TestWikitextParserParse:
    """Test full parse() pipeline."""

    def setup_method(self):
        self.parser = WikitextParser()

    def test_full_parse(self):
        wikitext = (
            "{{Wad\n"
            "| name = Eviternity\n"
            "| author = [[Dragonfly]]\n"
            "| year = 2018\n"
            "| iwad = [[Doom II]]\n"
            "| port = [[Boom]]\n"
            "| link = {{ig|file=levels/doom2/Ports/megawads/eviternity.zip}}\n"
            "}}\n\n"
            "'''Eviternity''' is a 32-level megawad for [[Doom II]]."
        )
        result = self.parser.parse(wikitext, "Eviternity", page_id=12345)

        assert result["page_id"] == 12345
        assert result["name"] == "Eviternity"
        assert result["author"] == "Dragonfly"
        assert result["year"] == 2018
        assert result["iwad"] == "Doom II"
        assert "doomworld.com/idgames/" in result["link"]
        assert "megawad" in result["description"].lower()

    def test_parse_no_template(self):
        wikitext = "Just a plain page with no infobox."
        result = self.parser.parse(wikitext, "Plain Page", page_id=1)
        assert result["title"] == "Plain Page"
        assert result["name"] == ""
        assert result["author"] == ""

    def test_parse_year_extraction(self):
        wikitext = "{{Wad|name=Test|year=December 10, 2019}}"
        result = self.parser.parse(wikitext, "Test", page_id=1)
        assert result["year"] == 2019


class TestWikitextParserLink:
    """Test _parse_link with various link formats."""

    def setup_method(self):
        self.parser = WikitextParser()

    def test_idgames_template(self):
        link = "{{ig|file=levels/doom2/a-c/btsx_e1.zip}}"
        result = self.parser._parse_link(link)
        assert result == "https://www.doomworld.com/idgames/levels/doom2/a-c/btsx_e1.zip"

    def test_direct_url(self):
        url = "https://example.com/wad.zip"
        assert self.parser._parse_link(url) == url

    def test_empty(self):
        assert self.parser._parse_link("") == ""


# =============================================================================
# Doomworld Parser — extract_complevel
# =============================================================================


class TestExtractComplevel:
    """Test complevel extraction from forum post text."""

    def test_explicit_complevel(self):
        assert extract_complevel("This map requires complevel 9") == 9

    def test_cl_shorthand(self):
        assert extract_complevel("Tested with cl21") == 21

    def test_dash_complevel_flag(self):
        assert extract_complevel("Run with -complevel 11") == 11

    def test_boom_compatible(self):
        assert extract_complevel("Boom compatible mapset") == 9

    def test_mbf21(self):
        assert extract_complevel("Built for MBF21") == 21

    def test_vanilla(self):
        assert extract_complevel("Vanilla compatible") == 2

    def test_no_match(self):
        assert extract_complevel("Just a regular description") is None


# =============================================================================
# Doomworld Parser — extract_iwad
# =============================================================================


class TestExtractIwad:
    """Test IWAD extraction from forum post text."""

    def test_doom2(self):
        assert extract_iwad("A megawad for Doom II") == "doom2"

    def test_doom2_wad(self):
        assert extract_iwad("Requires doom2.wad") == "doom2"

    def test_plutonia(self):
        assert extract_iwad("For the Plutonia Experiment") == "plutonia"

    def test_ultimate_doom(self):
        assert extract_iwad("Requires Ultimate Doom") == "doom"

    def test_heretic(self):
        assert extract_iwad("A heretic map") == "heretic"

    def test_no_match(self):
        assert extract_iwad("A great set of maps") is None


# =============================================================================
# Doomworld Parser — extract_sourceport
# =============================================================================


class TestExtractSourceport:
    """Test sourceport extraction from forum post text."""

    def test_gzdoom(self):
        assert extract_sourceport("Requires GZDoom") == "gzdoom"

    def test_dsda_doom(self):
        assert extract_sourceport("Tested in DSDA-Doom") == "dsda-doom"

    def test_eternity(self):
        assert extract_sourceport("For the Eternity Engine") == "eternity"

    def test_no_match(self):
        assert extract_sourceport("A standalone game") is None


# =============================================================================
# Doomworld Parser — extract_download_links
# =============================================================================


class TestExtractDownloadLinks:
    """Test download link extraction from HTML/text content."""

    def test_direct_zip(self):
        text = "Download here: https://example.com/mymap.zip"
        links = extract_download_links(text)
        assert len(links) == 1
        assert links[0] == "https://example.com/mymap.zip"

    def test_dropbox_link(self):
        text = 'Get it at <a href="https://www.dropbox.com/s/abc123/map.zip">Dropbox</a>'
        links = extract_download_links(text)
        assert any("dropbox.com" in l for l in links)

    def test_idgames_link(self):
        text = "https://www.doomworld.com/idgames/levels/doom2/a-c/btsx_e1"
        links = extract_download_links(text)
        assert len(links) == 1

    def test_deduplication(self):
        text = (
            "https://example.com/map.wad and again "
            "https://example.com/map.wad"
        )
        links = extract_download_links(text)
        assert len(links) == 1

    def test_no_links(self):
        assert extract_download_links("No download links here") == []

    def test_github_release(self):
        text = "https://github.com/user/repo/releases/download/v1.0/map.zip"
        links = extract_download_links(text)
        assert len(links) == 1

    def test_trailing_punctuation_stripped(self):
        text = "Download at https://example.com/map.zip."
        links = extract_download_links(text)
        assert links[0] == "https://example.com/map.zip"


# =============================================================================
# DoomworldParser class
# =============================================================================


class TestDoomworldParser:
    """Test the main DoomworldParser class."""

    def setup_method(self):
        self.parser = DoomworldParser()

    def test_extract_thread_id_standard(self):
        url = "https://www.doomworld.com/forum/topic/134292-myhousewad/"
        assert self.parser._extract_thread_id(url) == 134292

    def test_extract_thread_id_with_page(self):
        url = "https://www.doomworld.com/forum/topic/134292-myhousewad/?page=5"
        assert self.parser._extract_thread_id(url) == 134292

    def test_extract_thread_id_vbulletin(self):
        url = "https://www.doomworld.com/vb/thread/153124"
        assert self.parser._extract_thread_id(url) == 153124

    def test_extract_thread_id_invalid(self):
        assert self.parser._extract_thread_id("https://example.com/not-a-thread") == 0

    def test_extract_json_ld(self):
        html = '''
        <html><head>
        <script type="application/ld+json">
        {"@type": "DiscussionForumPosting", "headline": "My Cool WAD", "author": {"name": "Mapper"}}
        </script>
        </head><body></body></html>
        '''
        data = self.parser._extract_json_ld(html)
        assert data is not None
        assert data["headline"] == "My Cool WAD"

    def test_extract_json_ld_missing(self):
        html = "<html><head></head><body>No JSON-LD</body></html>"
        assert self.parser._extract_json_ld(html) is None

    def test_extract_html_title(self):
        # Suffix stripping happens before html.unescape, so &amp; prevents
        # the " - WADs & Mods" suffix from matching the raw &amp; string
        html = "<html><head><title>My WAD - Doomworld</title></head></html>"
        title = self.parser._extract_html_title(html)
        assert title == "My WAD"

    def test_html_to_text(self):
        html = "<p>Hello <b>World</b></p><p>Second paragraph</p>"
        text = self.parser._html_to_text(html)
        assert "Hello" in text
        assert "World" in text
        assert "Second paragraph" in text

    def test_html_to_text_empty(self):
        assert self.parser._html_to_text("") == ""
