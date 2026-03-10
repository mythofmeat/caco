"""Tests for caco.services.import_service — duplicate detection, normalization, and import flows."""

from unittest.mock import patch

import pytest

from caco.services.import_service import (
    ImportResult,
    ImportService,
    _normalize_title,
    _titles_match,
    normalize_tags,
)


# =============================================================================
# ImportResult
# =============================================================================


class TestImportResult:
    def test_ok_when_wad_id_set(self):
        r = ImportResult(wad_id=1)
        assert r.ok is True

    def test_not_ok_when_duplicate(self):
        r = ImportResult(is_duplicate=True, duplicate_id=1, duplicate_title="Test")
        assert r.ok is False

    def test_not_ok_when_error(self):
        r = ImportResult(error="something went wrong")
        assert r.ok is False

    def test_not_ok_when_wad_id_none(self):
        r = ImportResult()
        assert r.ok is False

    def test_not_ok_when_wad_id_and_error(self):
        r = ImportResult(wad_id=1, error="partial failure")
        assert r.ok is False

    def test_defaults(self):
        r = ImportResult()
        assert r.wad_id is None
        assert r.is_duplicate is False
        assert r.duplicate_id is None
        assert r.duplicate_title is None
        assert r.error is None


# =============================================================================
# normalize_tags
# =============================================================================


class TestNormalizeTags:
    def test_none_returns_none(self):
        assert normalize_tags(None) is None

    def test_comma_separated_string(self):
        assert normalize_tags("Cacoward, Megawad, Classic") == ["cacoward", "megawad", "classic"]

    def test_single_tag_string(self):
        assert normalize_tags("megawad") == ["megawad"]

    def test_empty_string_returns_none(self):
        assert normalize_tags("") is None

    def test_whitespace_only_returns_none(self):
        assert normalize_tags("  ,  ,  ") is None

    def test_list_input(self):
        assert normalize_tags(["Megawad", "Classic"]) == ["megawad", "classic"]

    def test_tuple_input(self):
        assert normalize_tags(("Megawad",)) == ["megawad"]

    def test_empty_list_returns_none(self):
        assert normalize_tags([]) is None

    def test_strips_whitespace(self):
        assert normalize_tags("  megawad  ,  classic  ") == ["megawad", "classic"]

    def test_mixed_empty_entries(self):
        assert normalize_tags("megawad,,classic,") == ["megawad", "classic"]


# =============================================================================
# Title normalization and matching
# =============================================================================


class TestNormalizeTitle:
    def test_lowercase(self):
        assert _normalize_title("SCYTHE") == "scythe"

    def test_strips_punctuation(self):
        assert _normalize_title("Scythe: Episode 2") == "scythe episode 2"

    def test_collapses_whitespace(self):
        assert _normalize_title("Scythe   2") == "scythe 2"

    def test_strips_accents(self):
        assert _normalize_title("Résurrection") == "resurrection"

    def test_mixed_normalization(self):
        assert _normalize_title("  Ancient Aliens!! (2016)  ") == "ancient aliens 2016"

    def test_empty_string(self):
        assert _normalize_title("") == ""

    def test_all_punctuation(self):
        assert _normalize_title("!!!---") == ""


class TestTitlesMatch:
    def test_exact_match(self):
        assert _titles_match("Scythe", "Scythe") is True

    def test_case_insensitive(self):
        assert _titles_match("Scythe", "scythe") is True

    def test_punctuation_ignored(self):
        assert _titles_match("Scythe: Episode 2", "Scythe Episode 2") is True

    def test_accents_ignored(self):
        assert _titles_match("Résurrection", "Resurrection") is True

    def test_different_titles(self):
        assert _titles_match("Scythe", "Sunlust") is False


# =============================================================================
# ImportService._auto_link_complevel
# =============================================================================


class TestAutoLinkComplevel:
    @pytest.mark.parametrize("port_text,expected_complevel", [
        ("Boom-compatible", 9),
        ("MBF21-compatible", 21),
        ("MBF", 11),
        ("Vanilla", 2),
        ("Limit-removing", 2),
    ])
    def test_port_mapping(self, db_mod, make_wad, port_text, expected_complevel):
        wad_id = make_wad(title="Test")
        ImportService._auto_link_complevel(wad_id, port_text)
        wad = db_mod.get_wad(wad_id)
        assert wad["complevel"] == expected_complevel

    def test_does_not_overwrite_existing(self, db_mod, make_wad):
        wad_id = make_wad(title="Test")
        db_mod.update_wad(wad_id, complevel=4)
        ImportService._auto_link_complevel(wad_id, "Boom-compatible")
        wad = db_mod.get_wad(wad_id)
        assert wad["complevel"] == 4  # unchanged

    def test_unknown_port_text_noop(self, db_mod, make_wad):
        wad_id = make_wad(title="Test")
        ImportService._auto_link_complevel(wad_id, "ZDoom only")
        wad = db_mod.get_wad(wad_id)
        assert wad["complevel"] is None


# =============================================================================
# ImportService.import_url
# =============================================================================


class TestImportUrl:
    def test_basic_url_import(self, db_mod):
        svc = ImportService()
        with patch.object(svc, "_auto_enrich_doomwiki"):
            result = svc.import_url("Test WAD", "https://example.com/test.wad")
        assert result.ok
        assert result.wad_id is not None
        wad = db_mod.get_wad(result.wad_id)
        assert wad["title"] == "Test WAD"
        assert wad["source_type"] == "url"
        assert wad["source_url"] == "https://example.com/test.wad"

    def test_url_import_with_metadata(self, db_mod):
        svc = ImportService()
        with patch.object(svc, "_auto_enrich_doomwiki"):
            result = svc.import_url(
                "Test WAD",
                "https://example.com/test.wad",
                author="Author",
                year=2024,
                description="A test WAD",
                tags=["megawad"],
            )
        assert result.ok
        wad = db_mod.get_wad(result.wad_id)
        assert wad["author"] == "Author"
        assert wad["year"] == 2024
        assert wad["description"] == "A test WAD"
        assert "megawad" in wad["tags"]

    def test_url_duplicate_detected(self, db_mod):
        svc = ImportService()
        with patch.object(svc, "_auto_enrich_doomwiki"):
            r1 = svc.import_url("Test WAD", "https://example.com/test.wad")
            r2 = svc.import_url("Test WAD 2", "https://example.com/test.wad")
        assert r1.ok
        assert r2.is_duplicate
        assert r2.duplicate_id == r1.wad_id

    def test_url_force_import_bypasses_duplicate(self, db_mod):
        svc = ImportService()
        with patch.object(svc, "_auto_enrich_doomwiki"):
            r1 = svc.import_url("Test WAD", "https://example.com/test.wad")
            r2 = svc.import_url("Test WAD 2", "https://example.com/test.wad", force=True)
        assert r1.ok
        assert r2.ok
        assert r2.wad_id != r1.wad_id


# =============================================================================
# ImportService.import_local
# =============================================================================


class TestImportLocal:
    def test_local_import_existing_file(self, db_mod, tmp_path):
        f = tmp_path / "test.wad"
        f.write_bytes(b"PWAD data")

        svc = ImportService()
        with patch.object(svc, "_auto_enrich_doomwiki"):
            result = svc.import_local("Test WAD", str(f))
        assert result.ok
        wad = db_mod.get_wad(result.wad_id)
        assert wad["filename"] == "test.wad"
        assert wad["cached_path"] is not None
        assert wad["source_type"] == "local"

    def test_local_import_nonexistent_file(self, db_mod, tmp_path):
        """Nonexistent file still creates a DB entry with NULL cached_path."""
        svc = ImportService()
        with patch.object(svc, "_auto_enrich_doomwiki"):
            result = svc.import_local("Test WAD", str(tmp_path / "nonexistent.wad"))
        assert result.ok
        wad = db_mod.get_wad(result.wad_id)
        assert wad["cached_path"] is None

    def test_local_duplicate_detected(self, db_mod, tmp_path):
        f = tmp_path / "test.wad"
        f.write_bytes(b"data")

        svc = ImportService()
        with patch.object(svc, "_auto_enrich_doomwiki"):
            r1 = svc.import_local("WAD 1", str(f))
            r2 = svc.import_local("WAD 2", str(f))
        assert r1.ok
        assert r2.is_duplicate


# =============================================================================
# ImportService._auto_enrich_doomwiki
# =============================================================================


class TestAutoEnrichDoomwiki:
    def test_disabled_by_config(self, db_mod, make_wad):
        wad_id = make_wad(title="Test")
        svc = ImportService()
        with patch("caco.config.get_auto_doomwiki_enrich", return_value=False):
            svc._auto_enrich_doomwiki(wad_id, "Test")
        # Should have done nothing
        wad = db_mod.get_wad(wad_id)
        assert wad["description"] is None

    def test_silently_handles_exceptions(self, db_mod, make_wad):
        wad_id = make_wad(title="Test")
        svc = ImportService()
        with patch("caco.config.get_auto_doomwiki_enrich", side_effect=Exception("network error")):
            # Should not raise
            svc._auto_enrich_doomwiki(wad_id, "Test")
