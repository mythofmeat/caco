"""Extended query parser tests — edge cases, SQL generation, and integration with search."""

import pytest

from caco.db import (
    QueryTerm,
    _build_term_sql,
    _build_query_sql,
    _glob_to_like,
    _split_or_groups,
    _parse_and_group,
    parse_query,
    normalize_status,
)


# =============================================================================
# Glob to LIKE conversion edge cases
# =============================================================================


class TestGlobToLikeExtended:
    def test_only_star(self):
        assert _glob_to_like("*") == "%"

    def test_only_question(self):
        assert _glob_to_like("?") == "_"

    def test_multiple_stars(self):
        assert _glob_to_like("*foo*bar*") == "%foo%bar%"

    def test_adjacent_wildcards(self):
        assert _glob_to_like("**??") == "%%__"

    def test_empty_string(self):
        assert _glob_to_like("") == ""


# =============================================================================
# OR group splitting edge cases
# =============================================================================


class TestSplitOrGroupsExtended:
    def test_empty_string(self):
        assert _split_or_groups("") == []

    def test_only_separator(self):
        result = _split_or_groups(" , ")
        assert result == []

    def test_single_quotes_preserved(self):
        result = _split_or_groups("'hello , world'")
        assert len(result) == 1

    def test_mixed_quotes(self):
        result = _split_or_groups('"a , b" , c')
        assert len(result) == 2

    def test_whitespace_trimmed(self):
        result = _split_or_groups("  a  ,  b  ")
        assert result == ["a", "b"]

    def test_leading_separator(self):
        result = _split_or_groups(" , a")
        assert result == ["a"]


# =============================================================================
# AND group parsing edge cases
# =============================================================================


class TestParseAndGroupExtended:
    def test_empty_string(self):
        terms = _parse_and_group("")
        assert terms == []

    def test_quoted_value(self):
        terms = _parse_and_group('title:"ancient aliens"')
        assert len(terms) == 1
        assert terms[0].field == "title"
        assert terms[0].value == "ancient aliens"

    def test_free_text_negation(self):
        terms = _parse_and_group("-slaughter")
        assert terms[0].negated is True
        assert terms[0].field is None
        assert terms[0].value == "slaughter"

    def test_colon_in_value(self):
        """Only first colon splits field:value."""
        terms = _parse_and_group("title:doom:2")
        assert terms[0].field == "title"
        assert terms[0].value == "doom:2"

    def test_single_dash_is_free_text(self):
        """A lone '-' is treated as a regular token (len==1, not negated)."""
        terms = _parse_and_group("-")
        assert len(terms) == 1
        assert terms[0].value == "-"

    def test_multiple_free_text_terms(self):
        terms = _parse_and_group("ancient aliens megawad")
        assert len(terms) == 3
        assert all(t.field is None for t in terms)


# =============================================================================
# Full query parsing edge cases
# =============================================================================


class TestParseQueryExtended:
    def test_complex_query(self):
        q = parse_query('status:playing author:"erik alm" , status:to-play tag:megawad')
        assert len(q.or_groups) == 2
        assert len(q.or_groups[0].terms) == 2
        assert len(q.or_groups[1].terms) == 2

    def test_only_negation(self):
        q = parse_query("^status:finished")
        assert len(q.or_groups) == 1
        assert q.or_groups[0].terms[0].negated is True

    def test_mixed_negation_and_positive(self):
        q = parse_query("status:playing ^tag:slaughter")
        terms = q.or_groups[0].terms
        assert terms[0].negated is False
        assert terms[1].negated is True

    def test_id_query(self):
        q = parse_query("id:42")
        assert q.or_groups[0].terms[0].field == "id"
        assert q.or_groups[0].terms[0].value == "42"


# =============================================================================
# Status normalization — all shortcuts
# =============================================================================


class TestNormalizeStatusComplete:
    """Test every documented status shortcut."""

    @pytest.mark.parametrize("shortcut,expected", [
        ("t", "to-play"), ("tp", "to-play"), ("toplay", "to-play"),
        ("b", "backlog"), ("back", "backlog"),
        ("p", "playing"), ("play", "playing"),
        ("f", "finished"), ("fin", "finished"), ("done", "finished"),
        ("a", "abandoned"), ("drop", "abandoned"), ("dropped", "abandoned"),
        ("w", "awaiting-update"), ("au", "awaiting-update"),
        ("await", "awaiting-update"), ("waiting", "awaiting-update"),
        ("wip", "awaiting-update"),
    ])
    def test_shortcut(self, shortcut, expected):
        assert normalize_status(shortcut) == expected

    def test_full_status_passthrough(self):
        for status in ["to-play", "backlog", "playing", "finished", "abandoned", "awaiting-update"]:
            assert normalize_status(status) == status

    def test_case_insensitive(self):
        assert normalize_status("P") == "playing"
        assert normalize_status("DONE") == "finished"


# =============================================================================
# SQL generation
# =============================================================================


class TestBuildTermSqlExtended:
    def test_title_field(self):
        term = QueryTerm(field="title", value="scythe")
        sql, params = _build_term_sql(term)
        assert "title LIKE" in sql
        assert params == ["%scythe%"]

    def test_author_field(self):
        term = QueryTerm(field="author", value="ribbiks")
        sql, params = _build_term_sql(term)
        assert "author LIKE" in sql
        assert params == ["%ribbiks%"]

    def test_filename_field(self):
        term = QueryTerm(field="filename", value="scythe")
        sql, params = _build_term_sql(term)
        assert "filename LIKE" in sql

    def test_source_field(self):
        term = QueryTerm(field="source", value="IDGAMES")
        sql, params = _build_term_sql(term)
        assert "source_type = ?" in sql
        assert params == ["idgames"]

    def test_config_field(self):
        term = QueryTerm(field="config", value="controller")
        sql, params = _build_term_sql(term)
        assert "custom_config LIKE" in sql

    def test_unknown_field_becomes_free_text(self):
        term = QueryTerm(field="nosuchfield", value="test")
        sql, params = _build_term_sql(term)
        assert "title LIKE" in sql
        assert len(params) == 3

    def test_tag_exact_match(self):
        """Non-glob tag query uses substring LIKE."""
        term = QueryTerm(field="tag", value="megawad")
        sql, params = _build_term_sql(term)
        assert "tag LIKE" in sql
        assert "ESCAPE" in sql

    def test_year_invalid_returns_empty(self):
        term = QueryTerm(field="year", value="not-a-year")
        sql, params = _build_term_sql(term)
        assert sql == ""


class TestBuildQuerySql:
    def test_empty_query(self):
        q = parse_query("")
        sql, params = _build_query_sql(q)
        assert sql == ""
        assert params == []

    def test_single_term(self):
        q = parse_query("status:playing")
        sql, params = _build_query_sql(q)
        assert "status = ?" in sql
        assert "playing" in params

    def test_or_query(self):
        q = parse_query("status:playing , status:finished")
        sql, params = _build_query_sql(q)
        assert " OR " in sql
        assert params == ["playing", "finished"]

    def test_and_query(self):
        q = parse_query("status:playing author:test")
        sql, params = _build_query_sql(q)
        assert " AND " in sql


# =============================================================================
# QueryTerm repr
# =============================================================================


class TestQueryTermRepr:
    def test_field_value(self):
        t = QueryTerm(field="status", value="playing")
        assert repr(t) == "status:playing"

    def test_negated(self):
        t = QueryTerm(field="tag", value="slaughter", negated=True)
        assert repr(t) == "-tag:slaughter"

    def test_free_text(self):
        t = QueryTerm(field=None, value="scythe")
        assert repr(t) == "scythe"

    def test_negated_free_text(self):
        t = QueryTerm(field=None, value="doom", negated=True)
        assert repr(t) == "-doom"
