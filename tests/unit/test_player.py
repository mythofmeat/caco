"""Tests for caco.player module."""

from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from caco.player import format_duration, _find_stats_file, _auto_track_stats


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


class TestFindStatsFile:
    def test_top_level(self, tmp_path):
        """Finds stats.txt at root of data dir."""
        (tmp_path / "stats.txt").write_text(SAMPLE_STATS_TXT)
        result = _find_stats_file(tmp_path)
        assert result is not None
        assert result.name == "stats.txt"

    def test_nested(self, tmp_path):
        """Finds stats.txt nested in subdirectory (nyan-doom layout)."""
        nested = tmp_path / "doom2" / "mywad"
        nested.mkdir(parents=True)
        (nested / "stats.txt").write_text(SAMPLE_STATS_TXT)
        result = _find_stats_file(tmp_path)
        assert result is not None
        assert result.name == "stats.txt"

    def test_levelstat_fallback(self, tmp_path):
        """Falls back to levelstat.txt when no stats.txt exists."""
        (tmp_path / "levelstat.txt").write_text(SAMPLE_LEVELSTAT_TXT)
        result = _find_stats_file(tmp_path)
        assert result is not None
        assert result.name == "levelstat.txt"

    def test_prefers_stats_txt(self, tmp_path):
        """Prefers stats.txt over levelstat.txt when both exist."""
        (tmp_path / "stats.txt").write_text(SAMPLE_STATS_TXT)
        (tmp_path / "levelstat.txt").write_text(SAMPLE_LEVELSTAT_TXT)
        result = _find_stats_file(tmp_path)
        assert result is not None
        assert result.name == "stats.txt"

    def test_missing(self, tmp_path):
        """Returns None when no stats file exists."""
        result = _find_stats_file(tmp_path)
        assert result is None


class TestAutoTrackStats:
    def test_updates_wad(self, tmp_path):
        """Auto-tracking parses stats and calls update_wad with snapshot."""
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
            _auto_track_stats(1, wad)
            mock_db.update_wad.assert_called_once()
            call_args = mock_db.update_wad.call_args
            assert call_args[0][0] == 1
            assert "stats_snapshot" in call_args[1]
            assert call_args[1]["stats_snapshot"] is not None

    def test_no_data_dir(self, tmp_path):
        """Gracefully skips when no data dir exists."""
        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=None),
            patch("caco.player.db") as mock_db,
        ):
            _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()

    def test_auto_stats_disabled(self, tmp_path):
        """Skips when auto_stats config is false."""
        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=False),
            patch("caco.player.db") as mock_db,
        ):
            _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()

    def test_manage_data_dirs_disabled(self, tmp_path):
        """Skips when manage_data_dirs is false."""
        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=False),
            patch("caco.player.db") as mock_db,
        ):
            _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()

    def test_parse_error_logged(self, tmp_path):
        """Parse errors are logged as warnings, not raised."""
        data_dir = tmp_path / "1_test-wad"
        data_dir.mkdir()
        (data_dir / "stats.txt").write_text("not a valid stats file\nreally\nnope")

        wad = {"id": 1, "title": "Test WAD"}

        with (
            patch("caco.player.get_auto_stats", return_value=True),
            patch("caco.player.get_manage_data_dirs", return_value=True),
            patch("caco.player.find_wad_data_dir", return_value=data_dir),
            patch("caco.player.db") as mock_db,
            patch("caco.player.logger") as mock_logger,
        ):
            # Should not raise
            _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()
            mock_logger.warning.assert_called_once()

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
            _auto_track_stats(1, wad)
            mock_db.update_wad.assert_not_called()
