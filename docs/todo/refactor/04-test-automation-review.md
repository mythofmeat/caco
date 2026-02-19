# Test Automation Infrastructure Report: Caco

## Executive Summary

The Caco project has a functional but narrow test suite. All 94 tests pass in under 0.4 seconds, which is a strong foundation. However, the suite covers only **8% of total source lines** and is entirely absent for the CLI, TUI, GUI, parser, player, and source-adapter layers. The fixture design is sound but incomplete, and there is no pytest configuration section, no CI pipeline, and no httpx mocking infrastructure.

---

## Current State Assessment

### Pytest Configuration
**Status: Minimal.** No `[tool.pytest.ini_options]` in `pyproject.toml`. No testpaths, addopts, coverage threshold, custom markers, or minimum version pinning.

### Fixture Design
**Status: Good for what exists, incomplete.**
- `tmp_db` - patches `get_db_path` to temp file, calls `init_db()`. Correct approach.
- `db_mod` - returns the `caco.db` module after setup.
- **Missing:** Pre-populated DB fixture, config isolation fixture, httpx transport fixture, WAD factory fixture.

### Test Isolation
**Status: Good for existing tests, risky at the boundary.** `player.py` calls `subprocess.run()` with no injection point. `config.py` reads from `~/.config/caco/config.toml` (real home dir).

### CLI Testing
**Status: Absent.** Click's `CliRunner` not used. Entire `cli/` package at 0% coverage.

### TUI Testing
**Status: Absent.** Textual's `Pilot` test harness not used.

### GUI Testing
**Status: Absent.** No `pytest-qt` integration.

### Mock/Stub Patterns
**Status: Absent.** No httpx transport mocking despite three HTTP clients.

---

## Recommended Infrastructure Additions

### Updated `pyproject.toml`

```toml
[project.optional-dependencies]
test = [
    "pytest>=7.0",
    "pytest-cov>=4.0",
    "pytest-mock>=3.12",
    "respx>=0.21",
]

[tool.pytest.ini_options]
testpaths = ["tests"]
addopts = [
    "--tb=short", "-ra", "--strict-markers",
    "--cov=src/caco", "--cov-report=term-missing",
    "--cov-fail-under=60",
]
markers = [
    "unit: pure unit tests with no I/O",
    "integration: tests that require network or filesystem",
    "cli: tests for CLI commands via CliRunner",
    "slow: tests that take more than 1 second",
]
```

### Key Fixtures to Add

- `make_wad` - Factory fixture for creating test WADs with sensible defaults
- `populated_db` - Pre-populated library with WADs in various states + sessions
- `tmp_config` - Isolates config reads/writes to temp directory
- `mock_idgames_transport` / `mock_doomwiki_transport` - respx mocks for HTTP
- `cli_runner` / `invoke_cli` - Click CliRunner with isolated test database

### Test Modules to Create

1. `tests/unit/test_db_sessions.py` - Session lifecycle, batch stats
2. `tests/unit/test_doomwiki_parser.py` - Pure parsing tests
3. `tests/unit/test_doomworld_parser.py` - Pure parsing tests
4. `tests/unit/test_cli_library.py` - list, info, update, delete commands
5. `tests/unit/test_idgames_client.py` - HTTP mock tests
6. `tests/unit/test_config.py` - Config load/save/resolve tests
7. `.github/workflows/test.yml` - CI pipeline

---

## Implementation Order

1. Add `[tool.pytest.ini_options]` + new test dependencies
2. Extend `conftest.py` with `make_wad`, `populated_db`, `tmp_config`, `invoke_cli`
3. Create DB session/stats tests
4. Create parser tests (zero-dependency, high-value)
5. Create CLI integration tests with `invoke_cli`
6. Create idgames client tests with respx
7. Create config tests
8. Create GitHub Actions workflow
9. Add coverage threshold enforcement

---

## Target Scorecard

| Dimension | Current | Target |
|---|---|---|
| Total tests | 94 | 300+ |
| Overall coverage | 8% | 60%+ |
| DB layer coverage | 69% | 90%+ |
| CLI coverage | 0% | 75%+ |
| Parser coverage | 12-18% | 85%+ |
| HTTP mocking | None | respx |
| CLI testing | None | CliRunner |
| CI pipeline | None | GitHub Actions |
| Execution time | 0.28s | Under 60s |
