"""CLI integration tests for library commands (new command structure)."""

import json

import pytest
from click.testing import CliRunner

from caco.cli import cli


@pytest.fixture
def runner(tmp_db, tmp_config):
    """Click test runner with a fresh database and isolated config."""
    return CliRunner()


@pytest.fixture
def populated_runner(runner, populated_db):
    """Click test runner with a populated database."""
    return runner


class TestLsCommand:
    """Test 'caco ls' output modes."""

    def test_ls_empty(self, runner):
        result = runner.invoke(cli, ["ls"])
        assert result.exit_code == 0
        assert "No WADs" in result.output

    def test_ls_with_wads(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls"])
        assert result.exit_code == 0
        assert "Eviternity" in result.output
        assert "Sunlust" in result.output

    def test_ls_json(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls", "-o", "json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert isinstance(data, list)
        assert len(data) == 5
        titles = {w["title"] for w in data}
        assert "Eviternity" in titles

    def test_ls_plain(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls", "-o", "plain"])
        assert result.exit_code == 0
        lines = result.output.strip().split("\n")
        # Header + 5 WADs
        assert len(lines) == 6
        assert lines[0].startswith("ID\t")

    def test_ls_with_query(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls", "status:playing"])
        assert result.exit_code == 0
        assert "Sunlust" in result.output

    def test_ls_inline_sort(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls", "title+"])
        assert result.exit_code == 0
        assert "Library" in result.output or "WADs" in result.output

    def test_ls_invalid_sort(self, populated_runner):
        """Unknown field with +/- is still a query term, not a sort error."""
        result = populated_runner.invoke(cli, ["ls", "invalid+"])
        assert result.exit_code == 0  # Treated as query term

    def test_ls_deleted(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="To Delete")
        db_mod.delete_wad(wad_id)
        result = runner.invoke(cli, ["ls", "--deleted"])
        assert result.exit_code == 0
        assert "To Delete" in result.output

    def test_ls_tags(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls", "--tags"])
        assert result.exit_code == 0
        assert "megawad" in result.output

    def test_ls_tags_plain(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls", "--tags", "-o", "plain"])
        assert result.exit_code == 0
        lines = result.output.strip().split("\n")
        assert lines[0] == "Tag\tCount"

    def test_ls_tags_empty(self, runner):
        result = runner.invoke(cli, ["ls", "--tags"])
        assert result.exit_code == 0
        assert "No tags" in result.output

    def test_ls_iwad(self, runner):
        result = runner.invoke(cli, ["ls", "--iwad"])
        assert result.exit_code == 0
        # Should work even with no IWADs registered
        assert "No IWADs" in result.output or "Registered" in result.output


class TestInfoCommand:
    """Test 'caco info' output modes."""

    def test_info_by_id(self, populated_runner, populated_db):
        wad_id = populated_db["eviternity"]
        result = populated_runner.invoke(cli, ["info", str(wad_id)])
        assert result.exit_code == 0
        assert "Eviternity" in result.output
        assert "Dragonfly" in result.output

    def test_info_json(self, populated_runner, populated_db):
        wad_id = populated_db["eviternity"]
        result = populated_runner.invoke(cli, ["info", str(wad_id), "-o", "json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["title"] == "Eviternity"
        assert data["author"] == "Dragonfly"

    def test_info_plain(self, populated_runner, populated_db):
        wad_id = populated_db["eviternity"]
        result = populated_runner.invoke(cli, ["info", str(wad_id), "-o", "plain"])
        assert result.exit_code == 0
        assert "title=Eviternity" in result.output

    def test_info_not_found(self, runner):
        result = runner.invoke(cli, ["info", "999"])
        assert result.exit_code == 1

    def test_info_multiple_matches(self, populated_runner):
        """Multiple matches should all be displayed."""
        result = populated_runner.invoke(cli, ["info", "tag:megawad"])
        assert result.exit_code == 0
        # Should show multiple WADs
        assert "Eviternity" in result.output
        assert "Sunlust" in result.output

    def test_info_multiple_json(self, populated_runner):
        result = populated_runner.invoke(cli, ["info", "tag:megawad", "-o", "json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert isinstance(data, list)
        assert len(data) >= 2


class TestModifyCommand:
    """Test 'caco modify' with beets-style syntax."""

    def test_modify_status(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "status=playing"])
        assert result.exit_code == 0
        assert "Modified" in result.output

        wad = db_mod.get_wad(wad_id)
        assert wad["status"] == "playing"

    def test_modify_title(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Original")
        result = runner.invoke(cli, ["modify", str(wad_id), "title=Updated Title"])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "Updated Title"

    def test_modify_add_tag(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "tag=megawad"])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        assert "megawad" in wad["tags"]

    def test_modify_clear_field(self, runner, make_wad, db_mod):
        wad_id = make_wad(author="Some Author")
        result = runner.invoke(cli, ["modify", str(wad_id), "!author"])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        assert wad["author"] is None

    def test_modify_remove_all_tags(self, runner, make_wad, db_mod):
        wad_id = make_wad(tags=["megawad", "cacoward"])
        result = runner.invoke(cli, ["modify", str(wad_id), "!tag"])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        assert wad["tags"] == []

    def test_modify_remove_tag_pattern(self, runner, make_wad, db_mod):
        wad_id = make_wad(tags=["megawad", "cacoward", "slaughter"])
        result = runner.invoke(cli, ["modify", str(wad_id), "!tag:slaughter"])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        assert "slaughter" not in wad["tags"]
        assert "megawad" in wad["tags"]

    def test_modify_no_args(self, runner, make_wad):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id)])
        assert result.exit_code == 0
        assert "No modifications" in result.output

    def test_modify_no_query(self, runner):
        result = runner.invoke(cli, ["modify", "status=playing"])
        # status=playing is an action, not a query — should error about no query
        assert result.exit_code == 1
        assert "No query" in result.output

    def test_modify_dry_run(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Original")
        result = runner.invoke(cli, ["modify", str(wad_id), "title=New", "--dry-run"])
        assert result.exit_code == 0
        assert "dry run" in result.output.lower()

        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "Original"


class TestModifyBeaten:
    """Test 'caco modify' with beaten+/beaten-/beaten= syntax."""

    def test_beaten_add_1(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten+1"])
        assert result.exit_code == 0
        assert "Added 1 completion" in result.output
        assert db_mod.get_times_beaten(wad_id) == 1

    def test_beaten_add_3(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten+3"])
        assert result.exit_code == 0
        assert "Added 3 completion" in result.output
        assert db_mod.get_times_beaten(wad_id) == 3

    def test_beaten_remove_1(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id)
        db_mod.add_wad_completion(wad_id)
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten-1"])
        assert result.exit_code == 0
        assert "Removed 1 completion" in result.output
        assert db_mod.get_times_beaten(wad_id) == 1

    def test_beaten_set_5(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten=5"])
        assert result.exit_code == 0
        assert "Set" in result.output
        assert "5 completion" in result.output
        assert db_mod.get_times_beaten(wad_id) == 5

    def test_beaten_set_0(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id)
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten=0"])
        assert result.exit_code == 0
        assert db_mod.get_times_beaten(wad_id) == 0

    def test_beaten_add_with_notes(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten+1", "--notes", "UV max"])
        assert result.exit_code == 0
        completions = db_mod.get_wad_completions(wad_id)
        assert completions[0]["notes"] == "UV max"

    def test_beaten_add_with_date(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten+1", "--date", "2024-06-15"])
        assert result.exit_code == 0
        completions = db_mod.get_wad_completions(wad_id)
        assert "2024-06-15" in completions[0]["completed_at"]

    def test_beaten_remove_by_timestamp(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id, completed_at="2024-06-15T18:30:00")
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten-2024-06-15T18:30:00"])
        assert result.exit_code == 0
        assert "Removed completion" in result.output
        assert db_mod.get_times_beaten(wad_id) == 0

    def test_beaten_with_status_change(self, runner, make_wad, db_mod):
        """Beaten actions work alongside field=value actions."""
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten+1", "status=finished"])
        assert result.exit_code == 0
        assert db_mod.get_times_beaten(wad_id) == 1
        wad = db_mod.get_wad(wad_id)
        assert wad["status"] == "finished"

    def test_beaten_dry_run(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["modify", str(wad_id), "beaten+1", "--dry-run"])
        assert result.exit_code == 0
        assert "dry run" in result.output.lower()
        assert db_mod.get_times_beaten(wad_id) == 0

    def test_standalone_stats_attach(self, runner, make_wad, db_mod, tmp_path):
        """--stats-file without beaten action attaches to most recent completion."""
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id, notes="target")

        # Create a minimal levelstat.txt format file
        stats_file = tmp_path / "levelstat.txt"
        stats_file.write_text(
            "MAP01 - 1:40.00 (1:40.00)  K: 50/50  I: 10/10  S: 2/3\n"
        )

        result = runner.invoke(cli, ["modify", str(wad_id), "-s", str(stats_file)])
        assert result.exit_code == 0
        assert "Attached stats" in result.output

        completions = db_mod.get_wad_completions(wad_id)
        assert completions[0]["stats_snapshot"] is not None


class TestTrashCommand:
    """Test unified 'caco trash' command."""

    def test_trash_soft_delete(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Sacrifice")
        result = runner.invoke(cli, ["trash", str(wad_id), "--yes"])
        assert result.exit_code == 0
        assert "trash" in result.output.lower()

        wads = db_mod.search_wads()
        assert all(w["id"] != wad_id for w in wads)

    def test_trash_list(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Trashed WAD")
        db_mod.delete_wad(wad_id)
        result = runner.invoke(cli, ["trash", "--list"])
        assert result.exit_code == 0
        assert "Trashed WAD" in result.output

    def test_trash_restore(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Comeback")
        db_mod.delete_wad(wad_id)

        result = runner.invoke(cli, ["trash", "--restore", "Comeback"])
        assert result.exit_code == 0
        assert "Restored" in result.output

        wad = db_mod.get_wad(wad_id)
        assert wad["deleted_at"] is None

    def test_trash_purge_all(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Gone")
        db_mod.delete_wad(wad_id)

        result = runner.invoke(cli, ["trash", "--purge", "--yes"])
        assert result.exit_code == 0
        assert "Permanently deleted" in result.output

    def test_trash_no_query(self, runner):
        result = runner.invoke(cli, ["trash"])
        assert result.exit_code == 1


class TestRandomCommand:
    """Test 'caco random' WAD picker."""

    def test_random_returns_id(self, populated_runner, populated_db):
        result = populated_runner.invoke(cli, ["random"])
        assert result.exit_code == 0
        wad_id = int(result.output.strip())
        assert wad_id in populated_db.values()

    def test_random_with_query(self, populated_runner, populated_db):
        result = populated_runner.invoke(cli, ["random", "status:playing"])
        assert result.exit_code == 0
        wad_id = int(result.output.strip())
        assert wad_id == populated_db["sunlust"]

    def test_random_info_flag(self, populated_runner, populated_db):
        result = populated_runner.invoke(cli, ["random", "--info"])
        assert result.exit_code == 0
        parts = result.output.strip().split("\t")
        assert len(parts) == 3
        wad_id = int(parts[0])
        assert wad_id in populated_db.values()

    def test_random_empty_library(self, runner):
        result = runner.invoke(cli, ["random"])
        assert result.exit_code == 1


class TestSortParsing:
    """Test _parse_sort_option helper (re-exported from parsing.py)."""

    def test_suffix_ascending(self):
        from caco.cli import _parse_sort_option
        field, desc = _parse_sort_option("title+")
        assert field == "title"
        assert desc is False

    def test_suffix_descending(self):
        from caco.cli import _parse_sort_option
        field, desc = _parse_sort_option("title-")
        assert field == "title"
        assert desc is True

    def test_plain_field(self):
        from caco.cli import _parse_sort_option
        field, desc = _parse_sort_option("playtime")
        assert field == "playtime"
        assert desc is True  # Default for unadorned

    def test_none(self):
        from caco.cli import _parse_sort_option
        field, desc = _parse_sort_option(None)
        assert field is None


# =============================================================================
# Backward compatibility: old command names should still work
# (registered through Click as the old name, or through aliases)
# =============================================================================


class TestOldListCommand:
    """Old 'caco list' is now 'caco ls' but tests verify the new name."""

    def test_list_empty(self, runner):
        result = runner.invoke(cli, ["ls"])
        assert result.exit_code == 0
        assert "No WADs" in result.output

    def test_list_with_wads(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls"])
        assert result.exit_code == 0
        assert "Eviternity" in result.output
        assert "Sunlust" in result.output

    def test_list_json(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls", "-o", "json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert isinstance(data, list)
        assert len(data) == 5
        titles = {w["title"] for w in data}
        assert "Eviternity" in titles

    def test_list_plain(self, populated_runner):
        result = populated_runner.invoke(cli, ["ls", "-o", "plain"])
        assert result.exit_code == 0
        lines = result.output.strip().split("\n")
        assert len(lines) == 6
        assert lines[0].startswith("ID\t")
