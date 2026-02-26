"""CLI tests for beaten stats and beaten export commands."""

import json

import pytest
from click.testing import CliRunner

from caco.cli import cli
from caco.wad_stats import MapStats, WadStats, stats_to_json


def _make_stats_json(format: str = "stats_txt", maps: int = 3) -> str:
    """Build a minimal stats JSON blob for testing."""
    map_list = []
    for i in range(1, maps + 1):
        lump = f"MAP{i:02d}"
        if format == "stats_txt":
            map_list.append(MapStats(
                lump=lump, best_skill=4, best_time=3500 * i,
                total_exits=1, kills=50, total_kills=50,
                items=10, total_items=10, secrets=2, total_secrets=3,
            ))
        else:
            map_list.append(MapStats(
                lump=lump, time_secs=60.0 * i, total_time_secs=60.0 * i * (i + 1) / 2,
                kills=50, total_kills=50, items=10, total_items=10,
                secrets=2, total_secrets=3,
            ))
    return stats_to_json(WadStats(format=format, maps=map_list))


@pytest.fixture
def runner(tmp_db, tmp_config):
    return CliRunner()


@pytest.fixture
def wad_with_stats(make_wad, db_mod):
    """Create a WAD with live stats and one completion with stats."""
    stats_json = _make_stats_json()
    wad_id = make_wad(title="Stats WAD", status="playing")

    # Set live stats on the WAD
    db_mod.update_wad(wad_id, stats_snapshot=stats_json)

    # Add a completion with stats
    comp_stats = _make_stats_json(maps=2)
    comp_id = db_mod.add_wad_completion(wad_id, stats_snapshot=comp_stats, notes="first run")

    return {"wad_id": wad_id, "comp_id": comp_id}


@pytest.fixture
def wad_live_only(make_wad, db_mod):
    """Create a WAD with only live stats (no completions)."""
    stats_json = _make_stats_json()
    wad_id = make_wad(title="Live Only WAD")
    db_mod.update_wad(wad_id, stats_snapshot=stats_json)
    return wad_id


@pytest.fixture
def wad_comp_only(make_wad, db_mod):
    """Create a WAD with only completion stats (no live)."""
    wad_id = make_wad(title="Comp Only WAD")
    comp_stats = _make_stats_json(maps=2)
    comp_id = db_mod.add_wad_completion(wad_id, stats_snapshot=comp_stats)
    return {"wad_id": wad_id, "comp_id": comp_id}


@pytest.fixture
def wad_no_stats(make_wad):
    """Create a WAD with no stats at all."""
    return make_wad(title="No Stats WAD")


class TestBeatenStatsAllEntries:
    """Test beaten stats showing all entries (live + completions)."""

    def test_shows_both_live_and_completion(self, runner, wad_with_stats):
        wad_id = wad_with_stats["wad_id"]
        result = runner.invoke(cli, ["beaten", "stats", str(wad_id)])
        assert result.exit_code == 0
        assert "Current (live)" in result.output
        assert "Completion #" in result.output
        assert "Map Statistics" in result.output

    def test_shows_only_live_when_no_completions(self, runner, wad_live_only):
        result = runner.invoke(cli, ["beaten", "stats", str(wad_live_only)])
        assert result.exit_code == 0
        assert "Current (live)" in result.output
        assert "Completion #" not in result.output

    def test_shows_only_completion_when_no_live(self, runner, wad_comp_only):
        wad_id = wad_comp_only["wad_id"]
        result = runner.invoke(cli, ["beaten", "stats", str(wad_id)])
        assert result.exit_code == 0
        assert "Completion #" in result.output
        assert "Current (live)" not in result.output

    def test_no_stats_available(self, runner, wad_no_stats):
        result = runner.invoke(cli, ["beaten", "stats", str(wad_no_stats)])
        assert result.exit_code == 0
        assert "No stats available" in result.output


class TestBeatenStatsLive:
    """Test beaten stats --live flag."""

    def test_live_flag_shows_only_live(self, runner, wad_with_stats):
        wad_id = wad_with_stats["wad_id"]
        result = runner.invoke(cli, ["beaten", "stats", str(wad_id), "--live"])
        assert result.exit_code == 0
        assert "Current (live)" in result.output
        assert "Completion #" not in result.output

    def test_live_flag_no_live_stats(self, runner, wad_comp_only):
        wad_id = wad_comp_only["wad_id"]
        result = runner.invoke(cli, ["beaten", "stats", str(wad_id), "--live"])
        assert result.exit_code == 0
        assert "No live stats" in result.output


class TestBeatenStatsCompletionId:
    """Test beaten stats with specific completion ID."""

    def test_specific_completion(self, runner, wad_with_stats):
        wad_id = wad_with_stats["wad_id"]
        comp_id = wad_with_stats["comp_id"]
        result = runner.invoke(cli, ["beaten", "stats", str(wad_id), str(comp_id)])
        assert result.exit_code == 0
        assert f"Completion #{comp_id}" in result.output
        assert "Current (live)" not in result.output

    def test_nonexistent_completion(self, runner, wad_with_stats):
        wad_id = wad_with_stats["wad_id"]
        result = runner.invoke(cli, ["beaten", "stats", str(wad_id), "999"])
        assert result.exit_code == 0
        assert "not found" in result.output


class TestBeatenStatsPlain:
    """Test beaten stats --plain output."""

    def test_plain_all_entries(self, runner, wad_with_stats):
        wad_id = wad_with_stats["wad_id"]
        result = runner.invoke(cli, ["beaten", "stats", str(wad_id), "--plain"])
        assert result.exit_code == 0
        assert "# Current (live)" in result.output
        assert "# Completion #" in result.output

    def test_plain_live_only(self, runner, wad_with_stats):
        wad_id = wad_with_stats["wad_id"]
        result = runner.invoke(cli, ["beaten", "stats", str(wad_id), "--live", "--plain"])
        assert result.exit_code == 0
        assert "# Current (live)" in result.output
        assert "# Completion #" not in result.output


class TestBeatenExportLive:
    """Test beaten export --live flag."""

    def test_export_live(self, runner, wad_with_stats):
        wad_id = wad_with_stats["wad_id"]
        result = runner.invoke(cli, ["beaten", "export", str(wad_id), "--live"])
        assert result.exit_code == 0
        # Should export stats.txt format data
        assert "MAP01" in result.output

    def test_export_live_no_stats(self, runner, wad_no_stats):
        result = runner.invoke(cli, ["beaten", "export", str(wad_no_stats), "--live"])
        assert result.exit_code == 0
        assert "No live stats" in result.output

    def test_export_live_to_file(self, runner, wad_with_stats, tmp_path):
        wad_id = wad_with_stats["wad_id"]
        outfile = str(tmp_path / "exported.txt")
        result = runner.invoke(cli, ["beaten", "export", str(wad_id), "--live", "-o", outfile])
        assert result.exit_code == 0
        assert "Exported" in result.output
        from pathlib import Path
        content = Path(outfile).read_text()
        assert "MAP01" in content

    def test_export_fallback_to_live(self, runner, wad_live_only):
        """When no completion has stats, export falls back to live stats."""
        result = runner.invoke(cli, ["beaten", "export", str(wad_live_only)])
        assert result.exit_code == 0
        assert "MAP01" in result.output
