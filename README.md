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
- **On-demand downloads** — idgames WADs are cached when you play, with configurable auto-cleanup. WADs that cannot be re-downloaded are kept on the portable side instead, and destructive actions leave them alone by default.
- **Cacowards browser** — a magazine-style year-by-year view of Doomworld's awards, doubling as an import queue.
- **Garbage collection** — reclaim disk space from completed / abandoned WADs, reviewing every file before it goes.
- **Smart collections** — save queries by name and pin them to the sidebar.

## Installation

### Arch Linux (recommended)

Every tagged release attaches a prebuilt `x86_64` package to its
[GitHub Release](https://github.com/mythofmeat/caco/releases):

```bash
gh release download --repo mythofmeat/caco --pattern '*.pkg.tar.zst'
sudo pacman -U caco-*.pkg.tar.zst
```

To build the package yourself instead:

```bash
git clone https://github.com/mythofmeat/caco && cd caco/contrib/arch
makepkg -si
```

`makepkg` compiles the working tree it sits in, so a local build reports version
`0.0.0` — the real version is injected from the release tag in CI.

### macOS

Releases also carry an `arm64` app bundle, built and ad-hoc signed on a macOS
runner:

```bash
gh release download --repo mythofmeat/caco --pattern '*-macos-arm64.zip'
unzip -o caco-*-macos-arm64.zip -d /Applications
```

Download it with `gh` or `curl` rather than a browser. Browsers attach the
`com.apple.quarantine` attribute and Gatekeeper then refuses to open an app
that is signed but not notarised; command-line downloads do not set it. If you
do end up quarantined, `xattr -dr com.apple.quarantine /Applications/Caco.app`
clears it.

To build the bundle yourself on a Mac:

```bash
cargo build --release -p caco
VERSION=0.0.0 contrib/macos/bundle.sh    # writes dist/Caco.app
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
| Cache | Managed WAD files; bulk clear skips anything that cannot be re-downloaded |
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
retrievable:manual              WADs caco cannot re-download on its own
```

**Fields:** `id`, `title`, `author`, `year`, `filename`, `tag`, `status`, `source`, `iwad`, `complevel`, `config`, `cacoward`, `retrievable`, `avail`

**`retrievable:`** splits the library by whether caco can fetch the file
again unattended. `retrievable:automatic` (`auto`, `a`) matches WADs on
idgames; `retrievable:manual` (`m`) matches everything else — a Doomworld
thread link or a one-off file host is not something caco can promise to
re-download, so those copies are worth keeping.

**`avail:`** is the other half of the picture — whether the file is on this
machine right now. `avail:cached` has a local copy, `avail:downloadable` has a
URL to try, `avail:unavailable` has neither.

**`retrievable:lost`** is the intersection worth watching: manual-only *and* no
local copy, meaning caco cannot produce the WAD and neither can you without
going and finding the file again. Those rows are flagged in the library, and a
chip beside the filter bar shows the count whenever it is not zero.

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

Config file: `config.toml` in the data directory (see Data Storage) — beside the
database rather than in `~/.config`, so the portable set is one directory. It is
app-managed state that happens to be readable: the settings dialog and first-run
detection write it far more often than you will. The Settings dialog edits the
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

Caco splits its files by whether they can be regenerated. The **data**
directory is worth carrying between machines; the **cache** directory is
disposable and rebuilds itself on demand. Both follow platform convention:

| | Linux | macOS |
|---|---|---|
| Data | `$XDG_DATA_HOME/caco`, else `~/.local/share/caco` | `~/Library/Application Support/caco` |
| Cache | `$XDG_CACHE_HOME/caco`, else `~/.cache/caco` | `~/Library/Caches/caco` |

Paths in the two tables below are relative to those roots.

**Keep these** — copy the data directory to move your library to another
machine:

| Path | Contents |
|------|----------|
| `config.toml` | Configuration |
| `gui-state.json` | GUI view/sort/filter state |
| `library.db` | Library database |
| `data/` | Per-WAD saves, stats, configs |
| `wads/` | WAD files caco cannot re-download |
| `iwads/` | Managed IWADs |
| `id24/` | Managed id24 WADs |
| `companions/` | Managed companion files |
| `sourceports/` | Per-sourceport config profiles |
| `ports/` | Sourceport build recipes and their patches |
| `backups/` | Save backups + pre-migration DB snapshots |

**Disposable** — safe to delete at any time:

| Path | Contents |
|------|----------|
| `wads/` | Cached WAD files, re-downloaded on demand |
| `thumbnails/` | Thumbnail cache, re-extracted from TITLEPIC |
| `ports/` | Built sourceport prefixes, rebuilt from the recipe |
| `ports-src/` | Sourceport checkouts and build trees |

Which of the two `wads/` directories a file lands in is decided by whether caco
can fetch it again on its own, and nothing else. Anything on idgames goes to the
cache, where it is fair game for cleanup because losing it costs a download.
Everything else — forum attachments, one-off file hosts, files you linked by
hand — goes to the portable side and stays until you delete it deliberately.
That is what makes copying the data directory to another machine actually carry
your library: see `retrievable:` under Queries.

Override the cache root with `CACO_CACHE_HOME`, or just the WAD cache with
`CACO_CACHE_DIR`.

### Recovering from a bad migration

Each time caco starts with pending schema migrations it first copies the live
library to `backups/pre-migration-<N>.db` in the data directory, where `<N>` is
the schema version before the migration runs. Migrations themselves are
transactional, so a crash or SQL error rolls back cleanly; the file-level
snapshot is for the rarer case where a migration commits successfully but
leaves user data in a bad state. To recover:

```bash
# stop any running caco instances, then:
data="${XDG_DATA_HOME:-$HOME/.local/share}/caco"   # macOS: ~/Library/Application\ Support/caco
cp "$data/library.db" "$data/library.db.broken"
cp "$data/backups/pre-migration-<N>.db" "$data/library.db"
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
flags or attach patches, drop a file in `ports/` inside the data directory — any
`*.toml` there is merged over the built-ins by name:

```toml
# <data dir>/ports/mine.toml
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
