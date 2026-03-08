# Reading Live Game Stats from Nyan Doom (No Source Changes)

## Key Global Variables

All stats are stored as global C variables. If the binary is built with symbols (default `RelWithDebInfo` build), they can be read directly from a running process.

| Variable | Type | Description |
|---|---|---|
| `gameskill` | `int` | Difficulty (0-4, displayed as 1-5) |
| `gameepisode` | `int` | Current episode |
| `gamemap` | `int` | Current map number |
| `leveltime` | `int` | Time in current map (tics, 35 tics/sec) |
| `totalleveltimes` | `int` | Sum of all prior level times (tics) |
| `gametic` | `int` | Total game tics elapsed |
| `totalkills` | `int` | Total killable monsters on map |
| `totalitems` | `int` | Total countable items on map |
| `totalsecret` | `int` | Total secrets on map |
| `totallive` | `int` | Monsters still alive |
| `gamestate` | `int` (enum) | GS_LEVEL, GS_INTERMISSION, etc. |
| `players` | `player_t[8]` | Player array (see below) |

### Player Struct Fields (`players[0]`)

| Field | Type | Description |
|---|---|---|
| `killcount` | `int` | Kills by this player |
| `itemcount` | `int` | Items collected |
| `secretcount` | `int` | Secrets found |
| `health` | `int` | Player health (between levels; use `mo->health` during play) |
| `armorpoints[NUMARMOR]` | `int[6]` | Armor values per type |
| `armortype` | `int` | Armor type (0-2) |
| `ammo[NUMAMMO]` | `int[4]` | Ammo counts (clip, shell, cell, missile) |
| `maxammo[NUMAMMO]` | `int[4]` | Max ammo per type |
| `readyweapon` | `int` (enum) | Currently equipped weapon |
| `weaponowned[NUMWEAPONS]` | `int[9]` | Which weapons are owned |
| `powers[NUMPOWERS]` | `int[12]` | Active power-up tic counters |
| `cards[NUMCARDS]` | `int[6]` | Keys/cards held |

### Derived Stats

- **Player kills**: `players[0].killcount` out of `totalkills`
- **Secrets found**: `players[0].secretcount` out of `totalsecret`
- **Items collected**: `players[0].itemcount` out of `totalitems`
- **Monsters remaining**: `totallive`
- **Level time in seconds**: `leveltime / 35`
- **Total run time in seconds**: `(totalleveltimes + leveltime) / 35`

## Source File Locations

| Data | File | Key Lines |
|---|---|---|
| `player_t` struct | `prboom2/src/d_player.h` | 157-288 |
| Intermission structs (`wbstartstruct_t`, `wbplayerstruct_t`) | `prboom2/src/d_player.h` | 295-339 |
| Global variable declarations | `prboom2/src/doomstat.h` | 150-302 |
| Global variable definitions | `prboom2/src/g_game.c` | 160-189 |
| `leveltime` increment | `prboom2/src/p_tick.c` | 48, 366 |
| Kill counting | `prboom2/src/p_mobj.c` | 2838 |
| Secret tracking | `prboom2/src/p_spec.c` | 1600-1614 |
| Item counting | `prboom2/src/p_inter.c` | 823 |
| Analysis output | `prboom2/src/dsda/analysis.c` | 77-118 |
| Level stat output | `prboom2/src/e6y.c` | 513-578 |
| WAD stats | `prboom2/src/dsda/wad_stats.c` | 32-60 |
| Console system | `prboom2/src/dsda/console.c` | 620-630, 2508 |

---

## Method 1: GDB Batch Mode (Best for Real-Time)

Attach to the running process non-interactively and print variables by name. Briefly pauses the process (typically imperceptible).

```bash
#!/bin/bash
PID=$(pgrep nyan-doom)
if [ -z "$PID" ]; then echo "nyan-doom not running"; exit 1; fi

gdb -batch -p $PID \
  -ex "print gameskill" \
  -ex "print gamemap" \
  -ex "print gameepisode" \
  -ex "print leveltime" \
  -ex "print totalkills" \
  -ex "print totalsecret" \
  -ex "print totalitems" \
  -ex "print totallive" \
  -ex "print totalleveltimes" \
  -ex "print gamestate" \
  -ex "print players[0].killcount" \
  -ex "print players[0].secretcount" \
  -ex "print players[0].itemcount" \
  -ex "print players[0].health" \
  -ex "print players[0].armorpoints" \
  -ex "print players[0].ammo" \
  -ex "print players[0].readyweapon" \
  -ex "print players[0].weaponowned" \
  -ex "detach" 2>/dev/null
```

Run in a loop for a live dashboard:

```bash
watch -n 1 ./read_stats.sh
```

## Method 2: `/proc/<pid>/mem` Direct Memory Reading

Read global variable addresses from the symbol table, then read bytes directly from process memory. No GDB required, but more manual.

```bash
#!/bin/bash
PID=$(pgrep nyan-doom)
if [ -z "$PID" ]; then echo "nyan-doom not running"; exit 1; fi

BINARY=$(readlink -f /proc/$PID/exe)

read_int() {
    local sym=$1
    local addr=$(nm "$BINARY" 2>/dev/null | grep " [BbDd] $sym$" | head -1 | awk '{print $1}')
    if [ -z "$addr" ]; then echo "?"; return; fi
    local dec_addr=$((16#$addr))
    dd if=/proc/$PID/mem bs=1 skip=$dec_addr count=4 2>/dev/null | od -A n -t d4 | tr -d ' '
}

echo "=== Nyan Doom Live Stats ==="
echo "Map: E$(read_int gameepisode)M$(read_int gamemap)"
echo "Skill: $(read_int gameskill)"
TICS=$(read_int leveltime)
echo "Level time: $((TICS / 35 / 60))m $((TICS / 35 % 60))s (${TICS} tics)"
echo "Kills on map: $(read_int totalkills)"
echo "Alive: $(read_int totallive)"
echo "Secrets on map: $(read_int totalsecret)"
echo "Items on map: $(read_int totalitems)"
echo "Game state: $(read_int gamestate)"
```

**Note:** For `players[0]` fields, you need to calculate struct offsets from the `players` symbol address. The GDB method is simpler for this.

**Note:** The `nm` addresses are virtual addresses from the ELF. For PIE (position-independent) binaries, you must add the base address from `/proc/<pid>/maps`. Non-PIE binaries use the addresses directly.

## Method 3: `-levelstat` Flag (Per-Level Output)

Launch with:

```bash
nyan-doom -levelstat
```

After each level completes, stats are appended to `levelstat.txt` in the working directory. Includes kills, items, secrets, and time per level.

Monitor with:

```bash
tail -f levelstat.txt
```

**Limitation:** Only writes when a level ends, not during play.

## Method 4: `-analysis` Flag (End-of-Run Summary)

Launch with:

```bash
nyan-doom -analysis
```

On exit, writes `analysis.txt` containing:

- Skill level
- Pacifist / reality / stroller status
- 100% kills / 100% secrets status
- Missed monsters and secrets count
- Run category classification (UV Max, UV Speed, NM Speed, etc.)
- Turbo / solo-net / coop-spawns flags

**Limitation:** Only written at game exit.

## Method 5: WAD Stats File (Persistent History)

The game automatically maintains `stats.txt` in its data directory (see `dsda_DataDir()`). Contains per-map historical bests:

- Best skill, best time, best max time, best NM time
- Total exits, best kills/items/secrets, max kills/items/secrets

Updated on normal exit.

## Method 6: In-Game Console

The game has a built-in console (see `docs/guides/console.md`). Relevant commands:

- `game.describe` — prints level name, skill, and monster params to terminal
- `check <attribute>` — prints config attribute info to terminal

The console outputs go to stdout/stderr via `lprintf()`, so redirecting the game's output captures them:

```bash
nyan-doom 2>&1 | tee game_output.log
```

## Combining Methods

For maximum coverage without source changes:

```bash
nyan-doom -levelstat -analysis 2>&1 | tee game_output.log &

# Real-time polling via GDB
watch -n 1 ./read_stats.sh

# Level-end stats
tail -f levelstat.txt
```
