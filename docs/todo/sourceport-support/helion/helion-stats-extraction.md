# Extracting Live Statistics from Helion

Two approaches for reading level statistics from a running Helion instance
without modifying the source code.

---

## Approach 1: Save Game Polling

### How It Works

Helion save games are ZIP archives (`.hsg` files) containing JSON. The file
`world.json` inside each archive holds the complete world state, including all
level statistics. By configuring frequent autosaves or quicksaves, an external
process can poll the most recent save file to extract near-real-time stats.

### Data Available in `world.json`

| Field | Type | Description |
|-------|------|-------------|
| `MapName` | string | Current map lump name (e.g. `MAP01`) |
| `Skill` | enum/int | Difficulty level (0-4) |
| `LevelTime` | int | Ticks spent in current map (÷35 = seconds) |
| `TotalTime` | int | Cumulative ticks across all maps in session |
| `Gametick` | int | Global game tick counter |
| `TotalMonsters` | int | Total monsters on the map |
| `KillCount` | int | Monsters killed |
| `TotalItems` | int | Total items on the map |
| `ItemCount` | int | Items picked up |
| `TotalSecrets` | int | Total secret sectors |
| `SecretCount` | int | Secrets found |
| `VisitedMaps` | string[] | List of maps completed this session |
| `WorldState` | enum | Current world state |

### Save File Location

- **Linux**: `$XDG_CONFIG_HOME/Helion/` (default `~/.config/Helion/`)
- **Windows**: `~/Saved Games/Helion/`
- **Portable mode**: Same directory as the executable

File naming: `autosave{N}.hsg`, `quicksave{N}.hsg`, `savegame{N}.hsg`

### Setup

Enable periodic quicksaves via the in-game console:

```
game.quicksaveseconds 5
```

This creates a quicksave every 5 seconds, giving your external reader
near-real-time data.

### Implementation Outline

```python
#!/usr/bin/env python3
"""Poll Helion save files for level statistics."""

import json
import zipfile
import time
from pathlib import Path

SAVE_DIR = Path.home() / ".config" / "Helion"
TICKS_PER_SECOND = 35

def find_latest_save(save_dir: Path) -> Path | None:
    """Find the most recently modified .hsg file."""
    saves = sorted(save_dir.glob("*.hsg"), key=lambda p: p.stat().st_mtime, reverse=True)
    return saves[0] if saves else None

def read_world_model(save_path: Path) -> dict | None:
    """Extract world.json from a Helion save archive."""
    try:
        with zipfile.ZipFile(save_path, "r") as zf:
            with zf.open("world.json") as f:
                return json.load(f)
    except (zipfile.BadZipFile, KeyError, json.JSONDecodeError):
        return None

def format_stats(world: dict) -> dict:
    """Extract and format the relevant statistics."""
    level_time_secs = world.get("LevelTime", 0) / TICKS_PER_SECOND
    total_time_secs = world.get("TotalTime", 0) / TICKS_PER_SECOND

    return {
        "map": world.get("MapName", "???"),
        "skill": world.get("Skill", -1),
        "level_time": f"{int(level_time_secs // 60)}:{level_time_secs % 60:05.2f}",
        "total_time": f"{int(total_time_secs // 60)}:{total_time_secs % 60:05.2f}",
        "kills": f"{world.get('KillCount', 0)}/{world.get('TotalMonsters', 0)}",
        "items": f"{world.get('ItemCount', 0)}/{world.get('TotalItems', 0)}",
        "secrets": f"{world.get('SecretCount', 0)}/{world.get('TotalSecrets', 0)}",
        "visited_maps": world.get("VisitedMaps", []),
    }

def poll(interval: float = 2.0):
    """Poll the latest save file on an interval."""
    last_mtime = 0.0
    while True:
        save_path = find_latest_save(SAVE_DIR)
        if save_path and save_path.stat().st_mtime != last_mtime:
            last_mtime = save_path.stat().st_mtime
            world = read_world_model(save_path)
            if world:
                stats = format_stats(world)
                print(stats)
        time.sleep(interval)

if __name__ == "__main__":
    poll()
```

### Pros and Cons

- **Pro**: No external dependencies beyond Python, fully portable
- **Pro**: Access to the complete world state including all entity/sector data
- **Con**: Resolution limited by save frequency (risk of I/O overhead at <5s)
- **Con**: Save must occur for data to update — no mid-tick granularity
- **Con**: ZIP decompression + JSON parsing on every poll adds latency

---

## Approach 2: Live Process Memory Reading (ClrMD)

### How It Works

Helion is a .NET application. Microsoft's `ClrMD` library
(`Microsoft.Diagnostics.Runtime`) can attach to a running .NET process and
walk the managed heap to read object fields directly. This gives true
real-time access to any managed object in the process without pausing it.

### Target Objects

| Type | Key Fields |
|------|-----------|
| `Helion.World.Stats.LevelStats` | `KillCount`, `TotalMonsters`, `ItemCount`, `TotalItems`, `SecretCount`, `TotalSecrets` |
| `Helion.World.Impl.SinglePlayer.SinglePlayerWorld` | `LevelTime`, `Gametick`, `GameTicker`, `SkillLevel`, `MapName`, `WorldState`, `Paused` |
| `Helion.World.GlobalData` | `TotalTime`, `VisitedMaps` |

### Implementation Outline

```csharp
// HelionStatsReader.csproj
// <PackageReference Include="Microsoft.Diagnostics.Runtime" Version="3.*" />

using System;
using System.Diagnostics;
using System.Linq;
using System.Threading;
using Microsoft.Diagnostics.Runtime;

class HelionStatsReader
{
    const int TicksPerSecond = 35;

    static void Main(string[] args)
    {
        // Find the Helion process
        var proc = Process.GetProcessesByName("Helion").FirstOrDefault()
                ?? Process.GetProcessesByName("helion").FirstOrDefault();
        if (proc == null)
        {
            Console.Error.WriteLine("Helion is not running.");
            return;
        }

        Console.WriteLine($"Attached to Helion (PID {proc.Id})");

        while (true)
        {
            ReadStats(proc.Id);
            Thread.Sleep(1000);
        }
    }

    static void ReadStats(int pid)
    {
        // AttachToProcess with suspend:false for non-invasive reading.
        // Each call creates a snapshot; re-attach each poll for fresh data.
        using var target = DataTarget.AttachToProcess(pid, suspend: false);
        var runtime = target.ClrVersions[0].CreateRuntime();
        var heap = runtime.Heap;

        ClrObject? levelStats = null;
        ClrObject? world = null;
        ClrObject? globalData = null;

        foreach (var obj in heap.EnumerateObjects())
        {
            if (obj.Type == null) continue;

            switch (obj.Type.Name)
            {
                case "Helion.World.Stats.LevelStats":
                    levelStats = obj;
                    break;
                case "Helion.World.Impl.SinglePlayer.SinglePlayerWorld":
                    world = obj;
                    break;
                case "Helion.World.GlobalData":
                    globalData = obj;
                    break;
            }

            // Stop early once we have everything
            if (levelStats != null && world != null && globalData != null)
                break;
        }

        if (levelStats == null)
        {
            Console.WriteLine("(no active world)");
            return;
        }

        // Read LevelStats fields
        int kills = levelStats.Value.ReadField<int>("KillCount");
        int totalMonsters = levelStats.Value.ReadField<int>("TotalMonsters");
        int items = levelStats.Value.ReadField<int>("ItemCount");
        int totalItems = levelStats.Value.ReadField<int>("TotalItems");
        int secrets = levelStats.Value.ReadField<int>("SecretCount");
        int totalSecrets = levelStats.Value.ReadField<int>("TotalSecrets");

        Console.WriteLine($"K: {kills}/{totalMonsters}  " +
                          $"I: {items}/{totalItems}  " +
                          $"S: {secrets}/{totalSecrets}");

        // Read world-level fields (if found)
        if (world != null)
        {
            // These are auto-property backing fields — the field names
            // may appear as "<PropertyName>k__BackingField" or as direct
            // fields depending on how they're implemented. You may need
            // to enumerate obj.Type.Fields to find the exact names.
            //
            // For interface-implemented properties on the concrete class,
            // check the actual field layout:
            //
            //   foreach (var f in world.Value.Type.Fields)
            //       Console.WriteLine($"  {f.Name} : {f.Type?.Name}");

            // Example (field names may need adjustment):
            // int levelTime = world.Value.ReadField<int>("<LevelTime>k__BackingField");
            // int gametick = world.Value.ReadField<int>("<Gametick>k__BackingField");
            // Console.WriteLine($"LevelTime: {levelTime / TicksPerSecond}s");
        }
    }
}
```

### Finding the Right Field Names

Auto-property backing fields in .NET are named `<PropertyName>k__BackingField`.
Regular fields keep their declared name. To discover the exact layout at runtime:

```csharp
foreach (var field in obj.Type.Fields)
    Console.WriteLine($"  {field.Name} ({field.Type?.Name}) offset={field.Offset}");
```

### Running

```bash
dotnet new console -n HelionStatsReader
cd HelionStatsReader
dotnet add package Microsoft.Diagnostics.Runtime
# paste the code into Program.cs
dotnet run
```

On Linux you may need `SYS_PTRACE` capability or to run as the same user:

```bash
# If permission denied:
sudo dotnet run

# Or grant ptrace to the specific binary:
sudo setcap cap_sys_ptrace=ep $(which dotnet)
```

### Pros and Cons

- **Pro**: True real-time — reads live object state at any moment
- **Pro**: Access to every managed field in the process (not just stats)
- **Pro**: No game-side configuration needed, no save overhead
- **Con**: Heap enumeration is relatively slow (~100-500ms per scan)
- **Con**: Attaching repeatedly has overhead; may cause brief GC pauses
- **Con**: Fragile — field names/layouts can change between Helion versions
- **Con**: Requires .NET SDK and matching runtime version
- **Con**: Needs ptrace permissions on Linux

### Optimization: Cache Object Addresses

After the first full heap scan, you can cache the addresses of the objects
you care about and read them directly on subsequent polls, skipping the full
enumeration. The addresses remain valid as long as no GC compaction occurs
(which you can detect by checking if the object at the cached address still
has the expected type).

---

## Comparison

| | Save Game Polling | ClrMD Memory Reading |
|-|-------------------|---------------------|
| **Latency** | Seconds (save interval) | Sub-second |
| **Data** | Full world state snapshot | Any managed object field |
| **Dependencies** | Python or any ZIP/JSON reader | .NET SDK + ClrMD NuGet |
| **Permissions** | File read access | ptrace / same user |
| **Stability** | Stable (JSON schema) | May break on version changes |
| **Game impact** | Save I/O (already built-in) | Minor GC pressure |
| **Best for** | Post-level summaries, overlays | Speedrun timers, live dashboards |
