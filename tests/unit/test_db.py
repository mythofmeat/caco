"""Tests for caco.db CRUD operations (using in-memory database)."""

import pytest

from caco.db import SourceType, Status


class TestAddAndGetWad:
    def test_add_wad_returns_id(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        assert isinstance(wad_id, int)
        assert wad_id >= 1

    def test_get_wad(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES, author="Erik Alm", year=2003)
        wad = db_mod.get_wad(wad_id)
        assert wad is not None
        assert wad["title"] == "Scythe"
        assert wad["author"] == "Erik Alm"
        assert wad["year"] == 2003
        assert wad["status"] == "backlog"

    def test_get_nonexistent_wad(self, db_mod):
        assert db_mod.get_wad(99999) is None

    def test_wad_has_tags_key(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        wad = db_mod.get_wad(wad_id)
        assert "tags" in wad
        assert wad["tags"] == []


class TestUpdateWad:
    def test_update_title(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.update_wad(wad_id, title="Scythe 2")
        wad = db_mod.get_wad(wad_id)
        assert wad["title"] == "Scythe 2"

    def test_update_status(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.update_wad(wad_id, status=Status.PLAYING)
        wad = db_mod.get_wad(wad_id)
        assert wad["status"] == "playing"

    def test_update_returns_true(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        assert db_mod.update_wad(wad_id, title="New") is True

    def test_update_no_fields_returns_false(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        assert db_mod.update_wad(wad_id) is False


class TestDeleteAndRestore:
    def test_soft_delete(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.delete_wad(wad_id)
        assert db_mod.get_wad(wad_id) is None

    def test_soft_delete_recoverable(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.delete_wad(wad_id)
        assert db_mod.get_wad(wad_id, include_deleted=True) is not None

    def test_restore(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.delete_wad(wad_id)
        db_mod.restore_wad(wad_id)
        assert db_mod.get_wad(wad_id) is not None

    def test_purge(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.delete_wad(wad_id, purge=True)
        assert db_mod.get_wad(wad_id, include_deleted=True) is None


class TestTags:
    def test_add_tag(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        assert db_mod.add_tag(wad_id, "megawad") is True
        wad = db_mod.get_wad(wad_id)
        assert "megawad" in wad["tags"]

    def test_add_duplicate_tag(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.add_tag(wad_id, "megawad")
        assert db_mod.add_tag(wad_id, "megawad") is False

    def test_remove_tag(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.add_tag(wad_id, "megawad")
        assert db_mod.remove_tag(wad_id, "megawad") is True
        wad = db_mod.get_wad(wad_id)
        assert "megawad" not in wad["tags"]

    def test_add_wad_with_tags(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES, tags=["megawad", "cacoward"])
        wad = db_mod.get_wad(wad_id)
        assert sorted(wad["tags"]) == ["cacoward", "megawad"]

    def test_get_all_tags(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES, tags=["megawad", "classic"])
        tags = db_mod.get_all_tags()
        assert "megawad" in tags
        assert "classic" in tags

    def test_get_tag_counts(self, db_mod):
        w1 = db_mod.add_wad("Scythe", SourceType.IDGAMES, tags=["megawad"])
        w2 = db_mod.add_wad("Scythe 2", SourceType.IDGAMES, tags=["megawad", "classic"])
        counts = db_mod.get_tag_counts()
        counts_dict = dict(counts)
        assert counts_dict["megawad"] == 2
        assert counts_dict["classic"] == 1

    def test_get_tag_counts_excludes_deleted(self, db_mod):
        w1 = db_mod.add_wad("Scythe", SourceType.IDGAMES, tags=["megawad"])
        w2 = db_mod.add_wad("Scythe 2", SourceType.IDGAMES, tags=["megawad"])
        db_mod.delete_wad(w2)
        counts = dict(db_mod.get_tag_counts())
        assert counts["megawad"] == 1


class TestSearch:
    def test_search_all(self, db_mod):
        db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.add_wad("Eviternity", SourceType.DOOMWIKI)
        results = db_mod.search_wads()
        assert len(results) == 2

    def test_search_by_status(self, db_mod):
        w1 = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.update_wad(w1, status=Status.PLAYING)
        db_mod.add_wad("Eviternity", SourceType.DOOMWIKI)
        results = db_mod.search_wads(query="status:playing")
        assert len(results) == 1
        assert results[0]["title"] == "Scythe"

    def test_search_free_text(self, db_mod):
        db_mod.add_wad("Scythe", SourceType.IDGAMES, author="Erik Alm")
        db_mod.add_wad("Eviternity", SourceType.DOOMWIKI)
        results = db_mod.search_wads(query="scythe")
        assert len(results) == 1

    def test_search_excludes_deleted(self, db_mod):
        w1 = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.delete_wad(w1)
        results = db_mod.search_wads()
        assert len(results) == 0


class TestCompletions:
    def test_add_completion(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        comp_id = db_mod.add_wad_completion(wad_id)
        assert isinstance(comp_id, int)

    def test_times_beaten(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.add_wad_completion(wad_id)
        db_mod.add_wad_completion(wad_id)
        assert db_mod.get_times_beaten(wad_id) == 2

    def test_set_completion_count(self, db_mod):
        wad_id = db_mod.add_wad("Scythe", SourceType.IDGAMES)
        db_mod.set_wad_completion_count(wad_id, 5)
        assert db_mod.get_times_beaten(wad_id) == 5
        db_mod.set_wad_completion_count(wad_id, 2)
        assert db_mod.get_times_beaten(wad_id) == 2
