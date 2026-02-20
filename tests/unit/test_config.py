"""Tests for config loading, saving, and round-trip behavior."""

import pytest

from caco import config


class TestLoadConfig:
    """Test load_config with various file states."""

    def test_load_missing_file(self, tmp_config):
        """Missing config file should return defaults."""
        cfg = config.load_config()
        assert cfg["sourceport"] == ""
        assert cfg["cache_dir"] == str(config.CACHE_DIR)
        assert cfg["iwad"] == ""

    def test_load_existing_file(self, tmp_config):
        """Config file with user values should override defaults."""
        tmp_config.write_text('sourceport = "gzdoom"\niwad = "doom2"\n')
        config._config_cache = None  # Clear cache

        cfg = config.load_config()
        assert cfg["sourceport"] == "gzdoom"
        assert cfg["iwad"] == "doom2"
        # Defaults still present for unspecified keys
        assert "cache_dir" in cfg

    def test_load_malformed_toml(self, tmp_config, capsys):
        """Malformed TOML should warn and return defaults."""
        tmp_config.write_text("not valid [[[toml stuff")
        config._config_cache = None

        cfg = config.load_config()
        # Should fall back to defaults
        assert cfg["sourceport"] == ""
        # Should print a warning to stderr
        captured = capsys.readouterr()
        assert "Invalid TOML" in captured.err or "Warning" in captured.err

    def test_load_caching(self, tmp_config):
        """Second load should return cached copy."""
        tmp_config.write_text('sourceport = "dsda-doom"\n')
        config._config_cache = None

        cfg1 = config.load_config()
        cfg2 = config.load_config()
        assert cfg1 == cfg2
        # Returns copies, not same object
        assert cfg1 is not cfg2

    def test_load_returns_copy(self, tmp_config):
        """Mutating returned config should not affect cache."""
        config._config_cache = None
        cfg = config.load_config()
        cfg["sourceport"] = "MUTATED"
        cfg2 = config.load_config()
        assert cfg2["sourceport"] != "MUTATED"


class TestSaveConfig:
    """Test save_config serialization."""

    def test_save_and_reload(self, tmp_config):
        """Save then load should round-trip basic values."""
        config._config_cache = None
        cfg = config.load_config()
        cfg["sourceport"] = "gzdoom"
        cfg["download_mirror"] = 2
        config.save_config(cfg)

        config._config_cache = None
        cfg2 = config.load_config()
        assert cfg2["sourceport"] == "gzdoom"
        assert cfg2["download_mirror"] == 2

    def test_save_with_list(self, tmp_config):
        """Lists should round-trip correctly."""
        config._config_cache = None
        cfg = config.load_config()
        cfg["iwad_dirs"] = ["/opt/doom", "/home/user/iwads"]
        config.save_config(cfg)

        config._config_cache = None
        cfg2 = config.load_config()
        assert cfg2["iwad_dirs"] == ["/opt/doom", "/home/user/iwads"]

    def test_save_with_nested_section(self, tmp_config):
        """Nested dicts should be emitted as TOML [sections]."""
        config._config_cache = None
        cfg = config.load_config()
        cfg["tui"] = {"default_tab": "playing", "default_sort": "playtime"}
        config.save_config(cfg)

        config._config_cache = None
        cfg2 = config.load_config()
        assert cfg2["tui"]["default_tab"] == "playing"
        assert cfg2["tui"]["default_sort"] == "playtime"

    def test_save_invalidates_cache(self, tmp_config):
        """save_config should clear _config_cache."""
        config._config_cache = {"fake": True}
        config.save_config(config.DEFAULT_CONFIG.copy())
        assert config._config_cache is None


class TestSectionConfig:
    """Test _merge_section_config and config section helpers."""

    def test_tui_defaults(self, tmp_config):
        """get_tui_config with no [tui] section returns defaults."""
        config._config_cache = None
        tui = config.get_tui_config()
        assert tui["default_tab"] == "all"
        assert tui["default_sort"] == "id"
        assert tui["default_sort_desc"] is False

    def test_tui_override(self, tmp_config):
        """User [tui] section overrides defaults."""
        tmp_config.write_text(
            'sourceport = "gzdoom"\n\n'
            "[tui]\n"
            'default_tab = "playing"\n'
            "default_sort_desc = true\n"
        )
        config._config_cache = None

        tui = config.get_tui_config()
        assert tui["default_tab"] == "playing"
        assert tui["default_sort_desc"] is True
        # Non-overridden key keeps default
        assert tui["default_sort"] == "id"

    def test_gui_defaults(self, tmp_config):
        """get_gui_config with no [gui] section returns defaults."""
        config._config_cache = None
        gui = config.get_gui_config()
        assert gui["default_view"] == "list"
        assert gui["window_width"] == 1200
        assert gui["thumbnail_size"] == 160

    def test_list_config_defaults(self, tmp_config):
        """get_list_config returns default columns."""
        config._config_cache = None
        lc = config.get_list_config()
        assert "id" in lc["format"]
        assert "title" in lc["format"]

    def test_merge_ignores_unknown_keys(self, tmp_config):
        """Unknown keys in user config section are NOT merged."""
        tmp_config.write_text(
            '[tui]\n'
            'default_tab = "finished"\n'
            'unknown_key = "should be ignored"\n'
        )
        config._config_cache = None
        tui = config.get_tui_config()
        assert tui["default_tab"] == "finished"
        assert "unknown_key" not in tui


class TestResolveIwad:
    """Test IWAD resolution from iwad_dirs."""

    def test_absolute_existing(self, tmp_path, tmp_config):
        """Absolute path to existing file returns as-is."""
        wad = tmp_path / "doom2.wad"
        wad.touch()
        result = config.resolve_iwad(str(wad))
        assert result == str(wad)

    def test_search_iwad_dirs(self, tmp_path, tmp_config):
        """Short name resolves against iwad_dirs."""
        iwad_dir = tmp_path / "iwads"
        iwad_dir.mkdir()
        (iwad_dir / "doom2.wad").touch()

        # Set iwad_dirs
        config._config_cache = None
        cfg = config.load_config()
        cfg["iwad_dirs"] = [str(iwad_dir)]
        config.save_config(cfg)
        config._config_cache = None

        result = config.resolve_iwad("doom2")
        assert result == str(iwad_dir / "doom2.wad")

    def test_not_found_returns_name(self, tmp_config):
        """Unresolvable name is returned unchanged."""
        config._config_cache = None
        result = config.resolve_iwad("nonexistent")
        assert result == "nonexistent"


class TestHelpers:
    """Test individual config helper functions."""

    def test_get_set_sourceport(self, tmp_config):
        config._config_cache = None
        assert config.get_default_sourceport() is None

        config.set_default_sourceport("dsda-doom")
        assert config.get_default_sourceport() == "dsda-doom"

    def test_get_set_cache_dir(self, tmp_config):
        config._config_cache = None
        config.set_cache_dir("/tmp/test_cache")
        assert str(config.get_cache_dir()) == "/tmp/test_cache"

    def test_get_sourceport_args(self, tmp_config):
        config._config_cache = None
        assert config.get_sourceport_args() == []

        config.set_sourceport_args(["-iwad", "doom2.wad"])
        assert config.get_sourceport_args() == ["-iwad", "doom2.wad"]

    def test_cache_config_defaults(self, tmp_config):
        config._config_cache = None
        assert config.get_cache_max_size() == 0
        assert config.get_cache_max_age() == 0
        assert config.get_cache_auto_clean() is False
