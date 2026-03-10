"""Tests for caco.services.resource_service — IWAD and id24 registration."""

from pathlib import Path
from unittest.mock import patch

from caco.services.resource_service import register_iwad, register_id24


# =============================================================================
# register_iwad
# =============================================================================


class TestRegisterIwad:
    def test_unrecognized_file_returns_none(self, db_mod, tmp_path):
        f = tmp_path / "random.wad"
        f.write_bytes(b"not a real IWAD")
        with patch("caco.services.resource_service.get_iwad_dir", return_value=tmp_path / "iwads"):
            result = register_iwad(f)
        assert result is None

    def test_recognized_iwad_registers(self, db_mod, tmp_path):
        f = tmp_path / "doom2.wad"
        f.write_bytes(b"fake data")

        iwad_dir = tmp_path / "iwads"
        iwad_dir.mkdir()

        with (
            patch("caco.db.identify_iwad", return_value=("doom2", "original", "Doom II")),
            patch("caco.db.get_iwad_variant", return_value=None),
            patch("caco.services.resource_service.get_iwad_dir", return_value=iwad_dir),
            patch("caco.db.managed_iwad_filename", return_value=Path("original/doom2.wad")),
            patch("caco.db.add_iwad") as mock_add,
            patch("caco.services.resource_service.compute_md5", return_value="abc123"),
        ):
            result = register_iwad(f)

        assert result == ("doom2", "original", "Doom II")
        mock_add.assert_called_once()

    def test_already_registered_returns_none(self, db_mod, tmp_path):
        f = tmp_path / "doom2.wad"
        f.write_bytes(b"fake")

        with (
            patch("caco.db.identify_iwad", return_value=("doom2", "original", "Doom II")),
            patch("caco.db.get_iwad_variant", return_value={"family": "doom2"}),
        ):
            result = register_iwad(f)

        assert result is None

    def test_copies_file_to_managed_dir(self, db_mod, tmp_path):
        f = tmp_path / "doom2.wad"
        f.write_bytes(b"IWAD data content")

        iwad_dir = tmp_path / "managed_iwads"
        iwad_dir.mkdir()

        with (
            patch("caco.db.identify_iwad", return_value=("doom2", "original", "Doom II")),
            patch("caco.db.get_iwad_variant", return_value=None),
            patch("caco.services.resource_service.get_iwad_dir", return_value=iwad_dir),
            patch("caco.db.managed_iwad_filename", return_value=Path("original/doom2.wad")),
            patch("caco.db.add_iwad"),
            patch("caco.services.resource_service.compute_md5", return_value="abc"),
        ):
            register_iwad(f)

        dest = iwad_dir / "original" / "doom2.wad"
        assert dest.exists()
        assert dest.read_bytes() == b"IWAD data content"


# =============================================================================
# register_id24
# =============================================================================


class TestRegisterId24:
    def test_unrecognized_file_returns_none(self, db_mod, tmp_path):
        f = tmp_path / "random.wad"
        f.write_bytes(b"not id24")
        with patch("caco.services.resource_service.get_id24_dir", return_value=tmp_path / "id24"):
            result = register_id24(f)
        assert result is None

    def test_recognized_id24_registers(self, db_mod, tmp_path):
        f = tmp_path / "id1.wad"
        f.write_bytes(b"id24 data")

        id24_dir = tmp_path / "id24"
        id24_dir.mkdir()

        with (
            patch("caco.db.identify_id24", return_value=("id1", "update2", "Legacy of Rust")),
            patch("caco.db.get_id24", return_value=None),
            patch("caco.services.resource_service.get_id24_dir", return_value=id24_dir),
            patch("caco.db.add_id24") as mock_add,
            patch("caco.services.resource_service.compute_md5", return_value="def456"),
        ):
            result = register_id24(f)

        assert result == ("id1", "update2", "Legacy of Rust")
        mock_add.assert_called_once()

    def test_already_registered_returns_none(self, db_mod, tmp_path):
        f = tmp_path / "id1.wad"
        f.write_bytes(b"data")

        with (
            patch("caco.db.identify_id24", return_value=("id1", "update2", "Legacy of Rust")),
            patch("caco.db.get_id24", return_value={"name": "id1"}),
        ):
            result = register_id24(f)

        assert result is None

    def test_copies_file_to_managed_dir(self, db_mod, tmp_path):
        f = tmp_path / "id1.wad"
        f.write_bytes(b"id24 WAD content")

        id24_dir = tmp_path / "managed_id24"
        id24_dir.mkdir()

        with (
            patch("caco.db.identify_id24", return_value=("id1", "update2", "Legacy of Rust")),
            patch("caco.db.get_id24", return_value=None),
            patch("caco.services.resource_service.get_id24_dir", return_value=id24_dir),
            patch("caco.db.add_id24"),
            patch("caco.services.resource_service.compute_md5", return_value="abc"),
        ):
            register_id24(f)

        dest = id24_dir / "id1.wad"
        assert dest.exists()
        assert dest.read_bytes() == b"id24 WAD content"
