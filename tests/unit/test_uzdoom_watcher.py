"""Tests for caco.watchers.uzdoom module."""

import zipfile
from pathlib import Path

import pytest

from caco.watchers.uzdoom import (
    UZDoomWatcher,
    _STATS_RE,
    _ensure_pk3,
)
from caco.wad_stats import _LEVELSTAT_MAP_RE


# Simulated console log lines as UZDoom would write them
LOG_MAP01 = "CACOSTATS|MAP01|3|1050|50/100|20/30|3/5\n"
LOG_MAP02 = "CACOSTATS|MAP02|3|2100|80/90|25/40|2/3\n"
LOG_MAP01_UPDATED = "CACOSTATS|MAP01|3|2450|75/100|28/30|5/5\n"

# Log with noise (other console output mixed in)
LOG_WITH_NOISE = """\
GZDoom version 4.13.0 - UZDoom variant
M_LoadDefaults: Load system defaults.
CACOSTATS|MAP01|3|350|10/100|2/30|0/5
W_Init: Init WADfiles.
CACOSTATS|MAP01|3|700|25/100|5/30|1/5
some other console output
CACOSTATS|MAP01|3|1050|50/100|20/30|3/5
"""

# Log with skill -1 (unknown)
LOG_NO_SKILL = "CACOSTATS|E1M1|-1|700|10/50|3/20|1/3\n"


def _write_log(path, text):
    """Write log file content."""
    path.write_text(text)


class TestUZDoomWatcher:
    """Test UZDoomWatcher stats collection."""

    def test_parses_stats_lines(self, tmp_path):
        """Watcher parses CACOSTATS lines from log file."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        _write_log(log_path, LOG_MAP01)
        watcher._read_new_lines()

        result = watcher.collect()
        assert result is not None
        assert "MAP01" in result

    def test_updates_map_stats(self, tmp_path):
        """Later STATS lines overwrite earlier ones for the same map."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        _write_log(log_path, LOG_MAP01)
        watcher._read_new_lines()

        # Append updated stats
        with open(log_path, "a") as f:
            f.write(LOG_MAP01_UPDATED)
        watcher._read_new_lines()

        result = watcher.collect()
        assert result is not None
        # Should have the updated kills (75, not 50)
        assert "75/100" in result

    def test_accumulates_multiple_maps(self, tmp_path):
        """Watcher tracks stats for multiple maps."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        _write_log(log_path, LOG_MAP01 + LOG_MAP02)
        watcher._read_new_lines()

        result = watcher.collect()
        assert result is not None
        assert "MAP01" in result
        assert "MAP02" in result

    def test_filters_noise(self, tmp_path):
        """Watcher ignores non-CACOSTATS lines in the log."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        _write_log(log_path, LOG_WITH_NOISE)
        watcher._read_new_lines()

        result = watcher.collect()
        assert result is not None
        # Should only have MAP01 with the LATEST values
        assert "MAP01" in result
        assert "50/100" in result  # last CACOSTATS line values

    def test_returns_none_when_no_stats(self, tmp_path):
        """Returns None when no CACOSTATS lines were captured."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()

        watcher = UZDoomWatcher(data_dir)
        assert watcher.collect() is None

    def test_handles_missing_log_file(self, tmp_path):
        """Gracefully handles missing log file."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()

        watcher = UZDoomWatcher(data_dir)
        watcher._read_new_lines()  # Should not raise
        assert watcher.collect() is None

    def test_stop_does_final_read(self, tmp_path):
        """stop() performs a final read of the log file."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        _write_log(log_path, LOG_MAP01)

        # stop() should catch the stats
        watcher.stop()
        result = watcher.collect()
        assert result is not None
        assert "MAP01" in result

    def test_incremental_reads(self, tmp_path):
        """Watcher reads incrementally from the log file."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)

        # First write
        _write_log(log_path, LOG_MAP01)
        watcher._read_new_lines()

        # Second write (append)
        with open(log_path, "a") as f:
            f.write(LOG_MAP02)
        watcher._read_new_lines()

        result = watcher.collect()
        assert "MAP01" in result
        assert "MAP02" in result

    def test_skill_conversion(self, tmp_path):
        """Skill is converted from 0-indexed to 1-indexed."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        # skill=3 (0-indexed UV) -> best_skill=4 (1-indexed)
        _write_log(log_path, "CACOSTATS|MAP01|3|1050|50/100|20/30|3/5\n")
        watcher._read_new_lines()

        # Check internal map_stats
        assert watcher._map_stats["MAP01"][0] == 3  # raw skill
        # Collect converts to 1-indexed
        result = watcher.collect()
        assert result is not None

    def test_unknown_skill_defaults_to_uv(self, tmp_path):
        """Skill -1 (unknown) defaults to best_skill=4 (UV)."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        _write_log(log_path, LOG_NO_SKILL)
        watcher._read_new_lines()

        assert watcher._map_stats["E1M1"][0] == -1  # raw skill
        # Should still produce valid output
        result = watcher.collect()
        assert result is not None
        assert "E1M1" in result

    def test_output_roundtrips(self, tmp_path):
        """Output matches levelstat.txt format."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        _write_log(log_path, LOG_MAP01 + LOG_MAP02)
        watcher._read_new_lines()

        result = watcher.collect()
        for line in result.strip().splitlines():
            assert _LEVELSTAT_MAP_RE.match(line), f"Line didn't match: {line!r}"

    def test_time_conversion(self, tmp_path):
        """Map time in tics is correctly converted to seconds."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        log_path = data_dir / "uzdoom_stats.log"

        watcher = UZDoomWatcher(data_dir)
        # 1050 tics / 35 = 30.0 seconds
        _write_log(log_path, "CACOSTATS|MAP01|3|1050|50/100|20/30|3/5\n")
        watcher._read_new_lines()

        result = watcher.collect()
        assert result is not None
        # 30 seconds = 0:30.00
        assert "0:30.00" in result

    def test_extra_args(self, tmp_path):
        """extra_args() returns -file pk3 and +logfile args."""
        data_dir = tmp_path / "data"
        data_dir.mkdir()
        mod_dir = tmp_path / "mods"
        mod_dir.mkdir()

        watcher = UZDoomWatcher(data_dir, mod_dir=mod_dir)
        args = watcher.extra_args()

        assert args[0] == "-file"
        assert args[1].endswith("caco_stats_reporter.pk3")
        assert args[2] == "+logfile"
        assert args[3] == str(data_dir / "uzdoom_stats.log")


class TestEnsurePk3:
    """Test _ensure_pk3() mod creation."""

    def test_creates_pk3(self, tmp_path):
        """Creates a valid .pk3 (ZIP) file."""
        pk3_path = _ensure_pk3(mod_dir=tmp_path)
        assert pk3_path.exists()
        assert pk3_path.name == "caco_stats_reporter.pk3"

        # Verify it's a valid ZIP with expected contents
        with zipfile.ZipFile(pk3_path, "r") as zf:
            names = zf.namelist()
            assert "zscript.zs" in names
            assert "MAPINFO" in names

            # Verify ZScript content
            zscript = zf.read("zscript.zs").decode()
            assert "CacoStatsReporter" in zscript
            assert "CACOSTATS" in zscript

            # Verify MAPINFO content
            mapinfo = zf.read("MAPINFO").decode()
            assert "CacoStatsReporter" in mapinfo
            assert "AddEventHandlers" in mapinfo

    def test_idempotent(self, tmp_path):
        """Calling twice returns same path without error."""
        path1 = _ensure_pk3(mod_dir=tmp_path)
        path2 = _ensure_pk3(mod_dir=tmp_path)
        assert path1 == path2

    def test_creates_parent_dirs(self, tmp_path):
        """Creates parent directories if they don't exist."""
        nested = tmp_path / "a" / "b" / "c"
        pk3_path = _ensure_pk3(mod_dir=nested)
        assert pk3_path.exists()


class TestStatsRegex:
    """Test the CACOSTATS regex pattern."""

    def test_matches_standard_line(self):
        line = "CACOSTATS|MAP01|3|1050|50/100|20/30|3/5"
        m = _STATS_RE.search(line)
        assert m is not None
        assert m.group(1) == "MAP01"
        assert m.group(2) == "3"
        assert m.group(3) == "1050"
        assert m.group(4) == "50"
        assert m.group(5) == "100"

    def test_matches_with_prefix(self):
        """Matches even with GZDoom log prefixes."""
        line = "[2024-01-01] CACOSTATS|E1M1|2|700|10/50|3/20|1/3"
        m = _STATS_RE.search(line)
        assert m is not None
        assert m.group(1) == "E1M1"

    def test_matches_negative_skill(self):
        line = "CACOSTATS|MAP01|-1|1050|50/100|20/30|3/5"
        m = _STATS_RE.search(line)
        assert m is not None
        assert m.group(2) == "-1"

    def test_no_match_on_garbage(self):
        assert _STATS_RE.search("just some random log output") is None
        assert _STATS_RE.search("STATS|MAP01|1050") is None
