"""Tests for caco.sourceports module."""

import pytest

from caco.sourceports import get_data_dir_args, identify_sourceport_family


class TestIdentifySourceportFamily:
    """Test sourceport family identification."""

    @pytest.mark.parametrize("exe,expected_save", [
        ("dsda-doom", "-save"),
        ("nyan-doom", "-save"),
        ("nugget-doom", "-save"),
        ("prboom+", "-save"),
        ("gzdoom", "-savedir"),
        ("lzdoom", "-savedir"),
        ("vkdoom", "-savedir"),
        ("chocolate-doom", "-savedir"),
        ("crispy-doom", "-savedir"),
        ("woof", "-save"),
        ("eternity", "-savedir"),
    ])
    def test_identify_known_sourceport(self, exe, expected_save):
        family = identify_sourceport_family(exe)
        assert family is not None
        assert family["save_arg"] == expected_save

    def test_identify_unknown_sourceport(self):
        assert identify_sourceport_family("my-custom-port") is None
        assert identify_sourceport_family("") is None

    def test_identify_with_path(self):
        """Full paths should match on basename."""
        family = identify_sourceport_family("/usr/bin/nyan-doom")
        assert family is not None
        assert family["save_arg"] == "-save"
        assert family["data_arg"] == "-data"

    def test_identify_with_deep_path(self):
        family = identify_sourceport_family("/opt/doom/ports/gzdoom")
        assert family is not None
        assert family["save_arg"] == "-savedir"


class TestGetDataDirArgs:
    """Test CLI arg generation for data dir redirection."""

    def test_dsda_family(self):
        args = get_data_dir_args("dsda-doom", "/tmp/data")
        assert args == ["-data", "/tmp/data", "-save", "/tmp/data"]

    def test_nyan_doom(self):
        args = get_data_dir_args("nyan-doom", "/tmp/data")
        assert args == ["-data", "/tmp/data", "-save", "/tmp/data"]

    def test_zdoom_family(self):
        args = get_data_dir_args("gzdoom", "/tmp/data")
        assert args == ["-savedir", "/tmp/data"]

    def test_chocolate_family(self):
        args = get_data_dir_args("crispy-doom", "/tmp/data")
        assert args == ["-savedir", "/tmp/data"]

    def test_woof(self):
        args = get_data_dir_args("woof", "/tmp/data")
        assert args == ["-data", "/tmp/data", "-save", "/tmp/data"]

    def test_eternity(self):
        args = get_data_dir_args("eternity", "/tmp/data")
        assert args == ["-savedir", "/tmp/data"]

    def test_unknown_returns_empty(self):
        assert get_data_dir_args("my-custom-port", "/tmp/data") == []

    def test_with_full_path(self):
        args = get_data_dir_args("/usr/bin/nyan-doom", "/tmp/data")
        assert args == ["-data", "/tmp/data", "-save", "/tmp/data"]
