# caco

A personal Doom WAD library manager inspired by [beets](https://beets.io). Import WADs from multiple sources, track what you've played, and launch them with your preferred sourceport — from a desktop GUI with a dark, Doom-inspired theme.

## Features

- **Import from anywhere** — idgames archive, Doom Wiki, Doomworld forums, URLs, or local files. Auto-enriches with Doom Wiki metadata.
- **Smart queries** — beets-style filters (`status:in-progress`, `tag:megawad`, `author:"erik alm"`) with OR, negation, and glob support.
- **Play tracking** — automatic playtime, session history, per-map stats (kills/items/secrets/time), and completion counts.
- **IWAD management** — register your IWADs once, and caco auto-detects which one each WAD needs.
- **Sourceports from source** — build nyan-doom or uzdoom into caco's own prefix, no system install required.
- **Companion files** — manage DEH patches, music WADs, and other companion files with automatic deduplication.
- **Per-WAD isolation** — saves, stats, and configs are separated per WAD so nothing gets mixed up.
- **On-demand downloads** — idgames WADs are cached when you play, with configurable auto-cleanup.
- **Cacowards browser** — a magazine-style year-by-year view of Doomworld's awards, doubling as an import queue.
- **Garbage collection** — reclaim disk space from completed / abandoned WADs, reviewing every file before it goes.
- **Smart collections** — save queries by name and pin them to the sidebar.

## Installation

### Arch Linux (pacman-managed, recommended)

Caco is built locally and published to a pacman repo on your own machine.
Nothing is built in CI and no package leaves the machine.

```bash
git clone git@github.com:mythofmeat/caco.git && cd caco

# One time: set up the local pacman repo (shared by all locally-built programs)
./contrib/arch/release.sh --init

# Cut a release: bump, build, publish, upgrade
./contrib/arch/release.sh
```

`--init` creates `/var/lib/pacman-local` and prints a `pacman.conf` stanza to
add — do that after the first release, since an empty repo has no database for
pacman to fetch. Every subsequent release is just `./contrib/arch/release.sh`
followed by the `pacman -Syu` it runs for you.

To build the package without cutting a release:

```bash
cd contrib/arch && makepkg -f -d --nocheck
```

### From source

```bash
git clone https://github.com/mythofmeat/caco && cd caco
cargo install --path crates/caco
```

## Quick Start

Launch `caco` (or pick it from your application menu), then:

1. **Settings** in the sidebar — set your sourceport and IWAD directories.
2. **IWADs** in the sidebar — register your IWADs. Drop in a directory and caco
   identifies each file by MD5.
3. **Import** in the sidebar — search idgames or the Doom Wiki, paste a
   Doomworld thread URL, or point at a local file.
4. Select a WAD and press **Enter** to play. On first launch caco detects the
   required IWAD and complevel from the WAD file itself.

Play time, sessions, and per-map stats are recorded automatically once you quit
the sourceport.

## The Interface

- Grid and list views with sortable columns and a live filter bar
- WAD thumbnails scraped from the Doom Wiki (or extracted from TITLEPIC) with on-disk caching
- Right-click context menu (play, edit, delete, sessions, map stats, saves & demos, new playthrough)
- Right-hand detail sidebar with metadata, play stats, and quick actions
- Cacowards view — a year-by-year browse of Doomworld's awards where entries you
  don't own yet are flagged `absent` with an inline Import button
- Keyboard shortcuts: `j/k`, `g`/`G`, `Home`/`End`, `Enter`, `E`, `D`, `S`, `M`, `F`, `P`, `Esc`

### Dialogs

| Sidebar | What it does |
|---------|--------------|
| Stats | Library statistics — playtime, completions, activity over time |
| Cache | Cached downloads, with per-entry and bulk removal |
| Files | Companion file registry: every managed file, the WADs using it, and orphan cleanup |
| Profiles | Per-sourceport config profiles, edited in place |
| IWADs | Registered IWADs and id24 resources |
| Ports | Build sourceports from source into caco's own prefix |
| Enrich | Re-run complevel / IWAD / port detection across the library, plus per-year Cacowards refresh |
| Clean | Reclaim disk space, reviewing every file first |
| Trash | Restore or permanently delete removed WADs |
| Settings | Sourceports, launch args, behavior toggles, cache limits, paths |

Per-WAD dialogs come from the context menu or a shortcut: **Edit** (`E`),
**Sessions** (`S`), **Map Stats** (`M`), **Saves & Demos** (`F`).

## Queries

The filter bar uses beets-style query syntax:

```
scythe                          Free text search
title:scythe author:alm         Field queries (AND)
status:in-progress , status:unplayed    OR queries
^status:completed               Negation
tag:caco*                       Glob patterns
cacoward:2023                   Cacoward entries for a year
```

**Fields:** `id`, `title`, `author`, `year`, `filename`, `tag`, `status`, `source`, `iwad`, `complevel`, `config`, `cacoward`

**Status values:** `unplayed`, `in-progress`, `completed`, `abandoned`

**Status shortcuts:** `u` (unplayed); `p`, `ip` (in-progress); `c`, `f`, `done` (completed); `a`, `d` (abandoned)

Save a query as a collection to pin it to the sidebar.

## Importing

The Import view auto-detects what you give it — an idgames search term or file
ID, a Doom Wiki or Doomworld URL, or a path to a local file or directory. WAD
files that turn out to be IWADs are registered as IWADs rather than library
entries.

Non-Doomwiki imports are auto-enriched with Doom Wiki metadata (author, year,
description, IWAD). Duplicate detection warns before re-importing.

## IWADs

Register your IWADs once and caco references them by family name everywhere.
They are organised by family (doom, doom2, tnt, plutonia) with variant support
(v1.9, bfg, kex). The preferred variant is resolved automatically, with Freedoom
as a cross-family fallback.

## Configuration

Config file: `~/.local/share/caco/config.toml` — beside the database, not in
`~/.config`, so the portable set is one directory. The Settings dialog edits the
common options; anything it does not expose survives a save untouched, so
hand-editing the file is safe.

Only settings you actually changed are written. A stock install has an empty (or
absent) config, and defaults resolve at runtime — which is what lets the data
directory move between machines without dragging one machine's absolute paths
along.

On a first launch with no sourceport configured, caco adopts the best one it
finds on `PATH` and tells you which.

### Example Config

```toml
sourceport = "nyan-doom"
iwad = "doom2"
iwad_dirs = ["/usr/share/games/doom", "~/games/iwads"]
sourceport_args = ["-nomusic"]          # passed to every sourceport

auto_detect_iwad = true
auto_detect_complevel = true
auto_doomwiki_enrich = true
cache_max_size_gb = 20.0
cache_auto_clean = true
port_update_check_days = 1              # 0 = never check built ports for updates

# Extra args for specific ports only (appended after sourceport_args)
[port_args]
nyan-doom = ["-geometry", "1920x1200"]

[list]
format = ["id", "title", "author", "status", "beaten", "playtime", "last_played"]
sort = "id+"

[gui]
default_view = "list"
thumbnail_size = 160
```

See `config.example.toml` for all available options.

## Data Storage

Caco splits its files by whether they can be regenerated. Everything under
`~/.local/share/caco` is worth carrying between machines; everything under
`~/.cache/caco` is disposable and rebuilds itself on demand.

**Keep these** — copy them to move your library to another machine:

| Location | Contents |
|----------|----------|
| `~/.local/share/caco/config.toml` | Configuration |
| `~/.local/share/caco/gui-state.json` | GUI view/sort/filter state |
| `~/.local/share/caco/library.db` | Library database |
| `~/.local/share/caco/data/` | Per-WAD saves, stats, configs |
| `~/.local/share/caco/iwads/` | Managed IWADs |
| `~/.local/share/caco/id24/` | Managed id24 WADs |
| `~/.local/share/caco/companions/` | Managed companion files |
| `~/.local/share/caco/sourceports/` | Per-sourceport config profiles |
| `~/.local/share/caco/ports/` | Sourceport build recipes and their patches |
| `~/.local/share/caco/backups/` | Save backups + pre-migration DB snapshots |

**Disposable** — safe to delete at any time:

| Location | Contents |
|----------|----------|
| `~/.cache/caco/wads/` | Cached WAD files, re-downloaded on demand |
| `~/.cache/caco/thumbnails/` | Thumbnail cache, re-extracted from TITLEPIC |
| `~/.cache/caco/ports/` | Built sourceport prefixes, rebuilt from the recipe |
| `~/.cache/caco/ports-src/` | Sourceport checkouts and build trees |

The WAD cache is usually the largest thing caco stores and is entirely
re-downloadable, which is why it lives on the disposable side. If you are
upgrading from a version that kept it at `~/.local/share/caco/wads/`, caco
moves it for you on first launch and updates `cache_dir` to match. A `cache_dir`
you set yourself is left alone.

Override the cache root with `CACO_CACHE_HOME`, or just the WAD cache with
`CACO_CACHE_DIR`.

### Recovering from a bad migration

Each time caco starts with pending schema migrations it first copies the live
library to `~/.local/share/caco/backups/pre-migration-<N>.db`, where `<N>` is
the schema version before the migration runs. Migrations themselves are
transactional, so a crash or SQL error rolls back cleanly; the file-level
snapshot is for the rarer case where a migration commits successfully but
leaves user data in a bad state. To recover:

```bash
# stop any running caco instances, then:
cp ~/.local/share/caco/library.db ~/.local/share/caco/library.db.broken
cp ~/.local/share/caco/backups/pre-migration-<N>.db ~/.local/share/caco/library.db
```

You will need a caco binary old enough to read schema version `N`. Please also
file a bug report with the broken DB attached.

## Supported Sourceports

Caco recognises six sourceport families. Family membership determines which per-WAD features caco can inject at launch time:

| Family | Members | Data dir | Save dir | Complevel |
|--------|---------|----------|----------|-----------|
| `dsda` | dsda-doom, nyan-doom, nugget-doom, prboom+, glboom+ | yes | yes | yes |
| `woof` | woof | yes | yes | yes |
| `zdoom` | uzdoom, gzdoom, lzdoom, vkdoom, qzdoom, zdoom | no | yes | — |
| `chocolate` | chocolate-doom, crispy-doom | no | yes | — |
| `eternity` | eternity | no | yes | — |
| `helion` | helion | no | yes | — |

Unknown sourceports still launch, they just skip isolation and auto-injection.

### Building ports from source

Not every port is packaged — nyan-doom and uzdoom are in no distro repo — and
requiring a global install undercuts the point of a portable library. The
**Ports** dialog clones, builds and installs a port into caco's own prefix,
and caco launches it from there without anything being installed
system-wide. A managed build wins over a same-named binary on `PATH`.

What travels between machines is the **recipe**, not the binary: a few
hundred bytes of TOML that rebuild the port on whatever machine and OS it
lands on. That is why recipes live in the portable data dir while the built
prefixes live in the cache with the WAD downloads — deleting the cache costs
a rebuild, never a reconfiguration.

Before building, caco checks the toolchain and asks your package manager
(`pacman -T` or `brew list`) which dependencies are missing, and shows the
exact install command. Missing packages warn rather than block: caco cannot
tell an optional dependency from a required one, so cmake stays the
authority.

Recipes for `nyan-doom` and `uzdoom` ship built in. To pin a ref, add build
flags or attach patches, drop a file in `~/.local/share/caco/ports/` — any
`*.toml` there is merged over the built-ins by name:

```toml
# ~/.local/share/caco/ports/mine.toml
[uzdoom]
repo = "https://github.com/UZDoom/uzdoom"
ref = "v1.0.0"                        # a branch or tag, not a bare commit
binary = "uzdoom"
patches = ["uzdoom-tweak.patch"]      # relative to this directory

[uzdoom.build]
system = "cmake"
generator = "Ninja"
args = ["-DCMAKE_BUILD_TYPE=Release", "-DINSTALL_PK3_PATH=bin"]

[uzdoom.deps]
arch = ["cmake", "ninja", "openal", "sdl2-compat", "libwebp", "bzip2", "libvpx", "zlib"]
brew = ["cmake", "ninja", "openal-soft", "sdl2", "webp", "bzip2", "libvpx"]
```

`-DINSTALL_PK3_PATH=bin` on uzdoom is load-bearing. Its default install puts
the pk3s in `share/games/uzdoom` while the binary looks beside itself, so a
stock build succeeds and then aborts at launch with `Cannot find uzdoom.pk3`.
Do not drop it when overriding the recipe.

A recipe's name must match a sourceport caco knows (see the family table
above), or the build works but complevel args, save directories and config
profiles quietly stop applying.

#### Update checks

Since a recipe usually tracks a branch, "is there a new version" means "does
the remote ref still point at the commit we built". At startup caco asks each
built port's remote exactly that, with one `git ls-remote` per port — no
objects are fetched — and shows a notification if any has moved. The Ports
dialog marks them `update available`; rebuilding is always your call.

The answer is cached and checked at most once a day, so most launches do no
network at all. Set `port_update_check_days = 0` (or the Settings field) to
turn it off entirely.

Per-map stat tracking (which feeds completion detection and progress bars) works with:

- **dsda / woof** — native `stats.txt` / `levelstat.txt` in the per-WAD data dir.
- **zdoom** — caco injects a small ZScript reporter PK3 plus `+logfile`, then converts the log into a managed `stats.txt` after each session.
- **helion** — caco passes `-levelstat` and consumes Helion's global `levelstat.txt` (from `~/.config/Helion`) into the WAD's managed `stats.txt` after each session.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The sourceport build driver has an end-to-end test that clones and compiles a
real port, kept out of the default run:

```bash
cargo test -p caco-core --test ports_build -- --ignored --nocapture
```

## License

MIT
