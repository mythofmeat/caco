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

## Auto Stats Tracking — IMPLEMENTED (v1.7.0)

**Goal:** After each play session, automatically read the WAD's stats.txt and update the stored per-map statistics.

**Implementation:** `player.py` (_find_stats_file, _auto_track_stats), `db/_schema.py` (migration #11: stats_snapshot column on wads), `db/_wads.py` (copies stats_snapshot to completion on status→finished), `cli/stats.py` (auto-attaches WAD stats_snapshot when `beaten add` is called without --stats-file)

**How it works:**
1. After the sourceport exits, search `{wad_data_dir}/**/stats.txt` (recursive, handles nyan-doom nesting)
2. Falls back to `levelstat.txt` if no stats.txt found
3. Parse with existing `parse_stats_file()`, serialize with `stats_to_json()`
4. Store JSON in `wads.stats_snapshot` column (live progress, not completion)
5. When user marks WAD as beaten (`beaten add` or `update --status finished`), the snapshot is automatically archived to the completion record

**Config:** `auto_stats = true` (default), requires `manage_data_dirs = true`

---

## Future Ideas (Not Designed Yet)

- **Playing IWADs directly:** `caco play --iwad doom2` or `caco iwad play doom2` to
  launch an IWAD directly without a PWAD, using the preferred variant's path.  Useful
  for playing vanilla Doom/Doom II campaigns or testing IWAD setups.
- **Save game management:** Browse, backup, restore saves per-WAD
- **Demo recording/playback:** Track demo files per-WAD
- **Sourceport config management:** Per-WAD sourceport configs (e.g., different compatibility settings)
- **Auto-detect sourceport:** Identify installed sourceports and auto-create profiles
