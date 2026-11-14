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
- **Get through your browser** — for a WAD behind a site that turns caco away, Get opens its page in your browser and watches your Downloads folder. When the file lands, caco asks if that's the one, then files it, hashes it and offers it to your mirror.
- **Mirrors** — your own store of WAD files, a directory or an HTTP file server, for what idgames does not have. caco looks there first and offers to upload what it could not otherwise get back, so a mirror fills itself as you play.
- **A local index of idgames** — every upload on the archive, searchable by title, author, file name and description, with the newest listed first, and none of it behind Doomworld's Cloudflare. Point caco at your own copy of the archive and every WAD on idgames is instant and offline.
- **Content identity** — every WAD file is hashed, along with the WADs inside a zip, so caco notices when a file has changed since you last played it and asks before launching it: saves and demos made with one version of a WAD often fail with another.
- **Cacowards browser** — a magazine-style year-by-year view of every WAD Doomworld has awarded since 2004, doubling as an import queue. The list ships with caco, so it works offline and needs no site to be up.
- **Several machines** — play on a desktop and a laptop between syncs and nothing is lost. Each machine keeps a log of what happens on it, in plain files that can't conflict, so git, Syncthing or a shared drive can carry them, and every machine merges them into the same library. See [Several Machines](#several-machines).
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

The version shown in the Help dialog (`F1`) is the workspace
`version` in the root `Cargo.toml`. Releases are cut by the **release** workflow
in the Actions tab, which bumps it — see `CLAUDE.md` for what it does.

## Quick Start

Launch `caco` (or pick it from your application menu), then:

1. **Settings** in the sidebar — set your sourceport and IWAD directories. caco
   adopts one it finds on its own, and an empty library says if it could not.
2. **IWADs** in the sidebar — register your IWADs. Drop in a directory and caco
   identifies each file by MD5.
3. **Import** in the sidebar — search idgames or the Doom Wiki, paste a
   Doomworld thread URL, or point at a local file, or drop one on the window (not under
   Wayland, which the windowing library can't take drops from yet).
4. Select a WAD and press **Enter** to play. On first launch caco detects the
   required IWAD and complevel from the WAD file itself.

Play time, sessions, and per-map stats are recorded automatically once you quit
the sourceport. dsda-doom and its relatives keep per-map stats themselves; for the
zdoom family caco loads a small mod that writes each map you finish to a log, and
for Helion it reads the sourceport's own level stats. A zdoom-family save game
remembers which maps of that game were finished too, so caco reads your latest one
around every session and counts whatever nothing else did. If uzdoom leaves no log,
the status bar says so rather than leaving progress quietly uncounted.

## The Interface

- Grid and list views, sorted by what you played last unless you say otherwise, or by whichever
  column heading you click in the list, and a live filter bar whose search can be kept as a
  collection with one click
- Details beside the library for the selected WAD: its picture, description, tags, awards, notes,
  how far through it you are, where its file is, and buttons for all of what can be done with it,
  with status and rating changed right there. `I` puts them away, and a window too small for both
  opens them in a dialog
- WAD thumbnails scraped from the Doom Wiki (or extracted from TITLEPIC) with on-disk caching
- Right-click context menu (play, details, status, rating, sessions, map stats, saves & demos, edit, delete, new playthrough, and get in browser for a WAD caco can't fetch)
- A status bar that says how many WADs are showing, and what there is to press. What it tells you
  stays until it could have been read, an error until you dismiss it, and it offers what to do about
  it: Undo after a delete, Show after an import
- Continue Playing — the WAD in progress you played last, in a banner that collapses to a
  line in a short window
- Cacowards view — a year-by-year list of Doomworld's awards, with posters a click away,
  where entries you don't have yet have an Import button, and a Get button for those that
  aren't on idgames. Hide Finished leaves what is left to play. A WAD imported
  any other way links itself to its entry, by file hash, idgames id, Doomworld
  thread, Doom Wiki page or an exact title match. A year's total counts the winners,
  runners-up, honorable mentions and Mordeth award; its other awards are listed
  under them
- Stats view — totals, playtime and finishes by month, how far through each year's Cacowards
  you are, and your most played authors, sourceports, IWADs and complevels. A status, an author or a
  year goes to what it counts
- Keyboard: `j/k` and the arrows, `Page Up/Down`, `g g`/`G`, `Home`/`End`, `Enter`, `I`, `E`,
  `D`/`Delete`, `S`, `M`, `F`, `P`, `Esc`, `/` to search, `Ctrl+1`-`4` for the views, `Ctrl+Enter` for
  a dialog's main button, and `?` or `F1` for the list of them, and how to write a filter

### Dialogs

| Sidebar | What it does |
|---------|--------------|
| Storage | Everything caco keeps on disk, in four tabs — see below |
| Profiles | Per-sourceport config profiles, edited in place |
| IWADs | Registered IWADs and id24 resources |
| Sourceports | Build sourceports from source into caco's own prefix |
| Enrich | Fill in the complevel, IWAD and sourceport of WADs that don't have them |
| Mirrors | Your own stores of WAD files: set one up, fill it, change it — see [Mirrors](#mirrors) |
| Sync | Where the logs are, and which machines this library has heard from — see [Several Machines](#several-machines) |
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
Sources tab shows the WAD's file: where it is (or was last seen), which of your
mirrors have it, its size, SHA-256 and SHA-1, the WADs inside it, and any other
WAD in the library that is the same file. It can also upload the file to a mirror
that does not have it.

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
idgames or on one of your [mirrors](#mirrors); `retrievable:manual` (`m`) matches
everything else — a Doomworld thread link or a one-off file host is not something
caco can promise to re-download, so those copies are worth keeping.

**`avail:`** is the other half of the picture — whether the file is on this
machine right now. `avail:cached` has a local copy, `avail:downloadable` has a
URL or a mirror to try, `avail:unavailable` has neither.

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

The Import view has a tab for each way in: searching idgames or the Doom Wiki, a
Doomworld thread, a link to a file, or a file you have, which can also be dropped
anywhere on the window, except under Wayland. WAD files that turn out to be IWADs are registered as IWADs
rather than library entries.

Non-Doomwiki imports are auto-enriched with Doom Wiki metadata (author, year,
description, IWAD). A search result already in your library has a check mark and a
button to go to it, and anything else that turns out to be there already says so.

A site can answer with a WAF challenge instead of the page caco asked for. The
error then names the site and the WAF. Retrying may not help, but your browser will
get through: see [Getting a WAD through your browser](#getting-a-wad-through-your-browser).
You can also add the WAD by hand with the URL or local form.

### The idgames index

The files on idgames come from several mirrors, so caco goes on to the next if one is down
or turns it away. What is known *about* them comes from one place, `www.doomworld.com`,
which sits behind Cloudflare. So caco keeps its own index of the archive, and the idgames
tab searches that: title, author, file name and description, any part of a word, ignoring
case and accents. Opening the tab shows what is newest on the archive.

It builds itself in the background, starting the first time you run this version:

1. **The listing.** Every mirror but one publishes `ls-laR.gz`, a listing of the whole
   archive. caco asks for it at most once a day, and it costs one empty answer unless it
   has changed. From the moment the first one lands, about 21,000 uploads can be found by
   file name and fetched.
2. **The API, for ids.** One call per directory the first time, about 300 at one a
   second, so ten minutes or so. After that, at most one call per new upload, and up to
   five a day to keep ratings current. That same answer has each upload's title, author,
   description and rating.
3. **The text files.** The file beside each upload says what it is, in the archive's own
   template. They are read off a mirror, at most four a second, for whatever the API did
   not cover: the first run reads a little over 2,000 of them, and only what the listing
   says is new or changed after that.

The API is allowed to fail. On a day Cloudflare has closed that door, all of it comes from
the text files instead: about 21,000 requests the first time, or a few hours of caco being
open, and it picks up where it left off if you close it. Until the index has
caught up, a search asks the API as well, and if it can't, says that this is only what
the index has so far. An upload with no id yet can still be imported and played, and gets
its id when the index learns it.

The line above the search box says how far along it is, with **Update** to ask for a new
listing now and **Stop** while a run is going. Set "Update the idgames index every (days)"
to 0 in Settings, or `idgames_index_check_days = 0` in `config.toml`, and caco asks nothing
of anyone at startup. The index lives in `idgames-index.db` on the cache side, as it can
all be fetched again. Delete it and it is rebuilt.

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

## Getting a WAD through your browser

Most of what idgames doesn't have lives on forum threads and share links, behind
challenges that stop caco and not your browser. **Get** leaves the download to your
browser and everything after it to caco:

1. Press **Get** on a Cacoward entry with no WAD yet, or **Get in Browser** in a WAD's
   right-click menu or in what Play shows for a file that isn't here. caco opens the
   best page it knows: a link straight to the file if the WAD's source gave one, then a
   share link, then the forum thread or wherever else the WAD came from.
2. Download it the way you normally would. The status bar says what caco is waiting
   for, and clicking it lists the other pages it knows, in case the first was a dead
   link. It can also stop waiting from there.
3. When a `.wad`, `.pk3`, `.pk7`, `.zip` or `.7z` finishes arriving in your Downloads
   folder, caco asks in the corner of its window: **Link `AftercareSessions.wad` to
   Aftercare Sessions?** The question doesn't get in the way of anything else you're
   doing. If several WADs are waiting, it asks about the one whose name looks closest
   first, and **Not This One** moves on to the next, or leaves the file alone.
4. **Link** files it the way caco files any WAD, hashes it, and offers it to a writable
   [mirror](#mirrors). A zip is kept as it is, like one from idgames. A Cacoward entry
   with no WAD gets one at this point, made from the award's own details, so it works
   on a day the forum won't talk to caco.

Linking doesn't start the game, since you may have asked for several. caco only looks
for an hour after you press Get, and only touches a file you said to link.

Two settings matter here. **Downloads folder** is where caco looks, and is worth setting
if your browser runs in a sandbox such as Flatpak and saves somewhere else. **Import link
mode** says whether the file is moved out of that folder, which is the default, or
copied. A file you pick yourself with **Link Local File** is always copied, because it
can be anywhere, including somewhere it's meant to stay.

## Mirrors

A large share of recent Cacoward entries never reach idgames, and the share links on
their forum threads rot. Keeping a file in the data directory only protects it for as
long as that directory has it. A mirror is your own store of WAD files, which any
machine with your library can get them back from.

caco ships no mirror and never will. Many WAD text files restrict redistribution, and
while a private archive of files you downloaded is your business, a default URL in
caco would be caco's.

### Setting one up

Everything happens in the **Mirrors** tool, with nothing to edit by hand. The first time
it opens on a wizard, which asks where you want to keep it and says what to do for that
kind of store:

| Kind of store | What it needs | caco does the rest |
|---|---|---|
| **A folder**, such as a NAS mount, an external drive or a synced folder | A path | Creates the folder if it isn't there |
| **Gitea**'s package registry | The server, an owner, and a token with `package` set to Read and Write | Opens Gitea's token page for you, and the package appears by itself |
| **WebDAV**: Nextcloud, a Synology or QNAP NAS, Apache, nginx, or anything else that speaks it | The address of a folder, and a username and password if it has a login | Creates the folder on the server. There is a one-line `rclone` command to copy if you have no server |
| **A web server**, read only | The address of a folder something else fills | Nothing to do: it only reads |

**Test It** tries the mirror before anything is saved: it reads the index and, if caco may
upload, writes a small file and removes it again. If that fails it says why and what to
check. A mirror that is down right now can be added anyway. **Finish** makes it ready, then
offers to upload what you already have: the WADs with a file on this machine that nothing
else could give you back, listed before anything goes anywhere.

From then on the Mirrors list shows what each one holds, says when one can't be read, and
can add more, edit, reorder, remove or fill them. Removing a mirror deletes nothing on it.

Tokens and passwords you paste in are saved in `~/.config/caco/mirror-tokens.toml` on
Linux and `~/Library/Preferences/caco/mirror-tokens.toml` on a Mac, readable only by you.
They are never written to the data directory, which you may copy or keep in git, so a
second machine asks for its own. If you would rather not save one at all, the wizard can
read it from an environment variable instead, which has to be set wherever caco starts: a
desktop launcher often does not see what a shell's rc file exports.

### What a mirror does

**Fetching.** Play looks on your mirrors before anywhere else for a WAD that is not on
this machine, in the order they are listed. A download is checked against the size and
SHA-256 in the mirror's index, so a corrupt or replaced file never reaches the
sourceport. If it fails, Play falls back to idgames for a WAD that is on it, and
otherwise says what went wrong.

**Uploading.** When you link a file, by hand or through Get, or import one for a WAD
caco could not otherwise get back, it offers to upload it to each writable mirror, after
the game closes if linking launched one. **Upload** in Edit's Sources tab does the same for any one WAD.

**Which WADs a mirror has.** caco reads each mirror's index when it starts and when you
ask it to. It finds a WAD's file by hash if the WAD has one: the same file, or the
same WAD in a different zip. A WAD with a hash goes by nothing else, so a mirror holding
a newer release is not a silent upgrade. A WAD that has never had a file goes by idgames
id, Cacoward, Doomworld thread or Doom Wiki page, and the newest upload wins.

A WAD a mirror holds counts as one caco can fetch, like one on idgames: it stops being
lost, a new copy goes in the cache, and Clean and Cache treat its file as re-downloadable.
A mirror that does not answer at startup keeps counting for what it held the last time,
with a warning in the status bar, so that a laptop away from home does not see half its
library go manual. So if a mirror is gone for good, remove it.

### In `config.toml`

The wizard writes this, and it can be edited by hand like anything else there:

```toml
[[mirrors]]
name      = "home"
url       = "https://files.example.lan/doom"   # or: path = "/mnt/nas/doom-mirror"
username  = "doom"                             # for a store that wants a login
token_env = "CACO_MIRROR_TOKEN"                # read the token or password from here
write     = true                               # caco may upload
```

A Gitea mirror's `url` is `https://gitea.example.com/api/packages/<owner>/generic/<name>`.

### A copy of idgames

All of idgames is about 64 GB, which is not much of a disk. Copy it there, tell caco where
it is, and every WAD on idgames is instant and offline. **Copy of idgames** in the wizard
says how, and tests that it is the right folder:

```bash
rsync -rt --exclude incoming --exclude newstuff \
    rsync://mirror.braindrainlan.nu/pub/idgames/ /mnt/nas/idgames/
```

Or from any mirror's web pages, with [rclone](https://rclone.org):

```bash
rclone copy --exclude 'incoming/**' --exclude 'newstuff/**' \
    --http-url https://youfailit.net/pub/idgames :http: /mnt/nas/idgames
```

Run it again now and then to catch up. In `config.toml` it is a mirror with one more line,
and `url` works too, for a server sharing that folder:

```toml
[[mirrors]]
name   = "idgames"
path   = "/mnt/nas/idgames"
layout = "idgames"
```

It is not like the mirrors above. caco did not make it and never writes to it, so it has
no index, and nothing is uploaded to it. A file goes by its path in the archive: Play
copies it into the cache like any download, which keeps cleaning the cache from ever
reaching your copy, and it falls back to the public mirrors for anything that isn't there.
So a copy of part of the archive, such as just `levels/`, is fine. The local index reads
its listing and text files from here too, if `ls-laR.gz` came along with the rest, so
building it asks nothing of any mirror.

### Layout

```text
<root>/index/index.json
<root>/<sha256>/<filename>
```

The index is an ordinary JSON file, so a mirror can be filled by hand or by a script.
Only the first three fields of an entry are required, and caco leaves anything it does
not understand in place when it rewrites the file:

```json
{
  "version": 1,
  "files": [
    {
      "sha256": "9a3f…",
      "size": 48211034,
      "filename": "lonesome-road.zip",
      "content_sha256": "c10b…",
      "title": "Lonesome Road",
      "author": "Thomas Seifert (tourniquet)",
      "doomworld": 151858,
      "doomwiki": "Lonesome Road",
      "cacowards": ["2025-winner-lonesome-road"],
      "added": "2026-11-03T18:02:11Z"
    }
  ]
}
```

An entry can also say `idgames`, for a file with an idgames id. `stored_as` appears when
a store refused the file's own name, which Gitea does for an apostrophe, a comma or
anything outside ASCII. The file then keeps its real name everywhere but on the mirror.

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

Sourceports, launch args, IWADs and paths differ from one machine to the next, so they
are kept in a file of each machine's own instead: see
[What stays on each machine](#what-stays-on-each-machine). The Settings dialog sorts that
out, and says so where it applies.

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
idgames_index_check_days = 1            # 0 = never ask a mirror for a new idgames listing
downloads_dir = "~/Downloads"           # where Get looks; unset means the system's
link_mode = "copy"                      # leave a file Get links in Downloads; default "move"

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

That example is every setting in one place, for reading. Written by hand, `sourceport`, `iwad`, `iwad_dirs`, `sourceport_args`, `downloads_dir` and `executable_args` belong in `machine.toml`, and once that file exists they are ignored in `config.toml`: see [What stays on each machine](#what-stays-on-each-machine).

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
machine, or see [Several Machines](#several-machines) to keep it on more than one:

| Path | Contents |
|------|----------|
| `config.toml` | Configuration |
| `gui-state.json` | GUI view/sort/filter state |
| `library.db` | Library database |
| `sync/` | What has happened on each machine, which `library.db` can be rebuilt from |
| `data/` | Per-WAD saves, stats, configs |
| `wads/` | WAD files caco cannot re-download |
| `iwads/` | Managed IWADs |
| `id24/` | Managed id24 WADs |
| `companions/` | Managed companion files |
| `profiles/` | Per-sourceport config profiles |
| `sourceports/` | Sourceport build recipes and their patches |
| `cacowards/` | Your corrections and additions to the Cacowards list |
| `backups/` | Save backups + pre-migration DB snapshots |

**On this machine only** — neither copied between machines nor regenerable:

| | Linux | macOS |
|---|---|---|
| Tokens and passwords for your [mirrors](#mirrors) | `~/.config/caco/mirror-tokens.toml` | `~/Library/Preferences/caco/mirror-tokens.toml` |
| [Settings that differ between machines](#what-stays-on-each-machine) | `~/.config/caco/machine.toml` | `~/Library/Preferences/caco/machine.toml` |
| Which machine this is | `~/.config/caco/machine-id` | `~/Library/Preferences/caco/machine-id` |

**Disposable** — safe to delete at any time:

| Path | Contents |
|------|----------|
| `wads/` | Cached WAD files, re-downloaded on demand |
| `unpacked/` | What a zip holds, unpacked for the sourceport: see below |
| `thumbnails/` | Thumbnail cache, re-extracted from TITLEPIC |
| `sourceports/` | Built sourceport prefixes, rebuilt from the recipe |
| `sourceports-src/` | Sourceport checkouts and build trees |
| `idgames-index.db` | The local [index of idgames](#the-idgames-index), rebuilt in the background |

Which of the two `wads/` directories a file lands in is decided by whether caco
can fetch it again on its own, and nothing else. Anything on idgames or a mirror goes
to the cache, where it is fair game for cleanup because losing it costs a download.
Everything else — forum attachments, one-off file hosts, files you linked by
hand — goes to the portable side and stays until you delete it deliberately.
That is what makes copying the data directory to another machine actually carry
your library: see `retrievable:` under Queries. A file that did not make the trip
shows up as not on this machine, with where it was last seen, rather than as
cached.

A WAD's file is kept as it came, often a zip holding the WAD and a text file, but a
sourceport is never handed the zip. uzdoom only loads a `.wad` at a zip's root, so a
`.pk3` in one played as the IWAD alone, and nyan-doom misses a WAD in a folder inside
one. So a zip that holds only what plays and text beside it is unpacked into `unpacked/`
the first time it is played, and the files in it are what the sourceport loads. The copy
is kept for as long as the zip is on this machine, and goes when caco next starts or
plays a zip after the zip has gone.

`library.db` also records what each WAD file is: its size, SHA-256 and SHA-1, the
same for every WAD, PK3 or DEH inside a zip, and what each play session loaded.
That is how caco tells that a file has changed since you last played it, whether
it was re-downloaded after an author's fix or replaced by a sync, and asks before
launching it. A zip re-packed around the same WAD does not count as a change.
caco hashes a file when it first sees it, and again only when the file's size or
modification time has moved. The first start after upgrading reads every file
once, in the background. The hashes stay after the file goes, because they are
what identifies the file when you go looking for it.

Override the cache root with `CACO_CACHE_HOME`, just the WAD cache with
`CACO_CACHE_DIR`, or just the idgames index with `CACO_IDGAMES_INDEX`.

Each WAD's folder under `data/` is named for a key that travels with the WAD, so that the
same folder means the same WAD on every machine. That key is the row's old id for anything
from before this existed, so nothing has to move, and a name like `idgames-18000` or a long
hex string for anything since.

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

## Several Machines

`library.db` is one binary file, so it cannot be merged. Play on two machines between syncs
and git conflicts on the whole file, and resolving that means keeping one machine's database
and throwing away the other's sessions.

So each machine also writes down what happens on it, as it happens, in
`sync/<machine>/<yyyy-mm>.jsonl`. A machine only ever writes to its own folder, so files from
different machines can't conflict, and anything that moves files can carry them. Every machine
reads everyone's, at startup and every half a minute after that, and they all end up with the
same library, whatever order the files turn up in.

### Setting it up

There is nothing to set up inside caco. Sync the data directory the way you do now, and leave
the database out of it:

| | Sync it? |
|---|---|
| `sync/` | Yes. This is what makes it work |
| `data/`, `companions/`, `profiles/`, `cacowards/`, `config.toml` | Yes, as you do now |
| `wads/` | If you like, or let a [mirror](#mirrors) hold those files instead |
| `library.db`, `library.db-wal`, `library.db-shm` | **No.** Each machine builds its own |

Start the new version once on each machine **before** you take `library.db` out of your sync,
so that what each has done since the last sync gets written to its log. The first start on an
existing library takes a moment, and writes a log about the size of the database (13 MB for
one with 1,100 sessions), once. After that a session adds some tens of kilobytes at most.
Then, with git:

```bash
git rm --cached library.db
printf 'library.db*\n' >> .gitignore
```

A machine that gets its `library.db` deleted by that pull just builds a new one on its next
start. So does a new machine: install caco, sync the folder, and start it. If your sync tool
can't take the whole data directory, Settings has a **Sync folder**, and `sync/` can live
anywhere on its own (`CACO_SYNC_DIR` overrides it).

The **Sync** tool says where the folder is, which machines this library has heard from and
when, and whether anything is waiting. Sync Now does a pass without waiting for the next one.

### What travels, and who wins

Sessions, playthroughs, completions, status, ratings, notes, tags, a WAD's metadata and
per-WAD settings, which companions it uses, smart collections, Cacoward links and flags, and
what each WAD file is.

Edits go by last writer wins, one field at a time: changing the notes on one machine and the
rating on another keeps both. "Last" goes by a clock that always puts what you did after
seeing another machine's edit after that edit, however wrong either machine's clock is. An
import is not an edit. The same WAD imported on two machines is one WAD, and a later import
somewhere else does not overwrite the rating you gave it.

- **Per-map progress** adds up rather than overwrites. Beat MAP10 on one machine and MAP20 on
  another and the library knows about both, and finishing the last map anywhere completes the
  WAD. Starting a new playthrough starts over everywhere.
- **Deleting a WAD for good** (Empty Trash) is final on every machine. Importing it again
  makes a new one. Moving it to the Trash, and restoring it, go by last writer like anything else.
- **Files go by hash.** A machine that does not have a WAD's file knows what the file is, so a
  [mirror](#mirrors) can be asked for exactly that file, and a copy of the same name already in
  `wads/` or the cache is linked, without a download, if its hash matches.

So `library.db` is no longer the only copy of anything. Delete it and caco builds it again
from the logs. Go back to a copy of another machine's, the way resolving a conflict used to go,
and whatever this machine had done that the copy hasn't seen is applied again.

### Saves

A save is a file a sourceport rewrites, and nothing can merge two of them. So caco detects
this and leaves it to you. After a session, each machine says what its saves are, and before a
launch caco compares what is on this disk with what the other machines last said. If another
machine has said something since this one did, and its saves aren't here, you are asked first:
that usually means the sync hasn't finished. Once they arrive the hashes agree, and there is
nothing to ask. If you keep a separate game going on each machine, turn it off in Settings.

Moving saves is the sync tool's job, in `data/`. Stats files are there too, and if two
machines both change one you can keep either: what they've reported is already in the logs.

### What stays on each machine

These settings live in `machine.toml` rather than `config.toml`, because they are about this
machine and not your library: `sourceport`, `zdoom_sourceport`, `sourceport_preferences`,
`sourceport_args`, `executable_args`, `iwad`, `iwad_dirs`, `iwad_dir`, `cache_dir`, `db_path`,
`data_dir`, `profile_dir`, `downloads_dir` and `sync_dir`. Saving any setting moves them across
from an older `config.toml`, and until then a machine goes by what `config.toml` says.

These never leave the machine they're true of:

- Where a WAD's file is. The file's hash travels, and each machine finds its own copy.
- A per-WAD IWAD or sourceport given as a path. `doom2` and `uzdoom` travel, and
  `/home/me/iwads/doom2.wad` does not.
- Which IWADs and id24 WADs are registered, since those go by path. Register them in **IWADs**
  once per machine. A file already in `iwads/` is found without that, but an id24 WAD isn't
  loaded until it is registered.
- What can be worked out again: map analysis, what each mirror holds, the Cacowards list.

The log is never compacted, and caco talks to no remote itself. Moving the files is left to
whatever already moves the rest of the data directory.

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

Each sourceport builds its **latest release** unless you pick another
**Version**: the development branch (`trunk` for uzdoom, `master` for
nyan-doom, whichever the project's repository says), or an older release. The
dialog asks the repository which versions there are when it shows a sourceport,
with one `git ls-remote` that downloads nothing. A release is a tag that is a
version number, after a name if it has one (`5.0.3`, `v1.6.2`, `woof_15.2.0`).
Release candidates, prereleases like uzdoom's `x-testing` and every other branch
are not offered. The choice applies to that build and does not rewrite the recipe.

What you picked decides what counts as an update. A build of the latest release
is updated by the next release, and rebuilding builds it and removes the one it
replaces. A build of the development branch is updated by new commits on it. An
older release is one you chose to stay on, so it never reports an update.

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
ref = "5.0.2"                         # optional: build this branch or tag by default
first_release = "4.14.3"              # optional: older version tags are another project's
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

Without `ref` a recipe builds its latest release, or its development branch if
it has never made one. `first_release` is for a project whose history carries an
older project's tags: uzdoom's has ZDoom's `2.8.1` and GZDoom's `g4.14.2`, which
would otherwise be offered as uzdoom releases and build no uzdoom.

`-DINSTALL_PK3_PATH=bin` on uzdoom is load-bearing. Its default install puts
the pk3s in `share/games/uzdoom` while the binary looks beside itself, so a
stock build succeeds and then aborts at launch with `Cannot find uzdoom.pk3`.
Do not drop it when overriding the recipe.

A recipe's name must match a sourceport caco knows (see the family table
above), or the build works but complevel args, save directories and config
profiles quietly stop applying.

#### Update checks

At startup caco asks each built sourceport's repository whether there is
something newer than what you built, by the rules above: a newer release for a
build of the latest release, new commits for a build of a branch, and nothing
for an older release you chose. It is one `git ls-remote` each, which fetches no
objects, and a notification if anything is newer. The **Sourceports** dialog
marks them `update available`; rebuilding is always your call.

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
