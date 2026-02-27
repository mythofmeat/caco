"""Tests for WAD lump-based complevel auto-detection."""

import struct

import pytest

from caco.complevel_detect import detect_complevel, _detect_from_dehacked


def _make_wad(lumps: dict[str, bytes]) -> bytes:
    """Build a minimal PWAD in memory with the given lumps.

    Args:
        lumps: dict of lump_name -> lump_data
    """
    # Calculate directory offset (after header + all lump data)
    header_size = 12
    lump_data_offset = header_size
    total_data_size = sum(len(d) for d in lumps.values())
    dir_offset = header_size + total_data_size

    # Build header: magic + num_lumps + dir_offset
    header = b"PWAD" + struct.pack("<II", len(lumps), dir_offset)

    # Build lump data and directory entries
    data_parts = []
    dir_entries = []
    current_offset = header_size

    for name, lump_data in lumps.items():
        data_parts.append(lump_data)
        # Pad name to 8 bytes
        name_bytes = name.encode("ascii")[:8].ljust(8, b"\x00")
        dir_entries.append(
            struct.pack("<II", current_offset, len(lump_data)) + name_bytes
        )
        current_offset += len(lump_data)

    return header + b"".join(data_parts) + b"".join(dir_entries)


class TestDetectComplevel:
    """Test detect_complevel() with synthetic WADs."""

    def test_umapinfo_returns_21(self, tmp_path):
        wad_data = _make_wad({
            "MAP01": b"",
            "UMAPINFO": b"map MAP01 { }",
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 21

    def test_exmy_maps_vanilla(self, tmp_path):
        wad_data = _make_wad({
            "E1M1": b"",
            "E1M2": b"",
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 2

    def test_mapxx_maps_ambiguous(self, tmp_path):
        """MAPxx without DEHACKED/UMAPINFO is ambiguous — returns None."""
        wad_data = _make_wad({
            "MAP01": b"",
            "MAP02": b"",
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) is None

    def test_dehacked_with_mbf_codepointer(self, tmp_path):
        deh_content = b"Frame 100\nAction = A_Mushroom\n"
        wad_data = _make_wad({
            "MAP01": b"",
            "DEHACKED": deh_content,
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 11

    def test_dehacked_with_mbf21_codepointer(self, tmp_path):
        deh_content = b"Frame 100\nAction = A_SpawnObject\n"
        wad_data = _make_wad({
            "MAP01": b"",
            "DEHACKED": deh_content,
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 21

    def test_dehacked_without_mbf_ambiguous(self, tmp_path):
        """DEHACKED without MBF features is ambiguous."""
        deh_content = b"Thing 1\nBits = SOLID\n"
        wad_data = _make_wad({
            "MAP01": b"",
            "DEHACKED": deh_content,
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) is None

    def test_nonexistent_file(self, tmp_path):
        assert detect_complevel(tmp_path / "nonexistent.wad") is None

    def test_empty_file(self, tmp_path):
        wad_path = tmp_path / "empty.wad"
        wad_path.write_bytes(b"")
        assert detect_complevel(wad_path) is None


class TestDetectFromDehacked:
    """Test _detect_from_dehacked() helper."""

    def test_mbf_mushroom(self):
        assert _detect_from_dehacked("Action = A_Mushroom") == 11

    def test_mbf21_spawnobject(self):
        assert _detect_from_dehacked("Action = A_SpawnObject") == 21

    def test_no_special_features(self):
        assert _detect_from_dehacked("Thing 1\nBits = SOLID") is None

    def test_case_insensitive(self):
        assert _detect_from_dehacked("action = a_mushroom") == 11
