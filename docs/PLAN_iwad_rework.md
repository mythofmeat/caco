# Plan: IWAD Family+Variant+Priority Rework

## Context

The current IWAD registry (just committed) uses a flat `name UNIQUE` model — one entry per IWAD. But IWADs are actually a finite set of **families** (doom, doom2, plutonia, tnt) with multiple **variants** per family (v1.9, BFG, Enhanced, KEX). Users may own multiple variants of the same IWAD. The system needs to:
- Track multiple variants per family
- Resolve `resolve_iwad("doom2")` to the preferred variant via a configurable priority list
- Use freedoom as a cross-family fallback (freedoom2 for doom2/plutonia/tnt, freedoom1 for doom)
- Support ~22 known MD5s across 4 primary families (PC releases only, no console/pre-release)

Also: add "Playing IWADs directly" to the roadmap as a follow-up feature.

## Schema Change

Migration #9 restructures the `iwads` table:

```sql
-- Old (migration #8):
--   name TEXT NOT NULL UNIQUE

-- New (migration #9):
CREATE TABLE iwads (
    id INTEGER PRIMARY KEY,
    family TEXT NOT NULL,
    variant TEXT NOT NULL DEFAULT 'unknown',
    title TEXT,
    path TEXT NOT NULL,
    md5 TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(family, variant)
);
```

Migration migrates existing rows: `name` → `family`, variant detected from MD5 or defaults to `"unknown"`.

## Constants Rework in `_iwads.py`

### `KNOWN_IWADS`: `md5 → (family, variant, title)` (was 2-tuple, now 3-tuple)

**doom family** (4 variant tiers):
- v1.9: `1cd63c5ddff1bf8ce844237f580e9cf3`
- v1.9ud: `c4fe9fd920207691a9f493668e0a2083`
- bfg: `fb35c4a5a9fd49ec29ab6e900572c524`
- enhanced: `8517c4e8f0eef90b82852667d345eb86`
- kex: `4461d4511386518e784c647e3128e7bc`, `3b37188f6337f15718b617c16e6e7a9c`

**doom1 family** (shareware): `f0cefca49926d00903cf57551d901abe`

**doom2 family** (4 variant tiers):
- v1.9: `25e1459ca71d321525f84628f45ca8cd`
- bfg: `c3bea40570c23e511a7ed3ebcd9865f7`
- enhanced: `8ab6d0527a29efdc1ef200e5687b5cae`
- kex: `9aa3cbf65b961d0bdac98ec403b832e1`, `64a4c88a871da67492aaa2020a068cd8`

**plutonia family** (3 variant tiers):
- v1.9: `75c8cf89566741fa9d22447604053bd7`
- v1.9alt: `3493be7e1e2588bc9c8b31eab2587a04`
- unity: `0b381ff7bae93bde6496f9547463619d`, `ae76c20366ff685d3bb9fab11b148b84`
- kex: `24037397056e919961005e08611623f4`, `e47cf6d82a0ccedf8c1c16a284bb5937`

**tnt family** (3 variant tiers):
- v1.9: `4e158d9953c79ccf97bd0663244cc6b6`
- v1.9alt: `1d39e405bf6ee3df69a8d2646c8d5c49`
- unity: `a6685de59ddf2c07f45deeec95296d98`, `f5528f6fd55cf9629141d79eda169630`
- kex: `8974e3117ed4a1839c752d5e11ab1b7b`, `ad7885c17a6b9b79b09d7a7634dd7e2c`

**Other families** (heretic, hexen, strife, chex — keep existing MD5s, single variant each)

Remove old Doom II v1.666 MD5 `30e3c2d0350b67bfbf47271970b74b2f` — not a "real" supported version per user decision.

### `KNOWN_IWAD_FILENAMES`: `filename → (family, variant, title)` (now 3-tuple)

Filename detection uses `"unknown"` as variant since MD5 didn't match. No other structural change.

### `IWAD_ALIASES`: No change needed — already maps free text → family names.

### New: `DEFAULT_IWAD_PRIORITY`

```python
DEFAULT_IWAD_PRIORITY: dict[str, list[str]] = {
    "doom":   ["v1.9ud", "v1.9", "bfg", "enhanced", "kex"],
    "doom1":  ["v1.0"],
    "doom2":  ["v1.9", "bfg", "enhanced", "kex"],
    "plutonia": ["v1.9", "v1.9alt", "unity", "kex"],
    "tnt":    ["v1.9", "v1.9alt", "unity", "kex"],
    # single-variant families
    "freedoom1": ["latest"], "freedoom2": ["latest"],
    "heretic": ["v1.3"], "heretic1": ["v1.0"],
    "hexen": ["v1.1"], "hexdd": ["v1.0"],
    "strife": ["v1.2"], "chex": ["v1.0"], "chex3": ["v1.0"],
}
```

### New: `FAMILY_FALLBACKS`

Cross-family fallbacks (freedoom as last resort):
```python
FAMILY_FALLBACKS: dict[str, list[str]] = {
    "doom":     ["freedoom1"],
    "doom2":    ["freedoom2"],
    "plutonia": ["freedoom2"],
    "tnt":      ["freedoom2"],
}
```

### Config override: `[iwad_priority]`

```toml
[iwad_priority]
doom2 = ["freedoom2", "v1.9", "bfg", "enhanced", "kex"]
```

New function `get_iwad_priority(family)` checks config first, falls back to `DEFAULT_IWAD_PRIORITY`.

## DB Function Changes in `_iwads.py`

| Old | New |
|-----|-----|
| `add_iwad(name, path, ...)` | `add_iwad(family, variant, path, ...)` |
| `get_iwad(name) → dict` | `get_iwad(family) → dict` — returns **preferred** variant via priority walk + fallback |
| — | `get_iwad_variant(family, variant) → dict` — specific variant lookup |
| — | `get_family_iwads(family) → list[dict]` — all variants, priority-sorted |
| `get_all_iwads()` | Same, now returns `family`/`variant` columns |
| `get_iwad_by_path(path)` | Unchanged (SELECT * still works) |
| `remove_iwad(name) → bool` | `remove_iwad(family, variant=None) → int` — removes one or all variants |
| `resolve_iwad_from_db(name)` | Unchanged — delegates to `get_iwad(family)` which now does priority resolution |
| `identify_iwad(path) → (name, title)` | `identify_iwad(path) → (family, variant, title)` — 3-tuple |
| — | `get_iwad_priority(family) → list[str]` — config + defaults |

## CLI Changes (`cli/iwad_cmd.py`)

- **`iwad list`**: Columns become Family, Variant, Title, Path. Mark preferred variant with `*`.
- **`iwad add <path>`**: `--name` becomes `--family`. Add `--variant` override. Auto-detects both from MD5.
- **`iwad remove <family> [variant]`**: Without variant, removes all (with warning). With variant, removes one.
- **`iwad scan`**: Uses 3-tuple detection, checks `(family, variant)` for duplicates.

## Other File Changes

- **`config.py`**: No change needed — `resolve_iwad()` already delegates to `resolve_iwad_from_db()`.
- **`services/import_service.py`**: `_auto_link_iwad()` — no change needed. `normalize_iwad_name()` returns family names, `get_iwad(family)` handles the rest.
- **`db/__init__.py`**: Add re-exports for `get_iwad_variant`, `get_family_iwads`, `get_iwad_priority`, `DEFAULT_IWAD_PRIORITY`, `FAMILY_FALLBACKS`.
- **`completions/caco.fish`**: Update `--name` to `--family`, add `--variant`, update `remove` completions.
- **`tests/unit/test_db_sessions.py`**: Migration count 8 → 9.
- **`docs/ROADMAP_env_manager.md`**: Add "Playing IWADs directly" to Future Ideas.
- **`CLAUDE.md`**, **`README.md`**, **`docs/CHANGELOG.md`**: Update docs.

## Files to modify
1. `src/caco/db/_iwads.py` — rewrite constants + CRUD + add priority logic
2. `src/caco/db/_schema.py` — migration #9
3. `src/caco/db/__init__.py` — update re-exports
4. `src/caco/cli/iwad_cmd.py` — update all 4 commands
5. `completions/caco.fish` — update completions
6. `tests/unit/test_iwads.py` — rewrite all tests + add priority/fallback tests
7. `tests/unit/test_db_sessions.py` — migration count
8. `docs/ROADMAP_env_manager.md` — add playing IWADs to roadmap
9. `CLAUDE.md`, `README.md`, `docs/CHANGELOG.md`

## Verification
1. `caco iwad scan --dir ~/iwads` discovers IWADs and identifies family+variant
2. `caco iwad add ~/doom2.wad` auto-detects doom2/v1.9
3. `caco iwad add ~/doom2_bfg.wad` auto-detects doom2/bfg (same family, different variant)
4. `caco iwad list` shows both with `*` on v1.9 (preferred)
5. `resolve_iwad("doom2")` returns v1.9 path (highest priority)
6. Config `[iwad_priority] doom2 = ["bfg", "v1.9"]` makes BFG preferred
7. With only freedoom2 registered, `resolve_iwad("doom2")` falls back to freedoom2 path
8. `caco iwad remove doom2 bfg` removes just BFG variant
9. `caco iwad remove doom2` warns and removes all doom2 variants
10. `pytest tests/ -v` — all tests pass
