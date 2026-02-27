"""Tests for the CLI argument parsing module."""

import pytest
import click

from caco.cli.parsing import (
    SORT_FIELDS,
    ModifyAction,
    extract_sort_from_args,
    parse_modify_args,
    _parse_sort_option,
)


class TestExtractSortFromArgs:
    """Test extract_sort_from_args()."""

    def test_no_sort(self):
        remaining, sort = extract_sort_from_args(["status:playing", "author:alm"])
        assert remaining == ["status:playing", "author:alm"]
        assert sort is None

    def test_ascending_sort(self):
        remaining, sort = extract_sort_from_args(["status:playing", "title+"])
        assert remaining == ["status:playing"]
        assert sort == "title+"

    def test_descending_sort(self):
        remaining, sort = extract_sort_from_args(["playtime-"])
        assert remaining == []
        assert sort == "playtime-"

    def test_bare_field_not_sort(self):
        """Bare field names without +/- are query terms, not sort terms."""
        remaining, sort = extract_sort_from_args(["title"])
        assert remaining == ["title"]
        assert sort is None

    def test_unknown_field_not_sort(self):
        """Unknown field with +/- suffix is kept as a query term."""
        remaining, sort = extract_sort_from_args(["unknown+"])
        assert remaining == ["unknown+"]
        assert sort is None

    def test_multiple_sort_error(self):
        with pytest.raises(click.UsageError, match="Multiple sort"):
            extract_sort_from_args(["title+", "playtime-"])

    def test_empty_args(self):
        remaining, sort = extract_sort_from_args([])
        assert remaining == []
        assert sort is None

    def test_all_sort_fields(self):
        """Every known sort field should be recognized with + or -."""
        for field in SORT_FIELDS:
            _, sort = extract_sort_from_args([f"{field}+"])
            assert sort == f"{field}+"
            _, sort = extract_sort_from_args([f"{field}-"])
            assert sort == f"{field}-"


class TestParseModifyArgs:
    """Test parse_modify_args()."""

    def test_set_status(self):
        query, actions, sort = parse_modify_args(["id:1", "status=playing"])
        assert query == ["id:1"]
        assert len(actions) == 1
        assert actions[0].field == "status"
        assert actions[0].value == "playing"
        assert actions[0].action == "set"

    def test_set_rating(self):
        query, actions, sort = parse_modify_args(["id:1", "rating=4"])
        assert actions[0].field == "rating"
        assert actions[0].value == "4"
        assert actions[0].action == "set"

    def test_set_iwad_maps_to_custom_iwad(self):
        query, actions, sort = parse_modify_args(["id:1", "iwad=doom2"])
        assert actions[0].field == "custom_iwad"
        assert actions[0].value == "doom2"

    def test_add_tag(self):
        query, actions, sort = parse_modify_args(["id:1", "tag=megawad"])
        assert actions[0].action == "add_tag"
        assert actions[0].value == "megawad"

    def test_clear_field(self):
        query, actions, sort = parse_modify_args(["id:1", "!author"])
        assert actions[0].field == "author"
        assert actions[0].action == "clear"

    def test_remove_all_tags(self):
        query, actions, sort = parse_modify_args(["id:1", "!tag"])
        assert actions[0].action == "remove_all_tags"

    def test_remove_tag_pattern(self):
        query, actions, sort = parse_modify_args(["id:1", "!tag:slaughter"])
        assert actions[0].action == "remove_tag"
        assert actions[0].pattern == "slaughter"

    def test_mixed_query_and_actions(self):
        query, actions, sort = parse_modify_args([
            "status:playing", "author:alm", "rating=5", "tag=great"
        ])
        assert query == ["status:playing", "author:alm"]
        assert len(actions) == 2
        assert actions[0].field == "rating"
        assert actions[1].action == "add_tag"

    def test_invalid_status(self):
        with pytest.raises(click.UsageError, match="Invalid status"):
            parse_modify_args(["id:1", "status=invalid_xyz_status"])

    def test_invalid_rating(self):
        with pytest.raises(click.UsageError, match="Rating must be 1-5"):
            parse_modify_args(["id:1", "rating=10"])

    def test_invalid_year(self):
        with pytest.raises(click.UsageError, match="Year must be an integer"):
            parse_modify_args(["id:1", "year=abc"])

    def test_unknown_clear_field(self):
        with pytest.raises(click.UsageError, match="Unknown field"):
            parse_modify_args(["id:1", "!nonexistent"])

    def test_sort_in_modify(self):
        query, actions, sort = parse_modify_args(["id:1", "title+", "status=playing"])
        assert sort == "title+"
        assert query == ["id:1"]
        assert len(actions) == 1

    def test_status_shortcut(self):
        query, actions, sort = parse_modify_args(["id:1", "status=p"])
        assert actions[0].value == "playing"

    def test_sourceport_maps_to_custom_sourceport(self):
        query, actions, sort = parse_modify_args(["id:1", "sourceport=dsda-doom"])
        assert actions[0].field == "custom_sourceport"

    def test_idgames_id_maps(self):
        query, actions, sort = parse_modify_args(["id:1", "idgames-id=12345"])
        assert actions[0].field == "idgames_id"

    def test_complevel_numeric(self):
        query, actions, sort = parse_modify_args(["id:1", "complevel=9"])
        assert actions[0].field == "custom_complevel"
        assert actions[0].value == "9"

    def test_complevel_shortcut_boom(self):
        query, actions, sort = parse_modify_args(["id:1", "complevel=boom"])
        assert actions[0].field == "custom_complevel"
        assert actions[0].value == "9"

    def test_complevel_shortcut_mbf21(self):
        query, actions, sort = parse_modify_args(["id:1", "complevel=mbf21"])
        assert actions[0].value == "21"

    def test_complevel_shortcut_vanilla(self):
        query, actions, sort = parse_modify_args(["id:1", "complevel=vanilla"])
        assert actions[0].value == "2"

    def test_complevel_invalid(self):
        with pytest.raises(click.UsageError, match="Invalid complevel"):
            parse_modify_args(["id:1", "complevel=notreal"])

    def test_clear_complevel(self):
        query, actions, sort = parse_modify_args(["id:1", "!complevel"])
        assert actions[0].field == "custom_complevel"
        assert actions[0].action == "clear"


class TestParseSortOption:
    """Test _parse_sort_option helper."""

    def test_suffix_ascending(self):
        field, desc = _parse_sort_option("title+")
        assert field == "title"
        assert desc is False

    def test_suffix_descending(self):
        field, desc = _parse_sort_option("title-")
        assert field == "title"
        assert desc is True

    def test_plain_field(self):
        field, desc = _parse_sort_option("playtime")
        assert field == "playtime"
        assert desc is True  # Default for unadorned

    def test_none(self):
        field, desc = _parse_sort_option(None)
        assert field is None
