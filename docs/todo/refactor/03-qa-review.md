# QA Review Report: Caco

## Executive Summary

The test suite is in a critically under-covered state. 94 tests pass and all are green, but they cover only **8.2% of the codebase (688 of 8,350 statements)**. The tests that do exist are well-written and test meaningful behavior, but they address only a narrow slice: the query parser, simple DB CRUD, Pydantic model validators, and the `format_duration` utility. Every major execution path -- the CLI, source adapters, `player.py`, `config.py`, the parsers, and all TUI/GUI code -- has zero test coverage.

**Test health rating: 2 / 10**

---

## Coverage Analysis

### What Is Tested (8.2% total)

| Module | Coverage | What is covered |
|---|---|---|
| `db.py` | 69% | add/get/update/delete wad, tags, basic search, completions |
| `idgames/models.py` | 95% | Pydantic validators |
| `doomwiki/models.py` | 100% | Pydantic validators |
| `doomworld/models.py` | 100% | Pydantic validators |
| `utils.py` | 71% | `coerce_str`, `extract_year` |
| `player.py` | 13% | `format_duration` only |

### What Is NOT Tested (zero coverage)

- `cli/` (all 8 modules, ~1,750 lines) - the entire CLI
- `config.py` (77% uncovered) - no write path, no `resolve_iwad`, no cache config
- `player.py` (87% uncovered) - all of `play()`, `get_wad_path()`, `auto_clean_cache()`
- `sources/` (all 3 adapters) - 0% coverage
- `doomwiki/parser.py` - 12% (complex regex/parsing logic)
- `doomworld/parser.py` - 18%
- `idgames/client.py` - 19%
- All TUI code - 0%
- All GUI code - 0%

### Database Coverage Gaps (db.py - 69% covered, 31% missing)

Missing tests for: `start_session()`/`end_session()`, all batch query functions, `find_duplicate()`, `get_wad_stats()`, cache management, `get_library_stats()`, `get_wads_played_by_period()`, `get_completion_rate()`, all migration functions, sort options, OR queries.

---

## Highest-Risk, Lowest-Coverage Areas

### Priority 1 - CLI Integration (Risk: CRITICAL, Coverage: 0%)
The CLI is the primary user interface. Zero tests verifying `caco list`, `caco update`, `caco delete`, or `caco import` work.

### Priority 2 - Source Adapters (Risk: HIGH, Coverage: 0%)
The import workflow involves HTTP, JSON parsing, duplicate detection, and DB writes. Untested.

### Priority 3 - Parsers (Risk: HIGH, Coverage: 12-18%)
`WikitextParser` and `DoomworldParser` contain significant regex logic with no meaningful coverage.

### Priority 4 - Database Batch Functions (Risk: HIGH, Coverage: 0%)
All batch stat functions used by TUI/GUI are completely untested.

### Priority 5 - Config and IWAD Resolution (Risk: MEDIUM, Coverage: 23%)
`resolve_iwad()` and `resolve_sourceport()` are critical for `caco play`.

### Priority 6 - Player Logic (Risk: MEDIUM, Coverage: 13%)
`get_wad_path()`, `play()`, `auto_clean_cache()` completely untested.

---

## Test Quality Assessment

### Strengths
- Query parser tests are exemplary (parameterized, edge cases covered)
- DB tests correctly use isolated temp database via fixture patching
- Model tests confirm coercion validators protect against `None`

### Weaknesses
- **Resource leak:** Dozens of `ResourceWarning: unclosed database` warnings during test runs
- **No behavior assertions:** e.g., `test_update_status` doesn't test auto-completion side-effect
- **No negative/error path tests:** No tests for invalid inputs, not-found cases
- **Player tests cover only formatting:** `play()` and `get_wad_path()` untested

---

## Infrastructure Problems

1. **Unclosed database connections** - `get_connection()` creates new connection per call, never explicitly closed
2. **Missing `[tool.pytest.ini_options]`** - No pytest configuration at all
3. **Missing `pytest-mock`/`respx` dependency** - No HTTP mocking infrastructure
4. **No CI configuration** - No `.github/workflows/` or equivalent

---

## Recommended Prioritized Actions

1. Fix fixture connection leak + add `filterwarnings = ["error::ResourceWarning"]` (1h)
2. Add `pytest-mock` and `respx` to test dependencies (30min)
3. Add CLI integration tests with `CliRunner` (4h)
4. Add missing DB tests (sessions, batch, find_duplicate, stats) (2h)
5. Add parser tests (WikitextParser, DoomworldParser) (2h)
6. Add config tests (resolve_iwad, load/save round-trip) (1.5h)
7. Add player unit tests with mocked subprocess (2h)
8. Add source adapter mock tests with respx (3h)
