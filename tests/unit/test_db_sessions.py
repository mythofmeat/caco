"""Tests for DB sessions, batch functions, completions, and duplicate detection."""

from datetime import datetime, timedelta

import pytest


class TestSessions:
    """Test play session lifecycle."""

    def test_start_and_end_session(self, db_mod, make_wad):
        wad_id = make_wad()
        session_id = db_mod.start_session(wad_id, sourceport="gzdoom")
        assert session_id > 0

        db_mod.end_session(session_id)
        sessions = db_mod.get_sessions(wad_id)
        assert len(sessions) == 1
        assert sessions[0]["sourceport"] == "gzdoom"
        assert sessions[0]["duration_seconds"] is not None

    def test_multiple_sessions(self, db_mod, make_wad):
        wad_id = make_wad()
        s1 = db_mod.start_session(wad_id, sourceport="gzdoom")
        db_mod.end_session(s1)
        s2 = db_mod.start_session(wad_id, sourceport="dsda-doom")
        db_mod.end_session(s2)

        sessions = db_mod.get_sessions(wad_id)
        assert len(sessions) == 2

    def test_get_sessions_empty(self, db_mod, make_wad):
        wad_id = make_wad()
        sessions = db_mod.get_sessions(wad_id)
        assert sessions == []


class TestBatchStats:
    """Test batch stat functions."""

    def test_get_wad_stats_batch_empty(self, db_mod):
        result = db_mod.get_wad_stats_batch([])
        assert result == {}

    def test_get_wad_stats_batch_no_sessions(self, db_mod, make_wad):
        wad_id = make_wad()
        result = db_mod.get_wad_stats_batch([wad_id])
        assert result[wad_id]["playtime"] == 0
        assert result[wad_id]["last_played"] is None
        assert result[wad_id]["session_count"] == 0
        assert result[wad_id]["times_beaten"] == 0

    def test_get_wad_stats_batch_with_session(self, db_mod, make_wad):
        wad_id = make_wad()
        s_id = db_mod.start_session(wad_id, sourceport="gzdoom")
        db_mod.end_session(s_id)

        result = db_mod.get_wad_stats_batch([wad_id])
        assert result[wad_id]["session_count"] == 1
        assert result[wad_id]["last_played"] is not None

    def test_get_wad_stats_batch_with_completions(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id)
        db_mod.add_wad_completion(wad_id)

        result = db_mod.get_wad_stats_batch([wad_id])
        assert result[wad_id]["times_beaten"] == 2

    def test_get_wad_stats_batch_multiple_wads(self, db_mod, make_wad):
        w1 = make_wad(title="WAD 1")
        w2 = make_wad(title="WAD 2")
        db_mod.add_wad_completion(w1)

        result = db_mod.get_wad_stats_batch([w1, w2])
        assert result[w1]["times_beaten"] == 1
        assert result[w2]["times_beaten"] == 0

    def test_get_total_playtime_batch(self, db_mod, make_wad):
        wad_id = make_wad()
        result = db_mod.get_total_playtime_batch([wad_id])
        # No sessions, so wad_id won't be in result
        assert result.get(wad_id, 0) == 0

    def test_get_last_played_batch(self, db_mod, make_wad):
        wad_id = make_wad()
        result = db_mod.get_last_played_batch([wad_id])
        assert wad_id not in result  # No sessions

    def test_get_times_beaten_batch(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id)
        result = db_mod.get_times_beaten_batch([wad_id])
        assert result[wad_id] == 1

    def test_get_session_count_batch(self, db_mod, make_wad):
        wad_id = make_wad()
        result = db_mod.get_session_count_batch([wad_id])
        assert result.get(wad_id, 0) == 0


class TestCompletions:
    """Test WAD completion tracking."""

    def test_add_completion(self, db_mod, make_wad):
        wad_id = make_wad()
        comp_id = db_mod.add_wad_completion(wad_id)
        assert comp_id > 0

    def test_get_times_beaten(self, db_mod, make_wad):
        wad_id = make_wad()
        assert db_mod.get_times_beaten(wad_id) == 0
        db_mod.add_wad_completion(wad_id)
        assert db_mod.get_times_beaten(wad_id) == 1
        db_mod.add_wad_completion(wad_id)
        assert db_mod.get_times_beaten(wad_id) == 2

    def test_delete_completion(self, db_mod, make_wad):
        wad_id = make_wad()
        comp_id = db_mod.add_wad_completion(wad_id)
        assert db_mod.delete_wad_completion(comp_id)
        assert db_mod.get_times_beaten(wad_id) == 0

    def test_set_completion_count(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.set_wad_completion_count(wad_id, 5)
        assert db_mod.get_times_beaten(wad_id) == 5

    def test_set_completion_count_decrease(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.set_wad_completion_count(wad_id, 5)
        db_mod.set_wad_completion_count(wad_id, 2)
        assert db_mod.get_times_beaten(wad_id) == 2

    def test_get_wad_completions(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id, notes="First clear")
        db_mod.add_wad_completion(wad_id, notes="Replay")
        completions = db_mod.get_wad_completions(wad_id)
        assert len(completions) == 2
        notes = {c["notes"] for c in completions}
        assert notes == {"First clear", "Replay"}


class TestDuplicateDetection:
    """Test find_duplicate() across source types."""

    def test_find_duplicate_idgames_by_source_id(self, db_mod, make_wad):
        make_wad(title="Original", source_id="12345")
        existing = db_mod.find_duplicate(
            source_type=db_mod.SourceType.IDGAMES,
            source_id="12345",
        )
        assert existing is not None
        assert existing["title"] == "Original"

    def test_find_duplicate_no_match(self, db_mod, make_wad):
        make_wad(title="Original", source_id="12345")
        existing = db_mod.find_duplicate(
            source_type=db_mod.SourceType.IDGAMES,
            source_id="99999",
        )
        assert existing is None

    def test_find_duplicate_by_url(self, db_mod, make_wad):
        from caco.db import SourceType
        make_wad(
            title="URL WAD",
            source_type=SourceType.URL,
            source_url="https://example.com/wad.zip",
        )
        existing = db_mod.find_duplicate(
            source_type=SourceType.URL,
            source_url="https://example.com/wad.zip",
        )
        assert existing is not None
        assert existing["title"] == "URL WAD"

    def test_find_duplicate_by_filename(self, db_mod, make_wad):
        make_wad(title="By Filename", filename="eviternity.wad", author="Dragonfly")
        existing = db_mod.find_duplicate(
            source_type=db_mod.SourceType.IDGAMES,
            filename="eviternity.wad",
            author="Dragonfly",
        )
        assert existing is not None


class TestStatsSnapshot:
    """Test StatsSnapshot and get_stats_snapshot."""

    def test_empty_db(self, db_mod):
        snap = db_mod.get_stats_snapshot()
        assert snap.total_wads == 0
        assert snap.total_sessions == 0
        assert snap.total_playtime == 0
        assert snap.played_wads == 0
        assert snap.wads_by_status == {}
        assert snap.activity == []

    def test_with_wads(self, populated_db, db_mod):
        snap = db_mod.get_stats_snapshot()
        assert snap.total_wads == 5
        assert snap.wads_by_status["finished"] == 1
        assert snap.wads_by_status["playing"] == 1
        assert snap.total_sessions == 0  # No sessions added


class TestUpdateWadFieldWhitelist:
    """Test that update_wad rejects invalid fields."""

    def test_reject_invalid_field(self, db_mod, make_wad):
        wad_id = make_wad()
        with pytest.raises(ValueError, match="Cannot update field"):
            db_mod.update_wad(wad_id, evil_field="injection")

    def test_accept_valid_fields(self, db_mod, make_wad):
        wad_id = make_wad(title="Old Title")
        db_mod.update_wad(wad_id, title="New Title", author="New Author")
        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "New Title"
        assert wad["author"] == "New Author"


class TestMigrationVersioning:
    """Test that schema_migrations table is populated."""

    def test_schema_migrations_populated(self, db_mod, tmp_db):
        conn = db_mod.get_connection()
        rows = conn.execute("SELECT version, name FROM schema_migrations ORDER BY version").fetchall()
        assert len(rows) == 7
        assert rows[0]["version"] == 1
        assert rows[6]["version"] == 7
        conn.close()
