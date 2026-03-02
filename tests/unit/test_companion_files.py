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
# Database: companion tables exist after migration
# =============================================================================


class TestCompanionFilesColumn:
    """Test that companion tables exist after migration."""

    def test_tables_exist(self, db_mod):
        """companion_files_registry and wad_companions tables should exist."""
        from caco.db import get_connection
        with get_connection() as conn:
            tables = conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table' "
                "AND name IN ('companion_files_registry', 'wad_companions')"
            ).fetchall()
            table_names = {t["name"] for t in tables}
        assert "companion_files_registry" in table_names
        assert "wad_companions" in table_names

    def test_register_and_link(self, db_mod, make_wad, tmp_path):
        """Can register a companion and link it to a WAD."""
        from unittest.mock import patch
        from caco.services.companion_service import register_companion

        wad_id = make_wad(title="Test")
        companion = tmp_path / "music.wad"
        companion.write_bytes(b"test wad data")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                comp_id, filename = register_companion(str(companion), wad_id)

        companions = db_mod.get_wad_companions(wad_id)
        assert len(companions) == 1
        assert companions[0]["filename"] == "music.wad"

    def test_unlink_clears(self, db_mod, make_wad, tmp_path):
        """Unlinking removes the association."""
        from unittest.mock import patch
        from caco.services.companion_service import register_companion, unregister_companion

        wad_id = make_wad(title="Test")
        companion = tmp_path / "file.wad"
        companion.write_bytes(b"data")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                comp_id, _ = register_companion(str(companion), wad_id)

        unregister_companion(wad_id, comp_id, orphan_policy="keep")
        assert db_mod.get_wad_companions(wad_id) == []


# =============================================================================
# CLI: --add-file / --remove-file on modify
# =============================================================================


@pytest.fixture
def runner(tmp_db, tmp_config):
    return CliRunner()


class TestModifyCompanionFiles:
    """Test --add-file and --remove-file options on modify command."""

    def test_add_file(self, runner, make_wad, db_mod, tmp_path):
        from unittest.mock import patch

        wad_id = make_wad(title="Test")
        companion = tmp_path / "music.wad"
        companion.write_bytes(b"test data for add")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                result = runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])

        assert result.exit_code == 0
        assert "Modified" in result.output

        companions = db_mod.get_wad_companions(wad_id)
        assert len(companions) == 1
        assert companions[0]["filename"] == "music.wad"

    def test_add_multiple_files(self, runner, make_wad, db_mod, tmp_path):
        from unittest.mock import patch

        wad_id = make_wad(title="Test")
        music = tmp_path / "music.wad"
        deh = tmp_path / "patch.deh"
        music.write_bytes(b"music data")
        deh.write_bytes(b"deh data")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                result = runner.invoke(cli, [
                    "modify", str(wad_id),
                    "--add-file", str(music),
                    "--add-file", str(deh),
                ])

        assert result.exit_code == 0
        companions = db_mod.get_wad_companions(wad_id)
        assert len(companions) == 2

    def test_add_file_dedup(self, runner, make_wad, db_mod, tmp_path):
        """Adding the same file twice should not duplicate."""
        from unittest.mock import patch

        wad_id = make_wad(title="Test")
        companion = tmp_path / "music.wad"
        companion.write_bytes(b"test data dedup")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
                runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])

        companions = db_mod.get_wad_companions(wad_id)
        assert len(companions) == 1

    def test_remove_file_by_basename(self, runner, make_wad, db_mod, tmp_path):
        from unittest.mock import patch

        wad_id = make_wad(title="Test")
        companion = tmp_path / "music.wad"
        companion.write_bytes(b"data for remove")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])
                result = runner.invoke(cli, ["modify", str(wad_id), "--remove-file", "music.wad"])

        assert result.exit_code == 0
        companions = db_mod.get_wad_companions(wad_id)
        assert len(companions) == 0

    def test_info_shows_companion_files(self, runner, make_wad, db_mod, tmp_path):
        from unittest.mock import patch

        wad_id = make_wad(title="Test WAD")
        companion = tmp_path / "music.wad"
        companion.write_bytes(b"data for info")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])

        result = runner.invoke(cli, ["info", str(wad_id)])
        assert result.exit_code == 0
        assert "Companion files" in result.output
        assert "music.wad" in result.output

    def test_info_json_includes_companion_files(self, runner, make_wad, db_mod, tmp_path):
        from unittest.mock import patch

        wad_id = make_wad(title="Test WAD")
        companion = tmp_path / "music.wad"
        companion.write_bytes(b"data for json")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])

        result = runner.invoke(cli, ["info", str(wad_id), "-o", "json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert "companion_files" in data
        assert len(data["companion_files"]) == 1

    def test_info_plain_includes_companion_files(self, runner, make_wad, db_mod, tmp_path):
        from unittest.mock import patch

        wad_id = make_wad(title="Test WAD")
        companion = tmp_path / "music.wad"
        companion.write_bytes(b"data for plain")

        companion_dir = tmp_path / "companions"
        with patch("caco.services.companion_service.get_companion_dir", return_value=companion_dir):
            with patch("caco.services.companion_service.get_link_mode", return_value="copy"):
                runner.invoke(cli, ["modify", str(wad_id), "--add-file", str(companion)])

        result = runner.invoke(cli, ["info", str(wad_id), "-o", "plain"])
        assert result.exit_code == 0
        assert "companion_files=" in result.output


# =============================================================================
# Player: command building with companion files
# =============================================================================


class TestPlayerCompanionFiles:
    """Test that play() builds correct commands with companion files."""

    def _make_play_cmd(self, wad_dict, companions=None, port="dsda-doom", tmp_path=None):
        """Helper: patch play() dependencies, capture the Popen command.

        companions: list of dicts with filename, path, enabled keys
        """
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
            mock_db.get_wad_companions.return_value = companions or []
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
            "custom_iwad": None, "custom_sourceport": None,
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
            "custom_iwad": None, "custom_sourceport": None,
        }
        companions = [
            {"filename": "music.wad", "path": "/path/to/music.wad", "enabled": 1},
        ]
        cmd = self._make_play_cmd(wad, companions=companions, tmp_path=tmp_path)
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
            "custom_iwad": None, "custom_sourceport": None,
        }
        companions = [
            {"filename": "patch.deh", "path": "/path/to/patch.deh", "enabled": 1},
        ]
        cmd = self._make_play_cmd(wad, companions=companions, port="dsda-doom", tmp_path=tmp_path)
        assert "-deh" in cmd
        deh_idx = cmd.index("-deh")
        assert cmd[deh_idx + 1] == "/path/to/patch.deh"

    def test_deh_with_gzdoom(self, tmp_path):
        """DEH files use -file with zdoom-family ports."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "custom_iwad": None, "custom_sourceport": None,
        }
        companions = [
            {"filename": "patch.deh", "path": "/path/to/patch.deh", "enabled": 1},
        ]
        cmd = self._make_play_cmd(wad, companions=companions, port="gzdoom", tmp_path=tmp_path)
        assert "-deh" not in cmd
        assert "/path/to/patch.deh" in cmd

    def test_bex_treated_as_deh(self, tmp_path):
        """BEX files are handled identically to DEH."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "custom_iwad": None, "custom_sourceport": None,
        }
        companions = [
            {"filename": "patch.bex", "path": "/path/to/patch.bex", "enabled": 1},
        ]
        cmd = self._make_play_cmd(wad, companions=companions, port="dsda-doom", tmp_path=tmp_path)
        assert "-deh" in cmd
        deh_idx = cmd.index("-deh")
        assert cmd[deh_idx + 1] == "/path/to/patch.bex"

    def test_mixed_companions(self, tmp_path):
        """Mix of WAD + DEH companions produces correct command."""
        wad = {
            "id": 1, "title": "Test", "source_type": "idgames",
            "status": "backlog", "custom_args": None,
            "custom_iwad": None, "custom_sourceport": None,
        }
        companions = [
            {"filename": "music.wad", "path": "/path/to/music.wad", "enabled": 1},
            {"filename": "patch.deh", "path": "/path/to/patch.deh", "enabled": 1},
        ]
        cmd = self._make_play_cmd(wad, companions=companions, port="dsda-doom", tmp_path=tmp_path)
        # DEH should use -deh
        assert "-deh" in cmd
        deh_idx = cmd.index("-deh")
        assert cmd[deh_idx + 1] == "/path/to/patch.deh"
        # WAD companion should be in -file list
        file_idx = cmd.index("-file")
        file_args = cmd[file_idx + 1:]
        assert "/path/to/music.wad" in file_args
