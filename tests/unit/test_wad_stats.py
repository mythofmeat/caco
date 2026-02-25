"""Tests for wad_stats parser module."""

import pytest

from caco.wad_stats import (
    MapStats,
    WadStats,
    format_stats,
    format_time_secs,
    format_time_tics,
    parse_stats_text,
    skill_name,
    stats_from_json,
    stats_to_json,
)

# Real stats.txt sample (truncated from nyan-doom/dsda-doom)
SAMPLE_STATS_TXT = """\
1
34663
MAP01 1 1 3 23193 -1 -1 1 198 127 5 1 150 7 3
MAP02 1 2 3 26043 -1 -1 1 91 83 71 2 83 137 5
MAP31 1 31 0 -1 -1 -1 0 0 0 0 0 -1 -1 -1
MAP35 1 35 4 294 294 -1 1 0 0 0 0 0 0 0
"""

SAMPLE_LEVELSTAT_TXT = """\
MAP01 - 0:32.97 (0:32.97)  K: 100/100  I: 50/50  S: 5/5
MAP02 - 1:23.45 (1:56.42)  K: 80/100  I: 40/50  S: 3/5
MAP03 - 2:10.00 (4:06.42)  K: 60/60  I: 20/20  S: 2/2
"""


class TestStatsTextParsing:
    """Test nyan-doom/dsda-doom stats.txt format."""

    def test_parse_basic(self):
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        assert stats.format == "stats_txt"
        assert stats.version == 1
        assert stats.header_total_kills == 34663
        assert len(stats.maps) == 4

    def test_parse_played_map(self):
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        m = stats.maps[0]  # MAP01
        assert m.lump == "MAP01"
        assert m.episode == 1
        assert m.map_num == 1
        assert m.best_skill == 3
        assert m.best_time == 23193
        assert m.best_max_time == -1
        assert m.best_nm_time == -1
        assert m.total_exits == 1
        assert m.cumulative_kills == 198
        assert m.kills == 127
        assert m.items == 5
        assert m.secrets == 1
        assert m.total_kills == 150
        assert m.total_items == 7
        assert m.total_secrets == 3
        assert m.played is True

    def test_parse_unplayed_map(self):
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        m = stats.maps[2]  # MAP31 (unplayed)
        assert m.lump == "MAP31"
        assert m.best_skill == 0
        assert m.best_time == -1
        assert m.total_exits == 0
        assert m.total_kills == -1
        assert m.played is False

    def test_played_maps_filter(self):
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        played = stats.played_maps
        assert len(played) == 3  # MAP01, MAP02, MAP35
        assert all(m.played for m in played)

    def test_total_time_display(self):
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        # MAP01: 23193 + MAP02: 26043 + MAP35: 294 = 49530 tics
        assert stats.total_time_display != "-"


class TestLevelstatParsing:
    """Test dsda-doom levelstat.txt format."""

    def test_parse_basic(self):
        stats = parse_stats_text(SAMPLE_LEVELSTAT_TXT)
        assert stats.format == "levelstat_txt"
        assert len(stats.maps) == 3

    def test_parse_map_entry(self):
        stats = parse_stats_text(SAMPLE_LEVELSTAT_TXT)
        m = stats.maps[0]  # MAP01
        assert m.lump == "MAP01"
        assert m.time_secs == pytest.approx(32.97)
        assert m.total_time_secs == pytest.approx(32.97)
        assert m.kills == 100
        assert m.total_kills == 100
        assert m.items == 50
        assert m.total_items == 50
        assert m.secrets == 5
        assert m.total_secrets == 5
        assert m.played is True

    def test_parse_time_accumulation(self):
        stats = parse_stats_text(SAMPLE_LEVELSTAT_TXT)
        m2 = stats.maps[1]  # MAP02
        assert m2.time_secs == pytest.approx(83.45)
        assert m2.total_time_secs == pytest.approx(116.42)

    def test_total_time_display(self):
        stats = parse_stats_text(SAMPLE_LEVELSTAT_TXT)
        # Last map's total_time_secs should be the display time
        assert stats.total_time_display == "4:06.42"


class TestAutoDetection:
    """Test format auto-detection."""

    def test_detect_stats_txt(self):
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        assert stats.format == "stats_txt"

    def test_detect_levelstat_txt(self):
        stats = parse_stats_text(SAMPLE_LEVELSTAT_TXT)
        assert stats.format == "levelstat_txt"

    def test_empty_file_raises(self):
        with pytest.raises(ValueError, match="Empty"):
            parse_stats_text("")

    def test_unrecognized_format_raises(self):
        with pytest.raises(ValueError, match="Unrecognized"):
            parse_stats_text("not a stats file\nreally not\nnope")


class TestRoundTrip:
    """Test parse → format → parse round-trip."""

    def test_stats_txt_round_trip(self):
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        text = format_stats(stats)
        stats2 = parse_stats_text(text)
        assert stats2.format == stats.format
        assert stats2.version == stats.version
        assert stats2.header_total_kills == stats.header_total_kills
        assert len(stats2.maps) == len(stats.maps)
        for a, b in zip(stats.maps, stats2.maps):
            assert a.lump == b.lump
            assert a.best_skill == b.best_skill
            assert a.best_time == b.best_time
            assert a.kills == b.kills
            assert a.total_kills == b.total_kills

    def test_levelstat_round_trip(self):
        stats = parse_stats_text(SAMPLE_LEVELSTAT_TXT)
        text = format_stats(stats)
        stats2 = parse_stats_text(text)
        assert stats2.format == stats.format
        assert len(stats2.maps) == len(stats.maps)
        for a, b in zip(stats.maps, stats2.maps):
            assert a.lump == b.lump
            assert a.kills == b.kills
            assert a.total_kills == b.total_kills
            assert a.time_secs == pytest.approx(b.time_secs, abs=0.01)


class TestJsonSerialization:
    """Test JSON round-trip for DB storage."""

    def test_stats_txt_json(self):
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        json_str = stats_to_json(stats)
        stats2 = stats_from_json(json_str)
        assert stats2.format == stats.format
        assert stats2.version == stats.version
        assert len(stats2.maps) == len(stats.maps)
        for a, b in zip(stats.maps, stats2.maps):
            assert a.lump == b.lump
            assert a.best_time == b.best_time
            assert a.kills == b.kills

    def test_levelstat_json(self):
        stats = parse_stats_text(SAMPLE_LEVELSTAT_TXT)
        json_str = stats_to_json(stats)
        stats2 = stats_from_json(json_str)
        assert stats2.format == stats.format
        assert len(stats2.maps) == len(stats.maps)
        for a, b in zip(stats.maps, stats2.maps):
            assert a.lump == b.lump
            assert a.time_secs == pytest.approx(b.time_secs)

    def test_json_full_round_trip_to_text(self):
        """Parse → JSON → back → format text should match."""
        stats = parse_stats_text(SAMPLE_STATS_TXT)
        json_str = stats_to_json(stats)
        stats2 = stats_from_json(json_str)
        text1 = format_stats(stats)
        text2 = format_stats(stats2)
        assert text1 == text2


class TestTimeFormatting:
    """Test time formatting helpers."""

    def test_tics_basic(self):
        assert format_time_tics(35 * 62) == "1:02"  # 62 seconds

    def test_tics_negative(self):
        assert format_time_tics(-1) == "-"

    def test_tics_zero(self):
        assert format_time_tics(0) == "0:00"

    def test_tics_hours(self):
        assert format_time_tics(35 * 3661) == "1:01:01"  # 1h 1m 1s

    def test_secs_basic(self):
        assert format_time_secs(32.97) == "0:32.97"

    def test_secs_minutes(self):
        assert format_time_secs(83.45) == "1:23.45"

    def test_secs_negative(self):
        assert format_time_secs(-1.0) == "-"

    def test_secs_hours(self):
        result = format_time_secs(3661.5)
        assert result == "1:01:01.50"


class TestSkillName:
    """Test skill name lookup."""

    def test_known_skills(self):
        assert skill_name(0) == "-"
        assert skill_name(1) == "ITYTD"
        assert skill_name(3) == "HMP"
        assert skill_name(4) == "UV"
        assert skill_name(5) == "NM"

    def test_unknown_skill(self):
        assert skill_name(99) == "99"
