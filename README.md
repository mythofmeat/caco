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
- **Content identity** — every WAD file is hashed, along with the WADs inside a zip, so caco notices when a file has changed since you last played it and asks before launching it: saves and demos made with one version of a WAD often fail with another.
- **Cacowards browser** — a magazine-style year-by-year view of every WAD Doomworld has awarded since 2004, doubling as an import queue. The list ships with caco, so it works offline and needs no site to be up.
- **Garbage collection** — reclaim disk space from completed / abandoned WADs, reviewing every file before it goes.
- **Smart collections** — save queries by name and pin them to the sidebar.

## Installation

Each [GitHub release](https://github.com/mythofmeat/caco/releases) carries a
prebuilt Arch package for x86_64, a Fedora package for aarch64 (Asahi
Linux) and a macOS app (Apple silicon). Otherwise caco is built from source.

### From source

```bash
git clone https://github.com/mythofmeat/caco && cd caco
cargo install --path crates/caco
```

### Arch Linux

Install the `.pkg.tar.zst` from a release with `pacman -U`, or build it:

```bash
git clone https://github.com/mythofmeat/caco && cd caco/contrib/arch
makepkg -si
```

`makepkg` clones the tag matching the PKGBUILD's `pkgver`, so it builds the
last release rather than the working tree.

### Fedora

Install the `.rpm` from a release with `sudo dnf install ./caco-*.rpm`. It is
built on Fedora 43 and installs on 43 and anything newer. To build one
yourself, with Rust from [rustup](https://rustup.rs):

```bash
sudo dnf install rpm-build gcc desktop-file-utils
git clone https://github.com/mythofmeat/caco && cd caco
version=$(sed -n 's/^Version: *//p' contrib/fedora/caco.spec)
git archive --prefix="caco-$version/" -o "caco-$version.tar.gz" HEAD
rpmbuild -bb contrib/fedora/caco.spec --define "_sourcedir $PWD"
```

Unlike `makepkg`, this packages the checked-out commit. The package lands
under `~/rpmbuild/RPMS/`.

### macOS

```bash
brew tap mythofmeat/caco https://github.com/mythofmeat/caco
brew install --cask mythofmeat/caco/caco
```

The repository is its own tap. The cask installs the prebuilt `Caco.app` from
the latest release into `/Applications` and links `caco` onto your `PATH`;
nothing is compiled. The app is signed ad hoc rather than notarised, so the
cask clears its quarantine flag on install. Apple silicon only.

### Versioning

The version shown in the sidebar and the About dialog is the workspace
`version` in the root `Cargo.toml`. Releases are cut by the **release** workflow
in the Actions tab, which bumps it — see `CLAUDE.md` for what it does.

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
  don't own yet are flagged `absent` with an inline Import button. A WAD imported
  any other way links itself to its entry, by file hash, idgames id, Doomworld
  thread, Doom Wiki page or an exact title match. A year's total counts the winners,
  runners-up, honorable mentions and Mordeth award; its other awards are listed
  under them
- Stats view — headline totals, a status breakdown, and month-by-month activity
- Keyboard shortcuts: `j/k`, `g`/`G`, `Home`/`End`, `Enter`, `E`, `D`, `S`, `M`, `F`, `P`, `Esc`

### Dialogs

| Sidebar | What it does |
|---------|--------------|
| Storage | Everything caco keeps on disk, in four tabs — see below |
| Profiles | Per-sourceport config profiles, edited in place |
| IWADs | Registered IWADs and id24 resources |
| Sourceports | Build sourceports from source into caco's own prefix |
| Enrich | Re-run complevel / IWAD / sourceport detection across the library |
| Settings | Sourceports, launch args, behavior toggles, cache limits, paths |

Storage tabs:

| Tab | What it does |
|-----|--------------|
| Cache | Downloaded WAD files; bulk clear skips anything that cannot be re-downloaded |
| Clean | Reclaim disk space, reviewing every file first |
| Trash | Restore or permanently delete removed WADs |
| Files | Companion file registry: every managed file, the WADs using it, and orphan cleanup |

Per-WAD dialogs come from the context menu or a shortcut: **Edit** (`E`),
**Sessions** (`S`), **Map Stats** (`M`), **Saves & Demos** (`F`). Edit's
Sources tab shows the WAD's file: where it is (or was last seen), its size,
SHA-256 and SHA-1, the WADs inside it, and any other WAD in the library that is
the same file.

## Queries

The filter bar uses beets-style query syntax:

```
scythe                          Free text search
title:scythe author:alm         Field queries (AND)
status:in-progress , status:unplayed    OR queries
^status:completed               Negation
tag:caco*                       Glob patterns
cacoward:2023                   WADs that won something in a year
cacoward:doom-moma              WADs given a particular award
retrievable:manual              WADs caco cannot re-download on its own
hash:a9993e36                   A WAD by any hash of its file, or the start of one
dup:yes                         WADs that are in the library twice
```

**Fields:** `id`, `title`, `author`, `year`, `filename`, `tag`, `status`, `source`, `iwad`, `complevel`, `config`, `cacoward`, `retrievable`, `avail`, `hash`, `dup`

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
going and finding the file again.

Both rely on caco knowing whether a file is really here, so it checks every
recorded file when it starts and again before each launch. A file that has gone
— deleted by hand, on a drive that isn't mounted, or recorded by a data directory
synced from a machine that has it — stops counting as cached, and caco remembers
where it was last seen: Play says where, and the next check that finds it there
again (or under the same name in either `wads/` directory) picks it back up.

**`hash:`** matches a WAD by any hash caco recorded for its file, or the start of
one: the file's SHA-256 or SHA-1, or those of a WAD inside it. Wad Archive's
index on archive.org is keyed by the SHA-1 of the WAD inside a zip, so a hash
from there finds the WAD it names.

**`dup:yes`** lists WADs whose files play the same as another WAD's in the
library: the same WAD imported twice from two places. A zip and the WAD extracted
from it count as the same, since what plays is the WAD.

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

A site can answer with a WAF challenge instead of the page caco asked for. The
error then names the site and the WAF. Retrying may not help, but you can still
add the WAD by hand with the URL or local form.

## Cacowards

Every WAD Doomworld has awarded since 2004 ships with caco, so the Cacowards view
needs no site to be up. The write-ups stay on Doomworld, linked from each year. A WAD
you import links itself to its entry by the strongest evidence it has: the file's hash,
an idgames id, a Doomworld thread, a Doom Wiki page, or failing those a title that
only one WAD in your library has. Anything else takes one right-click on the card.

The list covers what you can play. Awards to people and to tools (Creator of the
Year, the Espi, the Codeaward) aren't on it, and only the winners, runners-up,
honorable mentions and the Mordeth award count towards a year's total, since a
deathmatch map has no ending to reach.

To correct an entry or add your own, drop a `*.toml` file in `cacowards/` inside the
data directory. An entry repeating a built-in `key` replaces it, and any other adds
to the list. caco reads these at startup, and says in the status bar if one has a
mistake in it:

```toml
# <data dir>/cacowards/mine.toml
year = 2024

[[entry]]
key       = "2024-runner-up-tnt-overcharged"
category  = "runner-up"
order     = 12                  # where it falls in the category
title     = "TNT: Overcharged"
author    = "Veemon"
idgames   = 19951
# Also: doomworld = 151858 (forum topic), doomwiki = "TNT:_Overcharged" (page),
# sha256 = "...", and supported = false for what no sourceport can run.
```

Your links and the card menu's "Mark as unsupported" outlive any update to the list.

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
sourceport_update_check_days = 1        # 0 = never check built sourceports for updates

# Extra args for specific executables only (appended after sourceport_args)
[executable_args]
nyan-doom = ["-geometry", "1920x1200"]

[list]
format = ["id", "title", "author", "status", "beaten", "playtime", "last_played"]
sort = "id+"

[gui]
default_view = "list"
thumbnail_size = 160
```

Caco writes only the settings that differ from their defaults, so `config.toml` stays short. Every option and its default is defined on the `Config` struct in `crates/caco-core/src/config.rs`; most are also editable from the Settings dialog.

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
| `profiles/` | Per-sourceport config profiles |
| `sourceports/` | Sourceport build recipes and their patches |
| `cacowards/` | Your corrections and additions to the Cacowards list |
| `backups/` | Save backups + pre-migration DB snapshots |

**Disposable** — safe to delete at any time:

| Path | Contents |
|------|----------|
| `wads/` | Cached WAD files, re-downloaded on demand |
| `thumbnails/` | Thumbnail cache, re-extracted from TITLEPIC |
| `sourceports/` | Built sourceport prefixes, rebuilt from the recipe |
| `sourceports-src/` | Sourceport checkouts and build trees |

Which of the two `wads/` directories a file lands in is decided by whether caco
can fetch it again on its own, and nothing else. Anything on idgames goes to the
cache, where it is fair game for cleanup because losing it costs a download.
Everything else — forum attachments, one-off file hosts, files you linked by
hand — goes to the portable side and stays until you delete it deliberately.
That is what makes copying the data directory to another machine actually carry
your library: see `retrievable:` under Queries. A file that did not make the trip
shows up as not on this machine, with where it was last seen, rather than as
cached.

`library.db` also records what each WAD file is: its size, SHA-256 and SHA-1, the
same for every WAD, PK3 or DEH inside a zip, and what each play session loaded.
That is how caco tells that a file has changed since you last played it, whether
it was re-downloaded after an author's fix or replaced by a sync, and asks before
launching it. A zip re-packed around the same WAD does not count as a change.
caco hashes a file when it first sees it, and again only when the file's size or
modification time has moved. The first start after upgrading reads every file
once, in the background. The hashes stay after the file goes, because they are
what identifies the file when you go looking for it.

Override the cache root with `CACO_CACHE_HOME`, or just the WAD cache with
`CACO_CACHE_DIR`.

### Recovering from a bad migration

Each time caco starts with pending schema migrations it first copies the live
library to `backups/pre-migration-<N>.db` beside it, which is in the data directory
unless you have moved `db_path`. `<N>` is the schema version before the migration
runs. Migrations themselves are transactional, so a crash or SQL error rolls back
cleanly; the file-level snapshot is for the rarer case where a migration commits
successfully but leaves user data in a bad state. To recover:

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

### Building sourceports from source

Not every sourceport is packaged — nyan-doom and uzdoom are in no distro repo — and
requiring a global install undercuts the point of a portable library. The
**Sourceports** dialog clones, builds and installs one into caco's own prefix,
and caco launches it from there without anything being installed
system-wide. A managed build wins over a same-named binary on `PATH`.

Use **Check versions** in that dialog to lazily ask the sourceport's git remote for
its tags (treated as releases) and branches. Nothing is fetched when the dialog
opens. Once checked, **Build from** can target the latest release, a specific
release, or a development branch such as `main`, `master`, or `trunk`; the
choice applies to that build and does not rewrite the recipe. Caco also
performs a lightweight, throttled startup check for new commits on the ref
already being tracked, as controlled by `sourceport_update_check_days`.

What travels between machines is the **recipe**, not the binary: a few
hundred bytes of TOML that rebuild the sourceport on whatever machine and OS it
lands on. That is why recipes live in the portable data dir while the built
prefixes live in the cache with the WAD downloads — deleting the cache costs
a rebuild, never a reconfiguration.

Before building, caco checks the toolchain and asks your package manager
which dependencies are missing — `pacman -T` on Arch, `rpm -q` on Fedora,
`brew list` on macOS, and the same on distributions derived from Arch or
Fedora — and shows the exact install command. Elsewhere it checks only the
toolchain. Missing packages warn rather than block: caco cannot tell an
optional dependency from a required one, so cmake stays the authority.

Recipes for `nyan-doom` and `uzdoom` ship built in. To pin a ref, add build
flags or attach patches, drop a file in `sourceports/` inside the data directory — any
`*.toml` there is merged over the built-ins by name:

```toml
# <data dir>/sourceports/mine.toml
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
arch = ["cmake", "ninja", "openal", "sdl2-compat", "libwebp", "bzip2", "libvpx", "zlib", "glib2"]
fedora = ["gcc-c++", "cmake", "ninja-build", "openal-soft-devel", "sdl2-compat-devel",
          "libwebp-devel", "bzip2-devel", "libvpx-devel", "zlib-ng-compat-devel", "glib2-devel"]
brew = ["cmake", "ninja", "openal-soft", "sdl2-compat", "webp", "bzip2", "libvpx", "glib"]
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
built sourceport's remote exactly that, with one `git ls-remote` each — no
objects are fetched — and shows a notification if any has moved. The
**Sourceports** dialog marks them `update available`; rebuilding is always your call.

The answer is cached and checked at most once a day, so most launches do no
network at all. Set `sourceport_update_check_days = 0` (or the Settings field) to
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
real sourceport, kept out of the default run:

```bash
cargo test -p caco-core --test sourceports_build -- --ignored --nocapture
```

## License

MIT
