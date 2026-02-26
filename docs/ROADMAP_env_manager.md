# Roadmap: Caco as Doom Environment Manager

This document captures the vision for expanding caco from a WAD library manager into a full Doom environment manager. These are future features discussed and agreed upon in design sessions — not yet implemented unless noted.

---

## Per-WAD Data Directories — IMPLEMENTED (v1.6.0)

**Goal:** Each WAD gets its own isolated directory for saves, stats, configs, and other sourceport-generated data.

**Location:** `~/.local/share/caco/data/{id}_{sanitized_title}/`

**Implementation:** `config.py` (get_wad_data_dir, find_wad_data_dir, _sanitize_dirname), `player.py` (data dir arg injection), `sourceports.py` (family registry)

**Config:** `manage_data_dirs = true` (default), `data_dir` (custom base directory)

---

## Sourceport Families — IMPLEMENTED (v1.6.0)

**Goal:** Hardcoded mapping of sourceport executables to CLI flags for data/save directory redirection.

**Implementation:** `sourceports.py` — `SOURCEPORT_FAMILIES` dict with dsda, zdoom, chocolate, woof, eternity families. `identify_sourceport_family()` strips path and matches basename. `get_data_dir_args()` returns the appropriate flags.

**Families:** dsda (-data, -save), zdoom (-savedir), chocolate (-savedir), woof (-data, -save), eternity (-savedir)

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

## Future Ideas (Not Designed Yet)

- **Playing IWADs directly:** `caco play --iwad doom2` or `caco iwad play doom2` to
  launch an IWAD directly without a PWAD, using the preferred variant's path.  Useful
  for playing vanilla Doom/Doom II campaigns or testing IWAD setups.
- **Save game management:** Browse, backup, restore saves per-WAD
- **Demo recording/playback:** Track demo files per-WAD
- **Sourceport config management:** Per-WAD sourceport configs (e.g., different compatibility settings)
- **Auto-detect sourceport:** Identify installed sourceports and auto-create profiles
