# Performance Review: Caco

## Executive Summary

Caco has several good performance patterns already in place (batch queries, debounced GUI filter, async thumbnail loading). However, there are meaningful bottlenecks that compound at scale: repeated config file reads on every DB connection, four separate batch queries that could be one, missing SQLite pragmas, wiki scraping with N sequential HTTP round-trips, full WAD file reads for thumbnail extraction, and `update_row` opening four DB connections per row update.

---

## Findings

### Finding 1 -- Config File Read on Every DB Connection
**Impact: High** | **Effort: Low**

`get_connection()` -> `get_db_path()` -> `load_config()` -> disk I/O on every DB call. A typical list refresh triggers 5+ config file reads. `player.py` triggers 8 config reads in a single `play()` call.

**Fix:** Cache config with module-level singleton, invalidate on `save_config()`.

### Finding 2 -- No WAL Mode, No Connection Reuse, Missing PRAGMAs
**Impact: High** | **Effort: Low**

Default journal mode (DELETE) requires full fsync per write. No `PRAGMA synchronous = NORMAL`, no `PRAGMA cache_size`, no `PRAGMA temp_store = MEMORY`.

**Fix:** Add WAL + performance PRAGMAs. Use thread-local connection reuse.

### Finding 3 -- Four Separate Batch Queries Where One Would Do
**Impact: High** | **Effort: Low**

TUI and GUI both call four separate batch functions per list refresh, each opening its own connection and scanning the sessions table separately.

**Fix:** Add `get_wad_stats_batch()` that fetches all four aggregates in two queries on one connection.

### Finding 4 -- `update_row()` Opens Four Connections for One Row
**Impact: High** | **Effort: Low**

`update_row()` calls four `_batch_query` functions with single-element lists, paying full connection overhead four times.

**Fix:** Use proposed `get_wad_stats_batch([wad_id])`.

### Finding 5 -- `get_wad_stats()` Makes Two Queries for Same WAD
**Impact: Medium** | **Effort: Low**

Two separate queries (COUNT and SUM) against sessions for the same `wad_id`.

**Fix:** Combine into one `SELECT COUNT(*), COALESCE(SUM(duration_seconds), 0)`.

### Finding 6 -- Doom Wiki `search_wads()` Makes N+1 HTTP Requests
**Impact: High** | **Effort: Medium**

Fetches search results, then iterates each result making a separate synchronous HTTP request (21 round-trips for limit=20).

**Fix:** Use MediaWiki API's pipe-separated `titles` parameter for batch page fetch.

### Finding 7 -- Thumbnail Scraper Creates New httpx.Client per WAD
**Impact: Medium** | **Effort: Low**

No connection reuse across thumbnail workers. In grid view with 100+ WADs, dozens of concurrent clients.

**Fix:** Use module-level shared client (httpx is thread-safe).

### Finding 8 -- Info Panel Re-fetches WAD Already in Memory
**Impact: Medium** | **Effort: Low**

Both TUI `WadInfoPanel.update_wad()` and GUI `detail_panel` call `db.get_wad(wad_id)` even though the WAD dict is already in the table model.

**Fix:** Pass the wad dict directly alongside stats.

### Finding 9 -- Thumbnail Extractor Loads Entire WAD into RAM
**Impact: Medium** | **Effort: Medium**

`path.read_bytes()` loads entire WAD file. Megawads can be hundreds of MB.

**Fix:** Use `mmap` to read only the WAD directory and TITLEPIC lump offset.

### Finding 10 -- Missing Indexes
**Impact: Medium** | **Effort: Low**

No index on `wads.deleted_at`, `wads.cached_path`, `wads.source_url`. Full table scans for common filters.

**Fix:** Add `CREATE INDEX IF NOT EXISTS` for `deleted_at`, `cached_path`, `source_url`, `(source_type, source_id)`, `(wad_id, started_at DESC)`.

### Finding 11 -- `auto_clean_cache()` N+1 Last-Played Queries
**Impact: Medium** | **Effort: Low**

Calls `db.get_last_played(wad["id"])` in a loop. Batch function already available.

### Finding 12 -- TUI Filter Fires DB Query on Every Keystroke
**Impact: Medium** | **Effort: Low**

GUI has 300ms debounce; TUI fires `QueryChanged` synchronously on each keypress.

**Fix:** Add `set_timer(0.3, ...)` debounce to TUI `FilterInput`.

### Finding 13 -- `get_wad_by_id()` in GUI Model is O(N) Linear Scan
**Impact: Low** | **Effort: Low**

TUI already has `_wad_id_to_row` dict for O(1). GUI is missing it.

### Finding 14 -- `set_wad_completion_count()` Loop INSERT
**Impact: Low** | **Effort: Low**

**Fix:** Use `executemany()`.

### Finding 15 -- 8 KB Download Chunk Size
**Impact: Low** | **Effort: Low**

12,800 iterations for 100 MB file. **Fix:** Use 256 KB chunks.

---

## Quick Wins (1-2 hours each)

| # | Finding | Change |
|---|---------|--------|
| 1 | Config caching | `_config_cache` global + clear on save |
| 2 | WAL + PRAGMAs | 4 PRAGMA lines in `get_connection()` |
| 5 | Single-query stats | Combine two SELECTs into one |
| 10 | Missing indexes | Add CREATE INDEX statements |
| 11 | Batch cache cleanup | Move `get_last_played_batch()` outside loop |
| 15 | Chunk size | Change `8192` to `262144` |
| 13 | O(1) wad lookup | Add `_wad_index` dict to GUI model |
| 14 | executemany | Replace loop with `executemany` |

## Architectural Changes (half-day to full day)

| # | Finding | Change |
|---|---------|--------|
| 3+4 | Unified batch stats | New `get_wad_stats_batch()` function |
| 6 | DoomWiki batch fetch | Rework to use pipe-separated API |
| 8 | Pass WAD to panel | Add `wad` param to both TUI/GUI panels |
| 9 | mmap for thumbnails | Replace `read_bytes()` with `mmap` |
| 12 | TUI filter debounce | Add `set_timer` debounce |
