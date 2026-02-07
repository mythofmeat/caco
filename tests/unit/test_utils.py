"""Tests for caco.utils module."""

from caco.utils import coerce_str, extract_year


class TestCoerceStr:
    def test_none_becomes_empty(self):
        assert coerce_str(None) == ""

    def test_string_passes_through(self):
        assert coerce_str("hello") == "hello"

    def test_empty_string_passes_through(self):
        assert coerce_str("") == ""


class TestExtractYear:
    def test_iso_date(self):
        assert extract_year("2023-03-01") == 2023

    def test_iso_datetime(self):
        assert extract_year("2023-03-01T12:00:00") == 2023

    def test_year_only(self):
        assert extract_year("1994") == 1994

    def test_none_returns_none(self):
        assert extract_year(None) is None

    def test_empty_returns_none(self):
        assert extract_year("") is None
