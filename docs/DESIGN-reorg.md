# WAD Organization Redesign

Design document for replacing the single `status` enum with a multi-axis organizational model.

## Implementation Status

| Phase | Status | Description |
|-------|--------|-------------|
| 1. Schema + dual-write | Done | New columns, enums, bidirectional sync |
| 2. Playthroughs | Done | New table, CRUD, session integration |
| 3. Query system | Done | play:/intent:/avail: queries, smart collections |
| 4. CLI updates | Done | modify, import, gc, collection command |
| 5. TUI + GUI | Done | Tab redesign, theme, detail panels |
| 6. WAD analysis | Done | Linedef parsing, UMAPINFO, completion detection |
| 7. Cleanup | Pending | Drop old status column after testing |

734 tests passing. Old `status:` queries and dual-write remain active until Phase 7.

## Motivation

The current 6-state status enum (`to-play`, `backlog`, `playing`, `finished`, `abandoned`, `awaiting-update`) conflates three orthogonal concerns:

- **Objective play state** — has the user played/completed this?
- **Subjective intent** — does the user want to play this?
- **File lifecycle** — should GC clean this up?

This makes certain states inexpressible (e.g., "completed but queued for replay") and creates ambiguity (e.g., `to-play` vs `backlog`).

---

## Agreed: Three Independent Axes

Replace the single `status` field with three independent fields:

### Play State (objective, auto-tracked)

| Value | Meaning |
|-------|---------|
| `unplayed` | No playthrough exists |
| `started` | Active playthrough in progress |
| `completed` | Most recent playthrough is completed |

Derived from playthrough records, not manually set (though manual override is always available).

### Intent (subjective, user-managed)

| Value | Meaning |
|-------|---------|
| `inbox` | Newly imported, not yet triaged |
| `queued` | User wants to play this |
| `shelved` | Set aside for later |
| `dropped` | User has no interest |

Default for new imports: `inbox`.

### Availability (system-managed)

| Value | Meaning |
|-------|---------|
| `cached` | WAD file is on disk |
| `downloadable` | Source URL known, can be fetched |
| `unavailable` | No known way to obtain the file |

GC decisions key off availability + intent, not play state.

---

## Agreed: Playthroughs as First-Class Entities

A **playthrough** represents a single run through a WAD.

### Fields

- `id`, `wad_id`
- `started_at`, `completed_at` (null = in progress)
- Per-map stats accumulation (kills, items, secrets, time)
- Optional: notes, difficulty, settings

### Behavior

- Sessions belong to playthroughs (not directly to WADs)
- WAD `play_state` is derived from playthrough state
- "Replay" = start a new playthrough; old one archived with stats intact
- `times_beaten` = count of completed playthroughs (replaces `++beaten` counter)
- Each playthrough has its own independent stats tracking

### Replaying a WAD

Starting a new playthrough:
- Archives the current playthrough's stats
- Creates a fresh playthrough with clean stats
- WAD `play_state` transitions back to `started`

No manual counter increment needed.

---

## Agreed: Smart Collections (Saved Queries)

Users can save named queries that act as dynamic collections:

```
"Tonight"         = "play:started , (intent:queued tag:short -tag:hard)"
"Cacoward Catchup" = "tag:cacoward ^play:completed"
"Comfort Replay"   = "play:completed rating:5"
```

These update dynamically based on current WAD state. The existing beets-style query syntax extends naturally to the new axes.

---

## Agreed: Completion Detection

### Core Principle

A playthrough is considered complete when all **required maps** have been exited at least once, as reported by stats files (stats.txt / levelstat.txt).

**Required maps** = total maps − secret maps − dead-end/credits maps.

### Per-WAD override

User can always set expected map count or mark completion manually:
```
caco modify id:42 expected-maps=30
caco modify id:42 --complete
```

### Auto-transition with notification

When a play session ends and stats indicate completion, the system transitions `play_state` to `completed` and notifies the user. User can undo: `caco modify id:42 play:started`.

---

## Agreed: WAD Analysis via Linedef Parsing

Parse WAD files to build a map graph with exit/secret/dead-end classification.

### Exit Linedef Types

| Type | Trigger | Exit Type | Format |
|------|---------|-----------|--------|
| 11 | Switch (once) | Normal | Vanilla |
| 51 | Switch (once) | Secret | Vanilla |
| 52 | Walk-over (once) | Normal | Vanilla |
| 124 | Walk-over (once) | Secret | Vanilla |
| 197 | Gunshot (once) | Normal | Boom |
| 198 | Gunshot (once) | Secret | Boom |
| 243 | (activation flags) | Normal | UDMF |
| 244 | (activation flags) | Secret | UDMF |
| 74 | (activation flags) | Teleport_NewMap | UDMF |
| 75 | (activation flags) | Teleport_EndGame | UDMF |

MBF21 adds sector-based exits (sector special bit 12, damage bits 2/3) but no new linedef types.

### LINEDEFS Lump Format (vanilla/Boom)

Each linedef is 14 bytes. The type/special field is at bytes 6-7 (uint16 LE).

### UDMF Format

Text-based TEXTMAP lump. Parse `special` field on linedef blocks for types 243, 244, 74, 75.

### Secret Map Detection

**Vanilla Doom 2** (hardcoded in engine):
- MAP31, MAP32 are the only possible secret maps
- Secret exit from MAP15 → MAP31, from MAP31 → MAP32
- Secret exit linedef on any other map just goes to gamemap+1 (same as normal)

**Vanilla Doom 1** (hardcoded in engine):
- E*M9 is the secret map per episode
- E1M3→E1M9, E2M5→E2M9, E3M6→E3M9, E4M2→E4M9

**UMAPINFO**: Parse `next` and `nextsecret` fields to build a reachability graph. A map is secret **only if** it is reachable exclusively via `nextsecret` and **never** appears in any `next` chain. This distinction is critical — some WADs (e.g., Poogers) use `nextsecret` on every map to point to the next regular map, which would misclassify nearly all maps as secret under a naive rule. Unlimited secret maps possible with correct reachability analysis.

### Dead-End / Credits Map Detection

Maps with zero exit linedefs are dead-end candidates. However, boss death actions can trigger exits without exit linedefs (E2M8, E3M8, MAP07, etc.), and **dsda-doom stats.txt records these as exits** — so boss-exit maps are handled automatically by stats tracking.

### Terminal Map Identification (priority order)

1. UMAPINFO `endgame=true` / `endpic` / `endcast` / `endbunny`
2. UMAPINFO `next` self-referencing loop (e.g., MAP22 → MAP22)
3. UMAPINFO: highest map with no `next` field defined
4. Standard Doom conventions (MAP30 / E*M8)
5. Fallback: highest non-secret map number in the WAD

---

## Agreed: Completion Heuristics

Three heuristics layered on top of the base "all required maps exited" check. Validated against 17 real WADs from the library — **94% accuracy (16/17)**.

### Heuristic 1: Credits-Map Detection

Handles WADs where the final map is a no-exit credits/coda screen (very common for sub-30-map WADs).

**Conditions** (all must be true):
- Terminal map has `total_exits == 0` in stats AND `skill == 0` (never beaten)
- The immediately preceding map in sequence has `total_exits >= 1`
- Terminal is within 2 maps of the player's furthest progress

If all conditions met, exclude terminal map from required maps.

**Validated against**: Firefly (MAP08 credits), Witching Hour (MAP09 credits), 1x1 (MAP22 self-loop), Jaded (MAP04 "Game Over"), Lemoncholia (MAP07 credits).

### Heuristic 2: Dominant-Map Detection

Handles WADs where one map IS the WAD (e.g., a slaughter map with 4500 monsters + 2 tiny bonus maps).

**Conditions**:
- One map contains >70% of total monsters in the WAD
- That map has `total_exits >= 1`
- Remaining maps are bonus/extras

**Validated against**: Umbra (MAP01 has 4500 monsters, MAP02-MAP33 are extras).

### Heuristic 3: Stats-Opaque Fallback

Handles WADs where stats tracking is fundamentally broken (ACS-scripted transitions, GZDoom-only features, etc.).

**Conditions**:
- Significant session playtime exists in the database
- All stats are zeroed (every map shows skill=0, total_exits=0)

**Action**: Flag WAD as "stats-opaque" and prompt user for manual completion marking rather than silently failing.

**Validated against**: Lonesome Road Ch1 (9 maps, ACS-scripted transitions, zero stats). This was a retroactive manual entry (user remembered completing it previously), not an organic detection failure. Actual auto-detection accuracy on organically-tracked completions: **16/16 (100%)**.

### Key Technical Finding

**dsda-doom stats.txt records boss-death exits.** Maps with no exit linedefs but boss-triggered exits (Outpost 13 MAP01, UDINO E2M8/E3M8) show `total_exits=1` in stats. This eliminates the need for boss-action detection as a separate code path — stats already handle it.

---

## Audit Results (Real Library Validation)

| WAD | Maps | Terminal | No-Exit Maps | Stats Exits | Status | Detection |
|-----|------|----------|-------------|-------------|--------|-----------|
| D2ICO | 36 | MAP30 | MAP05,30 | 23 | playing | OK |
| Dead Skin | 2 | MAP02 | MAP02 | 0 | playing | OK |
| Firefly | 8 | MAP08 | MAP05,07,08 | 7 | finished | OK (credits) |
| Vogel | 1 | MAP01 | none | 1 | finished | OK |
| Crimson Horror | 9 | MAP08 | none | 8 | finished | OK |
| Demons Cry | 1 | MAP01 | none | 1 | finished | OK |
| Witching Hour | 9 | MAP09 | MAP08,09 | 8 | finished | OK (credits) |
| 1x1 | 25 | MAP22 | MAP03,07,19,21 | 21 | finished | OK (credits) |
| Jaded | 4 | MAP04 | MAP03,04 | 3 | finished | OK (credits) |
| Outpost 13 | 1 | MAP01 | MAP01 | 1 | finished | OK (boss exit) |
| Umbra | 3 | MAP33 | MAP02 | 1 | finished | OK (dominant) |
| Gossip | 9 | MAP09 | MAP09 | 4 | playing | OK |
| Lonesome Road | 9 | MAP09 | 6 of 9 | 0 | finished | N/A (manual entry) |
| Lemoncholia | 7 | MAP07 | MAP07 | 6 | finished | OK (credits) |
| UDINO | 36 | E*M8 | E2M8,E3M8 | 30 | playing | OK |
| TNT2 | 36 | MAP36 | MAP36 | 10 | playing | OK |
| Poogers | 36 | MAP36 | MAP09,23,30,36 | 0 | playing | OK |

### Inverse Audit: Non-Finished WADs (False Positive Check)

27 non-finished WADs tested. **Zero false positives** — no WAD would be incorrectly detected as complete.

6 WADs had both cached files and stats data to evaluate:

| WAD | Status | Playtime | Exited/Required | Verdict |
|-----|--------|----------|-----------------|---------|
| D2ICO | abandoned | 18h39m | 23/29 | Correctly incomplete |
| UDINO | abandoned | 9h07m | 30/32 | Correctly incomplete |
| Gossip | playing | 7h51m | 4/8 | Correctly incomplete |
| TNT 2: Devilution | playing | 3h43m | 10/33 | Correctly incomplete |
| House of Dead Skin | playing | 5s | 0/1 | Correctly incomplete |
| Poogers | abandoned | 0s | 0/30 | Correctly incomplete |

21 remaining WADs had no cached files or stats (backlog/to-play with 0 sessions) — trivially not complete.

### Design Issues Found During Inverse Audit

**UMAPINFO `nextsecret` reachability (critical fix):** Naive "pointed to by nextsecret = secret" is wrong. Poogers uses `nextsecret` on every map for normal progression. Fixed rule: secret = reachable exclusively via `nextsecret`, never via any `next` chain. See Secret Map Detection section above.

**Single-map WADs with no exit linedefs:** A 1-map WAD with no exit linedefs gets classified as "credits map", making required=0. If it later gets stats via boss-kill exit, the rubric vacuously passes. Low risk (boss-kill exits do appear in stats as `total_exits=1`) but worth noting.

---

## Not Yet Discussed

- Migration strategy from current `status` enum to new axes
- Query syntax for the new fields (`play:`, `intent:`, `avail:`?)
- How the TUI/GUI surfaces the new model (tabs? filters? columns?)
- Whether `awaiting-update` maps to an intent value or becomes a tag/flag
- Rating system changes (multi-dimensional?)
- WAD relationships / sequencing
- Collection ordering (is `queued` ordered?)
