"""Tests for IWAD auto-detection from WAD file contents."""

import struct
import zipfile
from pathlib import Path
from unittest.mock import patch

import pytest

from caco.iwad_detect import (
    PLUTONIA_ONLY_PATCHES,
    TNT_ONLY_PATCHES,
    detect_complvl,
    detect_iwad,
    _detect_from_maps,
    _detect_from_pnames,
    _get_lump_names,
    _load_wad_data,
    _parse_pnames,
)


# ─── WAD Building Helpers ────────────────────────────────────────────────


def _build_wad(lumps: list[tuple[str, bytes]]) -> bytes:
    """Build a minimal PWAD from a list of (name, data) tuples."""
    header_size = 12
    # Calculate total lump data size
    data_offset = header_size
    lump_entries = []
    lump_data = b""

    for name, data in lumps:
        offset = data_offset + len(lump_data)
        lump_entries.append((name, offset, len(data)))
        lump_data += data

    dir_offset = data_offset + len(lump_data)
    num_lumps = len(lump_entries)

    # Header: magic + num_lumps + dir_offset
    header = b"PWAD" + struct.pack("<ii", num_lumps, dir_offset)

    # Directory entries: offset + size + name (8 bytes, null-padded)
    directory = b""
    for name, offset, size in lump_entries:
        name_bytes = name.encode("ascii")[:8].ljust(8, b"\x00")
        directory += struct.pack("<ii", offset, size) + name_bytes

    return header + lump_data + directory


def _build_pnames(patch_names: list[str]) -> bytes:
    """Build a PNAMES lump from a list of patch name strings."""
    data = struct.pack("<i", len(patch_names))
    for name in patch_names:
        data += name.encode("ascii")[:8].ljust(8, b"\x00")
    return data


# ─── Patch Set Sanity ────────────────────────────────────────────────────


def test_patch_sets_disjoint():
    """TNT-only and Plutonia-only sets must not overlap."""
    overlap = TNT_ONLY_PATCHES & PLUTONIA_ONLY_PATCHES
    assert not overlap, f"Overlap found: {overlap}"


def test_patch_set_sizes():
    """Verify expected patch set sizes."""
    assert len(TNT_ONLY_PATCHES) == 197
    assert len(PLUTONIA_ONLY_PATCHES) == 78


# ─── Map Name Detection ─────────────────────────────────────────────────


def test_detect_doom1_maps(tmp_path):
    """ExMy map lumps should detect as doom."""
    wad = _build_wad([
        ("E1M1", b""),
        ("E1M2", b""),
        ("E2M1", b""),
    ])
    path = tmp_path / "doom1.wad"
    path.write_bytes(wad)
    assert detect_iwad(path) == "doom"


def test_detect_doom2_maps(tmp_path):
    """MAPxx map lumps should detect as doom2."""
    wad = _build_wad([
        ("MAP01", b""),
        ("MAP02", b""),
        ("MAP32", b""),
    ])
    path = tmp_path / "doom2.wad"
    path.write_bytes(wad)
    assert detect_iwad(path) == "doom2"


def test_detect_no_maps(tmp_path):
    """WAD with no map lumps should return None."""
    wad = _build_wad([
        ("THINGS", b"\x00" * 16),
        ("VERTEXES", b"\x00" * 8),
    ])
    path = tmp_path / "nomap.wad"
    path.write_bytes(wad)
    assert detect_iwad(path) is None


def test_detect_mixed_maps(tmp_path):
    """WAD with both ExMy and MAPxx lumps returns None (ambiguous)."""
    wad = _build_wad([
        ("E1M1", b""),
        ("MAP01", b""),
    ])
    path = tmp_path / "mixed.wad"
    path.write_bytes(wad)
    assert detect_iwad(path) is None


# ─── PNAMES Detection ───────────────────────────────────────────────────


def test_detect_tnt_from_pnames(tmp_path):
    """PNAMES referencing TNT-only patches should detect as tnt."""
    tnt_patches = sorted(TNT_ONLY_PATCHES)[:5]
    pnames_data = _build_pnames(tnt_patches + ["WALL00", "DOOR2_1"])
    wad = _build_wad([
        ("PNAMES", pnames_data),
        ("MAP01", b""),
    ])
    path = tmp_path / "tnt_wad.wad"
    path.write_bytes(wad)
    assert detect_iwad(path) == "tnt"


def test_detect_plutonia_from_pnames(tmp_path):
    """PNAMES referencing Plutonia-only patches should detect as plutonia."""
    plut_patches = sorted(PLUTONIA_ONLY_PATCHES)[:5]
    pnames_data = _build_pnames(plut_patches + ["WALL00", "DOOR2_1"])
    wad = _build_wad([
        ("PNAMES", pnames_data),
        ("MAP01", b""),
    ])
    path = tmp_path / "plut_wad.wad"
    path.write_bytes(wad)
    assert detect_iwad(path) == "plutonia"


def test_self_contained_tnt_patches(tmp_path):
    """TNT patches provided as lumps should not trigger TNT detection."""
    tnt_patches = sorted(TNT_ONLY_PATCHES)[:3]
    pnames_data = _build_pnames(tnt_patches + ["WALL00"])

    # Include the TNT patches as actual lumps in the WAD
    lumps = [("PNAMES", pnames_data), ("MAP01", b"")]
    for p in tnt_patches:
        lumps.append((p, b"\x00" * 64))  # dummy lump data

    wad = _build_wad(lumps)
    path = tmp_path / "self_contained.wad"
    path.write_bytes(wad)
    # Should fall through to map detection (doom2) since patches are self-contained
    assert detect_iwad(path) == "doom2"


def test_pnames_no_unique_patches(tmp_path):
    """PNAMES with only common patches should fall through to map detection."""
    pnames_data = _build_pnames(["WALL00", "DOOR2_1", "FLAT5_1"])
    wad = _build_wad([
        ("PNAMES", pnames_data),
        ("E1M1", b""),
    ])
    path = tmp_path / "common.wad"
    path.write_bytes(wad)
    # No unique patches, falls through to map detection -> doom
    assert detect_iwad(path) == "doom"


def test_pnames_priority_over_maps(tmp_path):
    """PNAMES detection should take priority over map detection."""
    tnt_patches = sorted(TNT_ONLY_PATCHES)[:3]
    pnames_data = _build_pnames(tnt_patches)
    wad = _build_wad([
        ("PNAMES", pnames_data),
        ("MAP01", b""),  # Would detect as doom2 without PNAMES
    ])
    path = tmp_path / "tnt_priority.wad"
    path.write_bytes(wad)
    assert detect_iwad(path) == "tnt"


# ─── ZIP-Wrapped WADs ───────────────────────────────────────────────────


def test_detect_zip_wrapped(tmp_path):
    """WADs inside ZIP files should be detected."""
    wad = _build_wad([("MAP01", b""), ("MAP02", b"")])
    zip_path = tmp_path / "wadzip.zip"
    with zipfile.ZipFile(zip_path, "w") as zf:
        zf.writestr("mywad.wad", wad)
    assert detect_iwad(zip_path) == "doom2"


def test_detect_zip_with_tnt_pnames(tmp_path):
    """ZIP-wrapped WAD with TNT-only PNAMES should detect tnt."""
    tnt_patches = sorted(TNT_ONLY_PATCHES)[:3]
    pnames_data = _build_pnames(tnt_patches)
    wad = _build_wad([("PNAMES", pnames_data), ("MAP01", b"")])
    zip_path = tmp_path / "tntwad.zip"
    with zipfile.ZipFile(zip_path, "w") as zf:
        zf.writestr("tntwad.wad", wad)
    assert detect_iwad(zip_path) == "tnt"


# ─── Edge Cases ──────────────────────────────────────────────────────────


def test_detect_empty_file(tmp_path):
    """Empty file should return None."""
    path = tmp_path / "empty.wad"
    path.write_bytes(b"")
    assert detect_iwad(path) is None


def test_detect_too_small(tmp_path):
    """File smaller than WAD header should return None."""
    path = tmp_path / "tiny.wad"
    path.write_bytes(b"PWAD")
    assert detect_iwad(path) is None


def test_detect_bad_magic(tmp_path):
    """Non-WAD file should return None."""
    path = tmp_path / "notawad.wad"
    path.write_bytes(b"NOT A WAD FILE AT ALL!!")
    assert detect_iwad(path) is None


def test_detect_nonexistent():
    """Nonexistent file should return None."""
    assert detect_iwad(Path("/nonexistent/path/to/nothing.wad")) is None


def test_detect_corrupt_pnames(tmp_path):
    """Corrupted PNAMES lump should be handled gracefully."""
    # PNAMES with count claiming more entries than available data
    bad_pnames = struct.pack("<i", 9999)  # claims 9999 patches but has no data
    wad = _build_wad([
        ("PNAMES", bad_pnames),
        ("MAP01", b""),
    ])
    path = tmp_path / "corrupt.wad"
    path.write_bytes(wad)
    # Should skip bad PNAMES and fall through to map detection
    assert detect_iwad(path) == "doom2"


def test_detect_bad_zip(tmp_path):
    """Invalid ZIP file with .zip extension should return None."""
    path = tmp_path / "bad.zip"
    path.write_bytes(b"this is not a zip file")
    assert detect_iwad(path) is None


# ─── Internal Helpers ────────────────────────────────────────────────────


def test_parse_pnames_empty():
    """_parse_pnames returns None when no PNAMES lump exists."""
    assert _parse_pnames(b"", []) is None


def test_get_lump_names():
    """_get_lump_names should return a set of names."""
    directory = [("MAP01", 12, 0), ("THINGS", 12, 100), ("MAP02", 112, 0)]
    assert _get_lump_names(directory) == {"MAP01", "THINGS", "MAP02"}


def test_detect_from_maps_doom1():
    assert _detect_from_maps({"E1M1", "E1M2", "THINGS"}) == "doom"


def test_detect_from_maps_doom2():
    assert _detect_from_maps({"MAP01", "MAP02", "THINGS"}) == "doom2"


def test_detect_from_maps_none():
    assert _detect_from_maps({"THINGS", "LINEDEFS"}) is None


def test_detect_from_pnames_tnt():
    pnames = {"ALTAQUA", "ASPHALT", "WALL00"}
    lump_names = {"MAP01", "WALL00"}
    assert _detect_from_pnames(pnames, lump_names) == "tnt"


def test_detect_from_pnames_plutonia():
    pnames = {"AROCK2", "AROCK3", "WALL00"}
    lump_names = {"MAP01", "WALL00"}
    assert _detect_from_pnames(pnames, lump_names) == "plutonia"


def test_detect_from_pnames_none():
    pnames = {"WALL00", "DOOR2_1"}
    lump_names = {"MAP01"}
    assert _detect_from_pnames(pnames, lump_names) is None


# ─── Player Integration ─────────────────────────────────────────────────


def test_player_auto_detect_persists(tmp_db, tmp_path):
    """play() should call detect_iwad and persist to custom_iwad."""
    from caco import db

    wad_id = db.add_wad(
        title="TNT Test WAD",
        source_type=db.SourceType.LOCAL,
        source_url=str(tmp_path / "test.wad"),
    )

    # Build a WAD with TNT-only PNAMES
    tnt_patches = sorted(TNT_ONLY_PATCHES)[:3]
    pnames_data = _build_pnames(tnt_patches)
    wad_data = _build_wad([("PNAMES", pnames_data), ("MAP01", b"")])
    wad_path = tmp_path / "test.wad"
    wad_path.write_bytes(wad_data)
    db.update_wad(wad_id, cached_path=str(wad_path))

    with (
        patch("caco.player.get_auto_detect_iwad", return_value=True),
        patch("caco.player.get_default_sourceport", return_value="dsda-doom"),
        patch("caco.player.resolve_sourceport", return_value="/usr/bin/dsda-doom"),
        patch("caco.player.resolve_iwad", return_value="/path/to/tnt.wad"),
        patch("caco.player.get_sourceport_args", return_value=[]),
        patch("caco.player.get_manage_data_dirs", return_value=False),
        patch("caco.player.get_cache_auto_clean", return_value=False),
        patch("shutil.which", return_value="/usr/bin/dsda-doom"),
        patch("subprocess.Popen") as mock_popen,
    ):
        mock_popen.return_value.wait.return_value = 0
        mock_popen.return_value.returncode = 0

        from caco.player import play
        play(wad_id)

    # Verify custom_iwad was persisted
    updated = db.get_wad(wad_id)
    assert updated["custom_iwad"] == "tnt"


def test_player_skips_when_already_set(tmp_db, tmp_path):
    """play() should skip detection when custom_iwad is already set."""
    from caco import db

    wad_id = db.add_wad(
        title="Already Set WAD",
        source_type=db.SourceType.LOCAL,
        source_url=str(tmp_path / "test.wad"),
    )

    wad_data = _build_wad([("MAP01", b"")])
    wad_path = tmp_path / "test.wad"
    wad_path.write_bytes(wad_data)
    db.update_wad(wad_id, cached_path=str(wad_path), custom_iwad="doom2")

    with (
        patch("caco.player.get_auto_detect_iwad", return_value=True),
        patch("caco.player.get_default_sourceport", return_value="dsda-doom"),
        patch("caco.player.resolve_sourceport", return_value="/usr/bin/dsda-doom"),
        patch("caco.player.resolve_iwad", return_value="/path/to/doom2.wad"),
        patch("caco.player.get_sourceport_args", return_value=[]),
        patch("caco.player.get_manage_data_dirs", return_value=False),
        patch("caco.player.get_cache_auto_clean", return_value=False),
        patch("shutil.which", return_value="/usr/bin/dsda-doom"),
        patch("subprocess.Popen") as mock_popen,
        patch("caco.iwad_detect.detect_iwad") as mock_detect,
    ):
        mock_popen.return_value.wait.return_value = 0
        mock_popen.return_value.returncode = 0

        from caco.player import play
        play(wad_id)

    # detect_iwad should NOT have been called
    mock_detect.assert_not_called()

    # custom_iwad should remain unchanged
    updated = db.get_wad(wad_id)
    assert updated["custom_iwad"] == "doom2"


def test_player_respects_config_disabled(tmp_db, tmp_path):
    """play() should skip detection when auto_detect_iwad is disabled."""
    from caco import db

    wad_id = db.add_wad(
        title="Config Disabled WAD",
        source_type=db.SourceType.LOCAL,
        source_url=str(tmp_path / "test.wad"),
    )

    # Build a WAD with TNT-only PNAMES
    tnt_patches = sorted(TNT_ONLY_PATCHES)[:3]
    pnames_data = _build_pnames(tnt_patches)
    wad_data = _build_wad([("PNAMES", pnames_data), ("MAP01", b"")])
    wad_path = tmp_path / "test.wad"
    wad_path.write_bytes(wad_data)
    db.update_wad(wad_id, cached_path=str(wad_path))

    with (
        patch("caco.player.get_auto_detect_iwad", return_value=False),
        patch("caco.player.get_default_sourceport", return_value="dsda-doom"),
        patch("caco.player.resolve_sourceport", return_value="/usr/bin/dsda-doom"),
        patch("caco.player.resolve_iwad", return_value="/path/to/doom2.wad"),
        patch("caco.player.get_iwad", return_value="doom2"),
        patch("caco.player.get_sourceport_args", return_value=[]),
        patch("caco.player.get_manage_data_dirs", return_value=False),
        patch("caco.player.get_cache_auto_clean", return_value=False),
        patch("shutil.which", return_value="/usr/bin/dsda-doom"),
        patch("subprocess.Popen") as mock_popen,
    ):
        mock_popen.return_value.wait.return_value = 0
        mock_popen.return_value.returncode = 0

        from caco.player import play
        play(wad_id)

    # custom_iwad should NOT have been set
    updated = db.get_wad(wad_id)
    assert not updated.get("custom_iwad")


# ─── COMPLVL Detection ────────────────────────────────────────────────


def test_detect_complvl_present(tmp_path):
    """WAD with COMPLVL lump should return its value."""
    wad = _build_wad([
        ("COMPLVL", bytes([21])),  # MBF21
        ("MAP01", b""),
    ])
    path = tmp_path / "id24.wad"
    path.write_bytes(wad)
    assert detect_complvl(path) == 21


def test_detect_complvl_zero(tmp_path):
    """COMPLVL lump with value 0 should return 0."""
    wad = _build_wad([
        ("COMPLVL", bytes([0])),
        ("MAP01", b""),
    ])
    path = tmp_path / "cl0.wad"
    path.write_bytes(wad)
    assert detect_complvl(path) == 0


def test_detect_complvl_absent(tmp_path):
    """WAD without COMPLVL lump should return None."""
    wad = _build_wad([("MAP01", b""), ("MAP02", b"")])
    path = tmp_path / "noid24.wad"
    path.write_bytes(wad)
    assert detect_complvl(path) is None


def test_detect_complvl_empty_lump(tmp_path):
    """COMPLVL lump with size 0 should return None."""
    wad = _build_wad([("COMPLVL", b""), ("MAP01", b"")])
    path = tmp_path / "empty_cl.wad"
    path.write_bytes(wad)
    assert detect_complvl(path) is None


def test_detect_complvl_zip_wrapped(tmp_path):
    """COMPLVL detection should work inside ZIP files."""
    wad = _build_wad([("COMPLVL", bytes([9])), ("MAP01", b"")])
    zip_path = tmp_path / "id24.zip"
    with zipfile.ZipFile(zip_path, "w") as zf:
        zf.writestr("mywad.wad", wad)
    assert detect_complvl(zip_path) == 9


def test_detect_complvl_nonexistent():
    """Nonexistent file should return None."""
    assert detect_complvl(Path("/nonexistent/path.wad")) is None


def test_detect_complvl_bad_file(tmp_path):
    """Non-WAD file should return None."""
    path = tmp_path / "notawad.wad"
    path.write_bytes(b"NOT A WAD FILE")
    assert detect_complvl(path) is None


# ─── _load_wad_data helper ────────────────────────────────────────────


def test_load_wad_data_direct(tmp_path):
    """_load_wad_data reads .wad files directly."""
    wad = _build_wad([("MAP01", b"")])
    path = tmp_path / "test.wad"
    path.write_bytes(wad)
    data = _load_wad_data(path)
    assert data == wad


def test_load_wad_data_zip(tmp_path):
    """_load_wad_data extracts .wad from ZIP files."""
    wad = _build_wad([("MAP01", b"")])
    zip_path = tmp_path / "test.zip"
    with zipfile.ZipFile(zip_path, "w") as zf:
        zf.writestr("inner.wad", wad)
    data = _load_wad_data(zip_path)
    assert data == wad


def test_load_wad_data_nonexistent():
    """_load_wad_data returns None for missing files."""
    assert _load_wad_data(Path("/nonexistent/path.wad")) is None


# ─── Player COMPLVL Integration ──────────────────────────────────────


def test_player_auto_detect_complevel(tmp_db, tmp_path):
    """play() should detect COMPLVL lump and persist to custom_complevel."""
    from caco import db

    wad_id = db.add_wad(
        title="id24 WAD",
        source_type=db.SourceType.LOCAL,
        source_url=str(tmp_path / "test.wad"),
    )

    # Build a WAD with COMPLVL lump
    wad_data = _build_wad([("COMPLVL", bytes([21])), ("MAP01", b"")])
    wad_path = tmp_path / "test.wad"
    wad_path.write_bytes(wad_data)
    db.update_wad(wad_id, cached_path=str(wad_path))

    with (
        patch("caco.player.get_auto_detect_iwad", return_value=False),
        patch("caco.player.get_auto_detect_complevel", return_value=True),
        patch("caco.player.get_default_sourceport", return_value="dsda-doom"),
        patch("caco.player.resolve_sourceport", return_value="/usr/bin/dsda-doom"),
        patch("caco.player.resolve_iwad", return_value="/path/to/doom2.wad"),
        patch("caco.player.get_iwad", return_value="doom2"),
        patch("caco.player.get_sourceport_args", return_value=[]),
        patch("caco.player.get_manage_data_dirs", return_value=False),
        patch("caco.player.get_cache_auto_clean", return_value=False),
        patch("shutil.which", return_value="/usr/bin/dsda-doom"),
        patch("subprocess.Popen") as mock_popen,
    ):
        mock_popen.return_value.wait.return_value = 0
        mock_popen.return_value.returncode = 0

        from caco.player import play
        play(wad_id)

    updated = db.get_wad(wad_id)
    assert updated["custom_complevel"] == "21"


def test_player_skips_complevel_when_set(tmp_db, tmp_path):
    """play() should skip COMPLVL detection when custom_complevel already set."""
    from caco import db

    wad_id = db.add_wad(
        title="Already Set CL",
        source_type=db.SourceType.LOCAL,
        source_url=str(tmp_path / "test.wad"),
    )

    wad_data = _build_wad([("COMPLVL", bytes([21])), ("MAP01", b"")])
    wad_path = tmp_path / "test.wad"
    wad_path.write_bytes(wad_data)
    db.update_wad(wad_id, cached_path=str(wad_path), custom_complevel="9")

    with (
        patch("caco.player.get_auto_detect_iwad", return_value=False),
        patch("caco.player.get_auto_detect_complevel", return_value=True),
        patch("caco.player.get_default_sourceport", return_value="dsda-doom"),
        patch("caco.player.resolve_sourceport", return_value="/usr/bin/dsda-doom"),
        patch("caco.player.resolve_iwad", return_value="/path/to/doom2.wad"),
        patch("caco.player.get_iwad", return_value="doom2"),
        patch("caco.player.get_sourceport_args", return_value=[]),
        patch("caco.player.get_manage_data_dirs", return_value=False),
        patch("caco.player.get_cache_auto_clean", return_value=False),
        patch("shutil.which", return_value="/usr/bin/dsda-doom"),
        patch("subprocess.Popen") as mock_popen,
    ):
        mock_popen.return_value.wait.return_value = 0
        mock_popen.return_value.returncode = 0

        from caco.player import play
        play(wad_id)

    # Should remain "9", not overwritten with "21"
    updated = db.get_wad(wad_id)
    assert updated["custom_complevel"] == "9"
