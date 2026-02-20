"""CLI integration tests for library and tag commands."""

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


class TestListCommand:
    """Test 'caco list' output modes."""

    def test_list_empty(self, runner):
        result = runner.invoke(cli, ["list"])
        assert result.exit_code == 0
        assert "No WADs" in result.output

    def test_list_with_wads(self, populated_runner):
        result = populated_runner.invoke(cli, ["list"])
        assert result.exit_code == 0
        assert "Eviternity" in result.output
        assert "Sunlust" in result.output

    def test_list_json(self, populated_runner):
        result = populated_runner.invoke(cli, ["list", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert isinstance(data, list)
        assert len(data) == 5
        titles = {w["title"] for w in data}
        assert "Eviternity" in titles

    def test_list_plain(self, populated_runner):
        result = populated_runner.invoke(cli, ["list", "--plain"])
        assert result.exit_code == 0
        lines = result.output.strip().split("\n")
        # Header + 5 WADs
        assert len(lines) == 6
        assert lines[0].startswith("ID\t")

    def test_list_with_query(self, populated_runner):
        result = populated_runner.invoke(cli, ["list", "status:playing"])
        assert result.exit_code == 0
        assert "Sunlust" in result.output

    def test_list_with_sort(self, populated_runner):
        result = populated_runner.invoke(cli, ["list", "--sort", "title+"])
        assert result.exit_code == 0
        # Should not error
        assert "Library" in result.output or "WADs" in result.output

    def test_list_invalid_sort(self, populated_runner):
        result = populated_runner.invoke(cli, ["list", "--sort", "invalid"])
        assert result.exit_code == 1

    def test_list_deleted(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="To Delete")
        db_mod.delete_wad(wad_id)
        result = runner.invoke(cli, ["list", "--deleted"])
        assert result.exit_code == 0
        assert "To Delete" in result.output


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
        result = populated_runner.invoke(cli, ["info", str(wad_id), "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["title"] == "Eviternity"
        assert data["author"] == "Dragonfly"

    def test_info_plain(self, populated_runner, populated_db):
        wad_id = populated_db["eviternity"]
        result = populated_runner.invoke(cli, ["info", str(wad_id), "--plain"])
        assert result.exit_code == 0
        assert "title=Eviternity" in result.output

    def test_info_not_found(self, runner):
        result = runner.invoke(cli, ["info", "999"])
        assert result.exit_code == 1


class TestUpdateCommand:
    """Test 'caco update' modifications."""

    def test_update_title(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Original")
        result = runner.invoke(cli, ["update", str(wad_id), "--title", "Updated Title"])
        assert result.exit_code == 0
        assert "Updated" in result.output

        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "Updated Title"

    def test_update_status(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["update", str(wad_id), "--status", "playing"])
        assert result.exit_code == 0

        wad = db_mod.get_wad(wad_id)
        assert wad["status"] == "playing"

    def test_update_no_args(self, runner, make_wad):
        wad_id = make_wad()
        result = runner.invoke(cli, ["update", str(wad_id)])
        assert result.exit_code == 0
        assert "No updates" in result.output

    def test_update_dry_run(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Original")
        result = runner.invoke(cli, ["update", str(wad_id), "--title", "New", "--dry-run"])
        assert result.exit_code == 0
        assert "dry run" in result.output.lower()

        # Original title unchanged
        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "Original"


class TestDeleteRestoreCommand:
    """Test delete (soft) and restore."""

    def test_delete_soft(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Sacrifice")
        result = runner.invoke(cli, ["delete", str(wad_id), "--yes"])
        assert result.exit_code == 0
        assert "trash" in result.output.lower()

        # WAD should not appear in normal list
        wads = db_mod.search_wads()
        assert all(w["id"] != wad_id for w in wads)

    def test_restore(self, runner, make_wad, db_mod):
        wad_id = make_wad(title="Comeback")
        db_mod.delete_wad(wad_id)

        # restore uses search_wads which treats bare strings as free text
        result = runner.invoke(cli, ["restore", "Comeback"])
        assert result.exit_code == 0
        assert "Restored" in result.output

        wad = db_mod.get_wad(wad_id)
        assert wad["deleted_at"] is None


class TestTagCommands:
    """Test tag add, remove, list."""

    def test_tag_add(self, runner, make_wad, db_mod):
        wad_id = make_wad()
        result = runner.invoke(cli, ["tag", "add", str(wad_id), "megawad", "slaughter"])
        assert result.exit_code == 0
        assert "Added" in result.output

        wad = db_mod.get_wad(wad_id)
        assert "megawad" in wad["tags"]
        assert "slaughter" in wad["tags"]

    def test_tag_remove(self, runner, make_wad, db_mod):
        wad_id = make_wad(tags=["megawad", "cacoward"])
        result = runner.invoke(cli, ["tag", "remove", str(wad_id), "megawad"])
        assert result.exit_code == 0
        assert "Removed" in result.output

        wad = db_mod.get_wad(wad_id)
        assert "megawad" not in wad["tags"]
        assert "cacoward" in wad["tags"]

    def test_tag_list(self, populated_runner):
        result = populated_runner.invoke(cli, ["tag", "list"])
        assert result.exit_code == 0
        assert "megawad" in result.output

    def test_tag_list_plain(self, populated_runner):
        result = populated_runner.invoke(cli, ["tag", "list", "--plain"])
        assert result.exit_code == 0
        lines = result.output.strip().split("\n")
        assert lines[0] == "Tag\tCount"

    def test_tag_list_empty(self, runner):
        result = runner.invoke(cli, ["tag", "list"])
        assert result.exit_code == 0
        assert "No tags" in result.output


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
    """Test _parse_sort_option helper."""

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
