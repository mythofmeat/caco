"""Tests for caco.saves — save file discovery, backup, restore, clean."""

import zipfile
from pathlib import Path
from unittest.mock import patch

import pytest

from caco.saves import (
    clean_save_files,
    create_backup,
    find_save_files,
    list_all_backups,
    list_backups,
    resolve_backup_path,
    restore_backup,
)


@pytest.fixture
def data_dir(tmp_path):
    """Create a mock WAD data directory with save files and stats."""
    d = tmp_path / "data" / "1_test-wad"
    d.mkdir(parents=True)

    # Create save files
    (d / "save1.dsg").write_bytes(b"\x00" * 100)
    (d / "save2.dsg").write_bytes(b"\x00" * 200)

    # Create a zdoom save
    (d / "save3.zds").write_bytes(b"\x00" * 150)

    # Create non-save files (stats, config)
    (d / "stats.txt").write_text("MAP01 - 1:23.45")
    (d / "config.cfg").write_text("some config")

    return d


@pytest.fixture
def nested_data_dir(tmp_path):
    """Create a dsda-style nested data directory."""
    d = tmp_path / "data" / "2_nested-wad"
    d.mkdir(parents=True)

    # dsda nested structure
    nested = d / "dsda_doom_data" / "doom2" / "mywad"
    nested.mkdir(parents=True)
    (nested / "save1.dsg").write_bytes(b"\x00" * 300)
    (nested / "stats.txt").write_text("MAP01 stats")

    return d


@pytest.fixture
def backup_dir(tmp_path):
    """Provide a temporary backup directory."""
    d = tmp_path / "backups"
    d.mkdir()
    return d


# =============================================================================
# find_save_files
# =============================================================================


class TestFindSaveFiles:
    def test_finds_dsg_and_zds(self, data_dir):
        saves = find_save_files(data_dir)
        names = {s["name"] for s in saves}
        assert names == {"save1.dsg", "save2.dsg", "save3.zds"}

    def test_excludes_non_save_files(self, data_dir):
        saves = find_save_files(data_dir)
        names = {s["name"] for s in saves}
        assert "stats.txt" not in names
        assert "config.cfg" not in names

    def test_returns_correct_fields(self, data_dir):
        saves = find_save_files(data_dir)
        s = next(s for s in saves if s["name"] == "save1.dsg")
        assert s["size"] == 100
        assert s["rel_path"] == "save1.dsg"
        assert isinstance(s["path"], Path)
        assert "mtime_iso" in s

    def test_finds_nested_saves(self, nested_data_dir):
        saves = find_save_files(nested_data_dir)
        assert len(saves) == 1
        assert saves[0]["name"] == "save1.dsg"
        assert "dsda_doom_data" in saves[0]["rel_path"]

    def test_nonexistent_dir(self, tmp_path):
        saves = find_save_files(tmp_path / "nope")
        assert saves == []

    def test_empty_dir(self, tmp_path):
        d = tmp_path / "empty"
        d.mkdir()
        saves = find_save_files(d)
        assert saves == []


# =============================================================================
# create_backup / restore_backup
# =============================================================================


class TestBackupRestore:
    def test_create_backup(self, data_dir, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            path = create_backup(1, "Test WAD", data_dir)

        assert path.exists()
        assert path.suffix == ".zip"
        assert path.name.startswith("1_test-wad_")

        # Verify zip contents
        with zipfile.ZipFile(path) as zf:
            names = zf.namelist()
            assert "save1.dsg" in names
            assert "stats.txt" in names

    def test_create_backup_nonexistent_dir(self, tmp_path, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            with pytest.raises(FileNotFoundError):
                create_backup(1, "Test", tmp_path / "nope")

    def test_restore_backup(self, data_dir, backup_dir, tmp_path):
        # Create backup
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            backup_path = create_backup(1, "Test WAD", data_dir)

        # Restore to a new location
        restore_dir = tmp_path / "restored"
        count = restore_backup(backup_path, restore_dir)

        assert count > 0
        assert (restore_dir / "save1.dsg").exists()
        assert (restore_dir / "stats.txt").exists()

    def test_restore_nonexistent_backup(self, tmp_path):
        with pytest.raises(FileNotFoundError):
            restore_backup(tmp_path / "nope.zip", tmp_path / "out")

    def test_restore_creates_dir(self, data_dir, backup_dir, tmp_path):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            backup_path = create_backup(1, "Test WAD", data_dir)

        restore_dir = tmp_path / "new" / "deep" / "dir"
        count = restore_backup(backup_path, restore_dir)
        assert count > 0
        assert restore_dir.is_dir()


# =============================================================================
# list_backups / list_all_backups
# =============================================================================


class TestListBackups:
    def test_list_backups_for_wad(self, data_dir, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            create_backup(1, "Test WAD", data_dir)
            create_backup(1, "Test WAD", data_dir)
            create_backup(2, "Other WAD", data_dir)

            backups = list_backups(1)
            assert len(backups) == 2
            assert all(b["name"].startswith("1_") for b in backups)

    def test_list_backups_empty(self, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            assert list_backups(99) == []

    def test_list_backups_no_dir(self, tmp_path):
        with patch("caco.saves.get_backup_dir", return_value=tmp_path / "nope"):
            assert list_backups(1) == []

    def test_list_all_backups(self, data_dir, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            create_backup(1, "Test WAD", data_dir)
            create_backup(2, "Other WAD", data_dir)

            backups = list_all_backups()
            assert len(backups) == 2
            wad_ids = {b["wad_id"] for b in backups}
            assert wad_ids == {1, 2}

    def test_list_backups_sorted_newest_first(self, data_dir, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            first = create_backup(1, "Test WAD", data_dir)
            second = create_backup(1, "Test WAD", data_dir)

            backups = list_backups(1)
            # Newest first
            assert backups[0]["name"] == second.name
            assert backups[1]["name"] == first.name


# =============================================================================
# clean_save_files
# =============================================================================


class TestCleanSaveFiles:
    def test_deletes_save_files(self, data_dir):
        deleted = clean_save_files(data_dir)
        names = {p.name for p in deleted}
        assert names == {"save1.dsg", "save2.dsg", "save3.zds"}

    def test_keeps_non_save_files(self, data_dir):
        clean_save_files(data_dir)
        assert (data_dir / "stats.txt").exists()
        assert (data_dir / "config.cfg").exists()

    def test_clean_empty_dir(self, tmp_path):
        d = tmp_path / "empty"
        d.mkdir()
        deleted = clean_save_files(d)
        assert deleted == []


# =============================================================================
# resolve_backup_path
# =============================================================================


class TestResolveBackupPath:
    def test_resolve_latest(self, data_dir, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            create_backup(1, "Test WAD", data_dir)
            latest = create_backup(1, "Test WAD", data_dir)

            result = resolve_backup_path(1)
            assert result == latest

    def test_resolve_by_filename(self, data_dir, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            backup = create_backup(1, "Test WAD", data_dir)

            result = resolve_backup_path(1, backup.name)
            assert result == backup

    def test_resolve_absolute_path(self, data_dir, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            backup = create_backup(1, "Test WAD", data_dir)

            result = resolve_backup_path(1, str(backup))
            assert result == backup

    def test_resolve_no_backups(self, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            assert resolve_backup_path(99) is None

    def test_resolve_nonexistent_filename(self, backup_dir):
        with patch("caco.saves.get_backup_dir", return_value=backup_dir):
            assert resolve_backup_path(1, "nonexistent.zip") is None
