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

## ZScript EventHandler Mod (Recommended)

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
