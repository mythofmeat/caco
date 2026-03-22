"""Tests for WAD lump-based complevel auto-detection."""

import struct

import pytest

from caco.complevel_detect import (
    detect_complevel,
    _detect_from_dehacked,
    _has_boom_linedefs,
    _MAX_VANILLA_LINEDEF_TYPE,
)
from caco.utils import parse_wad_directory


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


def _make_linedef(v1=0, v2=1, flags=0, special=0, tag=0, front=0, back=-1):
    """Build a single 14-byte Doom-format linedef entry."""
    return struct.pack("<HHHHHHH", v1, v2, flags, special, tag, front, back & 0xFFFF)


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

    def test_mapxx_maps_vanilla(self, tmp_path):
        """MAPxx without any advanced features -> vanilla (2)."""
        wad_data = _make_wad({
            "MAP01": b"",
            "MAP02": b"",
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 2

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

    def test_dehacked_vanilla_only(self, tmp_path):
        """DEHACKED with only vanilla features (text/thing mods) -> vanilla (2)."""
        deh_content = b"Thing 1\nBits = SOLID\n"
        wad_data = _make_wad({
            "MAP01": b"",
            "DEHACKED": deh_content,
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 2

    def test_dehacked_vanilla_with_boom_lumps(self, tmp_path):
        """Vanilla DEHACKED + ANIMATED lump -> Boom (9)."""
        deh_content = b"Thing 1\nBits = SOLID\n"
        wad_data = _make_wad({
            "MAP01": b"",
            "DEHACKED": deh_content,
            "ANIMATED": b"\x00",
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 9

    def test_animated_lump_boom(self, tmp_path):
        """ANIMATED lump without DEHACKED -> Boom (9)."""
        wad_data = _make_wad({
            "MAP01": b"",
            "ANIMATED": b"\x00",
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 9

    def test_switches_lump_boom(self, tmp_path):
        """SWITCHES lump -> Boom (9)."""
        wad_data = _make_wad({
            "MAP01": b"",
            "SWITCHES": b"\x00",
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 9

    def test_boom_linedefs(self, tmp_path):
        """Boom linedef types in LINEDEFS -> Boom (9)."""
        linedefs = _make_linedef(special=0) + _make_linedef(special=142)
        wad_data = _make_wad({
            "MAP01": b"",
            "LINEDEFS": linedefs,
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 9

    def test_vanilla_linedefs_not_boom(self, tmp_path):
        """All linedef types <= 141 -> vanilla (2)."""
        linedefs = _make_linedef(special=1) + _make_linedef(special=141)
        wad_data = _make_wad({
            "MAP01": b"",
            "LINEDEFS": linedefs,
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 2

    def test_no_maps_returns_none(self, tmp_path):
        """Resource WAD with no map lumps -> None."""
        wad_data = _make_wad({
            "TEXTURE1": b"\x00" * 4,
            "PNAMES": b"\x00" * 4,
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

    def test_dehacked_doom_version_2021(self, tmp_path):
        """DEHACKED with Doom version = 2021 -> MBF21."""
        deh_content = b"Patch File for DeHackEd v3.0\nDoom version = 2021\nPatch format = 6\n\nThing 1\nBits = SOLID\n"
        wad_data = _make_wad({
            "MAP01": b"",
            "DEHACKED": deh_content,
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 21

    def test_dehacked_mbf21_bits_field(self, tmp_path):
        """DEHACKED with MBF21 Bits field -> MBF21."""
        deh_content = b"Thing 1\nMBF21 Bits = 0x00000004\n"
        wad_data = _make_wad({
            "MAP01": b"",
            "DEHACKED": deh_content,
        })
        wad_path = tmp_path / "test.wad"
        wad_path.write_bytes(wad_data)
        assert detect_complevel(wad_path) == 21


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

    def test_doom_version_2021(self):
        assert _detect_from_dehacked("Doom version = 2021\nPatch format = 6") == 21

    def test_doom_version_21_not_mbf21(self):
        """Doom version = 21 (WhackEd4 artifact) without MBF21 features is not MBF21."""
        assert _detect_from_dehacked("Doom version = 21\nPatch format = 6\nThing 1\nBits = SOLID") is None

    def test_mbf21_bits_field(self):
        assert _detect_from_dehacked("Thing 1\nMBF21 Bits = 0x00000004") == 21

    def test_mbf_seektracer_alias(self):
        """A_SeekTracer (alias for A_SeekerMissile) should detect MBF."""
        assert _detect_from_dehacked("Action = A_SeekTracer") == 11

    def test_mbf_cleartracer_alias(self):
        """A_ClearTracer (alias for A_ClearTarget) should detect MBF."""
        assert _detect_from_dehacked("Action = A_ClearTracer") == 11


class TestHasBoomLinedefs:
    """Test _has_boom_linedefs() helper."""

    def test_vanilla_linedefs(self):
        linedefs = _make_linedef(special=1) + _make_linedef(special=141)
        wad_data = _make_wad({"MAP01": b"", "LINEDEFS": linedefs})
        directory = parse_wad_directory(wad_data)
        assert _has_boom_linedefs(wad_data, directory) is False

    def test_boom_linedef_type(self):
        linedefs = _make_linedef(special=0) + _make_linedef(special=142)
        wad_data = _make_wad({"MAP01": b"", "LINEDEFS": linedefs})
        directory = parse_wad_directory(wad_data)
        assert _has_boom_linedefs(wad_data, directory) is True

    def test_generalized_linedef(self):
        linedefs = _make_linedef(special=0x2F80)
        wad_data = _make_wad({"MAP01": b"", "LINEDEFS": linedefs})
        directory = parse_wad_directory(wad_data)
        assert _has_boom_linedefs(wad_data, directory) is True

    def test_empty_linedefs(self):
        wad_data = _make_wad({"MAP01": b"", "LINEDEFS": b""})
        directory = parse_wad_directory(wad_data)
        assert _has_boom_linedefs(wad_data, directory) is False

    def test_no_linedefs_lump(self):
        wad_data = _make_wad({"MAP01": b""})
        directory = parse_wad_directory(wad_data)
        assert _has_boom_linedefs(wad_data, directory) is False
