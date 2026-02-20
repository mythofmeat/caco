"""Shared test fixtures."""

from pathlib import Path
from unittest.mock import patch

import pytest


@pytest.fixture
def tmp_db(tmp_path):
    """Provide an in-memory-like SQLite database for testing.

    Patches get_db_path() to return a temporary file so init_db() and all
    db functions use the test database.
    """
    db_path = tmp_path / "test.db"
    with patch("caco.config.get_db_path", return_value=db_path):
        from caco import db
        # Clear config cache between tests
        from caco import config
        config._config_cache = None
        db.init_db()
        yield db_path


@pytest.fixture
def db_mod(tmp_db):
    """Return the caco.db module with a fresh test database."""
    from caco import db
    return db


@pytest.fixture
def make_wad(db_mod):
    """Factory fixture to create WADs with sensible defaults.

    Usage:
        wad_id = make_wad(title="Eviternity")
        wad_id = make_wad(title="Sunlust", author="Ribbiks", status="playing")
    """
    from caco.db import SourceType, Status

    def _make(
        title: str = "Test WAD",
        author: str | None = "Test Author",
        year: int | None = 2024,
        source_type: SourceType = SourceType.IDGAMES,
        status: str = "backlog",
        tags: list[str] | None = None,
        **kwargs,
    ) -> int:
        return db_mod.add_wad(
            title=title,
            source_type=source_type,
            author=author,
            year=year,
            status=Status(status),
            tags=tags,
            **kwargs,
        )

    return _make


@pytest.fixture
def populated_db(make_wad, db_mod):
    """Provide a database pre-populated with sample WADs.

    Returns dict mapping name -> wad_id for easy reference.
    """
    ids = {}
    ids["eviternity"] = make_wad(
        title="Eviternity", author="Dragonfly", year=2018,
        status="finished", tags=["cacoward", "megawad"],
        source_id="12345", source_url="https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/eviternity",
    )
    ids["sunlust"] = make_wad(
        title="Sunlust", author="Ribbiks", year=2015,
        status="playing", tags=["megawad", "slaughter"],
    )
    ids["scythe2"] = make_wad(
        title="Scythe 2", author="Erik Alm", year=2005,
        status="to-play", tags=["megawad"],
    )
    ids["ancient_aliens"] = make_wad(
        title="Ancient Aliens", author="skillsaw", year=2016,
        status="backlog", tags=["cacoward", "megawad"],
    )
    ids["abandoned"] = make_wad(
        title="Bad WAD", author="Unknown", year=2020,
        status="abandoned",
    )
    return ids


@pytest.fixture
def tmp_config(tmp_path):
    """Provide a temporary config directory with a fresh config file.

    Patches CONFIG_DIR and CONFIG_FILE so tests don't touch real config.
    """
    config_dir = tmp_path / "config"
    config_dir.mkdir()
    config_file = config_dir / "config.toml"

    with (
        patch("caco.config.CONFIG_DIR", config_dir),
        patch("caco.config.CONFIG_FILE", config_file),
    ):
        from caco import config
        config._config_cache = None
        yield config_file
