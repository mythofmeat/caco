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
    with patch("caco.db.get_db_path", return_value=db_path):
        from caco import db
        db.init_db()
        yield db_path


@pytest.fixture
def db_mod(tmp_db):
    """Return the caco.db module with a fresh test database."""
    from caco import db
    return db
