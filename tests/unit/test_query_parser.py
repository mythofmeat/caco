"""Tests for the query parser in caco.db."""

import pytest

from caco.db import (
    _glob_to_like,
    _is_glob_pattern,
    _split_or_groups,
    _parse_and_group,
    parse_query,
    normalize_status,
    _build_term_sql,
    QueryTerm,
)


class TestGlobToLike:
    def test_no_glob(self):
        assert _glob_to_like("hello") == "hello"

    def test_star_becomes_percent(self):
        assert _glob_to_like("caco*") == "caco%"

    def test_question_becomes_underscore(self):
        assert _glob_to_like("scythe?") == "scythe_"

    def test_mixed_wildcards(self):
        assert _glob_to_like("*map?") == "%map_"

    def test_escapes_existing_percent(self):
        assert _glob_to_like("100%*") == r"100\%%"

    def test_escapes_existing_underscore(self):
        assert _glob_to_like("my_map*") == r"my\_map%"


class TestIsGlobPattern:
    def test_star(self):
        assert _is_glob_pattern("caco*") is True

    def test_question(self):
        assert _is_glob_pattern("scythe?") is True

    def test_no_glob(self):
        assert _is_glob_pattern("scythe") is False

    def test_empty(self):
        assert _is_glob_pattern("") is False


class TestSplitOrGroups:
    def test_single_group(self):
        assert _split_or_groups("status:playing author:alm") == ["status:playing author:alm"]

    def test_two_groups(self):
        result = _split_or_groups("status:playing , status:to-play")
        assert result == ["status:playing", "status:to-play"]

    def test_no_spaces_around_comma(self):
        """Comma without spaces is NOT an OR separator."""
        result = _split_or_groups("status:playing,status:to-play")
        assert result == ["status:playing,status:to-play"]

    def test_three_groups(self):
        result = _split_or_groups("a , b , c")
        assert result == ["a", "b", "c"]

    def test_quoted_comma_not_split(self):
        result = _split_or_groups('"hello , world"')
        assert len(result) == 1


class TestParseAndGroup:
    def test_field_value(self):
        terms = _parse_and_group("status:playing")
        assert len(terms) == 1
        assert terms[0].field == "status"
        assert terms[0].value == "playing"
        assert terms[0].negated is False

    def test_negated_caret(self):
        terms = _parse_and_group("^status:finished")
        assert terms[0].negated is True
        assert terms[0].field == "status"
        assert terms[0].value == "finished"

    def test_negated_dash(self):
        terms = _parse_and_group("-tag:slaughter")
        assert terms[0].negated is True

    def test_free_text(self):
        terms = _parse_and_group("scythe")
        assert terms[0].field is None
        assert terms[0].value == "scythe"

    def test_multiple_terms(self):
        terms = _parse_and_group("status:playing author:alm")
        assert len(terms) == 2

    def test_name_alias(self):
        terms = _parse_and_group("name:scythe")
        assert terms[0].field == "title"


class TestParseQuery:
    def test_empty(self):
        q = parse_query("")
        assert q.is_empty()

    def test_none_like(self):
        q = parse_query("   ")
        assert q.is_empty()

    def test_single_term(self):
        q = parse_query("status:playing")
        assert len(q.or_groups) == 1
        assert len(q.or_groups[0].terms) == 1

    def test_or_groups(self):
        q = parse_query("status:playing , status:to-play")
        assert len(q.or_groups) == 2

    def test_and_within_group(self):
        q = parse_query("status:playing author:alm")
        assert len(q.or_groups) == 1
        assert len(q.or_groups[0].terms) == 2


class TestNormalizeStatus:
    def test_shortcut_p(self):
        assert normalize_status("p") == "playing"

    def test_shortcut_f(self):
        assert normalize_status("f") == "finished"

    def test_full_value(self):
        assert normalize_status("playing") == "playing"

    def test_unknown(self):
        assert normalize_status("xyz") == "xyz"


class TestBuildTermSql:
    def test_free_text(self):
        term = QueryTerm(field=None, value="scythe")
        sql, params = _build_term_sql(term)
        assert "LIKE" in sql
        assert len(params) == 3

    def test_status_field(self):
        term = QueryTerm(field="status", value="p")
        sql, params = _build_term_sql(term)
        assert "status = ?" in sql
        assert params == ["playing"]

    def test_tag_glob(self):
        term = QueryTerm(field="tag", value="caco*")
        sql, params = _build_term_sql(term)
        assert "LIKE" in sql
        assert "ESCAPE" in sql

    def test_negation(self):
        term = QueryTerm(field="status", value="finished", negated=True)
        sql, params = _build_term_sql(term)
        assert sql.startswith("NOT")

    def test_id_field(self):
        term = QueryTerm(field="id", value="42")
        sql, params = _build_term_sql(term)
        assert "id = ?" in sql
        assert params == [42]

    def test_year_field(self):
        term = QueryTerm(field="year", value="1994")
        sql, params = _build_term_sql(term)
        assert "year = ?" in sql
        assert params == [1994]

    def test_invalid_id(self):
        term = QueryTerm(field="id", value="abc")
        sql, params = _build_term_sql(term)
        assert sql == ""

    def test_iwad_field(self):
        term = QueryTerm(field="iwad", value="doom2")
        sql, params = _build_term_sql(term)
        assert "custom_iwad LIKE ?" in sql
        assert params == ["%doom2%"]

    def test_iwad_field_negated(self):
        term = QueryTerm(field="iwad", value="doom2", negated=True)
        sql, params = _build_term_sql(term)
        assert sql.startswith("NOT")

    def test_complevel_field_numeric(self):
        term = QueryTerm(field="complevel", value="9")
        sql, params = _build_term_sql(term)
        assert "custom_complevel = ?" in sql
        assert params == ["9"]

    def test_complevel_field_shortcut(self):
        term = QueryTerm(field="complevel", value="boom")
        sql, params = _build_term_sql(term)
        assert "custom_complevel = ?" in sql
        assert params == ["9"]

    def test_complevel_field_mbf21(self):
        term = QueryTerm(field="complevel", value="mbf21")
        sql, params = _build_term_sql(term)
        assert params == ["21"]

    def test_complevel_field_negated(self):
        term = QueryTerm(field="complevel", value="9", negated=True)
        sql, params = _build_term_sql(term)
        assert sql.startswith("NOT")
