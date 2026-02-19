# Unified Refactoring Plan: Caco

> Synthesized from 7 independent reviews: Architecture, Code Quality, QA, Test Automation, Refactoring Analysis, Python Quality, and Performance.

---

## Overview

Caco is a well-structured project with strong foundations (beets-style query parser, batch stat queries, source adapter pattern, three-UI architecture). The reviews collectively identified **~99 findings** across the seven domains. After deduplication and cross-referencing, these distill into **5 strategic themes** and **57 concrete action items** organized into 4 phases. Every finding from all 7 reports is accounted for below.

### The 5 Strategic Themes

1. **Database & Config Hot Path** -- Config is re-read from disk on every DB call; connections are never reused; SQLite lacks performance PRAGMAs. This is the lowest-effort, highest-impact fix category.

2. **Shared Logic Extraction** -- Display formatting, batch stats, import duplicate-checking, and stats assembly are duplicated across CLI/TUI/GUI. A thin service + utility layer eliminates this.

3. **Security Hardening** -- SQL column name injection in `update_wad`, unvalidated subprocess execution, `save_config` silently destroying nested sections, broad exception swallowing.

4. **Test Infrastructure** -- 8% coverage with no CLI, parser, config, or adapter tests. Missing pytest config, no mocking infrastructure, no CI pipeline.

5. **Python Modernization** -- Missing type annotations, untyped callbacks, mutable Pydantic defaults, hand-rolled TOML serializer.

---

## Phase 1: Critical Fixes & Quick Wins (1-2 days)

These items are independent, low-risk, and high-value. They can all be done in parallel.

### 1.1 Security: `update_wad` Field Whitelist
**Sources:** Architecture #4, Code Review C-1, Python Quality #7
**File:** `src/caco/db.py`
**Effort:** 15 min

Add `ALLOWED_UPDATE_FIELDS` frozenset. Validate `**fields` keys before SQL construction. Also build a clean copy instead of mutating `**fields` in-place.

```python
ALLOWED_UPDATE_FIELDS = frozenset({
    "title", "author", "year", "description", "status", "rating", "notes",
    "source_url", "filename", "cached_path", "custom_iwad",
    "custom_sourceport", "custom_args", "version", "idgames_id", "deleted_at",
})
```

### 1.2 Performance: Cache `load_config()`
**Sources:** Architecture #5, Performance #1, Refactoring RF-06
**File:** `src/caco/config.py`
**Effort:** 15 min

Apply `@functools.lru_cache(maxsize=1)` on `load_config()`. Invalidate in `save_config()`. Eliminates 5-18 redundant TOML reads per operation.

### 1.3 Performance: SQLite WAL Mode + PRAGMAs
**Sources:** Performance #2
**File:** `src/caco/db.py`
**Effort:** 15 min

Add to `get_connection()`:
```python
conn.execute("PRAGMA journal_mode = WAL")
conn.execute("PRAGMA synchronous = NORMAL")
conn.execute("PRAGMA cache_size = -20000")  # 20 MB
conn.execute("PRAGMA temp_store = MEMORY")
```

### 1.4 Performance: Add Missing Indexes
**Sources:** Performance #10
**File:** `src/caco/db.py`
**Effort:** 15 min

Add to SCHEMA:
```sql
CREATE INDEX IF NOT EXISTS idx_wads_deleted_at ON wads(deleted_at);
CREATE INDEX IF NOT EXISTS idx_wads_cached_path ON wads(cached_path);
CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(wad_id, started_at DESC);
```

### 1.5 Correctness: Fix `save_config` Dropping Nested Sections
**Sources:** Architecture #6, Code Review M-2, Python Quality #9
**File:** `src/caco/config.py`
**Effort:** 1-2 hours

Add dict handling to the serializer, or switch to `tomlkit` for round-trip writing.

### 1.6 Security: Validate Sourceport Before Execution
**Sources:** Code Review H-1, H-2
**File:** `src/caco/player.py`, `src/caco/cli/config_cmd.py`
**Effort:** 30 min

Validate `shutil.which(port)` before `subprocess.run()`. Check editor exists before launching.

### 1.7 Utility: Extract Shared Formatting Functions
**Sources:** Architecture #7, Refactoring RF-04, RF-05, RF-11, RF-15, RF-18
**File:** `src/caco/utils.py` + 15 consumer files
**Effort:** 2 hours

Add to `utils.py`:
- `format_rating(rating: int | None, max: int = 5) -> str` (replaces 7 duplicates)
- `format_author_year(author: str | None, year: int | None) -> str` (replaces 6 duplicates)
- `truncate(text: str, max_len: int, suffix: str = "...") -> str` (replaces 4 duplicates, fixes 2 bugs)
- `format_size(size_bytes: int) -> str` (moved from `cli/cache.py`)

### 1.8 Correctness: Fix Missing Status in Completions
**Sources:** Code Review L-3
**File:** `src/caco/cli/__init__.py`
**Effort:** 5 min

Add `"awaiting-update"` to `QUERY_STATUS_VALUES`.

### 1.9 Quality: Fix Pydantic Mutable Default
**Sources:** Python Quality #2
**File:** `src/caco/doomworld/models.py`
**Effort:** 5 min

Change `download_links: list[str] = []` to `download_links: list[str] = Field(default_factory=list)`.

### 1.10 Config: Extract Section Merge Helper
**Sources:** Refactoring RF-07
**File:** `src/caco/config.py`
**Effort:** 20 min

Replace three copy-paste merge patterns with `_merge_section_config()`.

### 1.11 Correctness: Fix Tag Query ESCAPE Clause
**Sources:** Code Review L-9
**File:** `src/caco/db.py`
**Effort:** 10 min

Non-glob tag LIKE query at line 530 has no `ESCAPE '\\'` clause, so `%` and `_` in tag names are interpreted as SQL wildcards. The glob path (line 526) correctly uses ESCAPE but the non-glob path does not.

### 1.12 Correctness: Fix Grayscale Fallback Palette
**Sources:** Code Review L-5
**File:** `src/caco/gui/thumbnails/extractor.py`
**Effort:** 5 min

`palette = bytes(range(256)) * 3` produces incorrect RGB mapping. Fix: `bytes(val for val in range(256) for _ in range(3))`.

### 1.13 Performance: GUI `get_wad_by_id()` O(1) Index
**Sources:** Performance #13
**File:** `src/caco/gui/models/wad_model.py`
**Effort:** 10 min

Add `_wad_index: dict[int, int]` mapping wad_id to row index, matching the pattern TUI already uses (`_wad_id_to_row`).

### 1.14 Performance: `executemany` for Completions
**Sources:** Performance #14
**File:** `src/caco/db.py`
**Effort:** 5 min

Replace loop INSERT in `set_wad_completion_count()` with `conn.executemany()`.

### 1.15 Performance: Increase Download Chunk Size
**Sources:** Performance #15
**File:** `src/caco/idgames/client.py`
**Effort:** 1 min

Change `chunk_size=8192` to `chunk_size=262144` (256 KB). Reduces Python callback overhead for large downloads.

### 1.16 Performance: Share httpx.Client in Thumbnail Scraper
**Sources:** Performance #7
**File:** `src/caco/gui/thumbnails/scraper.py`
**Effort:** 15 min

Use a module-level shared `httpx.Client` (thread-safe) instead of creating a new client per thumbnail worker.

---

## Phase 2: Database & Service Layer (3-5 days)

These items build on Phase 1 and address the core architectural issues.

### 2.1 Database: Unified `get_wad_stats_batch()`
**Sources:** Architecture #8, Performance #3, #4, #5, Refactoring RF-09
**File:** `src/caco/db.py` + `tui/widgets/wad_table.py` + `gui/models/wad_model.py`
**Effort:** 3 hours

Create single function that fetches playtime, last_played, session_count, times_beaten in 2 queries on 1 connection. Update TUI `WadTable`, GUI `WadTableModel`, and CLI renderers to use it.

### 2.2 Database: Atomic Completion Recording
**Sources:** Code Review H-3, Python Quality #14
**File:** `src/caco/db.py`
**Effort:** 30 min

Move `add_wad_completion` call inside the `update_wad` connection context so both execute in one transaction.

### 2.3 Database: Migration Version Tracking
**Sources:** Architecture #10, Refactoring RF-08
**File:** `src/caco/db.py`
**Effort:** 2 hours

Add `schema_migrations` table. Assign version numbers to existing 7 migrations. Only run migrations with version > current.

### 2.4 Database: Batch Query Chunking
**Sources:** Code Review L-4
**File:** `src/caco/db.py`
**Effort:** 1 hour

Chunk `IN (...)` queries at 900 items to stay under SQLite's `SQLITE_MAX_VARIABLE_NUMBER` limit.

### 2.4a Performance: Batch `auto_clean_cache()` Queries
**Sources:** Code Review M-7, Performance #11
**File:** `src/caco/player.py`
**Effort:** 15 min

Replace per-WAD `db.get_last_played(wad["id"])` loop with `db.get_last_played_batch(wad_ids)` before the loop. Classic N+1 fix; batch function already exists.

### 2.5 Status: Unified Status Metadata
**Sources:** Refactoring RF-01, RF-03, Python Quality #3
**Files:** `src/caco/db.py`, `src/caco/tui/theme.py`, `src/caco/gui/theme.py`, `src/caco/cli/__init__.py`
**Effort:** 2 hours

Single canonical `STATUS_METADATA` dict with display name + hex color. Both theme files import and convert. Single `_normalize_status` implementation.

### 2.6 Stats: StatsSnapshot Dataclass
**Sources:** Refactoring RF-10
**File:** `src/caco/db.py` + `cli/stats.py` + `tui/screens/stats.py` + `gui/dialogs/stats_dialog.py`
**Effort:** 1 hour

Single `get_stats_snapshot()` function returning a dataclass. Eliminates triplicated fetch logic.

### 2.7 Service: Import Service Layer
**Sources:** Architecture #1, Refactoring RF-02, RF-12, RF-13
**Files:** New `src/caco/services/import_service.py` + CLI/TUI/GUI import panes
**Effort:** 1-2 days

```python
@dataclass
class ImportResult:
    success: bool
    wad_id: int | None
    was_duplicate: bool
    message: str

class ImportService:
    def import_from_idgames(entry, tags=None, force=False) -> ImportResult
    def import_from_doomwiki(entry, tags=None, force=False) -> ImportResult
    def detect_and_import(source_str, ...) -> ImportResult
```

Eliminates 12 duplicate-check sites and unifies the import pipeline.

---

## Phase 3: Test Infrastructure (3-5 days)

### 3.1 Pytest Configuration
**Sources:** QA #Infrastructure, Test Automation #C1
**File:** `pyproject.toml`
**Effort:** 30 min

Add `[tool.pytest.ini_options]` with testpaths, addopts, markers, filterwarnings.

### 3.2 Test Dependencies
**Sources:** QA #Infrastructure, Test Automation #C3
**File:** `pyproject.toml`
**Effort:** 15 min

Add `pytest-mock>=3.12`, `respx>=0.21` to `[test]` extra.

### 3.3 Extended Fixtures
**Sources:** Test Automation #3.2
**File:** `tests/conftest.py`
**Effort:** 1 hour

Add: `make_wad` factory, `populated_db`, `tmp_config`, `cli_runner`/`invoke_cli`, `mock_*_transport`.

### 3.4 Database Session & Stats Tests
**Sources:** QA #Priority 4
**File:** `tests/unit/test_db_sessions.py` (new)
**Effort:** 2 hours

Cover: `start_session`, `end_session`, all batch functions, `get_library_stats`, `get_completion_rate`, `find_duplicate`.

### 3.5 Parser Tests
**Sources:** QA #Priority 3
**File:** `tests/unit/test_parsers.py` (new)
**Effort:** 2 hours

Cover: `WikitextParser.parse()`, `_extract_wad_template`, `_clean_value`, `extract_complevel`, `extract_iwad`, `extract_sourceport`, `extract_download_links`.

### 3.6 CLI Integration Tests
**Sources:** QA #Priority 1, Test Automation #C2
**Files:** `tests/unit/test_cli_library.py`, `test_cli_tags.py`, `test_cli_import.py` (new)
**Effort:** 4 hours

Cover: `list` (empty, filtered, JSON, plain), `info`, `update`, `delete`/`restore`, `tag add/remove`, sort options.

### 3.7 Config Tests
**Sources:** QA #Priority 5
**File:** `tests/unit/test_config.py` (new)
**Effort:** 1.5 hours

Cover: `resolve_iwad` (all paths), `load_config` (missing file, malformed TOML), `save_config`/`load_config` round-trip.

### 3.8 Source Adapter Mock Tests
**Sources:** QA #Priority 2
**File:** `tests/unit/test_sources.py` (new)
**Effort:** 3 hours

Cover: `IdgamesSource.import_wad` field mapping, `DoomwikiSource`, `_detect_source_type`.

### 3.9 CI Pipeline
**Sources:** Test Automation #M3
**File:** `.github/workflows/test.yml` (new)
**Effort:** 1 hour

Python 3.10/3.11/3.12 matrix. `pip install -e ".[test]"` + `pytest --cov`.

---

## Phase 4: Polish & Modernization (ongoing)

### 4.1 GUI: Expose Public API on LibraryTab
**Sources:** Architecture #9
**Effort:** 1-2 hours

### 4.2 Performance: DoomWiki Batch Page Fetch
**Sources:** Performance #6
**Effort:** Half day

Use MediaWiki API pipe-separated titles parameter.

### 4.3 Performance: mmap for Thumbnail Extraction
**Sources:** Performance #9, Code Review M-9, M-10
**Effort:** Half day

Replace `path.read_bytes()` with `mmap`. Add size limits for ZIP extraction.

### 4.4 Performance: TUI Filter Debounce
**Sources:** Performance #12
**Effort:** 30 min

Add `set_timer(0.3, ...)` debounce to `FilterInput`.

### 4.5 Performance: Pass WAD Dict to Info Panels
**Sources:** Performance #8
**Effort:** 1 hour

Both TUI and GUI info panels re-fetch WAD already in memory.

### 4.6 Quality: Type Annotations
**Sources:** Python Quality #1, #4, #11; Code Review M-8, L-1, L-10
**Effort:** 2-3 hours

Add full type annotations to `utils.py`, `player.py`, Click ParamType subclasses. Define `ProgressCallback` type alias (fixes `progress_callback: object`). Type `__enter__`/`__exit__` properly. Add hints to `coerce_str` and `_check_and_import_entry`.

### 4.7 Quality: Add Linting Tooling
**Sources:** Python Quality #Tooling
**Effort:** 1 hour

Add `[tool.ruff]` and `[tool.mypy]` config to `pyproject.toml`.

### 4.8 Architecture: Split db.py into Package
**Sources:** Architecture #3
**Effort:** 3-4 hours

Split into `db/` package with backward-compatible re-exports.

### 4.9 Architecture: WadRecord TypedDict
**Sources:** Architecture #14, Python Quality #Modernization
**Effort:** 1 day

Define `WadRecord` TypedDict. Update all consumers.

### 4.10 Quality: Narrow Exception Handling
**Sources:** Code Review M-4, Python Quality #5
**Effort:** 1 hour

Replace `except Exception: return None/pass` with specific exceptions + `logger.debug()`.

### 4.11 Architecture: Decouple player.py from Rich Console
**Sources:** Architecture #13
**File:** `src/caco/player.py`
**Effort:** 30 min

`play()` and `get_wad_path()` accept both a `Console` and a `progress_callback`, creating a dual progress-reporting interface. Use only the callback pattern; have the CLI wrap it with Rich.

### 4.12 Safety: ThumbnailLoader Thread Safety Assertion
**Sources:** Code Review M-3
**File:** `src/caco/gui/thumbnails/loader.py`
**Effort:** 10 min

`_pending` set accessed from main thread with no lock protection. Add `assert QThread.currentThread() == QApplication.instance().thread()` to `request()`, or add a `threading.Lock`.

### 4.13 Quality: Replace `.format()` in `_batch_query` SQL
**Sources:** Code Review H-4
**File:** `src/caco/db.py`
**Effort:** 15 min

`_batch_query` uses `.format(placeholders=...)` to inject `?` placeholders into SQL. While safe, using `.format()` on SQL strings is an anti-pattern. Refactor to use a callable that returns the query string.

### 4.14 Quality: Eliminate f-string SQL for `strftime`
**Sources:** Code Review M-5, Python Quality #10
**File:** `src/caco/db.py`
**Effort:** 15 min

`get_wads_played_by_period` embeds `fmt` variable via f-string in SQL. Use two separate static query strings keyed by `period` value.

### 4.15 Refactoring: Deduplicate WadIdRange / `_parse_id_range`
**Sources:** Code Review M-6, Refactoring RF-14
**File:** `src/caco/cli/__init__.py`
**Effort:** 30 min

Both implement the same `"3-6,9,11"` → `list[int]` parsing. Extract shared `_parse_id_range_core()` returning `list[int] | None`.

### 4.16 Quality: Move Stdlib Imports to Module Level
**Sources:** Python Quality #6
**Files:** `src/caco/cli/library.py`, `src/caco/config.py`, `src/caco/cli/config_cmd.py`
**Effort:** 15 min

Move `import json`, `import sys`, `import os` from inside function bodies to top-level. Keep only genuinely optional/circular-preventing deferred imports (source adapters, TUI, GUI).

### 4.17 Quality: Fix `_format_size` Type Reassignment
**Sources:** Python Quality #8
**File:** `src/caco/cli/cache.py`
**Effort:** 5 min

`size_bytes /= 1024` reassigns `int` param to `float`. Use a separate `value: float = float(size_bytes)` variable. (Moot if 1.7 moves this function to `utils.py`.)

### 4.18 Style: Normalize `.format()` to f-strings
**Sources:** Python Quality #13
**File:** `src/caco/cli/__init__.py`
**Effort:** 5 min

One cosmetic `.format()` at line 237 that should be an f-string. Leave `llm.py` and `db.py` usages as-is (intentional).

### 4.19 Refactoring: Consolidate Source Adapter Context Managers
**Sources:** Refactoring RF-16
**Files:** `src/caco/sources/idgames.py`, `doomwiki.py`, `doomworld.py`
**Effort:** 20 min

Three adapters define identical `__enter__`/`__exit__`. Extract into a `BaseSource` mixin or rely on `BaseHttpClient`'s implementation.

### 4.20 Refactoring: Name Magic Constants
**Sources:** Refactoring RF-17
**File:** `src/caco/tui/widgets/wad_info.py`
**Effort:** 5 min

Bare `120` for description snippet truncation → named constant `DESC_PREVIEW_LEN`. (Subsumes into `truncate()` call from 1.7.)

### 4.21 Performance: Optimize `random` Command
**Sources:** Code Review L-6
**File:** `src/caco/cli/library.py`
**Effort:** 15 min

`random_cmd` fetches all matching WADs to pick one. Add `limit` parameter to `search_wads()` or use `sort_by="random"` with `LIMIT 1`.

### 4.22 Quality: Remove Dead Import
**Sources:** Code Review L-7
**File:** `src/caco/cli/library.py`
**Effort:** 1 min

`import json` inside `info` command body is conditionally used. Minor -- no functional impact.

### 4.23 Quality: Document Sort Option Prefix Semantics
**Sources:** Code Review L-2
**File:** `src/caco/cli/__init__.py`
**Effort:** 15 min

Prefix `-` means ascending and `+` means descending (opposite of suffix notation). Document clearly in help text, or deprecate legacy prefix notation.

---

## Summary: Effort by Phase

| Phase | Items | Est. Effort | Impact |
|-------|-------|-------------|--------|
| Phase 1: Quick Wins | 17 | 2-3 days | Fixes security, perf, correctness bugs |
| Phase 2: DB & Service | 8 | 3-5 days | Eliminates architectural duplication |
| Phase 3: Test Infra | 9 | 3-5 days | Coverage from 8% to 60%+ |
| Phase 4: Polish | 23 | 7-10 days | Long-term maintainability |
| **Total** | **57** | **~4-5 weeks** | |

---

## Cross-Reference: Finding Sources

| Action Item | Arch | Code | QA | Test | Refactor | Python | Perf |
|---|---|---|---|---|---|---|---|
| 1.1 update_wad whitelist | #4 | C-1 | | | | #7 | |
| 1.2 Cache load_config | #5 | M-1 | | | RF-06 | | #1 |
| 1.3 WAL + PRAGMAs | | | | | | | #2 |
| 1.4 Missing indexes | | | | | | | #10 |
| 1.5 Fix save_config | #6 | M-2 | | | | #9 | |
| 1.6 Validate sourceport | | H-1,H-2 | | | | | |
| 1.7 Shared formatting | #7 | | | | RF-04,05,11,15,18 | | |
| 1.8 Missing status | | L-3 | | | | | |
| 1.9 Pydantic default | | | | | | #2 | |
| 1.10 Config merge helper | | | | | RF-07 | | |
| 1.11 Tag ESCAPE clause | | L-9 | | | | | |
| 1.12 Grayscale palette | | L-5 | | | | | |
| 1.13 GUI O(1) wad lookup | | | | | | | #13 |
| 1.14 executemany | | | | | | | #14 |
| 1.15 Chunk size | | | | | | | #15 |
| 1.16 Shared httpx client | | | | | | | #7 |
| 2.1 Unified batch stats | #8 | | | | RF-09 | | #3,4,5 |
| 2.2 Atomic completion | | H-3 | | | | #14 | |
| 2.3 Migration versioning | #10 | | | | RF-08 | | |
| 2.4a Batch cache cleanup | | M-7 | | | | | #11 |
| 2.5 Status metadata | | | | | RF-01,03 | #3 | |
| 2.7 Import service | #1,#11 | | | | RF-02,12,13 | | |
| 3.x Test infrastructure | #12 | L-8 | all | all | | #12 | |
| 4.6 Type annotations | | M-8,L-1,L-10 | | | | #1,4,11 | |
| 4.10 Narrow exceptions | | M-4 | | | | #5 | |
| 4.11 Rich decoupling | #13 | | | | | | |
| 4.12 Thread safety | | M-3 | | | | | |
| 4.13 _batch_query format | | H-4 | | | | | |
| 4.14 strftime SQL | | M-5 | | | | #10 | |
| 4.15 ID range dedup | | M-6 | | | RF-14 | | |
| 4.16 Stdlib imports | | | | | | #6 | |
| 4.17 _format_size type | | | | | | #8 | |
| 4.18 .format() normalization | | | | | | #13 | |
| 4.19 Adapter ctx managers | | | | | RF-16 | | |
| 4.20 Magic constants | | | | | RF-17 | | |
| 4.21 Optimize random cmd | | L-6 | | | | | |
| 4.22 Dead import | | L-7 | | | | | |
| 4.23 Sort semantics docs | | L-2 | | | | | |

---

## Individual Report Files

1. [01-architecture-review.md](./01-architecture-review.md) - Module coupling, layering, data flow
2. [02-code-review.md](./02-code-review.md) - Security, correctness, error handling (25 findings)
3. [03-qa-review.md](./03-qa-review.md) - Coverage analysis, highest-risk gaps
4. [04-test-automation-review.md](./04-test-automation-review.md) - Pytest config, fixtures, CI
5. [05-refactoring-plan.md](./05-refactoring-plan.md) - 18 refactoring opportunities with dependency graph
6. [06-python-quality-review.md](./06-python-quality-review.md) - Type safety, anti-patterns, modernization
7. [07-performance-review.md](./07-performance-review.md) - DB, HTTP, UI, memory optimizations (15 findings)
