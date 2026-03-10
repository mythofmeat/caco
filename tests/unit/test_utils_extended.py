"""Comprehensive tests for caco.utils module — all public functions.

Extends test_utils.py with coverage for format_rating, format_author_year,
truncate, format_size, compute_md5, parse_wad_directory, CacoSourceError,
and BaseHttpClient. Tests for coerce_str and extract_year that already exist
in test_utils.py are not duplicated here — only new edge cases are added.
"""

import struct
from pathlib import Path

import pytest

from caco.utils import (
    coerce_str,
    compute_md5,
    extract_year,
    format_author_year,
    format_rating,
    format_size,
    parse_wad_directory,
    truncate,
    CacoSourceError,
    BaseHttpClient,
)


# =============================================================================
# coerce_str — only edge cases not in test_utils.py
# =============================================================================


class TestCoerceStrEdgeCases:
    def test_integer_passes_through(self):
        assert coerce_str(42) == 42

    def test_zero_is_not_none(self):
        assert coerce_str(0) == 0


# =============================================================================
# format_rating
# =============================================================================


class TestFormatRating:
    def test_none_returns_empty(self):
        assert format_rating(None) == ""

    def test_zero_returns_empty(self):
        assert format_rating(0) == ""

    @pytest.mark.parametrize("rating,filled,empty", [
        (5, 5, 0),
        (3, 3, 2),
        (1, 1, 4),
    ])
    def test_star_counts(self, rating, filled, empty):
        result = format_rating(rating)
        assert result == "\u2605" * filled + "\u2606" * empty

    def test_custom_max(self):
        result = format_rating(3, max_stars=3)
        assert result == "\u2605\u2605\u2605"

    def test_custom_max_partial(self):
        result = format_rating(2, max_stars=10)
        assert len(result) == 10
        assert result.count("\u2605") == 2
        assert result.count("\u2606") == 8


# =============================================================================
# format_author_year
# =============================================================================


class TestFormatAuthorYear:
    def test_both(self):
        assert format_author_year("Ribbiks", 2015) == "Ribbiks (2015)"

    def test_author_only(self):
        assert format_author_year("Ribbiks", None) == "Ribbiks"

    def test_year_only(self):
        assert format_author_year(None, 2015) == "(2015)"

    def test_neither(self):
        assert format_author_year(None, None) == "Unknown author"

    def test_empty_author(self):
        assert format_author_year("", 2015) == "(2015)"

    def test_string_year(self):
        assert format_author_year("Author", "2020") == "Author (2020)"


# =============================================================================
# truncate
# =============================================================================


class TestTruncate:
    def test_none_returns_empty(self):
        assert truncate(None, 10) == ""

    def test_empty_returns_empty(self):
        assert truncate("", 10) == ""

    def test_short_text_unchanged(self):
        assert truncate("hello", 10) == "hello"

    def test_exact_length_unchanged(self):
        assert truncate("1234567890", 10) == "1234567890"

    def test_long_text_truncated(self):
        result = truncate("this is a very long string", 10)
        assert result == "this is..."
        assert len(result) == 10

    def test_custom_suffix(self):
        result = truncate("hello world", 8, suffix="~")
        assert result == "hello w~"
        assert len(result) == 8


# =============================================================================
# format_size
# =============================================================================


class TestFormatSize:
    @pytest.mark.parametrize("size,expected", [
        (0, "0 B"),
        (1, "1 B"),
        (500, "500 B"),
        (1024, "1.0 KB"),
        (1536, "1.5 KB"),
        (10 * 1024 * 1024, "10.0 MB"),
        (2 * 1024 * 1024 * 1024, "2.0 GB"),
        (1024 * 1024 * 1024 * 1024, "1.0 TB"),
    ])
    def test_format_size(self, size, expected):
        assert format_size(size) == expected


# =============================================================================
# extract_year — only edge cases not in test_utils.py
# =============================================================================


class TestExtractYearEdgeCases:
    def test_short_string(self):
        assert extract_year("ab") is None

    def test_non_numeric_prefix(self):
        assert extract_year("abcd-01-01") is None


# =============================================================================
# compute_md5
# =============================================================================


class TestComputeMd5:
    def test_known_content(self, tmp_path):
        f = tmp_path / "test.txt"
        f.write_bytes(b"hello")
        assert compute_md5(f) == "5d41402abc4b2a76b9719d911017c592"

    def test_empty_file(self, tmp_path):
        f = tmp_path / "empty.txt"
        f.write_bytes(b"")
        assert compute_md5(f) == "d41d8cd98f00b204e9800998ecf8427e"

    def test_accepts_string_path(self, tmp_path):
        f = tmp_path / "test.txt"
        f.write_bytes(b"data")
        result = compute_md5(str(f))
        assert isinstance(result, str) and len(result) == 32

    def test_large_file(self, tmp_path):
        """Ensure chunked reading works for files > 8192 bytes."""
        f = tmp_path / "large.bin"
        f.write_bytes(b"\x00" * 20000)
        result = compute_md5(f)
        assert isinstance(result, str) and len(result) == 32


# =============================================================================
# parse_wad_directory
# =============================================================================


def _build_wad(lumps: list[tuple[str, bytes]], magic: bytes = b"PWAD") -> bytes:
    """Build a minimal WAD binary with given lumps.

    Note: an identical helper exists in test_iwad_detect.py. If more test files
    need this, it should be extracted to conftest.py.
    """
    data_chunks = []
    dir_entries = []
    offset = 12  # after header

    for name, content in lumps:
        data_chunks.append(content)
        name_bytes = name.encode("ascii")[:8].ljust(8, b"\x00")
        dir_entries.append(struct.pack("<ii", offset, len(content)) + name_bytes)
        offset += len(content)

    dir_offset = offset
    body = b"".join(data_chunks)
    directory = b"".join(dir_entries)
    header = magic + struct.pack("<ii", len(lumps), dir_offset)
    return header + body + directory


class TestParseWadDirectory:
    def test_empty_data(self):
        assert parse_wad_directory(b"") == []

    def test_too_short(self):
        assert parse_wad_directory(b"PWAD\x00\x00") == []

    def test_invalid_magic(self):
        data = b"NOTW" + b"\x00" * 8
        assert parse_wad_directory(data) == []

    def test_pwad_magic(self):
        wad = _build_wad([("MAP01", b"\x00" * 10)])
        entries = parse_wad_directory(wad)
        assert len(entries) == 1
        assert entries[0][0] == "MAP01"
        assert entries[0][2] == 10  # size

    def test_iwad_magic(self):
        wad = _build_wad([("E1M1", b"data")], magic=b"IWAD")
        entries = parse_wad_directory(wad)
        assert len(entries) == 1
        assert entries[0][0] == "E1M1"

    def test_multiple_lumps(self):
        wad = _build_wad([
            ("MAP01", b"\x00" * 4),
            ("THINGS", b"\x01" * 8),
            ("LINEDEFS", b"\x02" * 12),
        ])
        entries = parse_wad_directory(wad)
        assert len(entries) == 3
        names = [e[0] for e in entries]
        assert names == ["MAP01", "THINGS", "LINEDEFS"]

    def test_lump_name_uppercased(self):
        wad = _build_wad([("map01", b"")])
        entries = parse_wad_directory(wad)
        assert entries[0][0] == "MAP01"

    def test_empty_lump(self):
        """Marker lumps have zero size."""
        wad = _build_wad([("MAP01", b""), ("THINGS", b"\x00" * 4)])
        entries = parse_wad_directory(wad)
        assert entries[0][2] == 0
        assert entries[1][2] == 4

    def test_truncated_directory(self):
        """WAD with directory pointing beyond file end should handle gracefully."""
        wad = _build_wad([("MAP01", b"x")])
        truncated = wad[:len(wad) - 8]
        entries = parse_wad_directory(truncated)
        assert len(entries) == 0

    def test_memoryview_input(self):
        wad = _build_wad([("MAP01", b"\x00")])
        entries = parse_wad_directory(memoryview(wad))
        assert len(entries) == 1


# =============================================================================
# CacoSourceError
# =============================================================================


class TestCacoSourceError:
    def test_is_exception(self):
        assert issubclass(CacoSourceError, Exception)

    def test_message(self):
        err = CacoSourceError("test error")
        assert str(err) == "test error"


# =============================================================================
# BaseHttpClient
# =============================================================================


class TestBaseHttpClient:
    def test_context_manager(self):
        with BaseHttpClient() as client:
            assert client is not None

    def test_close_is_idempotent(self):
        client = BaseHttpClient()
        client.close()
        client.close()  # should not raise
