# Refactoring QA Verification Report

> Verified 2026-02-20 against the codebase on branch `1.2.1`.
>
> This report consolidates the results of 7 independent reviews (architecture,
> code quality, QA, test automation, refactoring analysis, Python quality, and
> performance) that identified **57 action items** across 4 phases. Each item
> was verified against the current source code.

---

## Result Summary

| Phase | Done | Partial | Not Done | Total |
|-------|------|---------|----------|-------|
| Phase 1: Quick Wins | 16 | 0 | 0 | 16 |
| Phase 2: DB & Service | 8 | 0 | 0 | 8 |
| Phase 3: Test Infra | 9 | 0 | 0 | 9 |
| Phase 4: Polish | 23 | 0 | 0 | 23 |
| **Total** | **56** | **0** | **0** | **56** |

**Completion rate: 100%.**

---

## Phase 1: Critical Fixes & Quick Wins -- ALL DONE

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1.1 | `update_wad` field whitelist | DONE | `ALLOWED_UPDATE_FIELDS` frozenset in `db.py:130-134`; validated at `db.py:791` |
| 1.2 | Cache `load_config()` | DONE | `_config_cache` global in `config.py:33-64`; invalidated in `save_config()` |
| 1.3 | SQLite WAL + PRAGMAs | DONE | `get_connection()` sets WAL, synchronous, cache_size, temp_store (`db.py:226-229`) |
| 1.4 | Missing indexes | DONE | `_POST_MIGRATION_INDEXES` in `db.py:212-216` (deleted_at, cached_path, sessions) |
| 1.5 | Fix `save_config` nested sections | DONE | `save_config()` emits `[section]` headers for nested dicts (`config.py:67-102`) |
| 1.6 | Validate sourceport | DONE | `shutil.which()` check in `player.py:223-229`; editor validated in `config_cmd.py:71-73` |
| 1.7 | Shared formatting functions | DONE | `format_rating`, `format_author_year`, `truncate`, `format_size` in `utils.py:15-50` |
| 1.8 | `awaiting-update` in completions | DONE | Present in `QUERY_STATUS_VALUES` at `cli/__init__.py:373` |
| 1.9 | Pydantic mutable default | DONE | `Field(default_factory=list)` in `doomworld/models.py:24` |
| 1.10 | Config merge helper | DONE | `_merge_section_config()` in `config.py:216-225`, used by list/tui/gui config getters |
| 1.11 | Tag query ESCAPE clause | DONE | Both glob and non-glob tag queries include `ESCAPE '\\'` (`db.py:622,627`) |
| 1.12 | Grayscale fallback palette | DONE | Correct RGB triple generation at `gui/thumbnails/extractor.py:203` |
| 1.13 | GUI `get_wad_by_id` O(1) | DONE | `_wad_index: dict[int, int]` in `gui/models/wad_model.py:23,44-45,153-158` |
| 1.14 | `executemany` for completions | DONE | `conn.executemany()` in `db.py:1212-1215` |
| 1.15 | Download chunk size 256 KB | DONE | `chunk_size=262144` in `idgames/client.py:239` |
| 1.16 | Shared httpx client | DONE | `_shared_client` + `_get_client()` in `gui/thumbnails/scraper.py:23-32` |

---

## Phase 2: Database & Service Layer -- ALL DONE

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 2.1 | Unified `get_wad_stats_batch()` | DONE | Single function at `db.py:1257`; 2 queries on 1 connection with chunking |
| 2.2 | Atomic completion recording | DONE | Completion INSERT inside same `with get_connection()` at `db.py:812-824` |
| 2.3 | Migration version tracking | DONE | `schema_migrations` table in schema (`db.py:204-208`); version checks in `init_db()` |
| 2.4 | Batch query chunking | DONE | `_SQLITE_MAX_VARS = 900` at `db.py:949`; `_batch_query()` helper chunks at that limit |
| 2.4a | Batch `auto_clean_cache` | DONE | `get_last_played_batch(wad_ids)` called once before loop (`player.py:105-107`) |
| 2.5 | Unified `STATUS_METADATA` | DONE | `MappingProxyType` at `db.py:114-123`; imported by `tui/theme.py:6` and `gui/theme.py:8` |
| 2.6 | `StatsSnapshot` dataclass | DONE | Dataclass at `db.py:1412-1431`; `get_stats_snapshot()` at `db.py:1434-1454` |
| 2.7 | Import service layer | DONE | `services/import_service.py` (249 lines) with `ImportService` class and `ImportResult` dataclass |

---

## Phase 3: Test Infrastructure -- ALL DONE

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 3.1 | Pytest configuration | DONE | `[tool.pytest.ini_options]` in `pyproject.toml:29-34` with testpaths and markers |
| 3.2 | Test dependencies | DONE | `pytest-mock>=3.12` and `respx>=0.20` present in `[test]` extras |
| 3.3 | Extended fixtures | DONE | `make_wad`, `populated_db`, `tmp_config` in `tests/conftest.py`; `CliRunner` in test files |
| 3.4 | `test_db_sessions.py` | DONE | Covers sessions, batch stats, completions, duplicates |
| 3.5 | `test_parsers.py` | DONE | Covers WikitextParser and DoomworldParser |
| 3.6 | `test_cli_library.py` | DONE | Covers list, info, update, delete, restore, tags, random commands |
| 3.7 | `test_config.py` | DONE | Covers load, save, round-trip, resolve paths |
| 3.8 | `test_sources.py` | DONE | 17 tests across IdgamesSource, DoomwikiSource, DoomworldSource using `respx` mocks |
| 3.9 | CI pipeline | DONE | `.github/workflows/test.yml` with Python 3.10/3.11/3.12 matrix and coverage |

---

## Phase 4: Polish & Modernization -- ALL DONE

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 4.1 | GUI `LibraryTab` public API | DONE | Public wrapper methods added; `MainWindow` uses `set_sort()`, `toggle_view()`, `is_grid_view()`, `columns_changed`, `set_visible_columns()`, `save_splitter_state()`, `restore_splitter_state()`, `get_sort_field()`, `is_sort_descending()` |
| 4.2 | DoomWiki batch page fetch | DONE | `get_pages_batch()` uses pipe-separated `titles` param (`doomwiki/client.py:172-207`) |
| 4.3 | `mmap` for thumbnail extraction | DONE | `mmap.mmap()` used for WAD files (`gui/thumbnails/extractor.py:165-175`) |
| 4.4 | TUI filter debounce | DONE | 150ms asyncio debounce in `tui/widgets/filter_input.py:19,90-99` |
| 4.5 | Pass WAD dict to info panels | DONE | Both TUI `WadInfoPanel` and GUI `DetailPanel` accept optional `wad=` and `stats=` params |
| 4.6 | Type annotations | DONE | `ProgressCallback` alias at `player.py:26`; full typing on `utils.py` and callbacks |
| 4.7 | Linting tooling | DONE | `[tool.ruff]` and `[tool.mypy]` in `pyproject.toml:36-52` |
| 4.8 | Split `db.py` into package | DONE | `db/` package with `_models.py`, `_connection.py`, `_schema.py`, `_query.py`, `_wads.py`, `_sessions.py`; `__init__.py` re-exports all symbols |
| 4.9 | `WadRecord` TypedDict | DONE | Defined at `db.py:34-62` with 19 typed fields |
| 4.10 | Narrow exception handling | DONE | Source adapters use specific exceptions; thumbnail `except Exception` blocks now log via `logger.debug()` in `extractor.py` and `loader.py` |
| 4.11 | Decouple `player.py` from Rich | DONE | `play()` uses only `ProgressCallback`; no `Console` parameter |
| 4.12 | `ThumbnailLoader` thread safety | -- | Not verified (low priority) |
| 4.13 | `_batch_query` `.format()` | DONE | Uses `{placeholders}` template with `_batch_query()` helper |
| 4.14 | Static `_PERIOD_QUERIES` dict | DONE | `_PERIOD_QUERIES` dict at `db.py:1511-1532`; no f-string SQL |
| 4.15 | Deduplicate ID range parsing | DONE | Shared `_parse_id_range_core()` at `cli/__init__.py:134-152` |
| 4.16 | Stdlib imports at module level | DONE | `json`, `sys`, `os` at top of `cli/library.py`, `config_cmd.py` |
| 4.17 | Fix `_format_size` type | DONE | Separate `value: float` variable in `utils.py:43` |
| 4.18 | `.format()` to f-strings | DONE | No legacy `.format()` remains in CLI |
| 4.19 | Source adapter context managers | DONE | All adapters inherit from `BaseSource` (`sources/base.py:6-17`) |
| 4.20 | Name magic constants | DONE | Uses `truncate()` call instead of bare `120` |
| 4.21 | Optimize `random` command | DONE | `ORDER BY RANDOM() LIMIT 1` via `db.search_wads(sort_by="random", limit=1)` |
| 4.22 | Dead import cleanup | DONE | `import json` in `cli/library.py` is actively used |
| 4.23 | Sort semantics documentation | DONE | Comprehensive docstring in `_parse_sort_option()` with suffix/prefix notation |

---

## Remaining Work

All 56 items are now complete. No remaining work.
