"""Tests for caco.sourceports module."""

from unittest.mock import patch

import pytest

from caco.sourceports import get_data_dir_args, identify_sourceport_family, detect_sourceports


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


class TestDetectSourceports:
    """Test sourceport auto-detection."""

    def test_finds_installed(self):
        """Detects a sourceport that is on PATH."""
        with patch("shutil.which") as mock_which:
            mock_which.side_effect = lambda exe: "/usr/bin/gzdoom" if exe == "gzdoom" else None
            result = detect_sourceports()
            assert any(exe == "gzdoom" for exe, _path, _fam in result)
            gzdoom = [(e, p, f) for e, p, f in result if e == "gzdoom"][0]
            assert gzdoom[1] == "/usr/bin/gzdoom"
            assert gzdoom[2] == "zdoom"

    def test_finds_multiple(self):
        """Detects multiple sourceports from different families."""
        found = {"dsda-doom": "/usr/bin/dsda-doom", "gzdoom": "/usr/bin/gzdoom"}
        with patch("shutil.which") as mock_which:
            mock_which.side_effect = lambda exe: found.get(exe)
            result = detect_sourceports()
            names = [exe for exe, _path, _fam in result]
            assert "dsda-doom" in names
            assert "gzdoom" in names

    def test_none_found(self):
        """Returns empty list when nothing is installed."""
        with patch("shutil.which", return_value=None):
            result = detect_sourceports()
            assert result == []

    def test_returns_family_name(self):
        """Family name is the dict key, not executable name."""
        with patch("shutil.which") as mock_which:
            mock_which.side_effect = lambda exe: "/usr/bin/woof" if exe == "woof" else None
            result = detect_sourceports()
            assert any(fam == "woof" for _exe, _path, fam in result)
