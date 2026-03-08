# Extracting Live Level Statistics from DOOM Retro

## Overview

This document describes how to read real-time level statistics (map time, kills,
secrets, difficulty, etc.) from a running DOOM Retro process **without modifying
the source code**.

---

## Available Statistics

All the data is tracked in global variables and the `player_t` struct at runtime:

| Stat | Variable | Type | Defined In |
|------|----------|------|------------|
| Map time (tics) | `maptime` | `int` | `p_tick.c` (global) |
| Total session time (tics) | `totaltime` | `int` | `p_tick.c` (global) |
| Difficulty | `gameskill` | `skill_t` (enum/int) | `g_game.c` (global) |
| Map number | `gamemap` | `int` | `g_game.c` (global) |
| Episode number | `gameepisode` | `int` | `g_game.c` (global) |
| Game state | `gamestate` | `gamestate_t` (enum) | `doomstat.h` (global) |
| Player kills | `viewplayer->killcount` | `int` | `d_player.h` (`player_t`) |
| Player items | `viewplayer->itemcount` | `int` | `d_player.h` (`player_t`) |
| Player secrets | `viewplayer->secretcount` | `int` | `d_player.h` (`player_t`) |
| Total enemies in map | `totalkills` | `int` | `g_game.c` (global) |
| Total items in map | `totalitems` | `int` | `g_game.c` (global) |
| Total secrets in map | `totalsecrets` | `int` | `g_game.c` (global) |

### Notes

- The tic rate is **35 tics/second**, so `maptime / 35` gives seconds in the
  current map.
- `gameskill` values: 0 = I'm Too Young to Die, 1 = Hey Not Too Rough,
  2 = Hurt Me Plenty, 3 = Ultra-Violence, 4 = Nightmare.
- `gamestate` values: 0 = `GS_INTERMISSION`, 1 = `GS_LEVEL` (in-game),
  5 = `GS_TITLESCREEN`, etc. Check `doomstat.h` for the full enum.
- For DOOM II / MAP-format WADs, `gameepisode` is 1 and `gamemap` is 1–32.
  For DOOM I, `gameepisode` is 1–4 and `gamemap` is 1–9.

### Cumulative Session Stats (in config file)

DOOM Retro also tracks cumulative stats across all sessions as `uint64_t` values
in `m_config.h`. These are persisted to the config file:

| Stat | Variable |
|------|----------|
| Total play time | `stat_timeplayed` |
| Total kills (all sessions) | `stat_monsterskilled_total` |
| Maps completed | `stat_mapsfinished` |
| Maps started | `stat_mapsstarted` |
| Total secrets found | `stat_secretsfound` |
| Total items picked up | `stat_itemspickedup` |
| Player deaths | `stat_deaths` |
| Damage dealt | `stat_damageinflicted` |
| Damage taken | `stat_damagereceived` |
| Distance traveled | `stat_distancetraveled` |
| Cheats entered | `stat_cheatsentered` |

---

## The Problem: No Built-In External Access

The DOOM Retro source code has **no IPC mechanisms whatsoever**:

- No sockets, pipes, or FIFOs
- No shared memory segments
- No stdin reading for console commands
- No file-watching or hot-reload of config
- No HTTP/REST/WebSocket server

The in-game console (`playerstats` command, `condump` file dump, `exec` file
execution) is only accessible via the in-game UI — there is no way to send
commands to the process externally.

The config file (`~/.config/doomretro/doomretro.cfg` on Linux) is written on
many events (level transitions, menu changes, settings tweaks), but it only
contains the **cumulative** `stat_*` variables — not the live per-map values
like `maptime`, `killcount`, or `totalkills`.

---

## Extraction Approaches

### Approach 1: `/proc/pid/mem` + Symbol Table (Recommended)

Read global variables directly from the process memory on Linux. This is
**zero-overhead** and does not pause the game.

#### Step 1: Find symbol addresses

```bash
# Get addresses of key globals from the binary's symbol table
nm /path/to/doomretro | grep -w -E \
  'maptime|totaltime|gameskill|gamemap|gameepisode|totalkills|totalitems|totalsecrets|viewplayer|gamestate'
```

If the binary is stripped, you will need to rebuild with symbols (`-g` flag) or
use a debug build.

#### Step 2: Find player_t struct offsets

Use GDB once (does not need the game running):

```bash
gdb -batch \
  -ex 'ptype player_t' \
  -ex 'p/d &((player_t*)0)->killcount' \
  -ex 'p/d &((player_t*)0)->itemcount' \
  -ex 'p/d &((player_t*)0)->secretcount' \
  /path/to/doomretro
```

This prints the byte offsets of each field within the `player_t` struct.

#### Step 3: Read from a script

```python
#!/usr/bin/env python3
"""Read live stats from a running DOOM Retro process via /proc/pid/mem."""

import struct
import os
import sys

def read_int(pid, addr):
    """Read a 4-byte signed int from process memory."""
    with open(f"/proc/{pid}/mem", "rb") as f:
        f.seek(addr)
        return struct.unpack("<i", f.read(4))[0]

def read_ptr(pid, addr):
    """Read a 64-bit pointer from process memory."""
    with open(f"/proc/{pid}/mem", "rb") as f:
        f.seek(addr)
        return struct.unpack("<Q", f.read(8))[0]

# --- Configuration: fill these in from nm/gdb output ---
MAPTIME_ADDR      = 0x0  # from: nm doomretro | grep ' maptime'
TOTALTIME_ADDR    = 0x0  # from: nm doomretro | grep ' totaltime'
GAMESKILL_ADDR    = 0x0  # from: nm doomretro | grep ' gameskill'
GAMEMAP_ADDR      = 0x0  # from: nm doomretro | grep ' gamemap'
GAMEEPISODE_ADDR  = 0x0  # from: nm doomretro | grep ' gameepisode'
TOTALKILLS_ADDR   = 0x0  # from: nm doomretro | grep ' totalkills'
TOTALITEMS_ADDR   = 0x0  # from: nm doomretro | grep ' totalitems'
TOTALSECRETS_ADDR = 0x0  # from: nm doomretro | grep ' totalsecrets'
VIEWPLAYER_ADDR   = 0x0  # from: nm doomretro | grep ' viewplayer'
GAMESTATE_ADDR    = 0x0  # from: nm doomretro | grep ' gamestate'

# Struct field offsets (from gdb ptype/offset commands)
KILLCOUNT_OFFSET   = 0  # &((player_t*)0)->killcount
ITEMCOUNT_OFFSET   = 0  # &((player_t*)0)->itemcount
SECRETCOUNT_OFFSET = 0  # &((player_t*)0)->secretcount
# ---

TICRATE = 35
SKILL_NAMES = [
    "I'm Too Young to Die",
    "Hey, Not Too Rough",
    "Hurt Me Plenty",
    "Ultra-Violence",
    "Nightmare!",
]

def main():
    pid_str = os.popen("pidof doomretro").read().strip()
    if not pid_str:
        print("DOOM Retro is not running.", file=sys.stderr)
        sys.exit(1)

    pid = int(pid_str.split()[0])

    maptime      = read_int(pid, MAPTIME_ADDR)
    totaltime    = read_int(pid, TOTALTIME_ADDR)
    gameskill    = read_int(pid, GAMESKILL_ADDR)
    gamemap      = read_int(pid, GAMEMAP_ADDR)
    gameepisode  = read_int(pid, GAMEEPISODE_ADDR)
    totalkills   = read_int(pid, TOTALKILLS_ADDR)
    totalitems   = read_int(pid, TOTALITEMS_ADDR)
    totalsecrets = read_int(pid, TOTALSECRETS_ADDR)
    gamestate    = read_int(pid, GAMESTATE_ADDR)

    viewplayer_ptr = read_ptr(pid, VIEWPLAYER_ADDR)
    killcount      = read_int(pid, viewplayer_ptr + KILLCOUNT_OFFSET)
    itemcount      = read_int(pid, viewplayer_ptr + ITEMCOUNT_OFFSET)
    secretcount    = read_int(pid, viewplayer_ptr + SECRETCOUNT_OFFSET)

    seconds = maptime // TICRATE
    hours, remainder = divmod(seconds, 3600)
    minutes, secs = divmod(remainder, 60)
    centiseconds = (maptime % TICRATE) * 100 // TICRATE

    skill_name = SKILL_NAMES[gameskill] if 0 <= gameskill < 5 else f"Unknown ({gameskill})"
    state_name = {0: "Intermission", 1: "In Level", 5: "Title Screen"}.get(gamestate, f"Other ({gamestate})")

    print(f"Map:       E{gameepisode}M{gamemap}")
    print(f"Skill:     {skill_name}")
    print(f"State:     {state_name}")
    print(f"Map Time:  {hours:02d}:{minutes:02d}:{secs:02d}.{centiseconds:02d}")
    print(f"Kills:     {killcount} / {totalkills}", end="")
    if totalkills > 0:
        print(f"  ({100 * killcount // totalkills}%)")
    else:
        print()
    print(f"Items:     {itemcount} / {totalitems}", end="")
    if totalitems > 0:
        print(f"  ({100 * itemcount // totalitems}%)")
    else:
        print()
    print(f"Secrets:   {secretcount} / {totalsecrets}", end="")
    if totalsecrets > 0:
        print(f"  ({100 * secretcount // totalsecrets}%)")
    else:
        print()

if __name__ == "__main__":
    main()
```

#### Requirements

- The binary must **not be stripped** (needs symbol table for `nm`).
- `/proc/sys/kernel/yama/ptrace_scope` must be `0` (classic), or you must run
  as the same user / root. Check with:
  ```bash
  cat /proc/sys/kernel/yama/ptrace_scope
  # 0 = any process can read, 1 = only parent, 2 = admin only
  sudo sysctl kernel.yama.ptrace_scope=0  # to allow (temporary)
  ```
- For PIE (position-independent) executables, symbol addresses from `nm` are
  offsets — you must add the base load address:
  ```bash
  # Find the base address of the executable mapping
  grep -m1 'doomretro' /proc/$(pidof doomretro)/maps | cut -d'-' -f1
  ```
  Then: `actual_addr = base_addr + nm_offset`.

---

### Approach 2: GDB Scripted Attach (Simpler, Slower)

Attach GDB, read variables by name, detach. Briefly pauses the game (~10–50ms).

```bash
#!/bin/bash
PID=$(pidof doomretro)
if [ -z "$PID" ]; then
    echo "DOOM Retro is not running." >&2
    exit 1
fi

gdb -batch -nx \
  -ex "attach $PID" \
  -ex "printf \"maptime:     %d\n\", maptime" \
  -ex "printf \"gameskill:   %d\n\", gameskill" \
  -ex "printf \"gamemap:     %d\n\", gamemap" \
  -ex "printf \"gameepisode: %d\n\", gameepisode" \
  -ex "printf \"gamestate:   %d\n\", gamestate" \
  -ex "printf \"totalkills:  %d\n\", totalkills" \
  -ex "printf \"totalitems:  %d\n\", totalitems" \
  -ex "printf \"totalsecrets:%d\n\", totalsecrets" \
  -ex "printf \"killcount:   %d\n\", viewplayer->killcount" \
  -ex "printf \"itemcount:   %d\n\", viewplayer->itemcount" \
  -ex "printf \"secretcount: %d\n\", viewplayer->secretcount" \
  -ex "detach" \
  /path/to/doomretro 2>/dev/null
```

Suitable for occasional snapshots. **Do not poll this at high frequency** — each
attach/detach pauses the game briefly.

---

### Approach 3: `LD_PRELOAD` Hook (Most Powerful)

Inject a shared library at launch that hooks the game tick and exposes stats
via a Unix socket or shared memory file. No source changes required.

```c
/* libdoomstats.c — LD_PRELOAD hook for DOOM Retro stats export */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>

/*
 * This approach works by wrapping SDL_GL_SwapWindow (called every frame).
 * On each frame, it reads the global variables and writes them to a
 * shared memory file at /dev/shm/doomretro_stats.
 *
 * Build:
 *   gcc -shared -fPIC -o libdoomstats.so libdoomstats.c -ldl
 *
 * Run:
 *   LD_PRELOAD=./libdoomstats.so doomretro
 *
 * Read stats:
 *   cat /dev/shm/doomretro_stats
 */

/* Declare externs matching DOOM Retro's globals */
extern int maptime;
extern int totaltime;
extern int gameskill;
extern int gamemap;
extern int gameepisode;
extern int totalkills;
extern int totalitems;
extern int totalsecrets;
extern int gamestate;

/* player_t is complex; just read the fields we need via pointer arithmetic.
 * viewplayer is a pointer to the player struct. */
extern void *viewplayer;

/* These offsets must be determined via GDB (see Approach 1, Step 2).
 * Replace with actual values. */
#ifndef KILLCOUNT_OFFSET
#define KILLCOUNT_OFFSET   0
#endif
#ifndef ITEMCOUNT_OFFSET
#define ITEMCOUNT_OFFSET   0
#endif
#ifndef SECRETCOUNT_OFFSET
#define SECRETCOUNT_OFFSET 0
#endif

static int get_player_int(int offset) {
    if (!viewplayer) return 0;
    return *(int *)((char *)viewplayer + offset);
}

/* Wrap SDL_GL_SwapWindow to hook every frame */
typedef void (*sdl_swap_fn)(void *);
static sdl_swap_fn real_swap = NULL;

void SDL_GL_SwapWindow(void *window) {
    if (!real_swap)
        real_swap = (sdl_swap_fn)dlsym(RTLD_NEXT, "SDL_GL_SwapWindow");

    /* Write stats to /dev/shm */
    FILE *f = fopen("/dev/shm/doomretro_stats", "w");
    if (f) {
        fprintf(f, "maptime=%d\n", maptime);
        fprintf(f, "totaltime=%d\n", totaltime);
        fprintf(f, "gameskill=%d\n", gameskill);
        fprintf(f, "gamemap=%d\n", gamemap);
        fprintf(f, "gameepisode=%d\n", gameepisode);
        fprintf(f, "gamestate=%d\n", gamestate);
        fprintf(f, "totalkills=%d\n", totalkills);
        fprintf(f, "totalitems=%d\n", totalitems);
        fprintf(f, "totalsecrets=%d\n", totalsecrets);
        fprintf(f, "killcount=%d\n", get_player_int(KILLCOUNT_OFFSET));
        fprintf(f, "itemcount=%d\n", get_player_int(ITEMCOUNT_OFFSET));
        fprintf(f, "secretcount=%d\n", get_player_int(SECRETCOUNT_OFFSET));
        fclose(f);
    }

    real_swap(window);
}
```

Then from any other process:

```bash
# Live-tail the stats
watch -n 0.5 cat /dev/shm/doomretro_stats
```

**Note:** Because this hooks via `dlsym(RTLD_NEXT, ...)` and references DOOM
Retro's globals as `extern`, it relies on the symbols being present in the
binary's dynamic symbol table. If the binary is statically linked or stripped,
this approach will not work without additional effort (e.g., manually resolving
addresses).

---

### Approach 4: Config File Polling (Easiest, Cumulative Only)

The config file is written on many game events (not just exit). Poll it for
the `stat_*` values:

```bash
# Config file location on Linux
CONFIG="$HOME/.config/doomretro/doomretro.cfg"

# Extract cumulative stats
grep -E '^stat_' "$CONFIG"
```

This gives you cumulative all-time stats only. You can compute deltas between
reads to approximate per-session changes, but you **cannot** get live per-map
values (kills on current map, current map time, etc.) from this file.

---

## Comparison

| Approach | Live Per-Map Stats | Setup Effort | Performance Impact | Requires Symbols |
|----------|-------------------|--------------|-------------------|-----------------|
| `/proc/pid/mem` | Yes | Medium | None | Yes |
| GDB attach | Yes | Low | Brief pause per read | Yes |
| `LD_PRELOAD` | Yes | High | Negligible | Yes |
| Config file poll | No (cumulative only) | None | None | No |

---

## Key Source Code References

- `src/d_player.h:166-169` — `killcount`, `itemcount`, `secretcount` in `player_t`
- `src/d_player.h:254-276` — `wbstartstruct_t` intermission data
- `src/p_tick.c:253-254` — `maptime` increment per game tic
- `src/g_game.c:95-101` — global declarations of `gameskill`, `gamemap`, `totalkills`, etc.
- `src/g_game.c:1723-1729` — `wminfo` population at level end
- `src/doomstat.h:86` — `speciallumpname` (current map lump name, e.g. "E1M1")
- `src/m_config.h:252-292` — all `stat_*` cumulative variables
- `src/m_config.c:719` — `M_SaveCVARs()` config file writer
- `src/c_console.c:1888-1991` — `C_UpdatePlayerStatsOverlay()` overlay rendering
- `src/c_cmds.c:6880-7966` — `playerstats` console command implementation
- `src/c_cmds.c:3094-3141` — `exec` command (reads .cfg files as console commands)
- `src/m_misc.c:248-295` — `M_GetAppDataFolder()` config directory resolution
