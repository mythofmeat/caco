"""Tests for caco.demos — demo file management."""

import re
from pathlib import Path

from caco.demos import (
    DEMO_EXTENSION,
    clean_demo_files,
    find_demo_files,
    generate_demo_name,
    get_demos_dir,
)


class TestGetDemosDir:
    def test_returns_demos_subdir(self, tmp_path):
        result = get_demos_dir(tmp_path)
        assert result == tmp_path / "demos"


class TestFindDemoFiles:
    def test_finds_lmp_files(self, tmp_path):
        demos_dir = tmp_path / "demos"
        demos_dir.mkdir()
        (demos_dir / "run1.lmp").write_bytes(b"\x00" * 100)
        (demos_dir / "run2.lmp").write_bytes(b"\x00" * 200)

        result = find_demo_files(tmp_path)
        assert len(result) == 2
        names = {d["name"] for d in result}
        assert names == {"run1.lmp", "run2.lmp"}

    def test_excludes_non_demo_files(self, tmp_path):
        demos_dir = tmp_path / "demos"
        demos_dir.mkdir()
        (demos_dir / "run1.lmp").write_bytes(b"\x00" * 100)
        (demos_dir / "notes.txt").write_text("notes")
        (demos_dir / "save.dsg").write_bytes(b"\x00" * 50)

        result = find_demo_files(tmp_path)
        assert len(result) == 1
        assert result[0]["name"] == "run1.lmp"

    def test_returns_empty_for_missing_dir(self, tmp_path):
        result = find_demo_files(tmp_path)
        assert result == []

    def test_returns_empty_for_empty_dir(self, tmp_path):
        (tmp_path / "demos").mkdir()
        result = find_demo_files(tmp_path)
        assert result == []

    def test_result_dict_keys(self, tmp_path):
        demos_dir = tmp_path / "demos"
        demos_dir.mkdir()
        (demos_dir / "test.lmp").write_bytes(b"\x00" * 42)

        result = find_demo_files(tmp_path)
        assert len(result) == 1
        d = result[0]
        assert d["name"] == "test.lmp"
        assert d["rel_path"] == "demos/test.lmp"
        assert d["size"] == 42
        assert "mtime_iso" in d
        assert isinstance(d["path"], Path)

    def test_case_insensitive_extension(self, tmp_path):
        demos_dir = tmp_path / "demos"
        demos_dir.mkdir()
        (demos_dir / "run.LMP").write_bytes(b"\x00" * 10)

        result = find_demo_files(tmp_path)
        assert len(result) == 1


class TestCleanDemoFiles:
    def test_deletes_lmp_files(self, tmp_path):
        demos_dir = tmp_path / "demos"
        demos_dir.mkdir()
        lmp = demos_dir / "run1.lmp"
        lmp.write_bytes(b"\x00" * 100)

        deleted = clean_demo_files(tmp_path)
        assert len(deleted) == 1
        assert not lmp.exists()

    def test_preserves_non_demo_files(self, tmp_path):
        demos_dir = tmp_path / "demos"
        demos_dir.mkdir()
        (demos_dir / "run1.lmp").write_bytes(b"\x00" * 100)
        notes = demos_dir / "notes.txt"
        notes.write_text("keep me")

        clean_demo_files(tmp_path)
        assert notes.exists()

    def test_returns_empty_for_no_demos(self, tmp_path):
        (tmp_path / "demos").mkdir()
        deleted = clean_demo_files(tmp_path)
        assert deleted == []


class TestGenerateDemoName:
    def test_basic_format(self):
        name = generate_demo_name("eviternity")
        assert name.startswith("eviternity_")
        # Should have timestamp suffix like YYYYMMDD_HHMMSS
        assert re.match(r"eviternity_\d{8}_\d{6}$", name)

    def test_sanitizes_special_chars(self):
        name = generate_demo_name("My Cool WAD!!!")
        assert "!" not in name
        assert " " not in name
        assert name.startswith("my-cool-wad_")

    def test_handles_empty_stem(self):
        name = generate_demo_name("!!!")
        assert name.startswith("demo_")

    def test_no_extension(self):
        name = generate_demo_name("test")
        assert not name.endswith(".lmp")

    def test_truncates_long_names(self):
        name = generate_demo_name("a" * 100)
        # Should be truncated stem + _ + timestamp
        parts = name.rsplit("_", 2)  # stem, date, time
        assert len(parts[0]) <= 48
