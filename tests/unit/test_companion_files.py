"""Tests for companion files feature."""

import json

import pytest
from click.testing import CliRunner

from caco.cli import cli
from caco.sourceports import uses_deh_flag


# =============================================================================
# Sourceports: DEH flag detection
# =============================================================================


class TestUsesDehFlag:
    """Test uses_deh_flag() for sourceport family detection."""

    @pytest.mark.parametrize("exe", [
        "dsda-doom", "nyan-doom", "nugget-doom", "prboom+",
        "chocolate-doom", "crispy-doom", "woof", "eternity",
    ])
    def test_non_zdoom_uses_deh(self, exe):
        assert uses_deh_flag(exe) is True

    @pytest.mark.parametrize("exe", [
        "gzdoom", "lzdoom", "vkdoom", "qzdoom", "zdoom",
    ])
    def test_zdoom_uses_file(self, exe):
        assert uses_deh_flag(exe) is False

    def test_unknown_defaults_to_deh(self):
        assert uses_deh_flag("my-custom-port") is True

    def test_with_full_path(self):
        assert uses_deh_flag("/usr/bin/dsda-doom") is True
        assert uses_deh_flag("/usr/bin/gzdoom") is False


# =============================================================================
# Database: migration adds column
# =============================================================================


class TestCompanionFilesColumn:
    """Test that companion_files column exists after migration."""

    def test_column_exists(self, db_mod, make_wad):
        """companion_files column should be available after init_db."""
        wad_id = make_wad(title="Test")
        wad = db_mod.get_wad(wad_id)
        assert "companion_files" in wad or wad.get("companion_files") is None

    def test_update_companion_files(self, db_mod, make_wad):
        """Can set and retrieve companion_files via update_wad."""
        wad_id = make_wad(title="Test")
        files = ["/path/to/music.wad", "/path/to/patch.deh"]
        db_mod.update_wad(wad_id, companion_files=json.dumps(files))

        wad = db_mod.get_wad(wad_id)
        assert wad["companion_files"] is not None
        assert json.loads(wad["companion_files"]) == files

    def test_clear_companion_files(self, db_mod, make_wad):
        """Can clear companion_files by setting to None."""
        wad_id = make_wad(title="Test")
        db_mod.update_wad(wad_id, companion_files=json.dumps(["/path/to/file.wad"]))
        db_mod.update_wad(wad_id, companion_files=None)

        wad = db_mod.get_wad(wad_id)
        assert wad["companion_files"] is None


# =============================================================================
# CLI: --add-file / --remove-file on modify
# =============================================================================


@pytest.fixture
def runner(tmp_db, tmp_config):
    return CliRunner()


class TestModifyCompanionFiles:
    """Test --add-file and --remove-file options on modify command."""

    def test_add_file(self, runner, make_wad, db_mod, tmp_path):
        wad_id = make_wad(title="Test")
        companion = tmp_path / "music.wad"
        companion.touch()

        result = runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
        assert result.exit_code == 0
        assert "Modified" in result.output

        wad = db_mod.get_wad(wad_id)
        files = json.loads(wad["companion_files"])
        assert len(files) == 1
        assert files[0] == str(companion.resolve())

    def test_add_multiple_files(self, runner, make_wad, db_mod, tmp_path):
        wad_id = make_wad(title="Test")
        music = tmp_path / "music.wad"
        deh = tmp_path / "patch.deh"
        music.touch()
        deh.touch()

        result = runner.invoke(cli, [
            "modify", str(wad_id),
            "--add-file", str(music),
            "--add-file", str(deh),
        ])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        files = json.loads(wad["companion_files"])
        assert len(files) == 2

    def test_add_file_dedup(self, runner, make_wad, db_mod, tmp_path):
        """Adding the same file twice should not duplicate."""
        wad_id = make_wad(title="Test")
        companion = tmp_path / "music.wad"
        companion.touch()

        runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
        runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])

        wad = db_mod.get_wad(wad_id)
        files = json.loads(wad["companion_files"])
        assert len(files) == 1

    def test_remove_file_by_basename(self, runner, make_wad, db_mod, tmp_path):
        wad_id = make_wad(title="Test")
        companion = tmp_path / "music.wad"
        companion.touch()

        runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
        result = runner.invoke(cli, ["modify", str(wad_id), "--remove-file", "music.wad"])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        assert wad["companion_files"] is None

    def test_remove_file_by_full_path(self, runner, make_wad, db_mod, tmp_path):
        wad_id = make_wad(title="Test")
        companion = tmp_path / "music.wad"
        companion.touch()

        runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
        result = runner.invoke(cli, ["modify", str(wad_id), "--remove-file", str(companion.resolve())])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        assert wad["companion_files"] is None

    def test_info_shows_companion_files(self, runner, make_wad, db_mod, tmp_path):
        wad_id = make_wad(title="Test WAD")
        companion = tmp_path / "music.wad"
        companion.touch()

        runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
        result = runner.invoke(cli, ["info", str(wad_id)])
        assert result.exit_code == 0
        assert "Companion files" in result.output
        assert "music.wad" in result.output

    def test_info_json_includes_companion_files(self, runner, make_wad, db_mod, tmp_path):
        wad_id = make_wad(title="Test WAD")
        companion = tmp_path / "music.wad"
        companion.touch()

        runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
        result = runner.invoke(cli, ["info", str(wad_id), "-o", "json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert "companion_files" in data
        assert len(data["companion_files"]) == 1

    def test_info_plain_includes_companion_files(self, runner, make_wad, db_mod, tmp_path):
        wad_id = make_wad(title="Test WAD")
        companion = tmp_path / "music.wad"
        companion.touch()

        runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
        result = runner.invoke(cli, ["info", str(wad_id), "-o", "plain"])
        assert result.exit_code == 0
        assert "companion_files=" in result.output


# =============================================================================
# Player: command building with companion files
# =============================================================================


class TestPlayerCompanionFiles:
    """Test that play() builds correct commands with companion files."""

    def _make_play_cmd(self, wad_dict, port="dsda-doom", tmp_path=None):
        """Helper: patch play() dependencies, capture the Popen command."""
        from unittest.mock import patch, MagicMock

        mock_proc = MagicMock()
        mock_proc.wait.return_value = 0
        captured_cmd = []

        wad_path = tmp_path / "test.wad" if tmp_path else "/tmp/test.wad"
        if tmp_path:
            wad_path.touch()

        with (
            patch("caco.player.db") as mock_db,
            patch("caco.player.get_wad_path", return_value=wad_path),
            patch("caco.player.auto_clean_cache"),
            patch("caco.player.resolve_sourceport", return_value=port),
            patch("caco.player.shutil.which", return_value=f"/usr/bin/{port}"),
            patch("caco.player.get_default_sourceport", return_value=port),
            patch("caco.player.get_iwad", return_value=None),
            patch("caco.player.get_sourceport_args", return_value=[]),
            patch("caco.player.get_manage_data_dirs", return_value=False),
            patch("caco.player.get_auto_detect_iwad", return_value=False),
            patch("caco.player.get_auto_stats", return_value=False),
            patch("subprocess.Popen", return_value=mock_proc) as mock_popen,
        ):
            mock_db.get_wad.return_value = wad_dict
            mock_db.start_session.return_value = 1
            mock_db.get_sessions.return_value = [{"duration_seconds": 60}]

            from caco.player import play
            play(1, sourceport=port)

            captured_cmd = mock_popen.call_args[0][0]

        return captured_cmd

    def test_no_companion_files(self, tmp_path):
        """Without companion files, command has just -file <wad>."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "companion_files": None, "custom_iwad": None,
            "custom_sourceport": None,
        }
        cmd = self._make_play_cmd(wad, tmp_path=tmp_path)
        assert "-file" in cmd
        file_idx = cmd.index("-file")
        # Only one file after -file
        assert cmd[file_idx + 1].endswith("test.wad")

    def test_companion_wad(self, tmp_path):
        """Companion .wad file should appear in -file list before main WAD."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "companion_files": json.dumps(["/path/to/music.wad"]),
            "custom_iwad": None, "custom_sourceport": None,
        }
        cmd = self._make_play_cmd(wad, tmp_path=tmp_path)
        file_idx = cmd.index("-file")
        file_args = cmd[file_idx + 1:]
        # Should have companion + main WAD
        assert "/path/to/music.wad" in file_args
        assert any(a.endswith("test.wad") for a in file_args)
        # Companion should come before main WAD
        companion_idx = file_args.index("/path/to/music.wad")
        main_idx = next(i for i, a in enumerate(file_args) if a.endswith("test.wad"))
        assert companion_idx < main_idx

    def test_deh_with_dsda(self, tmp_path):
        """DEH files use -deh flag with dsda-family ports."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "companion_files": json.dumps(["/path/to/patch.deh"]),
            "custom_iwad": None, "custom_sourceport": None,
        }
        cmd = self._make_play_cmd(wad, port="dsda-doom", tmp_path=tmp_path)
        assert "-deh" in cmd
        deh_idx = cmd.index("-deh")
        assert cmd[deh_idx + 1] == "/path/to/patch.deh"

    def test_deh_with_gzdoom(self, tmp_path):
        """DEH files use -file with zdoom-family ports."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "companion_files": json.dumps(["/path/to/patch.deh"]),
            "custom_iwad": None, "custom_sourceport": None,
        }
        cmd = self._make_play_cmd(wad, port="gzdoom", tmp_path=tmp_path)
        assert "-deh" not in cmd
        assert "/path/to/patch.deh" in cmd

    def test_bex_treated_as_deh(self, tmp_path):
        """BEX files are handled identically to DEH."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "companion_files": json.dumps(["/path/to/patch.bex"]),
            "custom_iwad": None, "custom_sourceport": None,
        }
        cmd = self._make_play_cmd(wad, port="dsda-doom", tmp_path=tmp_path)
        assert "-deh" in cmd
        deh_idx = cmd.index("-deh")
        assert cmd[deh_idx + 1] == "/path/to/patch.bex"

    def test_mixed_companions(self, tmp_path):
        """Mix of WAD + DEH companions produces correct command."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "companion_files": json.dumps(["/path/to/music.wad", "/path/to/patch.deh"]),
            "custom_iwad": None, "custom_sourceport": None,
        }
        cmd = self._make_play_cmd(wad, port="dsda-doom", tmp_path=tmp_path)
        # DEH should use -deh
        assert "-deh" in cmd
        deh_idx = cmd.index("-deh")
        assert cmd[deh_idx + 1] == "/path/to/patch.deh"
        # WAD companion should be in -file list
        file_idx = cmd.index("-file")
        file_args = cmd[file_idx + 1:]
        assert "/path/to/music.wad" in file_args
