"""Extended companion DB tests — edge cases not covered by test_companions.py."""

import sqlite3

import pytest


# =============================================================================
# Companion CRUD edge cases
# =============================================================================


class TestCompanionCrudEdgeCases:
    def test_get_nonexistent_companion(self, db_mod):
        assert db_mod.get_companion(99999) is None

    def test_add_duplicate_md5_raises(self, db_mod):
        db_mod.add_companion("a.deh", "/a", "same_md5", 100)
        with pytest.raises(sqlite3.IntegrityError):
            db_mod.add_companion("b.deh", "/b", "same_md5", 200)

    def test_get_all_companions_empty(self, db_mod):
        assert db_mod.get_all_companions() == []


# =============================================================================
# Linking edge cases
# =============================================================================


class TestCompanionLinkingEdgeCases:
    def test_unlink_nonexistent_link(self, db_mod, make_wad):
        wad_id = make_wad(title="Test")
        cid = db_mod.add_companion("patch.deh", "/p", "md5abc", 100)
        assert db_mod.unlink_companion(wad_id, cid) == 0


# =============================================================================
# Orphan detection edge cases
# =============================================================================


class TestCompanionOrphanEdgeCases:
    def test_is_orphan_never_linked(self, db_mod):
        """Companion that was never linked to any WAD is an orphan."""
        cid = db_mod.add_companion("patch.deh", "/p", "md5", 100)
        assert db_mod.is_orphan(cid) is True

    def test_would_be_orphan_multiple_links(self, db_mod, make_wad):
        w1 = make_wad(title="A")
        w2 = make_wad(title="B")
        cid = db_mod.add_companion("patch.deh", "/p", "md5", 100)
        db_mod.link_companion(w1, cid)
        db_mod.link_companion(w2, cid)

        # Unlinking from either WAD won't orphan since the other still linked
        assert db_mod.would_be_orphan(cid, w1) is False
        assert db_mod.would_be_orphan(cid, w2) is False

    def test_would_be_orphan_single_link(self, db_mod, make_wad):
        wad_id = make_wad()
        cid = db_mod.add_companion("patch.deh", "/p", "md5", 100)
        db_mod.link_companion(wad_id, cid)
        assert db_mod.would_be_orphan(cid, wad_id) is True


# =============================================================================
# Reverse lookup edge cases
# =============================================================================


class TestCompanionReverseLookupEdgeCases:
    def test_get_companion_wads_none(self, db_mod):
        cid = db_mod.add_companion("lonely.deh", "/p", "md5", 100)
        assert db_mod.get_companion_wads(cid) == []

    def test_find_by_filename_different_wad(self, db_mod, make_wad):
        w1 = make_wad(title="A")
        w2 = make_wad(title="B")
        cid = db_mod.add_companion("patch.deh", "/p", "md5", 100)
        db_mod.link_companion(w1, cid)

        # patch.deh not linked to w2
        assert db_mod.get_wad_companion_by_filename(w2, "patch.deh") is None


# =============================================================================
# Companions with counts
# =============================================================================


class TestCompanionsWithCounts:
    def test_counts_match_links(self, db_mod, make_wad):
        w1 = make_wad(title="A")
        w2 = make_wad(title="B")
        c1 = db_mod.add_companion("shared.deh", "/a", "md5a", 100)
        c2 = db_mod.add_companion("solo.deh", "/b", "md5b", 100)
        db_mod.link_companion(w1, c1)
        db_mod.link_companion(w2, c1)
        db_mod.link_companion(w1, c2)

        comps = db_mod.get_all_companions_with_counts()
        counts = {c["filename"]: c["wad_count"] for c in comps}
        assert counts["shared.deh"] == 2
        assert counts["solo.deh"] == 1

    def test_unlinked_companion_has_zero_count(self, db_mod):
        db_mod.add_companion("orphan.deh", "/o", "md5o", 100)
        comps = db_mod.get_all_companions_with_counts()
        assert len(comps) == 1
        assert comps[0]["wad_count"] == 0
