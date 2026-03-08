# Extracting Live Level Statistics from a Running UZDoom Instance

## Overview

This document describes multiple methods for grabbing real-time level statistics
(kills, secrets, items, time, difficulty, map completion, etc.) from UZDoom
**without modifying the C++ source code**.

---

## Available Statistics

The engine tracks the following per-level data in the `FLevelLocals` struct
(`src/g_levellocals.h`):

| Field | Type | Description |
|---|---|---|
| `total_monsters` | int | Total monsters on the map |
| `killed_monsters` | int | Monsters killed so far |
| `total_items` | int | Total countable items on the map |
| `found_items` | int | Items picked up so far |
| `total_secrets` | int | Total secret sectors |
| `found_secrets` | int | Secrets discovered so far |
| `time` | int | Time spent in this hub (tics, 35 per second) |
| `maptime` | int | Time spent on the current map (tics) |
| `totaltime` | int | Total elapsed game time (tics) |
| `partime` | int | Par time for the map (tics) |
| `sucktime` | int | "Suck" time threshold (minutes) |

Per-player stats are in `player_t` (`src/playsim/d_player.h`):

| Field | Type | Description |
|---|---|---|
| `killcount` | int | This player's kill count |
| `itemcount` | int | This player's item count |
| `secretcount` | int | This player's secret count |
| `fragcount` | int | Frag count (multiplayer) |

Difficulty is stored in the `gameskill` CVAR (integer, 0-based skill index).

---

## Method 1: Console Log File + `printstats` Command

**Complexity:** Minimal
**Real-time:** Manual (on-demand)

### Setup

Launch with a log file:

```bash
uzdoom +logfile /tmp/uzdoom_stats.txt
```

### Usage

Open the in-game console (`~` key) and type:

```
printstats
```

This calls `StoreLevelStats()` and prints one line per visited level:

```
Level MAP01 - Kills: 35/50 - Items: 12/20 - Secrets: 1/3 - Time: 2:45
```

All console output (including `printstats`) goes to the log file. Monitor it
externally with:

```bash
tail -f /tmp/uzdoom_stats.txt
```

### Source Reference

- `src/gamedata/statistics.cpp:581` — `CCMD(printstats)`
- `src/gamedata/statistics.cpp:564` — `GetStatString()` formats the output
- `src/common/console/c_enginecmds.cpp:102` — `execLogfile()` implementation

---

## Method 2: `stat statistics` HUD Overlay

**Complexity:** Minimal
**Real-time:** Yes (every frame)

### Usage

In the console, type:

```
stat statistics
```

This enables a persistent on-screen text overlay that refreshes every frame,
showing kills/items/secrets/time for each visited level. The data is pulled
from `StoreLevelStats(primaryLevel)` which reads live values from the level
struct.

Also available:

```
stat velocity
```

Shows the player's current, max, and average movement velocity.

### Source Reference

- `src/gamedata/statistics.cpp:600` — `ADD_STAT(statistics)`
- `src/gamedata/statistics.cpp:606` — `ADD_STAT(velocity)`

---

## Method 3: `savestatistics` CVAR (Automatic File Output)

**Complexity:** Minimal
**Real-time:** No (writes at episode end only)

### Setup

Set these CVARs in the console or in your config file:

```
savestatistics 1
statfile "mystats.txt"
```

- `savestatistics 0` — disabled (default)
- `savestatistics 1` — save at end of episode
- `savestatistics 2` — reserved for single-level mode (not fully implemented)

### Output Format

When an episode ends, the engine writes a structured text file containing:

- Episode name and header
- Date played
- Skill level and player class
- Total time
- Per-level breakdown: `killcount/totalkills, itemcount/totalitems, secretcount/totalsecrets`
- Per-level completion times

### Limitation

Only writes when an episode finishes. Not suitable for continuous real-time
monitoring.

### Source Reference

- `src/gamedata/statistics.cpp:50-51` — CVAR definitions
- `src/gamedata/statistics.cpp:226` — `SaveStatistics()` function
- `src/gamedata/statistics.cpp:434` — called from `STAT_ChangeLevel()`

---

## Method 4: ZScript EventHandler Mod (Recommended)

**Complexity:** Low (one small `.pk3` file)
**Real-time:** Yes (configurable interval)
**No C++ changes required.**

This is the most powerful and flexible approach. Create a ZScript mod that hooks
into the game loop and outputs structured stats to the console (and thus the
log file).

### Step 1: Create the ZScript

`zscript.zs`:

```zscript
version "4.13"

class StatsReporter : EventHandler
{
    int tickCounter;

    override void WorldTick()
    {
        tickCounter++;
        // Report every 35 ticks (once per second at 35 tics/sec)
        if (tickCounter % 35 == 0)
        {
            Console.Printf("STATS|%s|%d|%d/%d|%d/%d|%d/%d|%d|%d",
                level.MapName,
                level.time,
                level.killed_monsters, level.total_monsters,
                level.found_items, level.total_items,
                level.found_secrets, level.total_secrets,
                level.maptime,
                level.totaltime
            );
        }
    }

    override void WorldLoaded(WorldEvent e)
    {
        Console.Printf("MAPSTART|%s|%d|%d|%d",
            level.MapName,
            level.total_monsters,
            level.total_items,
            level.total_secrets
        );
    }
}
```

`MAPINFO`:

```
GameInfo
{
    AddEventHandlers = "StatsReporter"
}
```

### Step 2: Package as a .pk3

```bash
mkdir stats_reporter
cp zscript.zs stats_reporter/
cp MAPINFO stats_reporter/
cd stats_reporter && zip -r ../stats_reporter.pk3 . && cd ..
```

### Step 3: Launch

```bash
uzdoom -file stats_reporter.pk3 +logfile /tmp/uzdoom_stats.txt
```

### Step 4: Parse Externally

```bash
tail -f /tmp/uzdoom_stats.txt | grep -E '^(STATS|MAPSTART)\|'
```

Example output:

```
MAPSTART|MAP01|50|20|3
STATS|MAP01|35|2/50|0/20|0/3|35|35
STATS|MAP01|70|5/50|1/20|0/3|70|70
STATS|MAP01|105|8/50|3/20|1/3|105|105
```

### Available ZScript Fields

All accessible via the `level` global in ZScript (`wadsrc/static/zscript/doombase.zs:547`):

- `level.MapName` — map lump name (e.g. "MAP01")
- `level.killed_monsters` / `level.total_monsters`
- `level.found_items` / `level.total_items`
- `level.found_secrets` / `level.total_secrets`
- `level.time` — hub time in tics
- `level.maptime` — current map time in tics
- `level.totaltime` — total game time in tics
- `level.partime` — par time in tics
- `level.sucktime` — suck time in minutes

For difficulty, you can read the `gameskill` CVAR from ZScript:

```zscript
let skill = CVar.FindCVar('gameskill');
if (skill) Console.Printf("SKILL|%d", skill.GetInt());
```

### EventHandler Hooks Available

The `EventHandler` class (`wadsrc/static/zscript/events.zs:166`) provides
callbacks for many game events:

| Hook | Fires When |
|---|---|
| `WorldTick()` | Every game tic |
| `WorldLoaded(WorldEvent e)` | Map finishes loading |
| `WorldUnloaded(WorldEvent e)` | Map is being unloaded |
| `WorldThingSpawned(WorldEvent e)` | An actor spawns |
| `WorldThingDied(WorldEvent e)` | An actor dies |
| `PlayerEntered(PlayerEvent e)` | Player enters the game |
| `PlayerDied(PlayerEvent e)` | Player dies |

---

## Method 5: ZScript DAP Debug Server (Network Socket)

**Complexity:** High
**Real-time:** Yes (on-demand queries over TCP)

UZDoom includes a Debug Adapter Protocol server for ZScript debugging.

### Setup

```bash
uzdoom -debug 19021
```

Or set CVARs at runtime:

```
vm_debug 1
vm_debug_port 19021
```

This opens a TCP socket on port 19021 speaking the DAP protocol. A client can
connect and inspect ZScript variables, set breakpoints, evaluate expressions,
and walk the call stack.

### Considerations

- Designed for IDE-based ZScript debugging, not general stat extraction
- Requires implementing a DAP client or using an existing one
- Can inspect any ZScript-accessible game state
- Heaviest-weight option; best suited if you need full introspection

### Source Reference

- `src/common/scripting/dap/` — DAP server implementation
- `src/d_main.cpp:2933` — `vm_debug` CVAR
- `src/d_main.cpp:2949` — `vm_debug_port` CVAR (default 19021)

---

## Method 6: ACS Script (In-WAD Scripting)

If you're authoring or modifying a WAD, ACS scripts can query stats via
`GetLevelInfo()`:

```acs
#include "zcommon.acs"

script "DumpStats" (void)
{
    int kills   = GetLevelInfo(LEVELINFO_KILLED_MONSTERS);
    int maxk    = GetLevelInfo(LEVELINFO_TOTAL_MONSTERS);
    int items   = GetLevelInfo(LEVELINFO_FOUND_ITEMS);
    int maxi    = GetLevelInfo(LEVELINFO_TOTAL_ITEMS);
    int secrets = GetLevelInfo(LEVELINFO_FOUND_SECRETS);
    int maxs    = GetLevelInfo(LEVELINFO_TOTAL_SECRETS);

    Log(s:"Kills: ", d:kills, s:"/", d:maxk,
        s:" Items: ", d:items, s:"/", d:maxi,
        s:" Secrets: ", d:secrets, s:"/", d:maxs);
}
```

Available `LEVELINFO_*` constants (`src/playsim/p_acs.cpp:505`):

- `LEVELINFO_PAR_TIME`
- `LEVELINFO_TOTAL_SECRETS` / `LEVELINFO_FOUND_SECRETS`
- `LEVELINFO_TOTAL_ITEMS` / `LEVELINFO_FOUND_ITEMS`
- `LEVELINFO_TOTAL_MONSTERS` / `LEVELINFO_KILLED_MONSTERS`
- `LEVELINFO_SUCK_TIME`
- `LEVELINFO_CLUSTERNUM` / `LEVELINFO_LEVELNUM`

---

## Summary and Recommendation

| Method | Real-Time | External Access | Effort | Best For |
|---|---|---|---|---|
| `printstats` + logfile | On-demand | File tail | Trivial | Quick manual checks |
| `stat statistics` | Every frame | On-screen only | Trivial | Visual monitoring |
| `savestatistics` | End of episode | File | Trivial | Post-game analysis |
| **ZScript EventHandler** | **Configurable** | **File tail / pipe** | **Low** | **Automated extraction** |
| DAP Debug Server | On-demand | TCP socket | High | Full introspection |
| ACS GetLevelInfo | In-script | Console/log | Low | WAD-specific scripting |

**Recommendation:** Use **Method 4 (ZScript EventHandler + logfile)** for
automated, real-time, structured stat extraction with zero C++ modifications.
It provides full control over what data is emitted, at what frequency, and in
what format — and the output can be trivially consumed by external tools.
