"""Tests for IWAD registry (caco.db._iwads)."""

import sqlite3
from pathlib import Path
from unittest.mock import patch

import pytest

from caco.db._iwads import (
    DEFAULT_IWAD_PRIORITY,
    FAMILY_FALLBACKS,
    IWAD_ALIASES,
    KNOWN_IWAD_FILENAMES,
    KNOWN_IWADS,
    _compute_md5,
    get_iwad_priority,
    identify_iwad,
    managed_iwad_filename,
    normalize_iwad_name,
    remove_iwad_with_paths,
)


class TestIdentifyIwad:
    def test_known_md5(self, tmp_path):
        """identify_iwad returns (family, variant, title) for a known MD5."""
        wad = tmp_path / "doom2.wad"
        wad.write_bytes(b"fake doom2 content")
        fake_md5 = _compute_md5(wad)

        with patch.dict(KNOWN_IWADS, {fake_md5: ("doom2", "v1.9", "Doom II: Hell on Earth")}):
            result = identify_iwad(wad)
            assert result == ("doom2", "v1.9", "Doom II: Hell on Earth")

    def test_filename_fallback(self, tmp_path):
        """identify_iwad falls back to filename when MD5 is unknown."""
        wad = tmp_path / "doom2.wad"
        wad.write_bytes(b"unknown content")

        result = identify_iwad(wad)
        assert result == ("doom2", "unknown", "Doom II: Hell on Earth")

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
        """All filename entries have valid family names and 3-tuple values."""
        for filename, (family, variant, title) in KNOWN_IWAD_FILENAMES.items():
            assert family, f"Empty family for {filename}"
            assert variant, f"Empty variant for {filename}"
            assert title, f"Empty title for {filename}"
            assert filename.endswith(".wad"), f"Non-.wad filename: {filename}"

    def test_known_iwads_are_3_tuples(self):
        """All MD5 entries are (family, variant, title) 3-tuples."""
        for md5, val in KNOWN_IWADS.items():
            assert len(val) == 3, f"Expected 3-tuple for MD5 {md5}, got {len(val)}"
            family, variant, title = val
            assert family, f"Empty family for MD5 {md5}"
            assert variant, f"Empty variant for MD5 {md5}"
            assert title, f"Empty title for MD5 {md5}"


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
        iwad_id = db_mod.add_iwad("doom2", "v1.9", "/path/to/doom2.wad", title="Doom II", md5="abc123")
        assert isinstance(iwad_id, int)

        iwad = db_mod.get_iwad("doom2")
        assert iwad is not None
        assert iwad["family"] == "doom2"
        assert iwad["variant"] == "v1.9"
        assert iwad["path"] == "/path/to/doom2.wad"
        assert iwad["title"] == "Doom II"
        assert iwad["md5"] == "abc123"

    def test_get_nonexistent(self, db_mod):
        assert db_mod.get_iwad("nonexistent") is None

    def test_get_by_path(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/path/to/doom2.wad")
        iwad = db_mod.get_iwad_by_path("/path/to/doom2.wad")
        assert iwad is not None
        assert iwad["family"] == "doom2"

    def test_get_by_path_nonexistent(self, db_mod):
        assert db_mod.get_iwad_by_path("/nonexistent/path.wad") is None

    def test_get_all(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/path/doom2.wad", title="Doom II")
        db_mod.add_iwad("doom", "v1.9ud", "/path/doom.wad", title="The Ultimate Doom")
        db_mod.add_iwad("tnt", "v1.9", "/path/tnt.wad", title="TNT: Evilution")

        all_iwads = db_mod.get_all_iwads()
        assert len(all_iwads) == 3
        # Should be ordered by family then variant
        families = [iw["family"] for iw in all_iwads]
        assert families == ["doom", "doom2", "tnt"]

    def test_remove_single_variant(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/path/doom2.wad")
        db_mod.add_iwad("doom2", "bfg", "/path/doom2_bfg.wad")
        assert db_mod.remove_iwad("doom2", "bfg") == 1
        # v1.9 should still exist
        assert db_mod.get_iwad_variant("doom2", "v1.9") is not None
        assert db_mod.get_iwad_variant("doom2", "bfg") is None

    def test_remove_all_variants(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/path/doom2.wad")
        db_mod.add_iwad("doom2", "bfg", "/path/doom2_bfg.wad")
        assert db_mod.remove_iwad("doom2") == 2
        assert db_mod.get_iwad("doom2") is None

    def test_remove_nonexistent(self, db_mod):
        assert db_mod.remove_iwad("nonexistent") == 0

    def test_duplicate_family_variant_raises(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/path/a.wad")
        with pytest.raises(sqlite3.IntegrityError):
            db_mod.add_iwad("doom2", "v1.9", "/path/b.wad")

    def test_same_family_different_variant(self, db_mod):
        """Two variants of the same family can coexist."""
        db_mod.add_iwad("doom2", "v1.9", "/path/doom2.wad")
        iwad_id = db_mod.add_iwad("doom2", "bfg", "/path/doom2_bfg.wad")
        assert iwad_id is not None

    def test_get_iwad_variant(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/path/doom2.wad", title="Doom II")
        db_mod.add_iwad("doom2", "bfg", "/path/doom2_bfg.wad", title="Doom II BFG")

        v19 = db_mod.get_iwad_variant("doom2", "v1.9")
        assert v19 is not None
        assert v19["title"] == "Doom II"

        bfg = db_mod.get_iwad_variant("doom2", "bfg")
        assert bfg is not None
        assert bfg["title"] == "Doom II BFG"

        assert db_mod.get_iwad_variant("doom2", "kex") is None

    def test_get_family_iwads(self, db_mod):
        db_mod.add_iwad("doom2", "bfg", "/path/doom2_bfg.wad")
        db_mod.add_iwad("doom2", "v1.9", "/path/doom2.wad")
        db_mod.add_iwad("doom2", "kex", "/path/doom2_kex.wad")

        family = db_mod.get_family_iwads("doom2")
        assert len(family) == 3
        # Should be sorted by priority: v1.9, bfg, kex
        variants = [iw["variant"] for iw in family]
        assert variants == ["v1.9", "bfg", "kex"]

    def test_get_family_iwads_empty(self, db_mod):
        assert db_mod.get_family_iwads("nonexistent") == []


class TestPriorityResolution:
    def test_preferred_variant(self, db_mod):
        """get_iwad returns the highest-priority variant."""
        db_mod.add_iwad("doom2", "kex", "/path/kex.wad")
        db_mod.add_iwad("doom2", "v1.9", "/path/v19.wad")
        db_mod.add_iwad("doom2", "bfg", "/path/bfg.wad")

        preferred = db_mod.get_iwad("doom2")
        assert preferred is not None
        assert preferred["variant"] == "v1.9"

    def test_fallback_to_lower_priority(self, db_mod):
        """If highest priority is missing, use next available."""
        db_mod.add_iwad("doom2", "kex", "/path/kex.wad")
        db_mod.add_iwad("doom2", "bfg", "/path/bfg.wad")

        preferred = db_mod.get_iwad("doom2")
        assert preferred is not None
        assert preferred["variant"] == "bfg"

    def test_unknown_variant_fallback(self, db_mod):
        """Filename-detected 'unknown' variant is used if no priority match."""
        db_mod.add_iwad("doom2", "unknown", "/path/doom2.wad")

        preferred = db_mod.get_iwad("doom2")
        assert preferred is not None
        assert preferred["variant"] == "unknown"

    def test_family_fallback_freedoom(self, db_mod):
        """get_iwad falls back to freedoom when family has no variants."""
        db_mod.add_iwad("freedoom2", "latest", "/path/freedoom2.wad")

        # doom2 has no registered variants, should fall back to freedoom2
        result = db_mod.get_iwad("doom2")
        assert result is not None
        assert result["family"] == "freedoom2"

    def test_no_fallback_when_family_has_variants(self, db_mod):
        """Fallback is not used when family has its own variants."""
        db_mod.add_iwad("doom2", "v1.9", "/path/doom2.wad")
        db_mod.add_iwad("freedoom2", "latest", "/path/freedoom2.wad")

        result = db_mod.get_iwad("doom2")
        assert result is not None
        assert result["family"] == "doom2"

    def test_no_fallback_available(self, db_mod):
        """Returns None when neither family nor fallback is registered."""
        result = db_mod.get_iwad("doom2")
        assert result is None

    def test_config_priority_override(self, db_mod, tmp_config):
        """Config [iwad_priority] overrides DEFAULT_IWAD_PRIORITY."""
        from caco import config

        # Write config with reversed priority
        tmp_config.write_text('[iwad_priority]\ndoom2 = ["bfg", "v1.9"]\n')
        config._config_cache = None

        db_mod.add_iwad("doom2", "v1.9", "/path/v19.wad")
        db_mod.add_iwad("doom2", "bfg", "/path/bfg.wad")

        preferred = db_mod.get_iwad("doom2")
        assert preferred is not None
        assert preferred["variant"] == "bfg"

    def test_doom_family_priority(self, db_mod):
        """doom family prefers v1.9ud over v1.9."""
        db_mod.add_iwad("doom", "v1.9", "/path/doom_v19.wad")
        db_mod.add_iwad("doom", "v1.9ud", "/path/doom_ud.wad")

        preferred = db_mod.get_iwad("doom")
        assert preferred is not None
        assert preferred["variant"] == "v1.9ud"


class TestGetIwadPriority:
    def test_default_priority(self):
        """get_iwad_priority returns default for known families."""
        assert get_iwad_priority("doom2") == ["v1.9", "bfg", "enhanced", "kex"]

    def test_unknown_family(self):
        """get_iwad_priority returns empty list for unknown families."""
        assert get_iwad_priority("myfamily") == []

    def test_config_override(self, tmp_config):
        """Config [iwad_priority] overrides defaults."""
        from caco import config

        tmp_config.write_text('[iwad_priority]\ndoom2 = ["kex", "v1.9"]\n')
        config._config_cache = None

        assert get_iwad_priority("doom2") == ["kex", "v1.9"]


class TestResolveIwadFromDb:
    def test_resolve_registered(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/usr/share/doom/doom2.wad")
        result = db_mod.resolve_iwad_from_db("doom2")
        assert result == "/usr/share/doom/doom2.wad"

    def test_resolve_unregistered(self, db_mod):
        result = db_mod.resolve_iwad_from_db("doom2")
        assert result is None

    def test_resolve_preferred_variant(self, db_mod):
        """resolve_iwad_from_db returns path of preferred variant."""
        db_mod.add_iwad("doom2", "kex", "/path/kex.wad")
        db_mod.add_iwad("doom2", "v1.9", "/path/v19.wad")

        result = db_mod.resolve_iwad_from_db("doom2")
        assert result == "/path/v19.wad"


class TestResolveIwadIntegration:
    """Test that resolve_iwad() checks the DB registry."""

    def test_resolve_from_registry(self, db_mod, tmp_config):
        """resolve_iwad() should find IWADs from the registry."""
        from caco.config import resolve_iwad

        db_mod.add_iwad("doom2", "v1.9", "/fake/doom2.wad")
        result = resolve_iwad("doom2")
        assert result == "/fake/doom2.wad"

    def test_resolve_falls_through(self, db_mod, tmp_config):
        """resolve_iwad() falls through when name is not in registry."""
        from caco.config import resolve_iwad

        result = resolve_iwad("unknown_iwad")
        assert result == "unknown_iwad"


class TestFamilyFallbacks:
    def test_fallback_structure(self):
        """FAMILY_FALLBACKS maps doom-engine families to freedoom."""
        assert "doom" in FAMILY_FALLBACKS
        assert "doom2" in FAMILY_FALLBACKS
        assert "plutonia" in FAMILY_FALLBACKS
        assert "tnt" in FAMILY_FALLBACKS

    def test_doom_falls_back_to_freedoom1(self):
        assert FAMILY_FALLBACKS["doom"] == ["freedoom1"]

    def test_doom2_falls_back_to_freedoom2(self):
        assert FAMILY_FALLBACKS["doom2"] == ["freedoom2"]


class TestDefaultIwadPriority:
    def test_all_main_families_have_priority(self):
        for family in ("doom", "doom1", "doom2", "plutonia", "tnt"):
            assert family in DEFAULT_IWAD_PRIORITY, f"Missing priority for {family}"

    def test_doom2_priority_order(self):
        assert DEFAULT_IWAD_PRIORITY["doom2"] == ["v1.9", "bfg", "enhanced", "kex"]

    def test_doom_priority_order(self):
        assert DEFAULT_IWAD_PRIORITY["doom"] == ["v1.9ud", "v1.9", "bfg", "enhanced", "kex"]


class TestManagedIwadFilename:
    def test_basic(self):
        assert managed_iwad_filename("doom2", "v1.9") == "doom2_v1.9.wad"

    def test_bfg(self):
        assert managed_iwad_filename("doom2", "bfg") == "doom2_bfg.wad"

    def test_unknown(self):
        assert managed_iwad_filename("doom", "unknown") == "doom_unknown.wad"


class TestRemoveIwadWithPaths:
    def test_remove_single_variant(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/managed/doom2_v1.9.wad")
        db_mod.add_iwad("doom2", "bfg", "/managed/doom2_bfg.wad")

        paths = remove_iwad_with_paths("doom2", "bfg")
        assert paths == ["/managed/doom2_bfg.wad"]
        # v1.9 still exists
        assert db_mod.get_iwad_variant("doom2", "v1.9") is not None
        assert db_mod.get_iwad_variant("doom2", "bfg") is None

    def test_remove_all_variants(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/managed/doom2_v1.9.wad")
        db_mod.add_iwad("doom2", "bfg", "/managed/doom2_bfg.wad")

        paths = remove_iwad_with_paths("doom2")
        assert set(paths) == {"/managed/doom2_v1.9.wad", "/managed/doom2_bfg.wad"}
        assert db_mod.get_iwad("doom2") is None

    def test_remove_nonexistent(self, db_mod):
        paths = remove_iwad_with_paths("nonexistent")
        assert paths == []

    def test_remove_nonexistent_variant(self, db_mod):
        db_mod.add_iwad("doom2", "v1.9", "/managed/doom2_v1.9.wad")
        paths = remove_iwad_with_paths("doom2", "kex")
        assert paths == []
        # v1.9 still exists
        assert db_mod.get_iwad_variant("doom2", "v1.9") is not None


class TestCacheMigration:
    """Test migration #10: WAD cache relocation."""

    def test_files_moved_and_paths_updated(self, tmp_path, tmp_db):
        """Migration moves files from old cache dir to new and updates DB."""
        from caco import db as db_mod
        from caco.db._schema import _migrate_relocate_wad_cache

        # Create the expected directory structure under a fake home
        home = tmp_path / "fakehome"
        old_cache = home / ".cache" / "caco" / "wads"
        new_cache = home / ".local" / "share" / "caco" / "wads"
        old_cache.mkdir(parents=True)

        wad_file = old_cache / "test.wad"
        wad_file.write_bytes(b"test content")
        old_cached_path = str(wad_file)

        # Insert a WAD with the old cached_path
        wad_id = db_mod.add_wad(
            title="Test WAD",
            source_type=db_mod.SourceType.IDGAMES,
            cached_path=old_cached_path,
            filename="test.wad",
        )

        # Run migration with patched Path.home and load_config
        conn = db_mod.get_connection()
        with patch("caco.config.load_config", return_value={"cache_dir": ""}):
            with patch("pathlib.Path.home", return_value=home):
                _migrate_relocate_wad_cache(conn)
                conn.commit()
        conn.close()

        # Verify: file moved to new location
        assert (new_cache / "test.wad").exists()
        assert not wad_file.exists()

        # Verify: DB path updated
        wad = db_mod.get_wad(wad_id)
        assert wad["cached_path"] == str(new_cache / "test.wad")

    def test_skip_if_old_dir_missing(self, tmp_path, tmp_db):
        """Migration is a no-op if old cache dir doesn't exist."""
        from caco import db as db_mod
        from caco.db._schema import _migrate_relocate_wad_cache

        home = tmp_path / "fakehome"
        # Don't create old cache dir

        conn = db_mod.get_connection()
        with patch("pathlib.Path.home", return_value=home):
            _migrate_relocate_wad_cache(conn)
            conn.commit()
        conn.close()

    def test_skip_if_custom_cache_dir(self, tmp_path, tmp_db):
        """Migration skips when user has a custom cache_dir."""
        from caco import db as db_mod
        from caco.db._schema import _migrate_relocate_wad_cache

        home = tmp_path / "fakehome"
        old_cache = home / ".cache" / "caco" / "wads"
        old_cache.mkdir(parents=True)
        (old_cache / "test.wad").write_bytes(b"data")

        conn = db_mod.get_connection()
        with patch("caco.config.load_config", return_value={"cache_dir": "/custom/path"}):
            with patch("pathlib.Path.home", return_value=home):
                _migrate_relocate_wad_cache(conn)
                conn.commit()
        conn.close()

        # File should NOT have been moved
        assert (old_cache / "test.wad").exists()


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
