"""Tests for caco.services.companion_service — registration, dedup, and orphan cleanup."""

from unittest.mock import patch

import pytest

from caco.services.companion_service import register_companion, unregister_companion


# =============================================================================
# Shared helper
# =============================================================================


def _setup_companion(db_mod, make_wad, tmp_path, *, link_mode="copy"):
    """Create a file, companion dir, and register a companion linked to a new WAD."""
    wad_id = make_wad(title="Test")
    f = tmp_path / "patch.deh"
    f.write_bytes(b"deh content")

    companion_dir = tmp_path / "companions"
    companion_dir.mkdir(exist_ok=True)

    with (
        patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir),
        patch("caco.services.companion_service.get_link_mode", return_value=link_mode),
    ):
        companion_id, filename = register_companion(str(f), wad_id)

    return wad_id, companion_id, companion_dir, f


# =============================================================================
# register_companion
# =============================================================================


class TestRegisterCompanion:
    def test_register_new_file(self, db_mod, make_wad, tmp_path):
        wad_id, companion_id, companion_dir, _ = _setup_companion(db_mod, make_wad, tmp_path)

        assert companion_id > 0

        # File should be copied to managed dir
        managed_files = list(companion_dir.iterdir())
        assert len(managed_files) == 1
        assert "patch.deh" in managed_files[0].name

    def test_register_dedup_existing_md5(self, db_mod, make_wad, tmp_path):
        """If a companion with the same MD5 exists, reuse it without copying."""
        wad_id, id1, companion_dir, f = _setup_companion(db_mod, make_wad, tmp_path)
        wad_id2 = make_wad(title="WAD 2")

        with (
            patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir),
            patch("caco.services.companion_service.get_link_mode", return_value="copy"),
        ):
            id2, _ = register_companion(str(f), wad_id2)

        assert id1 == id2

    def test_register_nonexistent_file_raises(self, db_mod, make_wad, tmp_path):
        wad_id = make_wad(title="Test")
        with pytest.raises(FileNotFoundError):
            register_companion(str(tmp_path / "nonexistent.deh"), wad_id)

    def test_register_move_mode(self, db_mod, make_wad, tmp_path):
        _, _, _, f = _setup_companion(db_mod, make_wad, tmp_path, link_mode="move")
        assert not f.exists()

    def test_links_companion_to_wad(self, db_mod, make_wad, tmp_path):
        wad_id, _, _, _ = _setup_companion(db_mod, make_wad, tmp_path)
        companions = db_mod.get_wad_companions(wad_id)
        assert len(companions) == 1
        assert companions[0]["filename"] == "patch.deh"


# =============================================================================
# unregister_companion
# =============================================================================


class TestUnregisterCompanion:
    def test_unlink_with_delete_policy(self, db_mod, make_wad, tmp_path):
        wad_id, companion_id, companion_dir, _ = _setup_companion(db_mod, make_wad, tmp_path)

        deleted = unregister_companion(wad_id, companion_id, orphan_policy="delete")
        assert deleted is True
        assert db_mod.get_companion(companion_id) is None
        assert list(companion_dir.iterdir()) == []

    def test_unlink_with_keep_policy(self, db_mod, make_wad, tmp_path):
        wad_id, companion_id, _, _ = _setup_companion(db_mod, make_wad, tmp_path)

        deleted = unregister_companion(wad_id, companion_id, orphan_policy="keep")
        assert deleted is False
        assert db_mod.get_companion(companion_id) is not None

    def test_unlink_not_orphaned(self, db_mod, make_wad, tmp_path):
        """Companion linked to 2 WADs — unlinking from one doesn't orphan it."""
        wad_id1, companion_id, _, _ = _setup_companion(db_mod, make_wad, tmp_path)
        wad_id2 = make_wad(title="WAD 2")
        db_mod.link_companion(wad_id2, companion_id)

        deleted = unregister_companion(wad_id1, companion_id, orphan_policy="delete")
        assert deleted is False

        companions = db_mod.get_wad_companions(wad_id2)
        assert len(companions) == 1

    def test_unlink_nonexistent_link(self, db_mod, make_wad, tmp_path):
        wad_id = make_wad(title="Test")
        deleted = unregister_companion(wad_id, 99999, orphan_policy="delete")
        assert deleted is False

    def test_unlink_reads_config_policy_default(self, db_mod, make_wad, tmp_path):
        wad_id, companion_id, _, _ = _setup_companion(db_mod, make_wad, tmp_path)

        with patch("caco.services.companion_service.get_companion_orphan_cleanup", return_value="keep"):
            deleted = unregister_companion(wad_id, companion_id)

        assert deleted is False
