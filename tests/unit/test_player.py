"""Tests for caco.player module."""

import pytest

from caco.player import format_duration


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
