"""Tests for caco.watchers.helion module."""

import json
import os
import zipfile
from pathlib import Path

import pytest

from caco.watchers.helion import (
    HelionWatcher,
    _find_latest_save,
    _get_helion_config_dir,
    _read_helion_save_skill,
)
from caco.wad_stats import _LEVELSTAT_MAP_RE


def _write_levelstat(path, maps_text):
    """Write levelstat.txt content and ensure mtime advances."""
    path.write_text(maps_text)
    # Force mtime to advance (filesystem resolution can be coarse)
    new_mtime = path.stat().st_mtime + 2
    os.utime(path, (new_mtime, new_mtime))


def _make_hsg(save_path, skill=2):
    """Create a fake .hsg ZIP with world.json containing Skill."""
    with zipfile.ZipFile(save_path, "w") as zf:
        zf.writestr("world.json", json.dumps({"Skill": skill}))


LEVELSTAT_MAP01 = "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\n"
LEVELSTAT_MAP02 = "MAP02 - 1:15.50 (1:48.47)  K: 80/90  I: 30/40  S: 2/3\n"


class TestHelionWatcher:
    """Test HelionWatcher stats collection."""

    def test_detects_mtime_change(self, tmp_path):
        """Watcher detects levelstat.txt changes and parses maps."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        levelstat = config_dir / "levelstat.txt"

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        watcher._poll_interval = 0.01

        # Record initial state (no file yet)
        watcher._last_mtime = 0.0

        # Simulate levelstat appearing
        _write_levelstat(levelstat, LEVELSTAT_MAP01)
        watcher._check_for_changes()

        result = watcher.collect()
        assert result is not None
        assert "MAP01" in result

    def test_ignores_preexisting_content(self, tmp_path):
        """Watcher ignores content that existed before start()."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        levelstat = config_dir / "levelstat.txt"
        levelstat.write_text(LEVELSTAT_MAP01)

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        # Simulate start() recording initial mtime
        watcher._last_mtime = levelstat.stat().st_mtime

        # No changes yet
        watcher._check_for_changes()
        assert watcher.collect() is None

    def test_accumulates_maps(self, tmp_path):
        """Watcher accumulates maps across multiple file updates."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        levelstat = config_dir / "levelstat.txt"

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        watcher._last_mtime = 0.0

        # First update: MAP01
        _write_levelstat(levelstat, LEVELSTAT_MAP01)
        watcher._check_for_changes()

        # Second update: MAP01 + MAP02
        _write_levelstat(levelstat, LEVELSTAT_MAP01 + LEVELSTAT_MAP02)
        watcher._check_for_changes()

        result = watcher.collect()
        assert result is not None
        assert "MAP01" in result
        assert "MAP02" in result

    def test_enriches_skill_from_save(self, tmp_path):
        """Skill is extracted from .hsg save and applied to map stats."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        levelstat = config_dir / "levelstat.txt"

        # Create a save with Skill:2 (HMP, 0-indexed)
        save_path = config_dir / "save1.hsg"
        _make_hsg(save_path, skill=2)

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        watcher._last_mtime = 0.0

        _write_levelstat(levelstat, LEVELSTAT_MAP01)
        watcher._check_for_changes()

        watcher._enrich_skill()

        # Skill 2 (0-indexed) -> best_skill 3 (1-indexed = HMP)
        for stats in watcher._accumulated:
            for m in stats.maps:
                assert m.best_skill == 3

    def test_returns_none_when_no_stats(self, tmp_path):
        """Returns None when no stats were captured."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        assert watcher.collect() is None

    def test_output_roundtrips(self, tmp_path):
        """Output matches _LEVELSTAT_MAP_RE format."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        levelstat = config_dir / "levelstat.txt"

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        watcher._last_mtime = 0.0

        _write_levelstat(levelstat, LEVELSTAT_MAP01)
        watcher._check_for_changes()

        result = watcher.collect()
        for line in result.strip().splitlines():
            assert _LEVELSTAT_MAP_RE.match(line), f"Line didn't match: {line!r}"

    def test_stop_does_final_read(self, tmp_path):
        """stop() performs a final check_for_changes."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        levelstat = config_dir / "levelstat.txt"

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        watcher._last_mtime = 0.0

        # Write stats without calling _check_for_changes first
        _write_levelstat(levelstat, LEVELSTAT_MAP01)

        # stop() should catch the final update
        watcher.stop()
        result = watcher.collect()
        assert result is not None
        assert "MAP01" in result

    def test_handles_missing_levelstat(self, tmp_path):
        """Gracefully handles missing levelstat.txt."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        watcher._check_for_changes()  # Should not raise
        assert watcher.collect() is None

    def test_handles_corrupt_save(self, tmp_path):
        """Gracefully handles corrupt/unreadable save files."""
        config_dir = tmp_path / "helion_config"
        config_dir.mkdir()
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        levelstat = config_dir / "levelstat.txt"

        # Write a corrupt "save" file
        corrupt_save = config_dir / "bad.hsg"
        corrupt_save.write_bytes(b"not a zip file")

        watcher = HelionWatcher(data_dir, helion_config_dir=config_dir)
        watcher._last_mtime = 0.0

        _write_levelstat(levelstat, LEVELSTAT_MAP01)
        watcher._check_for_changes()

        # enrich_skill should handle corrupt save gracefully
        watcher._enrich_skill()
        result = watcher.collect()
        assert result is not None  # Stats still collected, just unenriched


class TestHelionConfigDir:
    """Test _get_helion_config_dir() platform logic."""

    def test_respects_xdg_config_home(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
        monkeypatch.setattr("platform.system", lambda: "Linux")
        result = _get_helion_config_dir()
        assert result == tmp_path / "Helion"

    def test_default_linux(self, monkeypatch):
        monkeypatch.delenv("XDG_CONFIG_HOME", raising=False)
        monkeypatch.setattr("platform.system", lambda: "Linux")
        result = _get_helion_config_dir()
        assert result == Path.home() / ".config" / "Helion"


class TestReadHelionSaveSkill:
    """Test _read_helion_save_skill()."""

    def test_reads_skill(self, tmp_path):
        save = tmp_path / "test.hsg"
        _make_hsg(save, skill=2)
        assert _read_helion_save_skill(save) == 3  # 0-indexed -> 1-indexed

    def test_skill_0(self, tmp_path):
        """Skill 0 (ITYTD, 0-indexed) -> 1."""
        save = tmp_path / "test.hsg"
        _make_hsg(save, skill=0)
        assert _read_helion_save_skill(save) == 1

    def test_skill_4(self, tmp_path):
        """Skill 4 (NM, 0-indexed) -> 5."""
        save = tmp_path / "test.hsg"
        _make_hsg(save, skill=4)
        assert _read_helion_save_skill(save) == 5

    def test_returns_none_for_corrupt(self, tmp_path):
        save = tmp_path / "bad.hsg"
        save.write_bytes(b"not a zip")
        assert _read_helion_save_skill(save) is None

    def test_returns_none_for_missing_world_json(self, tmp_path):
        save = tmp_path / "empty.hsg"
        with zipfile.ZipFile(save, "w") as zf:
            zf.writestr("other.txt", "data")
        assert _read_helion_save_skill(save) is None


class TestFindLatestSave:
    """Test _find_latest_save()."""

    def test_finds_most_recent(self, tmp_path):
        old = tmp_path / "old.hsg"
        _make_hsg(old, skill=1)
        os.utime(old, (1000, 1000))

        new = tmp_path / "new.hsg"
        _make_hsg(new, skill=2)
        os.utime(new, (2000, 2000))

        assert _find_latest_save(tmp_path) == new

    def test_returns_none_when_empty(self, tmp_path):
        assert _find_latest_save(tmp_path) is None

    def test_ignores_non_hsg(self, tmp_path):
        (tmp_path / "save.dsg").write_text("data")
        assert _find_latest_save(tmp_path) is None
