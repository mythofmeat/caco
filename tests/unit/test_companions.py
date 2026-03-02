"""Tests for companion file registry, service, and CLI."""

import json
import shutil
from pathlib import Path
from unittest.mock import patch

import pytest

from caco import db
from caco.db._companions import (
    add_companion,
    get_companion,
    get_companion_by_md5,
    get_all_companions,
    get_wad_companions,
    get_wad_companion_by_filename,
    get_companion_wads,
    get_next_load_order,
    is_orphan,
    link_companion,
    remove_companion,
    remove_companion_with_path,
    set_companion_enabled,
    set_companion_load_order,
    unlink_companion,
)


# =============================================================================
# Companion CRUD tests
# =============================================================================


class TestCompanionCRUD:
    def test_add_and_get(self, db_mod):
        comp_id = add_companion("music.wad", "/tmp/abc_music.wad", "abc123", 1024)
        assert comp_id > 0

        comp = get_companion(comp_id)
        assert comp is not None
        assert comp["filename"] == "music.wad"
        assert comp["md5"] == "abc123"
        assert comp["size"] == 1024

    def test_get_by_md5(self, db_mod):
        add_companion("test.deh", "/tmp/def_test.deh", "def456", 512)
        comp = get_companion_by_md5("def456")
        assert comp is not None
        assert comp["filename"] == "test.deh"

    def test_get_by_md5_missing(self, db_mod):
        assert get_companion_by_md5("nonexistent") is None

    def test_get_all(self, db_mod):
        add_companion("a.wad", "/tmp/a.wad", "aaa", 100)
        add_companion("b.wad", "/tmp/b.wad", "bbb", 200)
        companions = get_all_companions()
        assert len(companions) == 2

    def test_remove(self, db_mod):
        comp_id = add_companion("rem.wad", "/tmp/rem.wad", "rem123", 100)
        assert remove_companion(comp_id) == 1
        assert get_companion(comp_id) is None

    def test_remove_with_path(self, db_mod):
        comp_id = add_companion("rem2.wad", "/tmp/rem2.wad", "rem456", 100)
        path = remove_companion_with_path(comp_id)
        assert path == "/tmp/rem2.wad"
        assert get_companion(comp_id) is None

    def test_remove_with_path_missing(self, db_mod):
        assert remove_companion_with_path(9999) is None


# =============================================================================
# Junction table tests
# =============================================================================


class TestWadCompanionLinking:
    def test_link_and_get(self, make_wad, db_mod):
        wad_id = make_wad(title="Test WAD")
        comp_id = add_companion("music.wad", "/tmp/music.wad", "m123", 100)
        link_companion(wad_id, comp_id)

        companions = get_wad_companions(wad_id)
        assert len(companions) == 1
        assert companions[0]["filename"] == "music.wad"
        assert companions[0]["enabled"] == 1
        assert companions[0]["load_order"] == 0

    def test_link_auto_load_order(self, make_wad, db_mod):
        wad_id = make_wad(title="Test WAD")
        c1 = add_companion("a.wad", "/tmp/a.wad", "a1", 100)
        c2 = add_companion("b.wad", "/tmp/b.wad", "b2", 200)
        link_companion(wad_id, c1)
        link_companion(wad_id, c2)

        companions = get_wad_companions(wad_id)
        assert len(companions) == 2
        assert companions[0]["load_order"] == 0
        assert companions[1]["load_order"] == 1

    def test_unlink(self, make_wad, db_mod):
        wad_id = make_wad(title="Test WAD")
        comp_id = add_companion("x.wad", "/tmp/x.wad", "x1", 100)
        link_companion(wad_id, comp_id)
        assert unlink_companion(wad_id, comp_id) == 1
        assert get_wad_companions(wad_id) == []

    def test_enable_disable(self, make_wad, db_mod):
        wad_id = make_wad(title="Test WAD")
        comp_id = add_companion("e.wad", "/tmp/e.wad", "e1", 100)
        link_companion(wad_id, comp_id)

        set_companion_enabled(wad_id, comp_id, False)
        comps = get_wad_companions(wad_id)
        assert comps[0]["enabled"] == 0

        # Enabled-only filter
        assert get_wad_companions(wad_id, enabled_only=True) == []

        set_companion_enabled(wad_id, comp_id, True)
        assert len(get_wad_companions(wad_id, enabled_only=True)) == 1

    def test_load_order(self, make_wad, db_mod):
        wad_id = make_wad(title="Test WAD")
        comp_id = add_companion("lo.wad", "/tmp/lo.wad", "lo1", 100)
        link_companion(wad_id, comp_id, load_order=5)

        comps = get_wad_companions(wad_id)
        assert comps[0]["load_order"] == 5

        set_companion_load_order(wad_id, comp_id, 10)
        comps = get_wad_companions(wad_id)
        assert comps[0]["load_order"] == 10

    def test_get_companion_wads(self, make_wad, db_mod):
        wad1 = make_wad(title="WAD 1")
        wad2 = make_wad(title="WAD 2")
        comp_id = add_companion("shared.wad", "/tmp/shared.wad", "s1", 100)
        link_companion(wad1, comp_id)
        link_companion(wad2, comp_id)

        wads = get_companion_wads(comp_id)
        assert len(wads) == 2

    def test_next_load_order(self, make_wad, db_mod):
        wad_id = make_wad(title="Test WAD")
        assert get_next_load_order(wad_id) == 0

        c1 = add_companion("n.wad", "/tmp/n.wad", "n1", 100)
        link_companion(wad_id, c1)
        assert get_next_load_order(wad_id) == 1

    def test_is_orphan(self, make_wad, db_mod):
        wad_id = make_wad(title="Test WAD")
        comp_id = add_companion("o.wad", "/tmp/o.wad", "o1", 100)
        link_companion(wad_id, comp_id)
        assert not is_orphan(comp_id)

        unlink_companion(wad_id, comp_id)
        assert is_orphan(comp_id)

    def test_get_by_filename(self, make_wad, db_mod):
        wad_id = make_wad(title="Test WAD")
        comp_id = add_companion("find.deh", "/tmp/find.deh", "f1", 100)
        link_companion(wad_id, comp_id)

        result = get_wad_companion_by_filename(wad_id, "find.deh")
        assert result is not None
        assert result["filename"] == "find.deh"

        assert get_wad_companion_by_filename(wad_id, "missing.deh") is None

    def test_link_duplicate_ignored(self, make_wad, db_mod):
        """INSERT OR IGNORE should silently skip duplicate links."""
        wad_id = make_wad(title="Test WAD")
        comp_id = add_companion("dup.wad", "/tmp/dup.wad", "d1", 100)
        link_companion(wad_id, comp_id)
        link_companion(wad_id, comp_id)  # Should not error
        assert len(get_wad_companions(wad_id)) == 1

    def test_cascade_delete_wad(self, make_wad, db_mod):
        """Purging a WAD should cascade-delete wad_companions entries."""
        wad_id = make_wad(title="Cascade WAD")
        comp_id = add_companion("cas.wad", "/tmp/cas.wad", "cas1", 100)
        link_companion(wad_id, comp_id)

        db_mod.delete_wad(wad_id, purge=True)
        assert is_orphan(comp_id)


# =============================================================================
# Service tests
# =============================================================================


class TestCompanionService:
    def test_register_companion(self, make_wad, db_mod, tmp_path):
        from caco.services.companion_service import register_companion

        wad_id = make_wad(title="Service WAD")

        # Create a test file
        test_file = tmp_path / "test_companion.wad"
        test_file.write_bytes(b"test data for companion")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                comp_id, filename = register_companion(str(test_file), wad_id)

        assert comp_id > 0
        assert filename == "test_companion.wad"

        # Verify it's linked
        companions = get_wad_companions(wad_id)
        assert len(companions) == 1
        assert companions[0]["filename"] == "test_companion.wad"

    def test_register_dedup(self, make_wad, db_mod, tmp_path):
        """Same file added to two WADs should share the companion entry."""
        from caco.services.companion_service import register_companion

        wad1 = make_wad(title="WAD 1")
        wad2 = make_wad(title="WAD 2")

        test_file = tmp_path / "shared.deh"
        test_file.write_bytes(b"shared data")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                id1, _ = register_companion(str(test_file), wad1)
                id2, _ = register_companion(str(test_file), wad2)

        assert id1 == id2  # Same companion ID due to dedup

        wads = get_companion_wads(id1)
        assert len(wads) == 2

    def test_unregister_companion_keep(self, make_wad, db_mod, tmp_path):
        from caco.services.companion_service import register_companion, unregister_companion

        wad_id = make_wad(title="Unreg WAD")
        test_file = tmp_path / "unreg.wad"
        test_file.write_bytes(b"data")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                comp_id, _ = register_companion(str(test_file), wad_id)

        deleted = unregister_companion(wad_id, comp_id, orphan_policy="keep")
        assert not deleted
        assert get_wad_companions(wad_id) == []
        # Companion still exists in registry
        assert get_companion(comp_id) is not None

    def test_unregister_companion_delete(self, make_wad, db_mod, tmp_path):
        from caco.services.companion_service import register_companion, unregister_companion

        wad_id = make_wad(title="Del WAD")
        test_file = tmp_path / "del.wad"
        test_file.write_bytes(b"data")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                comp_id, _ = register_companion(str(test_file), wad_id)

        managed_path = get_companion(comp_id)["path"]
        deleted = unregister_companion(wad_id, comp_id, orphan_policy="delete")
        assert deleted
        assert get_companion(comp_id) is None
        assert not Path(managed_path).exists()


# =============================================================================
# Migration test
# =============================================================================


class TestMigration:
    def test_migration_from_json(self, db_mod, tmp_path):
        """Verify that the companion_files_registry and wad_companions tables exist."""
        with db.get_connection() as conn:
            # Check tables exist
            tables = conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('companion_files_registry', 'wad_companions')"
            ).fetchall()
            table_names = {t["name"] for t in tables}
            assert "companion_files_registry" in table_names
            assert "wad_companions" in table_names
