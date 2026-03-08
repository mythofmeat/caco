# Extracting Live Statistics from Nugget-Doom

How to grab level statistics (time, kills, secrets, difficulty, map, etc.)
from a running Nugget-Doom process — **with no source code changes**.

---

## Key Data Structures & Variables

All stats live as global symbols in the binary, making them accessible externally.

### Level Totals (`src/doomstat.h`)

| Variable | Type | Description |
|----------|------|-------------|
| `totalkills` | `int` | Total killable monsters in current map |
| `totalitems` | `int` | Total collectible items in current map |
| `totalsecret` | `int` | Total secret sectors in current map |

### Per-Player Stats (`src/d_player.h`, `player_t` struct)

| Field | Type | Description |
|-------|------|-------------|
| `players[N].killcount` | `int` | Kills by player N |
| `players[N].itemcount` | `int` | Items collected by player N |
| `players[N].secretcount` | `int` | Secrets found by player N |
| `players[N].health` | `int` | Current health |
| `players[N].armorpoints` | `int` | Current armor |
| `players[N].armortype` | `int` | Armor type (0=none, 1=green, 2=blue) |

Player 0 is the local/console player in single-player.

### Time Tracking (`src/doomstat.h`, incremented in `src/p_tick.c`)

| Variable | Type | Description |
|----------|------|-------------|
| `leveltime` | `int` | Tics elapsed in current level |
| `totalleveltimes` | `int` | Cumulative tics for all completed levels |
| `levelstarttic` | `int` | Gametic value when level started |
| `basetic` | `int` | Adjusted levelstarttic for demo sync |

> **Conversion:** `TICRATE = 35` (defined in `src/doomdef.h`), so `seconds = leveltime / 35`.

### Map Identity (`src/doomstat.h`)

| Variable | Type | Description |
|----------|------|-------------|
| `gamemap` | `int` | Current map number |
| `gameepisode` | `int` | Current episode (Doom 1 style) |
| `gamemapinfo` | `mapentry_t*` | Map metadata (name, author, par time, etc.) |

### Difficulty (`src/doomstat.h`, `src/doomdef.h`)

| Variable | Type | Values |
|----------|------|--------|
| `gameskill` | `skill_t` (enum) | 0=ITYTD, 1=HNTR, 2=HMP, 3=UV, 4=NM, 5=custom |

```c
// From src/doomdef.h
typedef enum {
  sk_default=-2, sk_none=-1,
  sk_baby=0, sk_easy, sk_medium, sk_hard, sk_nightmare, sk_custom
} skill_t;
```

### Game / Completion State

| Variable | Type | Description |
|----------|------|-------------|
| `gamestate` | `gamestate_t` | GS_LEVEL, GS_INTERMISSION, GS_FINALE, etc. |
| `gameaction` | `gameaction_t` | Pending action (ga_completed, ga_died, etc.) |
| `secretexit` | `boolean` | True if exiting via secret exit |
| `wminfo` | `wbstartstruct_t` | Intermission/stats-screen data (populated at level end) |

### Intermission Stats Structure (`src/d_player.h`)

```c
typedef struct wbstartstruct_s {
    int epsd;           // Episode number
    boolean didsecret;  // Secret level accessed
    int last;           // Previous level number
    int next;           // Next level number
    int maxkills, maxitems, maxsecret, maxfrags;
    int partime;        // Par time in tics
    int pnum;           // Current player index
    wbplayerstruct_t plyr[MAXPLAYERS];
    int totaltimes;     // Total time across all levels
} wbstartstruct_t;

typedef struct {
    boolean in;     // Player active
    int skills;     // Kills
    int sitems;     // Items
    int ssecret;    // Secrets
    int stime;      // Level time (tics)
    int frags[4];   // Deathmatch frags
    int score;      // Current score
} wbplayerstruct_t;
```

---

## Method 1: `-levelstat` (Built-in, Per-Level)

Launch with:

```bash
nugget-doom -levelstat
```

After each level completion, the game appends to `levelstat.txt` in the
working directory (`src/g_game.c:2116`):

```
MAP01 - 2:34 (2:34)  K: 45/50  I: 12/15  S: 3/5
MAP02s - 1:12 (3:46)  K: 30/30  I: 8/10  S: 2/2
```

Format: `<map>[s] - <level_time> (<cumulative_time>)  K: <got>/<max>  I: <got>/<max>  S: <got>/<max>`

- The `s` suffix indicates a secret exit.
- Monitor live: `tail -f levelstat.txt`

**Limitation:** Only writes when a level is **completed**, not during gameplay.

---

## Method 2: `-statdump` (Built-in, End-of-Session)

```bash
nugget-doom -statdump stats.txt
# Or to stdout:
nugget-doom -statdump -
```

Captures detailed stats for up to 32 levels (`src/statdump.c`), including
kills/items/secrets percentages, par time comparisons, and multiplayer frag
tables. Written when the game **exits**.

**Limitation:** Only dumps at exit, not real-time.

---

## Method 3: GDB Attach (True Real-Time, Any Variable)

Attach to the running process and read any global variable on demand.
Requires the binary to retain symbols (a local debug or relwithdebinfo build;
`CPACK_STRIP_FILES` strips symbols only in packaging).

### One-shot query

```bash
gdb -batch -q -p $(pidof nugget-doom) \
  -ex 'print leveltime' \
  -ex 'print totalkills' \
  -ex 'print totalitems' \
  -ex 'print totalsecret' \
  -ex 'print gameskill' \
  -ex 'print gamemap' \
  -ex 'print gameepisode' \
  -ex 'print totalleveltimes' \
  -ex 'print players[0].killcount' \
  -ex 'print players[0].itemcount' \
  -ex 'print players[0].secretcount' \
  -ex 'print players[0].health'
```

### Compact one-liner format

```bash
gdb -batch -q -p $(pidof nugget-doom) \
  -ex 'printf "map=%d ep=%d skill=%d time=%d kills=%d/%d items=%d/%d secrets=%d/%d hp=%d\n", gamemap, gameepisode, gameskill, leveltime, players[0].killcount, totalkills, players[0].itemcount, totalitems, players[0].secretcount, totalsecret, players[0].health' \
  2>/dev/null | grep '^map='
```

### Polling script (1-second updates)

```bash
#!/bin/bash
while true; do
  gdb -batch -q -p $(pidof nugget-doom) \
    -ex 'printf "{\"map\":%d,\"episode\":%d,\"skill\":%d,\"leveltime\":%d,\"totaltime\":%d,\"kills\":%d,\"kills_max\":%d,\"items\":%d,\"items_max\":%d,\"secrets\":%d,\"secrets_max\":%d,\"health\":%d}\n", gamemap, gameepisode, gameskill, leveltime, totalleveltimes, players[0].killcount, totalkills, players[0].itemcount, totalitems, players[0].secretcount, totalsecret, players[0].health' \
    2>/dev/null | grep '^{'
  sleep 1
done
```

**Caveat:** Each `gdb -batch` attach briefly pauses the process (~1-5 ms).
Acceptable for 1/sec polling, but not suitable for frame-rate queries.

---

## Method 4: `/proc/<pid>/mem` Direct Read (Zero Pause)

Read global variable values straight from process memory with no ptrace stop.

### Step 1: Get symbol addresses from the binary

```bash
nm /path/to/nugget-doom | grep -E ' [BDbd] .*(leveltime|totalkills|totalitems|totalsecret|gamemap|gameskill|gameepisode|players)\b'
```

### Step 2: Read with Python

```python
#!/usr/bin/env python3
"""Read Nugget-Doom stats from process memory without pausing it."""

import struct, subprocess, os

BINARY = "/path/to/nugget-doom"  # adjust to your build

def get_symbols():
    """Parse nm output to get global variable addresses."""
    out = subprocess.check_output(["nm", BINARY], text=True)
    syms = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3:
            addr, typ, name = parts
            syms[name] = int(addr, 16)
    return syms

def read_int(pid, addr):
    """Read a 4-byte int from /proc/<pid>/mem."""
    with open(f"/proc/{pid}/mem", "rb") as f:
        f.seek(addr)
        return struct.unpack("i", f.read(4))[0]

def main():
    pid = int(subprocess.check_output(["pidof", "nugget-doom"]).strip())
    syms = get_symbols()

    print(f"map:      {read_int(pid, syms['gamemap'])}")
    print(f"episode:  {read_int(pid, syms['gameepisode'])}")
    print(f"skill:    {read_int(pid, syms['gameskill'])}")
    print(f"time:     {read_int(pid, syms['leveltime']) / 35:.1f}s")
    print(f"kills:    {read_int(pid, syms['totalkills'])}")
    print(f"items:    {read_int(pid, syms['totalitems'])}")
    print(f"secrets:  {read_int(pid, syms['totalsecret'])}")

if __name__ == "__main__":
    main()
```

**Note:** For PIE (position-independent) binaries, you must add the base
address from `/proc/<pid>/maps` to each symbol offset. Non-PIE binaries
use absolute addresses directly.

**Caveat:** Requires `CAP_SYS_PTRACE` or same-user access. Reading
`players[0].killcount` requires knowing the struct offset (compile with
debug info and use `pahole` or `offsetof` to find it).

---

## Method 5: `LD_PRELOAD` Hook (Most Flexible, Zero Source Changes)

Inject a shared library at launch that accesses the game's globals from
within the same address space — no ptrace, no pausing, full access.

### stats_hook.c

```c
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <time.h>

/* Global symbols from the nugget-doom binary — the dynamic linker
   resolves these because we're in the same process. */
extern int leveltime, totalkills, totalitems, totalsecret;
extern int gamemap, gameepisode;
extern int gameskill;
extern int totalleveltimes;

/*
 * Hook SDL_GL_SwapWindow (called every frame) to periodically export stats.
 * If the port uses SDL_RenderPresent instead, hook that.
 */
void SDL_GL_SwapWindow(void *window) {
    static void (*real_swap)(void *) = NULL;
    static time_t last_write = 0;

    if (!real_swap)
        real_swap = dlsym(RTLD_NEXT, "SDL_GL_SwapWindow");

    time_t now = time(NULL);
    if (now != last_write) {
        last_write = now;

        FILE *f = fopen("/tmp/nugget_doom_stats.json", "w");
        if (f) {
            fprintf(f,
                "{\n"
                "  \"map\": %d,\n"
                "  \"episode\": %d,\n"
                "  \"skill\": %d,\n"
                "  \"leveltime_tics\": %d,\n"
                "  \"leveltime_sec\": %.2f,\n"
                "  \"totaltime_tics\": %d,\n"
                "  \"kills_max\": %d,\n"
                "  \"items_max\": %d,\n"
                "  \"secrets_max\": %d\n"
                "}\n",
                gamemap, gameepisode, gameskill,
                leveltime, leveltime / 35.0,
                totalleveltimes,
                totalkills, totalitems, totalsecret);
            fclose(f);
        }
    }

    real_swap(window);
}
```

> **Note on player stats:** Accessing `players[0].killcount` etc. from the
> hook requires either (a) declaring the full `player_t` struct in the hook
> (copy from `d_player.h`), or (b) computing the byte offset and casting.
> The simpler globals above work without any struct definitions.

### Build & run

```bash
gcc -shared -fPIC -o stats_hook.so stats_hook.c -ldl
LD_PRELOAD=./stats_hook.so nugget-doom
```

### Monitor

```bash
watch -n1 cat /tmp/nugget_doom_stats.json
```

You can extend this to write to a Unix socket, shared memory segment, or
UDP broadcast for integration with OBS overlays, stream bots, etc.

---

## Quick Reference: Which Method to Use

| Need | Method | Real-time? | Effort |
|------|--------|-----------|--------|
| Per-level summary after completion | `-levelstat` | At level end | None |
| Full session dump | `-statdump` | At exit | None |
| Read any variable on demand | GDB attach | Yes (~1ms pause) | Low |
| High-frequency reads, no pause | `/proc/mem` | Yes (zero pause) | Medium |
| Full integration (overlays, bots) | `LD_PRELOAD` | Yes (per-frame) | Medium |

---

## Source File Reference

| File | What's There |
|------|-------------|
| `src/doomstat.h` | Global variable declarations (all the `extern`s) |
| `src/d_player.h` | `player_t`, `wbstartstruct_t`, `wbplayerstruct_t` structs |
| `src/doomdef.h` | `skill_t` enum, `TICRATE` (35), `gamestate_t` |
| `src/g_game.c:2116` | `G_WriteLevelStat()` — the `-levelstat` implementation |
| `src/g_game.c:2407` | `wminfo` population at level end |
| `src/statdump.c` | `-statdump` implementation (`StatCopy`, `StatDump`) |
| `src/p_tick.c:323` | `leveltime++` — where time advances each tic |
| `src/p_mobj.c:1533` | `totalkills`/`totalitems` incremented during map spawn |
| `src/p_inter.c` | Player kill/item/secret counts incremented during play |
| `src/p_setup.c:1658` | Stats zeroed at level load |
