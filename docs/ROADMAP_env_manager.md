# Roadmap: Caco as Doom Environment Manager

This document captures the vision for expanding caco from a WAD library manager into a full Doom environment manager. These are future features discussed and agreed upon in design sessions — not yet implemented unless noted.

---

## Per-WAD Data Directories

**Goal:** Each WAD gets its own isolated directory for saves, stats, configs, and other sourceport-generated data.

**Location:** `~/.local/share/caco/data/{id}_{sanitized_title}/`

**Benefits:**
- Zero-conflict stats tracking (each WAD's stats.txt is isolated)
- Organized save games per WAD
- Opens the door for save management, demo tracking, etc.
- No pre/post diffing needed — the file belongs to exactly one WAD

**How it works:**
- When playing a WAD, caco injects sourceport arguments to redirect the save/data directory to the WAD's data dir (e.g., nyan-doom/dsda-doom: `-save {wad_dir}`)
- The data dir is created automatically on first play
- Existing saves/stats from before this feature can be manually moved into the correct directory

**Opt-out by default:** A config option like `manage_data_dirs = true` (default true) controls this. Users who prefer the sourceport's default directory layout can set it to false.

---

## Sourceport Profiles

**Goal:** Define per-sourceport configurations that tell caco how to redirect data directories.

**Config example:**
```toml
[sourceports.nyan-doom]
path = "nyan-doom"
save_arg = "-save"        # Flag to redirect save/data directory

[sourceports.dsda-doom]
path = "dsda-doom"
save_arg = "-save"        # Same CLI structure as nyan-doom
```

**Why per-sourceport:** Different sourceports use different CLI flags for setting the save directory, stats output, etc. Initially we'd just support nyan-doom/dsda-doom (which share the same CLI structure), then add profiles for GZDoom, Crispy Doom, etc. as needed.

**Scope:** Start with just the save directory redirect. Future profiles could include config file paths, demo directories, etc.

**User addendum:** i actually think these values should be hardcoded. they really aren't supposed to be "changed." we define them in the code with different sourceport "families" mapping onto multiple sourports. e.g., dsda-doom family cli args are used for nyan-doom (and any other dsda-doom) sourceports i decide to support

---

## Auto Stats Tracking

**Goal:** After each play session, automatically read the WAD's stats.txt and update the stored per-map statistics.

**How it works:**
1. After the sourceport exits, check if `{wad_data_dir}/stats.txt` exists
2. Parse it with the existing `parse_stats_file()` infrastructure
3. Find the most recent completion record for this WAD
4. If it has a stats_snapshot, update it with the new file contents
5. If no completion exists, create one with the stats

**Why this is simple with per-WAD data dirs:** Since each WAD has its own stats.txt, there's no conflict detection needed. The file always belongs to exactly one WAD.

**Without per-WAD data dirs (shared stats.txt):** If multiple WADs share a stats file path, use pre/post snapshot comparison:
1. Read the stats file BEFORE launching the sourceport
2. After exit, read again
3. Only maps that changed during the session get attributed to this WAD
4. This inherently prevents cross-WAD contamination

---

## IWAD Management (Implemented)

**Status:** Implemented with family/variant model and priority resolution.

Proper IWAD registry with MD5-based identification, multiple variants per family (v1.9, BFG, Enhanced, KEX), configurable priority resolution, and freedoom cross-family fallbacks. See `caco iwad --help`.

---

## Future Ideas (Not Designed Yet)

- **Playing IWADs directly:** `caco play --iwad doom2` or `caco iwad play doom2` to
  launch an IWAD directly without a PWAD, using the preferred variant's path.  Useful
  for playing vanilla Doom/Doom II campaigns or testing IWAD setups.
- **Save game management:** Browse, backup, restore saves per-WAD
- **Demo recording/playback:** Track demo files per-WAD
- **Sourceport config management:** Per-WAD sourceport configs (e.g., different compatibility settings)
- **Auto-detect sourceport:** Identify installed sourceports and auto-create profiles
