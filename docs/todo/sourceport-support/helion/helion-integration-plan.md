# Helion Integration Plan

## Overview

Integrate Helion as a supported sourceport in caco, including a new stats watcher
framework that captures per-map statistics during play sessions. The watcher is
designed to be reusable for other sourceports (Doom Retro, UZDoom, Nugget, etc.).

### How Helion Stats Work

- Helion writes `levelstat.txt` to `~/.config/Helion/levelstat.txt` (hardcoded path,
  overwritten each session)
- The format is identical to dsda-doom's levelstat format — already parsed by
  `wad_stats.py`
- `levelstat.txt` does **not** include difficulty/skill level
- Helion save files (`.hsg` ZIP archives containing `world.json`) do include `Skill`
- Save files also contain `VisitedMaps`, but this is unreliable (breaks on pistol start)

### Approach

1. Watch `levelstat.txt` for changes during play (file mtime polling)
2. After exit, read the most recent `.hsg` save to extract `Skill`
3. Enrich the levelstat data with the real skill value
4. Write to the WAD's data directory — existing pipeline picks it up

No daemon process, no game-side config changes, no .NET SDK dependency. Just file
watching in a thread + two file reads after exit.

---

## Phase 1: Helion Sourceport Family Registration

**Files:** `src/caco/sourceports.py`, `src/caco/complevel.py`

### sourceports.py

Add `helion` family to `SOURCEPORT_FAMILIES`:

```python
"helion": {
    "executables": ["Helion", "helion"],
    "save_arg": "-savedir",
    "complevel_arg": "+complevel",
},
```

Add `.hsg` to `SAVE_EXTENSIONS`:

```python
"helion": {".hsg"},
```

Helion uses `+complevel` with **string** values (`vanilla`, `boom`, `mbf`, `mbf21`)
instead of integer values. `get_complevel_args()` needs to handle this.

### complevel.py

Add reverse mapping for Helion's complevel names:

```python
HELION_COMPLEVEL_NAMES: dict[int, str] = {
    2: "vanilla",
    9: "boom",
    11: "mbf",
    21: "mbf21",
}

def complevel_to_helion_name(complevel: int) -> str | None:
    """Map a numeric complevel to Helion's +complevel string."""
    return HELION_COMPLEVEL_NAMES.get(complevel)
```

### Notes

- Helion uses `.ini` config files, not `.cfg`. No `-config` flag is documented, so
  `get_config_args()` returns `[]` for helion family initially.
- `uses_deh_flag()` already returns `True` for non-zdoom families — helion is correct
  by default.

---

## Phase 2: Stats Watcher Framework

**New file:** `src/caco/stats_watcher.py`

### Interface

```python
class StatsWatcher(ABC):
    """Base class for sourceport-specific stats watchers.

    Runs in a background thread during a play session, monitoring for
    stats changes and returning accumulated results on completion.
    """

    @abstractmethod
    def start(self) -> None:
        """Begin watching. Called from the watcher thread. Blocks until stop()."""

    @abstractmethod
    def stop(self) -> None:
        """Signal the watcher to stop. Called from main thread. Must be thread-safe."""

    @abstractmethod
    def collect(self) -> str | None:
        """After stop()+join(), return levelstat.txt-format string, or None."""
```

### Registry

```python
_WATCHER_FACTORIES: dict[str, Callable[[Path, ...], StatsWatcher]] = {}

def register_watcher(family: str, factory: Callable) -> None:
    """Register a watcher factory for a sourceport family."""

def get_watcher(executable: str, wad_data_dir: Path, **kwargs) -> StatsWatcher | None:
    """Look up and instantiate a watcher for the given sourceport.
    Returns None if no watcher is registered for this family."""

def run_watcher_thread(watcher: StatsWatcher) -> threading.Thread:
    """Start a watcher in a daemon thread, return the thread handle."""
```

### Design

- `get_watcher()` returns `None` for sourceport families without a registered
  watcher — existing passive stats reading continues unchanged
- The thread is a daemon thread so it doesn't block process exit
- No locks needed: watcher thread writes to internal state during `start()`, main
  thread reads via `collect()` only after `join()` completes
- Stop signal via `threading.Event`

---

## Phase 3: Helion Watcher Adapter

**New files:** `src/caco/watchers/__init__.py`, `src/caco/watchers/helion.py`

### HelionWatcher

```python
class HelionWatcher(StatsWatcher):
    def __init__(self, wad_data_dir: Path, helion_config_dir: Path | None = None):
        self._config_dir = helion_config_dir or _get_helion_config_dir()
        self._levelstat_path = self._config_dir / "levelstat.txt"
        self._stop_event = threading.Event()
        self._last_mtime: float = 0.0
        self._last_content: str = ""
        self._accumulated_maps: dict[str, MapStats] = {}
        self._poll_interval: float = 1.0

    def start(self) -> None:
        """Poll levelstat.txt mtime every ~1 second until stopped."""
        # Record initial mtime to avoid capturing stale data
        if self._levelstat_path.exists():
            self._last_mtime = self._levelstat_path.stat().st_mtime

        while not self._stop_event.wait(self._poll_interval):
            self._check_for_changes()

    def stop(self) -> None:
        self._stop_event.set()
        self._check_for_changes()  # Final read after sourceport exit
        self._enrich_skill()       # Grab Skill from most recent save

    def collect(self) -> str | None:
        if not self._accumulated_maps:
            return None
        return self._format_levelstat()
```

### Key Behaviors

- **Polling:** Checks `levelstat.txt` mtime every 1 second. On change, parses content,
  diffs against previous read, accumulates new map entries.
- **Diff logic:** Helion overwrites `levelstat.txt` with all maps completed this
  session. The watcher compares current parse against previous to detect new entries.
  Accumulates using the existing `merge_stats` logic from `wad_stats.py`.
- **Skill enrichment:** After `stop()`, finds most recently modified `.hsg` in the
  Helion config/save directory, extracts `Skill` from `world.json`, patches all
  accumulated `MapStats.best_skill` values. Silently skips if no save exists.
- **Output:** `collect()` returns a `levelstat.txt`-format string. The caller writes
  this to `{wad_data_dir}/levelstat.txt`.

### Helion Config Directory

```python
def _get_helion_config_dir() -> Path:
    """Platform-dependent Helion config directory."""
    # Linux: $XDG_CONFIG_HOME/Helion/ (default ~/.config/Helion/)
    # Windows: ~/Saved Games/Helion/
    # Check both config dir and binary dir as fallback
```

### Save File Reading

```python
def _read_helion_save_skill(save_path: Path) -> int | None:
    """Extract Skill from a Helion .hsg save archive."""
    # zipfile.ZipFile -> world.json -> Skill field

def _find_latest_save(save_dir: Path) -> Path | None:
    """Find most recently modified .hsg file."""
```

---

## Phase 4: Player Integration

**File:** `src/caco/player.py`

### Changes to `play()`

After `subprocess.Popen`, before `proc.wait()`:

```python
# Start stats watcher if available for this sourceport
watcher = None
watcher_thread = None
if get_manage_data_dirs() and get_auto_stats():
    from caco.stats_watcher import get_watcher, run_watcher_thread
    watcher = get_watcher(port, wad_data_dir)
    if watcher:
        watcher_thread = run_watcher_thread(watcher)
```

After `proc.wait()`, in the cleanup:

```python
# Stop watcher and write collected stats
if watcher and watcher_thread:
    watcher.stop()
    watcher_thread.join(timeout=5.0)
    collected = watcher.collect()
    if collected:
        stats_file = wad_data_dir / "levelstat.txt"
        stats_file.write_text(collected)
```

The existing `_auto_track_stats()` call then reads the written file through the
normal `_read_stats_snapshot()` pipeline — no changes needed downstream.

### Precondition

`wad_data_dir` must be computed before watcher creation. Currently it's only computed
inside the `if get_manage_data_dirs():` block for CLI arg injection. May need to
extract that computation earlier.

---

## Phase 5: Documentation

- Update `CLAUDE.md` with watcher framework docs, helion family, new file descriptions
- Update `README.md` supported sourceports list
- Update `docs/todo/sourceport-support/helion/helion.todo.md` with completed items
- Update completions if needed (Helion executables auto-discovered via
  `SOURCEPORT_FAMILIES`)

---

## Testing Strategy

### Watcher Framework (`test_stats_watcher.py`)

- `get_watcher()` returns `None` for unknown/unsupported families
- `get_watcher()` returns `HelionWatcher` for helion executables
- Thread lifecycle: start, stop, join
- Registry: register and retrieve

### Helion Watcher (`test_helion_watcher.py`)

- Detects `levelstat.txt` mtime changes and parses new maps
- Ignores pre-existing `levelstat.txt` content from before session start
- Accumulates maps across multiple file updates
- Enriches skill from fake `.hsg` ZIP with `world.json`
- Returns `None` when no stats captured
- Output matches `_LEVELSTAT_MAP_RE` format (parseable by existing code)
- `stop()` does a final read (captures stats written just before exit)
- Handles missing `levelstat.txt` gracefully
- Handles corrupt/missing save files gracefully
- Correct config dir on Linux, respects `$XDG_CONFIG_HOME`

### Sourceport Registration (`test_sourceports.py`)

- `identify_sourceport_family("Helion")` returns helion family
- `.hsg` in helion save extensions
- `get_complevel_args()` returns `["+complevel", "mbf21"]` (string, not int)
- `get_data_dir_args()` returns `-savedir` only
- `get_config_args()` returns `[]`

### Player Integration

- Watcher thread starts for helion, not for dsda
- Collected stats written to data dir
- All done with mocked subprocess (no real Helion needed)

---

## Open Questions

1. **Levelstat location fallback:** Docs disagree on whether Helion writes to
   `~/.config/Helion/` or the binary's directory. Current testing shows
   `~/.config/Helion/`. Should the watcher check both as a fallback?

2. **Skill default:** If no save file exists (player never saved), what skill should
   the default be? Currently `best_skill=4` (UV) as a "played" marker. Could instead
   leave as 0 (unknown).

3. **Future watcher reuse:** The framework is designed for other sourceports:
   - Doom Retro: `/proc/pid/mem` symbol reads (only option, no file output)
   - UZDoom: watch ZScript mod output file
   - Nugget-Doom: watch `-levelstat` output
   - Nyan-Doom: could watch `stats.txt` for real-time events

4. **Helion config injection:** No `-config` flag documented. Can Helion be told to
   use a specific `.ini` file? Deferred until confirmed.
