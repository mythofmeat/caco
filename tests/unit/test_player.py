"""Tests for caco.player module."""

import json
import os
import threading
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from caco.player import format_duration, _find_stats_files, _auto_track_stats, _read_stats_snapshot, play_iwad, PlayResult


SAMPLE_STATS_TXT = """\
1
34663
MAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3
MAP02 1 2 3 26043 -1 -1 1 91 83 71 2 83 137 5
"""

SAMPLE_LEVELSTAT_TXT = """\
MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5
MAP02 - 1:23.45 (1:56.42)  K: 80/100  I: 40/50  S: 3/5
"""


class TestPlayResult:
    def test_crashed_true_nonzero(self):
        r = PlayResult(duration=60, exit_code=255)
        assert r.crashed is True

    def test_crashed_true_negative(self):
        r = PlayResult(duration=60, exit_code=-1)
        assert r.crashed is True

    def test_crashed_false_zero(self):
        r = PlayResult(duration=60, exit_code=0)
        assert r.crashed is False

    def test_crashed_false_none(self):
        r = PlayResult(duration=60, exit_code=None)
        assert r.crashed is False

    def test_duration_none(self):
        r = PlayResult(duration=None, exit_code=0)
        assert r.duration is None
        assert r.crashed is False


class TestFormatDuration:
    @pytest.mark.parametrize("seconds,expected", [
        (0, "0s"),
        (30, "30s"),
        (59, "59s"),
        (60, "1m 0s"),
        (90, "1m 30s"),
        (3599, "59m 59s"),
        (3600, "1h 0m"),
        (3661, "1h 1m"),
        (7200, "2h 0m"),
    ])
    def test_format_duration(self, seconds, expected):
        assert format_duration(seconds) == expected


class TestFindStatsFiles:
    def test_top_level(self, tmp_path):
        """Finds stats.txt at root of data dir."""
        (tmp_path / "stats.txt").write_text(SAMPLE_STATS_TXT)
        result = _find_stats_files(tmp_path)
        assert len(result) == 1
        assert result[0].name == "stats.txt"

    def test_nested(self, tmp_path):
        """Finds stats.txt nested in subdirectory (nyan-doom layout)."""
        nested = tmp_path / "doom2" / "mywad"
        nested.mkdir(parents=True)
        (nested / "stats.txt").write_text(SAMPLE_STATS_TXT)
        result = _find_stats_files(tmp_path)
        assert len(result) == 1
        assert result[0].name == "stats.txt"

    def test_levelstat_fallback(self, tmp_path):
        """Finds levelstat.txt when no stats.txt exists."""
        (tmp_path / "levelstat.txt").write_text(SAMPLE_LEVELSTAT_TXT)
        result = _find_stats_files(tmp_path)
        assert len(result) == 1
        assert result[0].name == "levelstat.txt"

    def test_finds_both(self, tmp_path):
        """Finds both stats.txt and levelstat.txt."""
        (tmp_path / "stats.txt").write_text(SAMPLE_STATS_TXT)
        (tmp_path / "levelstat.txt").write_text(SAMPLE_LEVELSTAT_TXT)
        result = _find_stats_files(tmp_path)
        assert len(result) == 2
        names = {p.name for p in result}
        assert names == {"stats.txt", "levelstat.txt"}

    def test_multiple_nested(self, tmp_path):
        """Finds stats.txt in multiple nested dirs (IWAD/port change)."""
        doom = tmp_path / "dsda_doom_data" / "doom" / "mywad"
        doom.mkdir(parents=True)
        (doom / "stats.txt").write_text(SAMPLE_STATS_TXT)
        doom2 = tmp_path / "dsda_doom_data" / "doom2" / "mywad"
        doom2.mkdir(parents=True)
        (doom2 / "stats.txt").write_text(SAMPLE_STATS_TXT)
        result = _find_stats_files(tmp_path)
        assert len(result) == 2

    def test_missing(self, tmp_path):
        """Returns empty list when no stats file exists."""
        result = _find_stats_files(tmp_path)
        assert result == []


class TestPlayIwad:
    def test_iwad_not_found(self, tmp_path):
        """Raises FileNotFoundError for missing IWAD."""
        with patch("caco.player.resolve_iwad", return_value=str(tmp_path / "nope.wad")):
            with pytest.raises(FileNotFoundError, match="IWAD.*not found"):
                play_iwad("nope")

    def test_no_sourceport(self, tmp_path):
        """Raises ValueError when no sourceport configured."""
        wad = tmp_path / "doom2.wad"
        wad.touch()
        with (
            patch("caco.player.resolve_iwad", return_value=str(wad)),
            patch("caco.player.get_default_sourceport", return_value=None),
        ):
            with pytest.raises(ValueError, match="No sourceport"):
                play_iwad("doom2")

    def test_launches_sourceport(self, tmp_path):
        """Calls subprocess with correct args and returns PlayResult."""
        wad = tmp_path / "doom2.wad"
        wad.touch()
        mock_proc = MagicMock()
        mock_proc.wait.return_value = 0
        mock_proc.returncode = 0
        profile = tmp_path / "gzdoom" / "default.cfg"

        with (
            patch("caco.player.resolve_iwad", return_value=str(wad)),
            patch("caco.player.resolve_sourceport", return_value="/usr/bin/gzdoom"),
            patch("caco.player.shutil.which", return_value="/usr/bin/gzdoom"),
            patch("caco.player.get_sourceport_args", return_value=[]),
            patch("caco.player.get_profile_path", return_value=profile),
            patch("subprocess.Popen", return_value=mock_proc) as mock_popen,
        ):
            result = play_iwad("doom2", sourceport="gzdoom", extra_args=["-warp", "1"])
            assert isinstance(result, PlayResult)
            assert isinstance(result.duration, int)
            assert result.exit_code == 0
            assert result.crashed is False
            cmd = mock_popen.call_args[0][0]
            assert cmd[0] == "/usr/bin/gzdoom"
            assert "-iwad" in cmd
            assert str(wad) in cmd
            assert "-warp" in cmd
            assert "1" in cmd

    def test_includes_default_sourceport_args(self, tmp_path):
        """Default sourceport args from config are included."""
        wad = tmp_path / "doom2.wad"
        wad.touch()
        mock_proc = MagicMock()
        mock_proc.wait.return_value = 0
        mock_proc.returncode = 0
        profile = tmp_path / "gzdoom" / "default.cfg"

        with (
            patch("caco.player.resolve_iwad", return_value=str(wad)),
            patch("caco.player.resolve_sourceport", return_value="/usr/bin/gzdoom"),
            patch("caco.player.shutil.which", return_value="/usr/bin/gzdoom"),
            patch("caco.player.get_sourceport_args", return_value=["-nomusic"]),
            patch("caco.player.get_profile_path", return_value=profile),
            patch("subprocess.Popen", return_value=mock_proc) as mock_popen,
        ):
            play_iwad("doom2", sourceport="gzdoom")
            cmd = mock_popen.call_args[0][0]
            assert "-nomusic" in cmd


class TestReadStatsSnapshot:
    """Test _read_stats_snapshot helper."""

    def test_returns_json(self, tmp_path):
        """Returns JSON string when stats file exists."""
        data_dir = tmp_path / "1_test-wad"
        data_dir.mkdir()
        (data_dir / "stats.txt").write_text(SAMPLE_STATS_TXT)

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=data_dir),
        ):
            result = _read_stats_snapshot(1)
            assert result is not None
            import json
            data = json.loads(result)
            assert data["format"] == "stats_txt"
            assert len(data["maps"]) == 2

    def test_returns_none_no_data_dir(self):
        """Returns None when no data dir exists."""
        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=None),
        ):
            assert _read_stats_snapshot(1) is None

    def test_returns_none_disabled(self):
        """Returns None when auto_stats is disabled."""
        with patch("caco.player.get_auto_stats", return_value=False):
            assert _read_stats_snapshot(1) is None

    def test_returns_none_on_parse_error(self, tmp_path):
        """Returns None (not raises) on parse errors."""
        data_dir = tmp_path / "1_test-wad"
        data_dir.mkdir()
        (data_dir / "stats.txt").write_text("invalid\ndata\nnope")

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=data_dir),
        ):
            assert _read_stats_snapshot(1) is None


class TestAutoTrackStats:
    def test_updates_wad_and_returns_json(self, tmp_path):
        """Auto-tracking parses stats, calls update_wad, and returns JSON."""
        data_dir = tmp_path / "1_test-wad"
        data_dir.mkdir()
        (data_dir / "stats.txt").write_text(SAMPLE_STATS_TXT)

        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=data_dir),
            patch("caco.player.db") as mock_db,
        ):
            result = _auto_track_stats(1, wad)
            mock_db.update_wad.assert_called_once()
            call_args = mock_db.update_wad.call_args
            assert call_args[0][0] == 1
            assert "stats_snapshot" in call_args[1]
            assert call_args[1]["stats_snapshot"] is not None
            assert result is not None

    def test_no_data_dir_returns_none(self, tmp_path):
        """Returns None when no data dir exists."""
        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=None),
            patch("caco.player.db") as mock_db,
        ):
            result = _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()
            assert result is None

    def test_auto_stats_disabled(self, tmp_path):
        """Skips when auto_stats config is false."""
        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=False),
            patch("caco.player.db") as mock_db,
        ):
            result = _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()
            assert result is None

    def test_manage_data_dirs_disabled(self, tmp_path):
        """Skips when manage_data_dirs is false."""
        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=False),
            patch("caco.player.db") as mock_db,
        ):
            result = _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()
            assert result is None

    def test_parse_error_returns_none(self, tmp_path):
        """Parse errors are logged as warnings, returns None."""
        data_dir = tmp_path / "1_test-wad"
        data_dir.mkdir()
        (data_dir / "stats.txt").write_text("not a valid stats file\nreally\nnope")

        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=data_dir),
            patch("caco.player.db") as mock_db,
        ):
            result = _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()
            assert result is None

    def test_no_stats_file(self, tmp_path):
        """Skips when data dir exists but contains no stats file."""
        data_dir = tmp_path / "1_test-wad"
        data_dir.mkdir()

        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=data_dir),
            patch("caco.player.db") as mock_db,
        ):
            result = _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()
            assert result is None


class TestWatcherIntegration:
    """Test that play() starts/stops the stats watcher for Helion."""

    def test_helion_watcher_writes_stats_to_data_dir(self, tmp_db, tmp_path):
        """play() with Helion sourceport: watcher collects stats and writes levelstat.txt."""
        from caco import db

        # Create a WAD in the DB with a cached file
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(b"PWAD")
        wad_id = db.add_wad(
            title="Watcher Test",
            source_type=db.SourceType.LOCAL,
            source_url=str(wad_path),
        )
        db.update_wad(wad_id, cached_path=str(wad_path), custom_iwad="doom2")

        # Set up fake Helion config dir with a levelstat.txt that will appear
        helion_config_dir = tmp_path / "helion_config"
        helion_config_dir.mkdir()
        levelstat_path = helion_config_dir / "levelstat.txt"

        # Data dir where caco manages WAD data
        wad_data_dir = tmp_path / "data" / f"{wad_id}_watcher-test"
        wad_data_dir.mkdir(parents=True)

        def mock_wait():
            """Simulate sourceport: write levelstat.txt, give watcher time to poll."""
            levelstat_path.write_text(
                "MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5\n"
            )
            # Ensure mtime advances so watcher detects the change
            new_mtime = levelstat_path.stat().st_mtime + 2
            os.utime(levelstat_path, (new_mtime, new_mtime))
            # Give watcher thread time to poll and detect the change
            import time
            time.sleep(0.15)
            return 0

        mock_proc = MagicMock()
        mock_proc.wait.side_effect = mock_wait
        mock_proc.returncode = 0

        profile = tmp_path / "Helion" / "default.ini"

        with (
            patch("caco.player.get_default_sourceport", return_value="Helion"),
            patch("caco.player.resolve_sourceport", return_value="Helion"),
            patch("caco.player.get_sourceport_args", return_value=[]),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_auto_detect_iwad", return_value=False),
            patch("caco.player.get_auto_detect_complevel", return_value=False),
            patch("caco.player.get_cache_auto_clean", return_value=False),
            patch("caco.player.resolve_iwad", return_value="/path/to/doom2.wad"),
            patch("caco.player.find_wad_data_dir", return_value=wad_data_dir),
            patch("caco.player.get_wad_data_dir", return_value=wad_data_dir),
            patch("caco.player.get_profile_path", return_value=profile),
            patch("shutil.which", return_value="/usr/bin/Helion"),
            patch("subprocess.Popen", return_value=mock_proc) as mock_popen,
            patch(
                "caco.watchers.helion._get_helion_config_dir",
                return_value=helion_config_dir,
            ),
        ):
            from caco.player import play
            result = play(wad_id)

        # Verify -levelstat was injected into the command
        cmd = mock_popen.call_args[0][0]
        assert "-levelstat" in cmd, f"-levelstat not in command: {cmd}"

        # Verify the watcher wrote levelstat.txt to the data dir
        output_file = wad_data_dir / "levelstat.txt"
        assert output_file.exists(), "Watcher should have written levelstat.txt to data dir"
        content = output_file.read_text()
        assert "MAP01" in content
        assert result.exit_code == 0

    def test_dsda_no_watcher(self, tmp_db, tmp_path):
        """play() with dsda-doom: no watcher started, no extra files written."""
        from caco import db

        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(b"PWAD")
        wad_id = db.add_wad(
            title="No Watcher Test",
            source_type=db.SourceType.LOCAL,
            source_url=str(wad_path),
        )
        db.update_wad(wad_id, cached_path=str(wad_path), custom_iwad="doom2")

        wad_data_dir = tmp_path / "data" / f"{wad_id}_no-watcher-test"
        wad_data_dir.mkdir(parents=True)

        mock_proc = MagicMock()
        mock_proc.wait.return_value = 0
        mock_proc.returncode = 0
        profile = tmp_path / "dsda-doom" / "default.cfg"

        with (
            patch("caco.player.get_default_sourceport", return_value="dsda-doom"),
            patch("caco.player.resolve_sourceport", return_value="dsda-doom"),
            patch("caco.player.get_sourceport_args", return_value=[]),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_auto_detect_iwad", return_value=False),
            patch("caco.player.get_auto_detect_complevel", return_value=False),
            patch("caco.player.get_cache_auto_clean", return_value=False),
            patch("caco.player.resolve_iwad", return_value="/path/to/doom2.wad"),
            patch("caco.player.find_wad_data_dir", return_value=wad_data_dir),
            patch("caco.player.get_wad_data_dir", return_value=wad_data_dir),
            patch("caco.player.get_profile_path", return_value=profile),
            patch("shutil.which", return_value="/usr/bin/dsda-doom"),
            patch("subprocess.Popen", return_value=mock_proc),
        ):
            from caco.player import play
            play(wad_id)

        # No levelstat.txt should be written by a watcher
        assert not (wad_data_dir / "levelstat.txt").exists()
