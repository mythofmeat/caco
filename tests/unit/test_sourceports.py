"""Tests for caco.sourceports module."""

from unittest.mock import patch

import pytest

from caco.sourceports import get_complevel_args, get_config_args, get_data_dir_args, get_dsda_save_dir, get_family_name, get_profile_ext, identify_sourceport_family, detect_sourceports


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
        ("Helion", "-savedir"),
        ("helion", "-savedir"),
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


class TestGetDsdaSaveDir:
    """Test dsda-family nested save directory computation."""

    def test_nyan_doom(self, tmp_path):
        result = get_dsda_save_dir("nyan-doom", str(tmp_path), "tnt", "/wads/73_DrakeRC2.wad")
        assert result == str(tmp_path / "nyan_doom_data" / "tnt" / "73_drakerc2")
        assert (tmp_path / "nyan_doom_data" / "tnt" / "73_drakerc2").is_dir()

    def test_dsda_doom(self, tmp_path):
        result = get_dsda_save_dir("dsda-doom", str(tmp_path), "doom2", "/wads/MyWad.wad")
        assert result == str(tmp_path / "dsda_doom_data" / "doom2" / "mywad")

    def test_full_path_executable(self, tmp_path):
        result = get_dsda_save_dir("/usr/bin/nyan-doom", str(tmp_path), "doom2", "/wads/test.wad")
        assert result == str(tmp_path / "nyan_doom_data" / "doom2" / "test")

    def test_creates_directory(self, tmp_path):
        save_dir = get_dsda_save_dir("dsda-doom", str(tmp_path), "doom", "/wads/e1m1.wad")
        assert (tmp_path / "dsda_doom_data" / "doom" / "e1m1").is_dir()


class TestGetDataDirArgsDsdaNested:
    """Test that dsda family uses nested save dir when iwad and wad_path are provided."""

    def test_dsda_with_iwad_and_wad_path(self, tmp_path):
        args = get_data_dir_args(
            "dsda-doom", str(tmp_path),
            iwad="doom2", wad_path="/wads/MyWad.wad",
        )
        expected_save = str(tmp_path / "dsda_doom_data" / "doom2" / "mywad")
        assert args == ["-data", str(tmp_path), "-save", expected_save]

    def test_nyan_with_iwad_and_wad_path(self, tmp_path):
        args = get_data_dir_args(
            "nyan-doom", str(tmp_path),
            iwad="tnt", wad_path="/wads/73_DrakeRC2.wad",
        )
        expected_save = str(tmp_path / "nyan_doom_data" / "tnt" / "73_drakerc2")
        assert args == ["-data", str(tmp_path), "-save", expected_save]

    def test_dsda_without_iwad_falls_back(self):
        """Without iwad, dsda family falls back to same dir for both."""
        args = get_data_dir_args("dsda-doom", "/tmp/data", wad_path="/wads/test.wad")
        assert args == ["-data", "/tmp/data", "-save", "/tmp/data"]

    def test_dsda_without_wad_path_falls_back(self):
        """Without wad_path, dsda family falls back to same dir for both."""
        args = get_data_dir_args("dsda-doom", "/tmp/data", iwad="doom2")
        assert args == ["-data", "/tmp/data", "-save", "/tmp/data"]

    def test_woof_unaffected(self):
        """Woof also has -data/-save but should NOT use nested save dir."""
        args = get_data_dir_args("woof", "/tmp/data", iwad="doom2", wad_path="/wads/test.wad")
        assert args == ["-data", "/tmp/data", "-save", "/tmp/data"]

    def test_zdoom_unaffected(self):
        """zdoom family should be completely unaffected."""
        args = get_data_dir_args("gzdoom", "/tmp/data", iwad="doom2", wad_path="/wads/test.wad")
        assert args == ["-savedir", "/tmp/data"]

    def test_unknown_unaffected(self):
        assert get_data_dir_args("unknown-port", "/tmp/data", iwad="doom2", wad_path="/wads/test.wad") == []


class TestGetComplevelArgs:
    """Test complevel CLI arg generation."""

    def test_dsda_family(self):
        assert get_complevel_args("dsda-doom", 9) == ["-complevel", "9"]

    def test_nyan_doom(self):
        assert get_complevel_args("nyan-doom", 21) == ["-complevel", "21"]

    def test_woof(self):
        assert get_complevel_args("woof", 2) == ["-complevel", "2"]

    def test_zdoom_unsupported(self):
        """zdoom family doesn't support -complevel."""
        assert get_complevel_args("gzdoom", 9) == []

    def test_chocolate_unsupported(self):
        assert get_complevel_args("chocolate-doom", 2) == []

    def test_eternity_unsupported(self):
        assert get_complevel_args("eternity", 11) == []

    def test_unknown_port(self):
        assert get_complevel_args("my-custom-port", 9) == []

    def test_with_full_path(self):
        assert get_complevel_args("/usr/bin/dsda-doom", 21) == ["-complevel", "21"]

    def test_helion_boom(self):
        assert get_complevel_args("Helion", 9) == ["+complevel", "boom"]

    def test_helion_vanilla(self):
        assert get_complevel_args("Helion", 2) == ["+complevel", "vanilla"]

    def test_helion_mbf(self):
        assert get_complevel_args("Helion", 11) == ["+complevel", "mbf"]

    def test_helion_mbf21(self):
        assert get_complevel_args("Helion", 21) == ["+complevel", "mbf21"]

    def test_helion_unsupported_complevel(self):
        """Complevel 4 (Final Doom) has no Helion name — returns []."""
        assert get_complevel_args("Helion", 4) == []

    def test_helion_with_full_path(self):
        assert get_complevel_args("/usr/bin/Helion", 9) == ["+complevel", "boom"]


class TestHelionFamily:
    """Helion-specific sourceport integration tests."""

    def test_identify_helion(self):
        family = identify_sourceport_family("Helion")
        assert family is not None
        assert family["save_arg"] == "-savedir"

    def test_identify_helion_lowercase(self):
        family = identify_sourceport_family("helion")
        assert family is not None

    def test_helion_no_data_arg(self):
        """Helion has no data_arg — only -savedir."""
        family = identify_sourceport_family("Helion")
        assert "data_arg" not in family

    def test_data_dir_args_savedir_only(self):
        args = get_data_dir_args("Helion", "/tmp/data")
        assert args == ["-savedir", "/tmp/data"]

    def test_config_args(self):
        """Helion supports -config with .ini files."""
        assert get_config_args("Helion", "/tmp/config.ini") == ["-config", "/tmp/config.ini"]

    def test_profile_ext(self):
        """Helion uses .ini extension for config profiles."""
        assert get_profile_ext("Helion") == ".ini"
        assert get_profile_ext("helion") == ".ini"

    def test_get_family_name(self):
        assert get_family_name("Helion") == "helion"
        assert get_family_name("helion") == "helion"

    def test_get_family_name_unknown(self):
        assert get_family_name("unknown-port") is None

    def test_get_family_name_dsda(self):
        assert get_family_name("dsda-doom") == "dsda"

    def test_get_family_name_with_path(self):
        assert get_family_name("/usr/bin/Helion") == "helion"


class TestUZDoom:
    def test_identify_family(self):
        family = identify_sourceport_family("uzdoom")
        assert family is not None
        assert family["save_arg"] == "-savedir"

    def test_get_family_name(self):
        assert get_family_name("uzdoom") == "uzdoom"

    def test_data_dir_args_savedir_only(self):
        args = get_data_dir_args("uzdoom", "/tmp/data")
        assert args == ["-savedir", "/tmp/data"]

    def test_config_args(self):
        """UZDoom supports -config with .ini files."""
        assert get_config_args("uzdoom", "/tmp/config.ini") == ["-config", "/tmp/config.ini"]

    def test_profile_ext(self):
        """UZDoom uses .ini extension for config profiles."""
        assert get_profile_ext("uzdoom") == ".ini"

    def test_complevel_vanilla_strict(self, monkeypatch):
        monkeypatch.setattr("caco.config.load_config", lambda: {"uzdoom_strict_compat": True})
        assert get_complevel_args("uzdoom", 2) == ["-compatmode", "2"]

    def test_complevel_boom_strict(self, monkeypatch):
        monkeypatch.setattr("caco.config.load_config", lambda: {"uzdoom_strict_compat": True})
        assert get_complevel_args("uzdoom", 9) == ["-compatmode", "6"]

    def test_complevel_mbf_strict(self, monkeypatch):
        monkeypatch.setattr("caco.config.load_config", lambda: {"uzdoom_strict_compat": True})
        assert get_complevel_args("uzdoom", 11) == ["-compatmode", "7"]

    def test_complevel_mbf21_strict(self, monkeypatch):
        monkeypatch.setattr("caco.config.load_config", lambda: {"uzdoom_strict_compat": True})
        assert get_complevel_args("uzdoom", 21) == ["-compatmode", "9"]

    def test_complevel_relaxed(self, monkeypatch):
        monkeypatch.setattr("caco.config.load_config", lambda: {"uzdoom_strict_compat": False})
        assert get_complevel_args("uzdoom", 9) == ["-compatmode", "3"]
        assert get_complevel_args("uzdoom", 2) == ["-compatmode", "1"]
        assert get_complevel_args("uzdoom", 11) == ["-compatmode", "5"]
        assert get_complevel_args("uzdoom", 21) == ["-compatmode", "8"]

    def test_complevel_unsupported(self, monkeypatch):
        monkeypatch.setattr("caco.config.load_config", lambda: {"uzdoom_strict_compat": True})
        assert get_complevel_args("uzdoom", 17) == []

    def test_complevel_default_strict(self, monkeypatch):
        """Config missing the key defaults to strict."""
        monkeypatch.setattr("caco.config.load_config", lambda: {})
        assert get_complevel_args("uzdoom", 9) == ["-compatmode", "6"]
