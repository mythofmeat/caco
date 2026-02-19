# Refactoring Analysis Report: Caco

## Executive Summary

Caco is well-structured for its age -- the layered architecture (db, sources, adapters, three UIs) is sound, and several good patterns exist (`BaseSearchPane`, `_batch_query`, `BaseHttpClient`). However, organic growth has produced identifiable code smells: **duplicated display logic** across three UIs, **config.py with repetitive I/O**, **CLI import logic duplicating what sources/adapters own**, and **scattered magic values**. `db.py` at 1,361 lines is large but well-sectioned -- its primary problem is the ad hoc migration system.

---

## P0 -- Must Fix

### RF-01: Duplicate `_normalize_status` in db.py and cli/__init__.py
**Effort: S** | Two independent implementations of status normalization. Any shortcut added to `STATUS_SHORTCUTS` must be reconciled across both.

**Fix:** Keep `db._normalize_status` as canonical. CLI helper becomes a one-liner delegate.

### RF-02: Duplicate "Already in library" pattern repeated 12 times
**Effort: S** | The duplicate-check-and-print pattern appears 7 times in CLI, 5 times in TUI widgets.

**Fix:** Centralize into service layer (see RF-13).

---

## P1 -- High Value

### RF-03: Status display and colors split across two theme files
**Effort: S** | `tui/theme.py` and `gui/theme.py` define the same six status display names independently.

**Fix:** Extract canonical `STATUS_METADATA` into shared module.

### RF-04: Rating rendering duplicated 7 times
**Effort: S** | The star expression `"★" * rating + "☆" * (5 - rating)` appears in 7 files.

**Fix:** Add `format_rating()` to `utils.py`.

### RF-05: Author+year display formatting duplicated 6 times
**Effort: S** | Building `"Author (Year)"` from a WAD dict appears in 6 places.

**Fix:** Add `format_author_year()` to `utils.py`.

### RF-06: config.py loads TOML file 18+ times per process
**Effort: S** | Every getter function independently calls `load_config()`.

**Fix:** `@functools.lru_cache(maxsize=1)` on `load_config()`, invalidate in `save_config`.

### RF-07: `get_tui_config` and `get_gui_config` repeat merge pattern
**Effort: S** | Same config merge logic repeated for `[list]`, `[tui]`, `[gui]` sections.

**Fix:** Extract `_merge_section_config()` helper.

### RF-08: db.py migration system is ad hoc
**Effort: M** | No migration version tracking. 7 migrations run existence checks on every startup.

**Fix:** Create `schema_migrations` table with version number.

---

## P2 -- Medium Priority

### RF-09: Batch stats loading duplicated in WadTable (TUI) and WadTableModel (GUI)
**Effort: M** | Identical 4-batch-query + 4-map + `get_wad_stats()` pattern copied between files.

**Fix:** Extract `WadStatsCache` class.

### RF-10: Stats display logic triplicated across CLI, TUI, and GUI
**Effort: M** | Same three DB calls + result structuring in three places.

**Fix:** Create `get_stats_snapshot()` returning a dataclass.

### RF-11: Description truncation has 4 different cutoff lengths
**Effort: S** | Truncated at 120, 300, 500, 500 across files, some unconditionally.

**Fix:** Add `truncate()` to `utils.py`. Fixes 2 unconditional-truncation bugs.

### RF-12: cli/import_cmds.py contains 120+ lines of import business logic
**Effort: M** | Mix of business logic and UI feedback that can't be reused by TUI.

**Fix:** Factor business logic into `ImportService` (see RF-13).

### RF-13: Extract minimal service layer for WAD import operations
**Effort: L** | Root cause of RF-02 and RF-12. All three UIs independently call `db.find_duplicate` and source adapters.

**Fix:** Create `src/caco/services/import_service.py` with `ImportResult` dataclass and `ImportService` class.

---

## P3 -- Low Priority

### RF-14: WadIdRange and _parse_id_range do the same thing
**Effort: S** | Nearly identical ID range parsing logic in two places.

### RF-15: `_format_size` in cli/cache.py should be in utils.py
**Effort: XS** | General utility placed in CLI module.

### RF-16: Source adapters have redundant `__enter__`/`__exit__`
**Effort: S** | Three adapters define identical context manager methods.

### RF-17: Magic number 120 for description snippet
**Effort: XS** | Bare magic number in rendering code.

### RF-18: `_infer_title_from_filename/url` belong in utils.py
**Effort: XS** | Helpers in CLI that contain no CLI logic.

---

## Dependency Graph

```
Level 1 (independent):
  RF-04, RF-05, RF-06, RF-07, RF-11, RF-14, RF-15, RF-17, RF-18

Level 2 (depends on Level 1):
  RF-01, RF-03, RF-08, RF-10, RF-16

Level 3 (depends on Level 2):
  RF-09, RF-13

Level 4 (depends on Level 3):
  RF-02, RF-12
```

---

## Quick Wins (~3 hours total)

| ID | What | Time |
|----|------|------|
| RF-04 | `format_rating()` in utils.py | 30 min |
| RF-05 | `format_author_year()` in utils.py | 30 min |
| RF-06 | `lru_cache` on `load_config()` | 15 min |
| RF-07 | `_merge_section_config` helper | 20 min |
| RF-11 | `truncate()` in utils.py + fix bugs | 20 min |
| RF-15 | Move `_format_size` to utils.py | 10 min |
| RF-17 | Name magic 120 constant | 5 min |
| RF-18 | Move title inference helpers | 10 min |
| RF-01 | Single `_normalize_status` | 30 min |
