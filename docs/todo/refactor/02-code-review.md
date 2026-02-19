# Code Review Report: Caco

## Executive Summary

Caco demonstrates good practices in many areas: parameterized SQL queries, context managers for resource cleanup, proper error hierarchy, and batch queries to avoid N+1 problems. However, the review identified 25 findings across security, error handling, resource management, type safety, and code quality. The most important findings relate to: (1) a potential SQL injection vector in `update_wad`, (2) unchecked subprocess execution in `player.py`, (3) excessive database connection churn, and (4) missing thread safety guarantees in the GUI thumbnail loader.

---

## Critical

### C-1: SQL Injection via Dynamic Column Names in `update_wad`
**File:** `db.py:705`

The `update_wad` function constructs SQL column names directly from `**fields` dictionary keys without validation. While values are parameterized, column names are injected directly.

**Fix:** Add `ALLOWED_FIELDS` frozenset and validate keys before SQL construction.

---

## High

### H-1: Subprocess Execution Without Argument Validation in `player.py`
**File:** `player.py:220-253`

The `play` function builds a command list from user-controlled sources (custom_sourceport, custom_args, extra_args) passed to `subprocess.run(cmd)`. No validation that `resolve_sourceport()` returns a path to an actual binary.

**Fix:** Validate sourceport exists via `shutil.which()` before execution; check return codes.

### H-2: Subprocess Execution with Editor Path in `config_cmd.py`
**File:** `config_cmd.py:71`

Editor from `$EDITOR` env var is not validated.

**Fix:** Check `shutil.which(editor)` before running.

### H-3: `update_wad` Uses Separate Connection for Completion Recording
**File:** `db.py:707-717`

`update_wad` opens one connection for the UPDATE, closes it, then calls `add_wad_completion` which opens a new connection. Breaks atomicity.

**Fix:** Record completion within the same transaction.

### H-4: `_batch_query` Uses `.format()` for SQL Construction
**File:** `db.py:840-862`

While the `placeholders` string is safe (only `?` chars), using `.format()` on SQL strings is an anti-pattern.

**Fix:** Use a function that builds the query instead of format string substitution.

---

## Medium

### M-1: Excessive Database Connection Churn
Every database function re-reads config.toml and opens a new connection.

### M-2: `save_config` Loses Nested TOML Sections
**File:** `config.py:57-73`

Nested sections like `[tui]`, `[gui]`, `[list]` are silently dropped.

### M-3: Thread Safety of `ThumbnailLoader._pending` Set
**File:** `gui/thumbnails/loader.py:116-134`

No lock protection on `_pending` set accessed from multiple threads.

### M-4: Silently Swallowed Exceptions in Multiple Places
Broad `except Exception` with no logging in: thumbnail loader, scraper, sources/doomworld, cli completions.

### M-5: `get_wads_played_by_period` Uses f-string for strftime Format in SQL
**File:** `db.py:1297-1316`

### M-6: `WadIdRange` and `_parse_id_range` Duplicate Logic
**File:** `cli/__init__.py:132-178`

### M-7: `auto_clean_cache` Calls `get_last_played` Per-WAD Instead of Batch
**File:** `player.py:108`

N+1 query pattern; `get_last_played_batch()` already available.

### M-8: `progress_callback` Typed as `object` Instead of `Callable`
**File:** `player.py:26-27`

### M-9: Binary WAD Parsing Has No Size Limits
**File:** `gui/thumbnails/extractor.py:161`

Reads entire WAD file into memory. WAD files can be hundreds of MB.

### M-10: ZIP Bomb Potential in `extract_titlepic`
**File:** `gui/thumbnails/extractor.py:150-157`

No check on uncompressed size of ZIP entries.

---

## Low

### L-1: Missing Type Hints on `coerce_str`
### L-2: `_parse_sort_option` Has Inconsistent Direction Semantics
### L-3: `QUERY_STATUS_VALUES` Missing `awaiting-update`
**File:** `cli/__init__.py:381` - tab completion won't suggest `awaiting-update`.

### L-4: `_fetch_tags_batch` Could Fail for Very Large WAD Lists
SQLite `SQLITE_MAX_VARIABLE_NUMBER` limit of 999. Same issue affects all batch functions.

**Fix:** Chunk queries at 900 items.

### L-5: Grayscale Fallback Palette is Incorrect
**File:** `gui/thumbnails/extractor.py:190-192`

### L-6: `random_cmd` Fetches All WADs Just to Pick One
### L-7: Dead Import in `library.py` Info Command
### L-8: No Test Coverage for CLI, Source Adapters, GUI, or TUI
### L-9: `_glob_to_like` Escape Character Not Matched in Non-Glob Tag Query
**File:** `db.py:522-531`

Non-glob tag LIKE query has no ESCAPE clause.

### L-10: `_check_and_import_entry` Has No Type Hints

---

## Summary Statistics

| Severity | Count |
|----------|-------|
| Critical | 1     |
| High     | 4     |
| Medium   | 10    |
| Low      | 10    |
| **Total**| **25**|

### By Category

| Category | Count |
|----------|-------|
| Security | 5 |
| Error Handling | 2 |
| Resource Management / Performance | 4 |
| Data Integrity | 2 |
| Thread Safety | 1 |
| Type Safety | 3 |
| Code Quality / DRY | 3 |
| Correctness | 3 |
| Test Coverage | 1 |
| Robustness | 2 |
