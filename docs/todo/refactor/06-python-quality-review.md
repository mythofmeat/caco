# Python Quality Review: Caco

## Executive Summary

Caco is a well-structured Python project with idiomatic usage of modern Python (3.10+ union types, dataclasses, Pydantic v2, pathlib throughout). The main quality gaps are: missing type annotations on several public functions, a duplicated `_normalize_status` function, weak typing on callback parameters, `except Exception: pass`-style swallowing in adapters, deferred `import` statements for stdlib modules, a mutable-default-argument Pydantic antipattern, and a hand-written TOML serializer that drops nested sections.

---

## Findings

### Finding 1 -- Missing Type Annotations on Public Functions
**Severity: Medium**

`coerce_str` in `utils.py`, Click `ParamType.convert` methods in `cli/__init__.py`, `BaseHttpClient.__enter__/__exit__` lack proper type signatures.

**Fix:** Add full type annotations. Convert `LLMExtractedMetadata` from hand-rolled class to `@dataclass`.

### Finding 2 -- Mutable Default Argument in Pydantic Model
**Severity: High**

`ForumThread.download_links: list[str] = []` in `doomworld/models.py`. While Pydantic v2 handles this safely, it's non-idiomatic and confusing.

**Fix:** Use `Field(default_factory=list)`.

### Finding 3 -- Duplicated `_normalize_status` Function
**Severity: Medium**

Two implementations in `db.py` and `cli/__init__.py` with different semantics.

**Fix:** Rename to distinct names (`_normalize_status_query` vs `_normalize_status_input`) or unify.

### Finding 4 -- `progress_callback: object` Is Untyped
**Severity: Medium**

In `player.py` and `sources/idgames.py`, `object` type says nothing useful for a callable parameter.

**Fix:** Define `ProgressCallback = Callable[[int, int, str], None]`.

### Finding 5 -- Broad `except Exception: return None` in Source Adapters
**Severity: Medium**

`sources/doomworld.py` catches all exceptions and returns `None` without logging.

**Fix:** Narrow to `DoomworldError` or at minimum add `logger.debug()`.

### Finding 6 -- Deferred Imports for Stdlib Modules
**Severity: Low**

`import json` and `import sys` deferred inside function bodies in CLI and config modules. These have zero import overhead and should be top-level.

### Finding 7 -- `update_wad` Mutates `**fields` Dict In-Place
**Severity: Medium**

Mutating during iteration is technically safe but fragile. Adding `updated_at` key after enum conversion loop.

**Fix:** Build a clean `serialized` copy instead of mutating in place.

### Finding 8 -- `_format_size` Reassigns int Parameter as float
**Severity: Low**

`size_bytes /= 1024` changes the type from `int` to `float`. Strict type checkers will flag this.

**Fix:** Use a separate `value: float = float(size_bytes)` variable.

### Finding 9 -- Hand-Written TOML Serializer
**Severity: Medium**

`save_config` drops nested sections, doesn't escape quotes in strings, doesn't handle `None`.

**Fix:** Use `tomlkit` for round-trip writing, or targeted key-value replacement with regex.

### Finding 10 -- f-String SQL Interpolation for strftime Format
**Severity: Low**

Controlled but trains toward unsafe patterns. Use two separate static query strings instead.

### Finding 11 -- `__enter__`/`__exit__` Missing Protocol Types
**Severity: Low**

Six pairs of identical context manager boilerplate across the codebase.

### Finding 12 -- Test Coverage Gap
**Severity: High**

0-13% coverage on sources, player, config. 0% on CLI.

### Finding 13 -- `.format()` Mixed with f-strings
**Severity: Low**

One cosmetic `.format()` in `cli/__init__.py:237` that should be an f-string.

### Finding 14 -- Cross-Function Side Effect in `update_wad`
**Severity: Low**

`add_wad_completion(wad_id)` called after closing the update connection. Not in same transaction.

---

## Modernization Opportunities

- `match`/`case` for source type dispatch in `player.py`
- `TypedDict` for WAD dicts (medium effort, catches KeyErrors at type-check time)

## Tooling Recommendations

```toml
[tool.mypy]
python_version = "3.10"
warn_return_any = true
check_untyped_defs = true

[tool.ruff]
target-version = "py310"
line-length = 100

[tool.ruff.lint]
select = ["E", "W", "F", "I", "UP", "B", "C4", "SIM"]
```
