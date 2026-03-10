"""Tests for caco.db._id24 — id24 WAD registry CRUD operations."""

import sqlite3
from unittest.mock import patch

import pytest

from caco.db import identify_id24, KNOWN_ID24_WADS


class TestId24Crud:
    """Test basic CRUD operations for id24 WADs."""

    def test_add_and_get(self, db_mod):
        db_mod.add_id24("id1", "/path/to/id1.wad", version="update2", title="Legacy of Rust", md5="abc123")
        entry = db_mod.get_id24("id1")
        assert entry is not None
        assert entry["name"] == "id1"
        assert entry["version"] == "update2"
        assert entry["title"] == "Legacy of Rust"
        assert entry["path"] == "/path/to/id1.wad"
        assert entry["md5"] == "abc123"

    def test_get_nonexistent(self, db_mod):
        assert db_mod.get_id24("nonexistent") is None

    def test_add_duplicate_raises(self, db_mod):
        db_mod.add_id24("id1", "/path/a.wad")
        with pytest.raises(sqlite3.IntegrityError):
            db_mod.add_id24("id1", "/path/b.wad")

    def test_get_all_id24(self, db_mod):
        db_mod.add_id24("id1", "/p/id1.wad", title="Legacy of Rust")
        db_mod.add_id24("id24res", "/p/id24res.wad", title="id24 Resource WAD")
        db_mod.add_id24("id1-res", "/p/id1-res.wad", title="LoR Resources")

        all_entries = db_mod.get_all_id24()
        assert len(all_entries) == 3
        # Ordered by name
        names = [e["name"] for e in all_entries]
        assert names == ["id1", "id1-res", "id24res"]

    def test_get_all_id24_empty(self, db_mod):
        assert db_mod.get_all_id24() == []

    def test_get_id24_by_path(self, db_mod):
        db_mod.add_id24("id1", "/managed/id1.wad")
        entry = db_mod.get_id24_by_path("/managed/id1.wad")
        assert entry is not None
        assert entry["name"] == "id1"

    def test_get_id24_by_path_no_match(self, db_mod):
        assert db_mod.get_id24_by_path("/nonexistent") is None

    def test_remove_id24(self, db_mod):
        db_mod.add_id24("id1", "/p/id1.wad")
        removed = db_mod.remove_id24("id1")
        assert removed == 1
        assert db_mod.get_id24("id1") is None

    def test_remove_id24_nonexistent(self, db_mod):
        removed = db_mod.remove_id24("nonexistent")
        assert removed == 0

    def test_remove_id24_with_paths(self, db_mod):
        db_mod.add_id24("id1", "/managed/id1.wad")
        paths = db_mod.remove_id24_with_paths("id1")
        assert paths == ["/managed/id1.wad"]
        assert db_mod.get_id24("id1") is None

    def test_remove_id24_with_paths_nonexistent(self, db_mod):
        paths = db_mod.remove_id24_with_paths("nonexistent")
        assert paths == []


class TestId24Identify:
    """Test identify_id24 with known hashes and filenames."""

    def test_known_md5(self, tmp_path):
        md5, expected = next(iter(KNOWN_ID24_WADS.items()))
        f = tmp_path / "test.wad"
        f.touch()

        with patch("caco.db._id24.compute_md5", return_value=md5):
            result = identify_id24(f)

        assert result is not None
        assert result == expected

    def test_known_filename_fallback(self, tmp_path):
        f = tmp_path / "id1.wad"
        f.touch()

        with patch("caco.db._id24.compute_md5", return_value="unknown_md5"):
            result = identify_id24(f)

        assert result is not None
        assert result[0] == "id1"

    def test_unrecognized_returns_none(self, tmp_path):
        f = tmp_path / "random_name.wad"
        f.touch()

        with patch("caco.db._id24.compute_md5", return_value="aaaa"):
            result = identify_id24(f)

        assert result is None

    def test_nonexistent_file_returns_none(self, tmp_path):
        result = identify_id24(tmp_path / "doesnt_exist.wad")
        assert result is None

    def test_case_insensitive_filename(self, tmp_path):
        f = tmp_path / "ID1.WAD"
        f.touch()

        with patch("caco.db._id24.compute_md5", return_value="aaaa"):
            result = identify_id24(f)

        assert result is not None
        assert result[0] == "id1"


class TestId24AddOptionalFields:
    """Test optional fields on add_id24."""

    def test_add_without_optional_fields(self, db_mod):
        id_ = db_mod.add_id24("test", "/p/test.wad")
        assert id_ > 0
        entry = db_mod.get_id24("test")
        assert entry["version"] is None
        assert entry["title"] is None
        assert entry["md5"] is None

    def test_add_returns_valid_id(self, db_mod):
        id1 = db_mod.add_id24("a", "/p/a.wad")
        id2 = db_mod.add_id24("b", "/p/b.wad")
        assert id2 > id1
