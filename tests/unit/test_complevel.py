"""Tests for the shared complevel module."""

import pytest

from caco.complevel import COMPLEVEL_ALIASES, COMPLEVEL_NAMES, HELION_COMPLEVEL_NAMES, UZDOOM_COMPATMODE_STRICT, UZDOOM_COMPATMODE_RELAXED, complevel_name, complevel_to_helion_name, complevel_to_uzdoom_compatmode, parse_complevel


class TestComplevelName:
    """Test complevel_name() display function."""

    def test_known_complevels(self):
        assert complevel_name(2) == "Doom v1.9 / Vanilla"
        assert complevel_name(9) == "Boom"
        assert complevel_name(11) == "MBF"
        assert complevel_name(21) == "MBF21"

    def test_unknown_complevel(self):
        assert complevel_name(99) == "Complevel 99"
        assert complevel_name(17) == "Complevel 17"

    def test_none_complevel(self):
        assert complevel_name(None) == "Unknown"


class TestParseComplevel:
    """Test parse_complevel() string->int parsing."""

    def test_integer_string(self):
        assert parse_complevel("9") == 9
        assert parse_complevel("2") == 2
        assert parse_complevel("21") == 21
        assert parse_complevel("0") == 0

    def test_aliases(self):
        assert parse_complevel("vanilla") == 2
        assert parse_complevel("boom") == 9
        assert parse_complevel("mbf") == 11
        assert parse_complevel("mbf21") == 21

    def test_case_insensitive(self):
        assert parse_complevel("BOOM") == 9
        assert parse_complevel("Vanilla") == 2
        assert parse_complevel("MBF21") == 21

    def test_invalid(self):
        assert parse_complevel("invalid") is None
        assert parse_complevel("") is None
        assert parse_complevel("hello world") is None

    def test_limit_removing_alias(self):
        assert parse_complevel("limit-removing") == 2
        assert parse_complevel("lr") == 2


class TestComplevelAliases:
    """Test that COMPLEVEL_ALIASES maps correctly."""

    def test_all_aliases_resolve(self):
        for alias, cl in COMPLEVEL_ALIASES.items():
            assert isinstance(cl, int)
            assert parse_complevel(alias) == cl


class TestComplevelToHelionName:
    """Test complevel_to_helion_name() mapping."""

    def test_vanilla(self):
        assert complevel_to_helion_name(2) == "vanilla"

    def test_boom(self):
        assert complevel_to_helion_name(9) == "boom"

    def test_mbf(self):
        assert complevel_to_helion_name(11) == "mbf"

    def test_mbf21(self):
        assert complevel_to_helion_name(21) == "mbf21"

    def test_unsupported_returns_none(self):
        assert complevel_to_helion_name(4) is None
        assert complevel_to_helion_name(0) is None
        assert complevel_to_helion_name(99) is None

    def test_all_entries_are_strings(self):
        for cl, name in HELION_COMPLEVEL_NAMES.items():
            assert isinstance(name, str)
            assert isinstance(cl, int)


class TestComplevelToUZDoomCompatmode:
    """Test complevel_to_uzdoom_compatmode() mapping."""

    def test_vanilla_strict(self):
        assert complevel_to_uzdoom_compatmode(2, strict=True) == 2

    def test_boom_strict(self):
        assert complevel_to_uzdoom_compatmode(9, strict=True) == 6

    def test_mbf_strict(self):
        assert complevel_to_uzdoom_compatmode(11, strict=True) == 7

    def test_mbf21_strict(self):
        assert complevel_to_uzdoom_compatmode(21, strict=True) == 9

    def test_vanilla_relaxed(self):
        assert complevel_to_uzdoom_compatmode(2, strict=False) == 1

    def test_boom_relaxed(self):
        assert complevel_to_uzdoom_compatmode(9, strict=False) == 3

    def test_mbf_relaxed(self):
        assert complevel_to_uzdoom_compatmode(11, strict=False) == 5

    def test_mbf21_relaxed(self):
        assert complevel_to_uzdoom_compatmode(21, strict=False) == 8

    def test_ultimate_doom_maps_to_vanilla(self):
        assert complevel_to_uzdoom_compatmode(3, strict=True) == 2
        assert complevel_to_uzdoom_compatmode(4, strict=True) == 2

    def test_unsupported_returns_none(self):
        assert complevel_to_uzdoom_compatmode(0) is None
        assert complevel_to_uzdoom_compatmode(17) is None
        assert complevel_to_uzdoom_compatmode(99) is None

    def test_default_is_strict(self):
        assert complevel_to_uzdoom_compatmode(9) == 6

    def test_strict_and_relaxed_tables_have_same_keys(self):
        assert set(UZDOOM_COMPATMODE_STRICT.keys()) == set(UZDOOM_COMPATMODE_RELAXED.keys())
