"""Extended DB tests — edge cases for CRUD, search, sort, tags, completions, cache, and library stats."""

import pytest

from caco.db import SourceType, Status


# =============================================================================
# WAD CRUD edge cases
# =============================================================================


class TestAddWadEdgeCases:
    def test_add_wad_all_fields(self, db_mod):
        wad_id = db_mod.add_wad(
            "Full WAD",
            SourceType.IDGAMES,
            author="Author",
            year=2024,
            description="A great WAD",
            source_id="99999",
            source_url="https://example.com",
            filename="full.wad",
            cached_path="/tmp/full.wad",
            status=Status.PLAYING,
            tags=["megawad", "cacoward"],
            version="1.0",
        )
        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "Full WAD"
        assert wad["author"] == "Author"
        assert wad["year"] == 2024
        assert wad["description"] == "A great WAD"
        assert wad["source_id"] == "99999"
        assert wad["source_url"] == "https://example.com"
        assert wad["filename"] == "full.wad"
        assert wad["cached_path"] == "/tmp/full.wad"
        assert wad["status"] == "playing"
        assert sorted(wad["tags"]) == ["cacoward", "megawad"]
        assert wad["version"] == "1.0"

    def test_add_wad_minimal(self, db_mod):
        wad_id = db_mod.add_wad("Minimal", SourceType.LOCAL)
        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "Minimal"
        assert wad["author"] is None
        assert wad["year"] is None
        assert wad["status"] == "backlog"

    def test_add_multiple_wads_unique_ids(self, db_mod):
        ids = [db_mod.add_wad(f"WAD {i}", SourceType.LOCAL) for i in range(10)]
        assert len(set(ids)) == 10
        assert all(i > 0 for i in ids)

    def test_tags_are_lowercased(self, db_mod):
        wad_id = db_mod.add_wad("Test", SourceType.LOCAL, tags=["MEGAWAD", "Cacoward"])
        wad = db_mod.get_wad(wad_id)
        assert "megawad" in wad["tags"]
        assert "cacoward" in wad["tags"]


class TestUpdateWadEdgeCases:
    def test_update_multiple_fields(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.update_wad(
            wad_id,
            title="New Title",
            author="New Author",
            year=2025,
            rating=5,
            notes="Great!",
        )
        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "New Title"
        assert wad["author"] == "New Author"
        assert wad["year"] == 2025
        assert wad["rating"] == 5
        assert wad["notes"] == "Great!"

    def test_update_sets_updated_at(self, db_mod, make_wad):
        wad_id = make_wad()
        wad_before = db_mod.get_wad(wad_id)
        db_mod.update_wad(wad_id, title="Changed")
        wad_after = db_mod.get_wad(wad_id)
        assert wad_after["updated_at"] >= wad_before["updated_at"]

    def test_update_nonexistent_wad(self, db_mod):
        result = db_mod.update_wad(999999, title="Ghost")
        assert result is False

    def test_update_status_to_finished_no_auto_completion(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.update_wad(wad_id, status=Status.FINISHED)
        assert db_mod.get_times_beaten(wad_id) == 0

    def test_update_status_to_playing_no_completion(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.update_wad(wad_id, status=Status.PLAYING)
        assert db_mod.get_times_beaten(wad_id) == 0

    def test_update_with_enum_value(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.update_wad(wad_id, status=Status.ABANDONED)
        wad = db_mod.get_wad(wad_id)
        assert wad["status"] == "abandoned"

    def test_update_reject_multiple_invalid_fields(self, db_mod, make_wad):
        wad_id = make_wad()
        with pytest.raises(ValueError, match="Cannot update"):
            db_mod.update_wad(wad_id, evil="inject", another_bad="field")


class TestDeleteRestoreEdgeCases:
    def test_double_delete_returns_false(self, db_mod, make_wad):
        wad_id = make_wad()
        assert db_mod.delete_wad(wad_id) is True
        assert db_mod.delete_wad(wad_id) is False  # already deleted

    def test_restore_non_deleted_returns_false(self, db_mod, make_wad):
        wad_id = make_wad()
        assert db_mod.restore_wad(wad_id) is False

    def test_delete_nonexistent_returns_false(self, db_mod):
        assert db_mod.delete_wad(999999) is False

    def test_purge_all_deleted(self, db_mod, make_wad):
        w1 = make_wad(title="A")
        w2 = make_wad(title="B")
        w3 = make_wad(title="C")
        db_mod.delete_wad(w1)
        db_mod.delete_wad(w3)
        count = db_mod.purge_all_deleted()
        assert count == 2
        assert db_mod.get_wad(w2) is not None

    def test_purge_all_deleted_empty(self, db_mod):
        count = db_mod.purge_all_deleted()
        assert count == 0


# =============================================================================
# Tags edge cases
# =============================================================================


class TestTagsExtended:
    def test_remove_nonexistent_tag(self, db_mod, make_wad):
        wad_id = make_wad()
        assert db_mod.remove_tag(wad_id, "nonexistent") is False

    def test_remove_all_tags(self, db_mod, make_wad):
        wad_id = make_wad(tags=["a", "b", "c"])
        count = db_mod.remove_all_tags(wad_id)
        assert count == 3
        wad = db_mod.get_wad(wad_id)
        assert wad["tags"] == []

    def test_remove_all_tags_when_none(self, db_mod, make_wad):
        wad_id = make_wad()
        count = db_mod.remove_all_tags(wad_id)
        assert count == 0

    def test_remove_tags_by_glob_pattern(self, db_mod, make_wad):
        wad_id = make_wad(tags=["cacoward", "classic", "megawad", "challenge"])
        count = db_mod.remove_tags_by_pattern(wad_id, "c*")
        assert count == 3  # cacoward, classic, challenge
        wad = db_mod.get_wad(wad_id)
        assert wad["tags"] == ["megawad"]

    def test_remove_tags_by_exact_match(self, db_mod, make_wad):
        wad_id = make_wad(tags=["cacoward", "megawad"])
        count = db_mod.remove_tags_by_pattern(wad_id, "cacoward")
        assert count == 1
        wad = db_mod.get_wad(wad_id)
        assert wad["tags"] == ["megawad"]

    def test_tag_case_normalization(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.add_tag(wad_id, "MEGAWAD")
        wad = db_mod.get_wad(wad_id)
        assert "megawad" in wad["tags"]

    def test_get_all_tags_across_wads(self, db_mod, make_wad):
        make_wad(tags=["a", "b"])
        make_wad(tags=["b", "c"])
        tags = db_mod.get_all_tags()
        assert sorted(tags) == ["a", "b", "c"]


# =============================================================================
# Search edge cases
# =============================================================================


class TestSearchExtended:
    def test_search_sort_by_title_asc(self, db_mod, make_wad):
        """sort_desc=True + title uses reverse_dir=ASC → A-Z."""
        make_wad(title="Zebra")
        make_wad(title="Alpha")
        make_wad(title="Middle")
        results = db_mod.search_wads(sort_by="title", sort_desc=True)
        titles = [r["title"] for r in results]
        assert titles == ["Alpha", "Middle", "Zebra"]

    def test_search_sort_by_title_desc(self, db_mod, make_wad):
        """sort_desc=False + title uses reverse_dir=DESC → Z-A."""
        make_wad(title="Zebra")
        make_wad(title="Alpha")
        results = db_mod.search_wads(sort_by="title", sort_desc=False)
        titles = [r["title"] for r in results]
        assert titles == ["Zebra", "Alpha"]

    def test_search_sort_by_year(self, db_mod, make_wad):
        make_wad(title="Old", year=1994)
        make_wad(title="New", year=2024)
        make_wad(title="Mid", year=2010)
        results = db_mod.search_wads(sort_by="year", sort_desc=True)
        years = [r["year"] for r in results]
        assert years == [2024, 2010, 1994]

    def test_search_sort_by_id(self, db_mod, make_wad):
        """sort_desc=True + id uses reverse_dir=ASC → ascending IDs."""
        w1 = make_wad(title="First")
        w2 = make_wad(title="Second")
        results = db_mod.search_wads(sort_by="id", sort_desc=True)
        assert results[0]["id"] == w1
        assert results[1]["id"] == w2

    def test_search_sort_random(self, db_mod, make_wad):
        """Random sort should not crash."""
        for i in range(5):
            make_wad(title=f"WAD {i}")
        results = db_mod.search_wads(sort_by="random")
        assert len(results) == 5

    def test_search_invalid_sort_raises(self, db_mod):
        with pytest.raises(ValueError, match="Invalid sort field"):
            db_mod.search_wads(sort_by="evil_column; DROP TABLE wads;--")

    def test_search_with_limit(self, db_mod, make_wad):
        for i in range(10):
            make_wad(title=f"WAD {i}")
        results = db_mod.search_wads(limit=3)
        assert len(results) == 3

    def test_search_by_source_type(self, db_mod, make_wad):
        make_wad(title="idgames WAD", source_type=SourceType.IDGAMES)
        make_wad(title="wiki WAD", source_type=SourceType.DOOMWIKI)
        results = db_mod.search_wads(query="source:idgames")
        assert len(results) == 1
        assert results[0]["title"] == "idgames WAD"

    def test_search_by_author(self, db_mod, make_wad):
        make_wad(title="WAD 1", author="Ribbiks")
        make_wad(title="WAD 2", author="skillsaw")
        results = db_mod.search_wads(query="author:ribbiks")
        assert len(results) == 1

    def test_search_by_filename(self, db_mod, make_wad):
        make_wad(title="Test", filename="eviternity.wad")
        results = db_mod.search_wads(query="filename:eviternity")
        assert len(results) == 1

    def test_search_include_deleted(self, db_mod, make_wad):
        w1 = make_wad(title="Active")
        w2 = make_wad(title="Trashed")
        db_mod.delete_wad(w2)
        results = db_mod.search_wads(include_deleted=True)
        assert len(results) == 1
        assert results[0]["title"] == "Trashed"

    def test_search_or_query(self, db_mod, make_wad):
        make_wad(title="A", status="playing")
        make_wad(title="B", status="to-play")
        make_wad(title="C", status="finished")
        results = db_mod.search_wads(query="status:playing , status:to-play")
        assert len(results) == 2
        titles = {r["title"] for r in results}
        assert titles == {"A", "B"}

    def test_search_negation(self, db_mod, make_wad):
        make_wad(title="A", status="playing")
        make_wad(title="B", status="finished")
        make_wad(title="C", status="backlog")
        results = db_mod.search_wads(query="^status:finished")
        assert len(results) == 2
        titles = {r["title"] for r in results}
        assert "B" not in titles

    def test_search_free_text_matches_description(self, db_mod, make_wad):
        make_wad(title="WAD", description="This is a megawad with lots of slaughter maps")
        results = db_mod.search_wads(query="slaughter")
        assert len(results) == 1

    def test_search_free_text_matches_author(self, db_mod, make_wad):
        make_wad(title="WAD", author="skillsaw")
        results = db_mod.search_wads(query="skillsaw")
        assert len(results) == 1

    def test_search_by_tag(self, db_mod, make_wad):
        make_wad(title="Tagged", tags=["megawad"])
        make_wad(title="Untagged")
        results = db_mod.search_wads(query="tag:megawad")
        assert len(results) == 1
        assert results[0]["title"] == "Tagged"

    def test_search_by_tag_glob(self, db_mod, make_wad):
        make_wad(title="A", tags=["cacoward"])
        make_wad(title="B", tags=["classic"])
        results = db_mod.search_wads(query="tag:caco*")
        assert len(results) == 1
        assert results[0]["title"] == "A"

    def test_search_negated_tag(self, db_mod, make_wad):
        make_wad(title="A", tags=["slaughter"])
        make_wad(title="B", tags=["classic"])
        results = db_mod.search_wads(query="^tag:slaughter")
        assert len(results) == 1
        assert results[0]["title"] == "B"

    def test_search_by_complevel(self, db_mod, make_wad):
        w1 = make_wad(title="Boom")
        db_mod.update_wad(w1, complevel=9)
        make_wad(title="Vanilla")
        results = db_mod.search_wads(query="complevel:boom")
        assert len(results) == 1
        assert results[0]["title"] == "Boom"

    def test_search_by_iwad(self, db_mod, make_wad):
        w1 = make_wad(title="D2")
        db_mod.update_wad(w1, custom_iwad="doom2")
        make_wad(title="D1")
        results = db_mod.search_wads(query="iwad:doom2")
        assert len(results) == 1

    def test_search_by_config(self, db_mod, make_wad):
        w1 = make_wad(title="Configured")
        db_mod.update_wad(w1, custom_config="controller")
        make_wad(title="Default")
        results = db_mod.search_wads(query="config:controller")
        assert len(results) == 1

    def test_search_results_include_tags(self, db_mod, make_wad):
        make_wad(title="Tagged", tags=["megawad", "cacoward"])
        results = db_mod.search_wads()
        assert sorted(results[0]["tags"]) == ["cacoward", "megawad"]

    def test_search_sort_by_playtime(self, db_mod, make_wad):
        """Playtime sort requires GROUP BY — test it doesn't crash."""
        make_wad(title="A")
        make_wad(title="B")
        results = db_mod.search_wads(sort_by="playtime")
        assert len(results) == 2

    def test_search_sort_by_last_played(self, db_mod, make_wad):
        make_wad(title="A")
        results = db_mod.search_wads(sort_by="last_played")
        assert len(results) == 1

    def test_search_combined_and_query(self, db_mod, make_wad):
        make_wad(title="Target", status="playing", author="Ribbiks")
        make_wad(title="Wrong Status", status="backlog", author="Ribbiks")
        make_wad(title="Wrong Author", status="playing", author="skillsaw")
        results = db_mod.search_wads(query="status:playing author:Ribbiks")
        assert len(results) == 1
        assert results[0]["title"] == "Target"


# =============================================================================
# Duplicate detection extended
# =============================================================================


class TestDuplicateDetectionExtended:
    def test_doomwiki_duplicate_by_source_id(self, db_mod, make_wad):
        make_wad(title="Wiki WAD", source_type=SourceType.DOOMWIKI, source_id="42")
        dup = db_mod.find_duplicate(source_type=SourceType.DOOMWIKI, source_id="42")
        assert dup is not None

    def test_doomworld_duplicate_by_source_id(self, db_mod, make_wad):
        make_wad(title="Forum WAD", source_type=SourceType.DOOMWORLD, source_id="100")
        dup = db_mod.find_duplicate(source_type=SourceType.DOOMWORLD, source_id="100")
        assert dup is not None

    def test_local_duplicate_by_source_url(self, db_mod, make_wad):
        make_wad(title="Local", source_type=SourceType.LOCAL, source_url="/home/user/test.wad")
        dup = db_mod.find_duplicate(source_type=SourceType.LOCAL, source_url="/home/user/test.wad")
        assert dup is not None

    def test_filename_only_match(self, db_mod, make_wad):
        make_wad(title="Test", filename="scythe.wad")
        dup = db_mod.find_duplicate(
            source_type=SourceType.IDGAMES,
            filename="scythe.wad",
        )
        assert dup is not None

    def test_filename_strips_extension(self, db_mod, make_wad):
        make_wad(title="Test", filename="scythe.wad")
        dup = db_mod.find_duplicate(
            source_type=SourceType.IDGAMES,
            filename="scythe.zip",
        )
        # Should match because "scythe" matches "scythe"
        assert dup is not None

    def test_no_match_returns_none(self, db_mod, make_wad):
        make_wad(title="Existing")
        dup = db_mod.find_duplicate(source_type=SourceType.IDGAMES)
        assert dup is None


# =============================================================================
# Cache management
# =============================================================================


class TestCacheManagement:
    def test_get_cached_wads(self, db_mod, make_wad):
        make_wad(title="Cached", cached_path="/tmp/cached.wad")
        make_wad(title="Not Cached")
        cached = db_mod.get_cached_wads()
        assert len(cached) == 1
        assert cached[0]["title"] == "Cached"

    def test_get_cached_wads_excludes_deleted(self, db_mod, make_wad):
        w = make_wad(title="Cached", cached_path="/tmp/c.wad")
        db_mod.delete_wad(w)
        assert db_mod.get_cached_wads() == []

    def test_clear_cached_path(self, db_mod, make_wad):
        w = make_wad(title="Cached", cached_path="/tmp/c.wad")
        assert db_mod.clear_cached_path(w)
        wad = db_mod.get_wad(w)
        assert wad["cached_path"] is None

    def test_clear_all_cached_paths(self, db_mod, make_wad):
        make_wad(title="A", cached_path="/tmp/a.wad")
        make_wad(title="B", cached_path="/tmp/b.wad")
        make_wad(title="C")  # no cache
        count = db_mod.clear_all_cached_paths()
        assert count == 2

    def test_get_wad_by_cached_filename(self, db_mod, make_wad):
        make_wad(title="Test", cached_path="/cache/dir/eviternity.wad")
        wad = db_mod.get_wad_by_cached_filename("eviternity.wad")
        assert wad is not None
        assert wad["title"] == "Test"

    def test_get_wad_by_cached_filename_no_match(self, db_mod, make_wad):
        assert db_mod.get_wad_by_cached_filename("nonexistent.wad") is None


# =============================================================================
# Completion extended tests
# =============================================================================


class TestCompletionExtended:
    def test_add_completion_with_stats(self, db_mod, make_wad):
        wad_id = make_wad()
        stats = '{"maps":[{"name":"MAP01"}]}'
        comp_id = db_mod.add_wad_completion(wad_id, stats_snapshot=stats)
        completions = db_mod.get_wad_completions(wad_id)
        assert completions[0]["stats_snapshot"] == stats

    def test_add_completion_with_backdated_timestamp(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id, completed_at="2020-01-01T00:00:00")
        completions = db_mod.get_wad_completions(wad_id)
        assert "2020-01-01" in completions[0]["completed_at"]

    def test_delete_completion_by_timestamp(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id, completed_at="2020-06-15T12:00:00")
        assert db_mod.delete_wad_completion_by_timestamp(wad_id, "2020-06-15T12:00:00")
        assert db_mod.get_times_beaten(wad_id) == 0

    def test_find_completion_by_timestamp_prefix(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.add_wad_completion(wad_id, completed_at="2024-06-15T18:30:00")
        found = db_mod.find_completion_by_timestamp(wad_id, "2024-06-15")
        assert found is not None

    def test_find_completion_by_timestamp_no_match(self, db_mod, make_wad):
        wad_id = make_wad()
        found = db_mod.find_completion_by_timestamp(wad_id, "2099-01-01")
        assert found is None

    def test_update_completion_stats(self, db_mod, make_wad):
        wad_id = make_wad()
        comp_id = db_mod.add_wad_completion(wad_id)
        assert db_mod.update_wad_completion(comp_id, stats_snapshot='{"new":true}')
        completions = db_mod.get_wad_completions(wad_id)
        assert completions[0]["stats_snapshot"] == '{"new":true}'

    def test_update_completion_notes(self, db_mod, make_wad):
        wad_id = make_wad()
        comp_id = db_mod.add_wad_completion(wad_id)
        assert db_mod.update_wad_completion(comp_id, notes="UV-Max")
        completions = db_mod.get_wad_completions(wad_id)
        assert completions[0]["notes"] == "UV-Max"

    def test_update_completion_no_fields(self, db_mod, make_wad):
        wad_id = make_wad()
        comp_id = db_mod.add_wad_completion(wad_id)
        assert db_mod.update_wad_completion(comp_id) is False

    def test_delete_nonexistent_completion(self, db_mod):
        assert db_mod.delete_wad_completion(999999) is False

    def test_set_completion_count_zero(self, db_mod, make_wad):
        wad_id = make_wad()
        db_mod.set_wad_completion_count(wad_id, 3)
        db_mod.set_wad_completion_count(wad_id, 0)
        assert db_mod.get_times_beaten(wad_id) == 0


# =============================================================================
# Library statistics
# =============================================================================


class TestLibraryStats:
    def test_stats_with_sessions(self, db_mod, make_wad):
        w1 = make_wad(title="A", status="playing")
        w2 = make_wad(title="B", status="finished")
        s1 = db_mod.start_session(w1)
        db_mod.end_session(s1)
        s2 = db_mod.start_session(w2)
        db_mod.end_session(s2)

        stats = db_mod.get_library_stats()
        assert stats["total_wads"] == 2
        assert stats["total_sessions"] == 2
        assert stats["wads_with_sessions"] == 2
        assert stats["wads_by_status"]["playing"] == 1
        assert stats["wads_by_status"]["finished"] == 1

    def test_completion_rate(self, db_mod, make_wad):
        w1 = make_wad(title="Finished", status="finished")
        w2 = make_wad(title="Playing", status="playing")
        # Both need sessions for completion rate
        s1 = db_mod.start_session(w1)
        db_mod.end_session(s1)
        s2 = db_mod.start_session(w2)
        db_mod.end_session(s2)

        rate = db_mod.get_completion_rate()
        assert rate["played_wads"] == 2
        assert rate["finished_wads"] == 1
        assert rate["completion_rate"] == 0.5

    def test_completion_rate_no_sessions(self, db_mod, make_wad):
        make_wad(title="Unplayed")
        rate = db_mod.get_completion_rate()
        assert rate["completion_rate"] == 0.0

    def test_get_most_recently_played(self, db_mod, make_wad):
        w1 = make_wad(title="First")
        s1 = db_mod.start_session(w1)
        db_mod.end_session(s1)

        w2 = make_wad(title="Second")
        s2 = db_mod.start_session(w2)
        db_mod.end_session(s2)

        most_recent = db_mod.get_most_recently_played()
        assert most_recent is not None
        assert most_recent["title"] == "Second"

    def test_get_most_recently_played_empty(self, db_mod):
        assert db_mod.get_most_recently_played() is None

    def test_get_wad_stats(self, db_mod, make_wad):
        wad_id = make_wad()
        s1 = db_mod.start_session(wad_id)
        db_mod.end_session(s1)
        s2 = db_mod.start_session(wad_id)
        db_mod.end_session(s2)

        stats = db_mod.get_wad_stats(wad_id)
        assert stats["session_count"] == 2
        assert stats["total_playtime"] >= 0

    def test_get_wad_stats_no_sessions(self, db_mod, make_wad):
        wad_id = make_wad()
        stats = db_mod.get_wad_stats(wad_id)
        assert stats["session_count"] == 0
        assert stats["total_playtime"] == 0


# =============================================================================
# Session demo tracking
# =============================================================================


class TestSessionDemo:
    def test_attach_demo_file(self, db_mod, make_wad):
        wad_id = make_wad()
        session_id = db_mod.start_session(wad_id)
        db_mod.end_session(session_id)
        db_mod.update_session_demo(session_id, "/data/demos/run01.lmp")

        sessions = db_mod.get_sessions(wad_id)
        assert sessions[0]["demo_file"] == "/data/demos/run01.lmp"

    def test_session_without_demo(self, db_mod, make_wad):
        wad_id = make_wad()
        session_id = db_mod.start_session(wad_id)
        db_mod.end_session(session_id)

        sessions = db_mod.get_sessions(wad_id)
        assert sessions[0]["demo_file"] is None
