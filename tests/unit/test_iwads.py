"""Tests for IWAD registry (caco.db._iwads)."""

import sqlite3
from pathlib import Path
from unittest.mock import patch

import pytest

from caco.db._iwads import (
    IWAD_ALIASES,
    KNOWN_IWAD_FILENAMES,
    KNOWN_IWADS,
    _compute_md5,
    identify_iwad,
    normalize_iwad_name,
)


class TestIdentifyIwad:
    def test_known_md5(self, tmp_path):
        """identify_iwad returns (name, title) for a known MD5."""
        # Create a fake file with the Doom II MD5
        wad = tmp_path / "doom2.wad"
        wad.write_bytes(b"fake doom2 content")
        fake_md5 = _compute_md5(wad)

        # Patch KNOWN_IWADS to include our fake MD5
        with patch.dict(KNOWN_IWADS, {fake_md5: ("doom2", "Doom II: Hell on Earth")}):
            result = identify_iwad(wad)
            assert result == ("doom2", "Doom II: Hell on Earth")

    def test_filename_fallback(self, tmp_path):
        """identify_iwad falls back to filename when MD5 is unknown."""
        wad = tmp_path / "doom2.wad"
        wad.write_bytes(b"unknown content")

        result = identify_iwad(wad)
        assert result == ("doom2", "Doom II: Hell on Earth")

    def test_unknown_file(self, tmp_path):
        """identify_iwad returns None for completely unknown files."""
        wad = tmp_path / "mywad.wad"
        wad.write_bytes(b"random content")

        result = identify_iwad(wad)
        assert result is None

    def test_nonexistent_file(self, tmp_path):
        """identify_iwad returns None for files that don't exist."""
        result = identify_iwad(tmp_path / "missing.wad")
        assert result is None

    def test_known_filenames_coverage(self):
        """All filename entries have valid short names."""
        for filename, (name, title) in KNOWN_IWAD_FILENAMES.items():
            assert name, f"Empty name for {filename}"
            assert title, f"Empty title for {filename}"
            assert filename.endswith(".wad"), f"Non-.wad filename: {filename}"


class TestNormalizeIwadName:
    def test_doom2_variants(self):
        assert normalize_iwad_name("Doom II") == "doom2"
        assert normalize_iwad_name("doom 2") == "doom2"
        assert normalize_iwad_name("DOOM2") == "doom2"
        assert normalize_iwad_name("Doom II: Hell on Earth") == "doom2"

    def test_doom_variants(self):
        assert normalize_iwad_name("The Ultimate Doom") == "doom"
        assert normalize_iwad_name("ultimate doom") == "doom"
        assert normalize_iwad_name("Doom") == "doom"

    def test_plutonia(self):
        assert normalize_iwad_name("Plutonia") == "plutonia"
        assert normalize_iwad_name("The Plutonia Experiment") == "plutonia"

    def test_tnt(self):
        assert normalize_iwad_name("TNT") == "tnt"
        assert normalize_iwad_name("TNT: Evilution") == "tnt"
        assert normalize_iwad_name("Evilution") == "tnt"

    def test_heretic(self):
        assert normalize_iwad_name("Heretic") == "heretic"

    def test_unknown(self):
        assert normalize_iwad_name("Some Random WAD") is None
        assert normalize_iwad_name("") is None

    def test_case_insensitive(self):
        assert normalize_iwad_name("DOOM II") == "doom2"
        assert normalize_iwad_name("doom ii") == "doom2"
        assert normalize_iwad_name("Doom II") == "doom2"

    def test_whitespace_stripped(self):
        assert normalize_iwad_name("  doom2  ") == "doom2"


class TestIwadCrud:
    def test_add_and_get(self, db_mod):
        iwad_id = db_mod.add_iwad("doom2", "/path/to/doom2.wad", title="Doom II", md5="abc123")
        assert isinstance(iwad_id, int)

        iwad = db_mod.get_iwad("doom2")
        assert iwad is not None
        assert iwad["name"] == "doom2"
        assert iwad["path"] == "/path/to/doom2.wad"
        assert iwad["title"] == "Doom II"
        assert iwad["md5"] == "abc123"

    def test_get_nonexistent(self, db_mod):
        assert db_mod.get_iwad("nonexistent") is None

    def test_get_by_path(self, db_mod):
        db_mod.add_iwad("doom2", "/path/to/doom2.wad")
        iwad = db_mod.get_iwad_by_path("/path/to/doom2.wad")
        assert iwad is not None
        assert iwad["name"] == "doom2"

    def test_get_by_path_nonexistent(self, db_mod):
        assert db_mod.get_iwad_by_path("/nonexistent/path.wad") is None

    def test_get_all(self, db_mod):
        db_mod.add_iwad("doom2", "/path/doom2.wad", title="Doom II")
        db_mod.add_iwad("doom", "/path/doom.wad", title="The Ultimate Doom")
        db_mod.add_iwad("tnt", "/path/tnt.wad", title="TNT: Evilution")

        all_iwads = db_mod.get_all_iwads()
        assert len(all_iwads) == 3
        # Should be ordered by name
        names = [iw["name"] for iw in all_iwads]
        assert names == ["doom", "doom2", "tnt"]

    def test_remove(self, db_mod):
        db_mod.add_iwad("doom2", "/path/doom2.wad")
        assert db_mod.remove_iwad("doom2") is True
        assert db_mod.get_iwad("doom2") is None

    def test_remove_nonexistent(self, db_mod):
        assert db_mod.remove_iwad("nonexistent") is False

    def test_duplicate_name_raises(self, db_mod):
        db_mod.add_iwad("doom2", "/path/a.wad")
        with pytest.raises(sqlite3.IntegrityError):
            db_mod.add_iwad("doom2", "/path/b.wad")

    def test_same_path_different_name(self, db_mod):
        """Two different names can't have the same name, but same path is allowed."""
        db_mod.add_iwad("doom2", "/path/doom2.wad")
        # Different name, same path — this is technically allowed by schema
        iwad_id = db_mod.add_iwad("doom2alt", "/path/doom2.wad")
        assert iwad_id is not None


class TestResolveIwadFromDb:
    def test_resolve_registered(self, db_mod):
        db_mod.add_iwad("doom2", "/usr/share/doom/doom2.wad")
        result = db_mod.resolve_iwad_from_db("doom2")
        assert result == "/usr/share/doom/doom2.wad"

    def test_resolve_unregistered(self, db_mod):
        result = db_mod.resolve_iwad_from_db("doom2")
        assert result is None


class TestResolveIwadIntegration:
    """Test that resolve_iwad() checks the DB registry."""

    def test_resolve_from_registry(self, db_mod, tmp_config):
        """resolve_iwad() should find IWADs from the registry."""
        from caco.config import resolve_iwad

        db_mod.add_iwad("doom2", "/fake/doom2.wad")
        result = resolve_iwad("doom2")
        assert result == "/fake/doom2.wad"

    def test_resolve_falls_through(self, db_mod, tmp_config):
        """resolve_iwad() falls through when name is not in registry."""
        from caco.config import resolve_iwad

        result = resolve_iwad("unknown_iwad")
        assert result == "unknown_iwad"


class TestComputeMd5:
    def test_consistent(self, tmp_path):
        """MD5 computation is consistent."""
        wad = tmp_path / "test.wad"
        wad.write_bytes(b"hello world")
        assert _compute_md5(wad) == _compute_md5(wad)

    def test_different_content(self, tmp_path):
        """Different files produce different MD5s."""
        a = tmp_path / "a.wad"
        b = tmp_path / "b.wad"
        a.write_bytes(b"content a")
        b.write_bytes(b"content b")
        assert _compute_md5(a) != _compute_md5(b)
