//! What a WAD file is, as opposed to where it is.
//!
//! [`crate::presence`] keeps `cached_path` honest about where a WAD's file is.
//! This module records what that file *is*: its size, SHA-256 and SHA-1, the
//! loadable files inside it when it is an archive, and what a sourceport ends up
//! loading from it. Until it existed caco knew a file only by its path and its
//! name, so a file replaced under the same name went unnoticed: idgames swapping
//! in an author's fix after the cache was cleared, a sync bringing another
//! machine's copy. So did every save and demo it broke, since a sourceport
//! needs the file set they were made with.
//!
//! The hashes live in `library.db`, on the portable side of the data split. The
//! hash of a file that is no longer here cannot be regenerated, and it is what
//! names the file to anyone asked for it again (#54, #57), which is why nothing
//! clears it when the file goes.
//!
//! # When a file is hashed
//!
//! Not when caco writes one. Every identity carries a stamp, the file's size
//! and modification time when it was hashed, and [`verify`] hashes the file
//! again whenever the stamp no longer matches. That one rule covers a download,
//! a linked copy, a relink by name and a file a sync replaced under caco, since
//! each leaves a file whose stamp is not the recorded one, and it leaves every
//! write path the single line it was. [`check_library`] applies the rule to
//! every row at startup, which is also how a library from before this module
//! gets its hashes, and every launch applies it to the file it is about to hand
//! the sourceport.
//!
//! # Content
//!
//! Saves and demos depend on what the sourceport loads, which is not always the
//! file caco hands it. An idgames upload is a zip holding a WAD and a text file,
//! and re-zipping it changes the zip's hash and nothing a game could notice: a
//! corrected readme, or the zip fetched from idgames for a WAD whose copy was
//! extracted by hand on another machine. So an identity also carries
//! `content_sha256`, the WAD inside a zip that only [wraps](is_container) loadable
//! files (a digest of their hashes, when it holds several), and otherwise the
//! file's own hash. That is what decides whether a WAD has changed since it was
//! last played, and what two WADs share when they are the same WAD.

use std::fs::{self, File};
use std::io::{self, Read, Seek};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::db::{self, WadRecord};
use crate::utils::zip_entry_names;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The hashes of one stream of bytes, both taken from the same read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hashes {
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
    /// Lowercase hex. What Wad Archive's index is keyed by.
    pub sha1: String,
}

/// What caco recorded a WAD's file to be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    /// The file as it is stored: what a mirror would be asked for, and what a
    /// download is checked against.
    pub file: Hashes,
    /// What a sourceport loads from it: see the module docs.
    pub content_sha256: String,
}

impl FileIdentity {
    /// Whether the file only wraps what plays, so that its own hash is not the
    /// content's.
    pub fn is_container(&self) -> bool {
        self.content_sha256 != self.file.sha256
    }
}

/// A loadable file inside an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerFile {
    /// The entry's path inside the archive, as the archive spells it.
    pub path: String,
    pub hashes: Hashes,
}

/// A file as [`identify`] read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identified {
    pub identity: FileIdentity,
    /// Every loadable file in it, when it is a zip caco could read into.
    pub inner: Vec<InnerFile>,
    /// The file's modification time before it was read, in whole seconds. With
    /// the size, the stamp [`verify`] compares.
    pub mtime: Option<i64>,
}

// ---------------------------------------------------------------------------
// Reading a file
// ---------------------------------------------------------------------------

/// Read size per chunk. Large enough that hashing, not syscalls, is the cost.
pub(crate) const READ_CHUNK: usize = 64 * 1024;

/// A zip's local-file-header signature, which a zip starts with unless it is
/// a self-extractor (taken whole, as any other file is).
const ZIP_MAGIC: [u8; 4] = *b"PK\x03\x04";

/// More loadable files than this and an archive is not looked into at all: no
/// WAD ships that many, and the rows would be noise.
pub(crate) const MAX_INNER_FILES: usize = 1024;

/// Ceiling on one entry's decompressed size, so a zip bomb costs a bounded read
/// rather than an unbounded one. Far above any real WAD, the largest of which
/// run to a few hundred megabytes.
pub(crate) const MAX_INNER_BYTES: u64 = 4 << 30;

/// Hash a file and, when it is a zip, the loadable files inside it.
///
/// Reads the whole file, and a zip's loadable entries a second time, so it
/// belongs on a worker thread and never the UI's.
pub fn identify(path: &Path) -> io::Result<Identified> {
    // The stamp is taken before the read, so a file that changes while it is
    // being hashed no longer matches it, and the next check hashes it again.
    let mtime = fs::metadata(path)?.modified().ok().and_then(unix_secs);

    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    let is_zip = match file.read_exact(&mut magic) {
        Ok(()) => magic == ZIP_MAGIC,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => false,
        Err(e) => return Err(e),
    };
    file.rewind()?;
    let hashes = hash_reader(&mut file)?;

    let inside = if is_zip { look_inside(file) } else { None };
    let content_sha256 = match &inside {
        Some(inside) if inside.container => content_of(&inside.inner),
        _ => hashes.sha256.clone(),
    };
    Ok(Identified {
        identity: FileIdentity {
            file: hashes,
            content_sha256,
        },
        inner: inside.map(|inside| inside.inner).unwrap_or_default(),
        mtime,
    })
}

/// Hash a whole file, and nothing inside it: what a download is checked against.
pub fn hash_file(path: &Path) -> io::Result<Hashes> {
    hash_reader(File::open(path)?)
}

/// Hash everything `reader` yields.
pub(crate) fn hash_reader(mut reader: impl Read) -> io::Result<Hashes> {
    let mut sha256 = Sha256::new();
    let mut sha1 = Sha1::new();
    let mut size = 0u64;
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        sha256.update(&buf[..n]);
        sha1.update(&buf[..n]);
        size += n as u64;
    }
    Ok(Hashes {
        size,
        sha256: hex::encode(sha256.finalize()),
        sha1: hex::encode(sha1.finalize()),
    })
}

/// What a zip holds, as far as caco can tell without a sourceport.
struct Inside {
    /// Every loadable file in it, wherever it sits.
    inner: Vec<InnerFile>,
    /// See [`is_container`].
    container: bool,
}

/// List and hash a zip's loadable files.
///
/// `None` when any of them cannot be read (an encrypted entry, a compression
/// method this build does not decode, one larger than [`MAX_INNER_BYTES`]). A
/// content hash missing a file the sourceport loads would call two different
/// games the same, so the zip is taken whole instead.
fn look_inside<R: Read + Seek>(reader: R) -> Option<Inside> {
    let mut archive = ZipArchive::new(reader).ok()?;
    let container = {
        let names: Vec<_> = zip_entry_names(&archive)
            .map(|(_, name)| name)
            .filter(|name| !is_junk(name))
            .collect();
        is_container(names.iter().map(|name| name.as_ref()))
    };

    let loadable = loadable_entries(&archive);
    if loadable.len() > MAX_INNER_FILES {
        return None;
    }

    let mut inner = Vec::with_capacity(loadable.len());
    for (index, name) in loadable {
        let entry = archive.by_index(index).ok()?;
        let hashes = hash_reader(entry.take(MAX_INNER_BYTES)).ok()?;
        if hashes.size >= MAX_INNER_BYTES {
            return None;
        }
        inner.push(InnerFile { path: name, hashes });
    }

    Some(Inside { inner, container })
}

/// A zip's loadable files, wherever they sit, as `(index, name)` in archive order.
///
/// The one spelling of which files in a zip are hashed into its content, which
/// [`crate::unpack`] shares, so that what a launch unpacks is what that hash
/// was taken over.
pub(crate) fn loadable_entries<R: Read + Seek>(archive: &ZipArchive<R>) -> Vec<(usize, String)> {
    zip_entry_names(archive)
        .filter(|(_, name)| !is_junk(name) && is_loadable(name))
        .map(|(index, name)| (index, name.into_owned()))
        .collect()
}

/// The content hash of a container's loadable files: the one file's own hash,
/// so that a zip and the WAD extracted from it agree, or a digest of all of
/// theirs, which does not depend on the order the zip lists them in.
fn content_of(files: &[InnerFile]) -> String {
    content_digest(files.iter().map(|f| f.hashes.sha256.as_str()))
}

/// [`content_of`] over the files' SHA-256 hashes alone, for what has only those.
pub(crate) fn content_digest<'a>(hashes: impl IntoIterator<Item = &'a str>) -> String {
    let mut hashes: Vec<&str> = hashes.into_iter().collect();
    hashes.sort_unstable();
    // The same file twice is still the one file.
    hashes.dedup();
    match hashes.as_slice() {
        [one] => (*one).to_string(),
        many => {
            // Domain-separated, so no single file can share a digest with a set.
            let mut digest = Sha256::new();
            digest.update(b"caco-content-v1\n");
            for hash in many {
                digest.update(hash.as_bytes());
                digest.update(b"\n");
            }
            hex::encode(digest.finalize())
        }
    }
}

// ---------------------------------------------------------------------------
// What an archive holds
// ---------------------------------------------------------------------------

/// Extensions of the files a sourceport loads, as `-file` and `-deh` take them.
const LOADABLE: &[&str] = &["wad", "iwad", "pk3", "ipk3", "pk7", "pke", "deh", "bex"];

/// Extensions of what an upload carries beside a WAD and no sourceport plays:
/// readmes, screenshots, DOS launchers, demos.
const EXTRAS: &[&str] = &[
    "txt", "md", "nfo", "diz", "rtf", "pdf", "doc", "docx", "odt", "htm", "html", "url", "png",
    "jpg", "jpeg", "gif", "bmp", "webp", "bat", "lmp",
];

/// Directories a sourceport mounting a zip as a resource archive reads lumps
/// from: GZDoom's namespaces. `filter/` holds what is loaded as if it were the
/// root, for one game.
const LUMP_NAMESPACES: &[&str] = &[
    "acs",
    "brightmaps",
    "colormaps",
    "filter",
    "flats",
    "fonts",
    "graphics",
    "hires",
    "maps",
    "materials",
    "models",
    "music",
    "patches",
    "shaders",
    "sounds",
    "sprites",
    "textures",
    "voices",
    "voxels",
];

/// Lumps a sourceport reads by name. A sourceport that mounts a zip as a
/// resource archive, as GZDoom does, turns each file in it into the lump its
/// name spells, so `mapinfo.txt` is a MAPINFO and not a readme.
const DEFINITION_LUMPS: &[&str] = &[
    "althudcf", "animated", "animdefs", "colormap", "cvarinfo", "decaldef", "decorate", "dehacked",
    "dmxgus", "doomdefs", "emapinfo", "endoom", "fontdefs", "fsglobal", "gameinfo", "genmidi",
    "gldefs", "hexndefs", "hticdefs", "iwadinfo", "keyconf", "language", "loadacs", "lockdefs",
    "mapinfo", "menudef", "modeldef", "musinfo", "palvers", "playpal", "pnames", "reverbs",
    "sbarinfo", "secrets", "skininfo", "sndinfo", "sndseq", "strfdefs", "switches", "teaminfo",
    "terrain", "textcolo", "texture1", "texture2", "textures", "trnslate", "umapinfo", "voxeldef",
    "x11r6rgb", "xhairs", "zmapinfo", "zscript",
];

/// Whether a zip only wraps loadable files, so that they, and not the zip,
/// are what a sourceport plays.
///
/// True for the shapes WADs travel in: a WAD (sometimes a DEH beside it) and
/// its text files, at the root as idgames packs them or inside one folder as a
/// zip made by hand usually has them. False for anything a sourceport could
/// read more out of, such as a `mapinfo.txt`, a `sounds/` directory or a WAD
/// under `maps/`. Errs towards false, because the two mistakes cost different
/// amounts. Taking a container for a resource archive costs a "changed"
/// warning when it is re-zipped. The opposite would hide a change that breaks
/// saves.
fn is_container<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    let mut holds_loadable = false;
    for name in names {
        if name.ends_with('/') || name.ends_with('\\') {
            continue; // a directory entry
        }
        if in_lump_namespace(name) {
            return false;
        }
        if is_loadable(name) {
            holds_loadable = true;
        } else if is_definition_lump(name) || !has_extension_in(name, EXTRAS) {
            return false;
        }
    }
    holds_loadable
}

fn is_loadable(name: &str) -> bool {
    has_extension_in(name, LOADABLE)
}

/// Archivers' bookkeeping, which no sourceport reads: the resource forks a Mac
/// adds under `__MACOSX/`, Finder and Explorer metadata.
fn is_junk(name: &str) -> bool {
    name.starts_with("__MACOSX/")
        || matches!(
            base_name(name).to_ascii_lowercase().as_str(),
            ".ds_store" | "thumbs.db" | "desktop.ini"
        )
}

/// Whether an entry sits under a directory a sourceport reads as a lump
/// namespace, which only a resource archive has. At any depth, so a resource
/// archive wrapped in a folder is still taken whole.
fn in_lump_namespace(name: &str) -> bool {
    let mut parts: Vec<&str> = name.split(['/', '\\']).collect();
    parts.pop(); // the file itself
    parts
        .iter()
        .any(|dir| LUMP_NAMESPACES.contains(&dir.to_ascii_lowercase().as_str()))
}

fn is_definition_lump(name: &str) -> bool {
    // A lump's name is its file name up to the first dot, so DECORATE.monsters
    // is a DECORATE.
    let stem = base_name(name).split('.').next().unwrap_or_default();
    DEFINITION_LUMPS.contains(&stem.to_ascii_lowercase().as_str())
}

fn has_extension_in(name: &str, extensions: &[&str]) -> bool {
    base_name(name).rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && extensions.contains(&ext.to_ascii_lowercase().as_str())
    })
}

/// The last component of an archive path. Zips made on Windows sometimes
/// separate with backslashes.
pub(crate) fn base_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

fn unix_secs(time: SystemTime) -> Option<i64> {
    let secs = time.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

// ---------------------------------------------------------------------------
// Recording an identity
// ---------------------------------------------------------------------------

/// Record `found` as the identity of the WAD's file at `path`.
///
/// `false`, writing nothing, when the row no longer points at `path`: a check
/// that spent minutes hashing a file on a slow mount must not overwrite what
/// the row was relinked to in the meantime. Runs in the caller's transaction
/// when it has one open (an import writes the row and its identity together),
/// otherwise in one of its own.
///
/// A file that plays differently from the one recorded before also drops the
/// WAD's map analysis, which describes the old file, so the next library
/// reload analyses the new one. `updated_at` is left alone: nobody edited the
/// WAD.
pub fn record(
    conn: &Connection,
    wad_id: i64,
    path: &Path,
    found: &Identified,
) -> crate::Result<bool> {
    if conn.is_autocommit() {
        db::with_transaction(conn, |tx| write(tx, wad_id, path, found))
    } else {
        write(conn, wad_id, path, found)
    }
}

fn write(conn: &Connection, wad_id: i64, path: &Path, found: &Identified) -> crate::Result<bool> {
    let previous: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT file_sha256, content_sha256 FROM wads WHERE id = ?1 AND cached_path IS ?2",
            rusqlite::params![wad_id, path.to_string_lossy()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((previous_file, previous_content)) = previous else {
        return Ok(false);
    };

    let identity = &found.identity;
    conn.execute(
        "UPDATE wads SET file_size = ?1, file_sha256 = ?2, file_sha1 = ?3, \
         content_sha256 = ?4, file_mtime = ?5 WHERE id = ?6",
        rusqlite::params![
            sql_size(identity.file.size),
            identity.file.sha256,
            identity.file.sha1,
            identity.content_sha256,
            found.mtime,
            wad_id
        ],
    )?;

    conn.execute("DELETE FROM wad_files WHERE wad_id = ?1", [wad_id])?;
    let mut insert = conn.prepare(
        "INSERT OR IGNORE INTO wad_files (wad_id, inner_path, size, sha256, sha1) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for file in &found.inner {
        insert.execute(rusqlite::params![
            wad_id,
            file.path,
            sql_size(file.hashes.size),
            file.hashes.sha256,
            file.hashes.sha1
        ])?;
    }

    if previous_content.is_some_and(|previous| previous != identity.content_sha256) {
        conn.execute("DELETE FROM wad_analysis WHERE wad_id = ?1", [wad_id])?;
    }
    if previous_file.is_none() {
        attribute_newest_session(conn, wad_id, &identity.content_sha256)?;
    }
    Ok(true)
}

/// Take a file hashed for the first time to be the one the WAD's newest
/// session played.
///
/// Sessions from before caco recorded identities say nothing about the file
/// they ran on. Without this, a WAD in progress across the upgrade would have
/// nothing to compare its next launch against, and a file replaced in between
/// would go unremarked, which is the case this module exists for. The file
/// caco finds at its first look is the best evidence there is: it is what the
/// row pointed at, and nothing recorded says otherwise. If it is wrong, the
/// file had already changed before caco could have noticed, and the outcome is
/// the same silence there would have been without it. Only the newest session,
/// and only one that recorded nothing itself.
fn attribute_newest_session(conn: &Connection, wad_id: i64, content: &str) -> crate::Result<()> {
    conn.execute(
        "UPDATE sessions SET content_sha256 = ?1 \
         WHERE id = (SELECT id FROM sessions WHERE wad_id = ?2 \
                     ORDER BY started_at DESC, id DESC LIMIT 1) \
           AND content_sha256 IS NULL",
        rusqlite::params![content, wad_id],
    )?;
    Ok(())
}

/// SQLite integers are signed. No file comes near the difference.
fn sql_size(size: u64) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

// ---------------------------------------------------------------------------
// Checking a file against its identity
// ---------------------------------------------------------------------------

/// What [`verify`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// The file is the copy the row's identity was taken from.
    Unchanged(FileIdentity),
    /// The file was hashed, and the row now records what was found.
    Hashed(FileIdentity),
    /// The file was hashed, but by then the row pointed at another file, so
    /// nothing was written. The identity still describes the file at the path
    /// that was checked.
    Superseded(FileIdentity),
}

impl Checked {
    pub fn into_identity(self) -> FileIdentity {
        match self {
            Checked::Unchanged(identity)
            | Checked::Hashed(identity)
            | Checked::Superseded(identity) => identity,
        }
    }
}

/// Which copy of a file a set of hashes was taken from.
struct Stamp {
    size: u64,
    mtime: Option<i64>,
}

impl Stamp {
    fn of(meta: &fs::Metadata) -> Self {
        Self {
            size: meta.len(),
            mtime: meta.modified().ok().and_then(unix_secs),
        }
    }

    /// Whether `identity`, taken from a file modified at `recorded_mtime`,
    /// still describes this one. An unknown modification time never matches,
    /// so such a file is always hashed again.
    fn matches(&self, identity: &FileIdentity, recorded_mtime: Option<i64>) -> bool {
        self.size == identity.file.size && self.mtime.is_some() && self.mtime == recorded_mtime
    }
}

/// The identity of `wad`'s file at `path`, hashing it first unless it is the
/// copy the row's identity was taken from.
///
/// Takes `path` rather than reading it off `wad` because
/// [`crate::presence::check_wad`] may just have relinked the row. `None` when
/// the file cannot be read, which is logged: a launch goes ahead without an
/// identity, and the next check tries again.
pub fn verify(conn: &Connection, wad: &WadRecord, path: &Path) -> crate::Result<Option<Checked>> {
    let stamp = match fs::metadata(path) {
        Ok(meta) => Stamp::of(&meta),
        Err(e) => {
            tracing::warn!("cannot read {}: {e}", path.display());
            return Ok(None);
        }
    };
    if let Some(identity) = &wad.identity
        && stamp.matches(identity, wad.file_mtime)
    {
        return Ok(Some(Checked::Unchanged(identity.clone())));
    }

    let found = match identify(path) {
        Ok(found) => found,
        Err(e) => {
            tracing::warn!("cannot hash {}: {e}", path.display());
            return Ok(None);
        }
    };
    Ok(Some(if record(conn, wad.id, path, &found)? {
        Checked::Hashed(found.identity)
    } else {
        Checked::Superseded(found.identity)
    }))
}

/// Bring every recorded file's identity up to date with the file at its path.
/// Returns how many live rows' files it hashed, because they were new to caco
/// or had been replaced. A trashed WAD's file is checked too, since it can be
/// restored, but is not counted, as in [`crate::presence::CheckReport`].
///
/// Run it off the UI thread, after [`crate::presence::check_library`] has
/// settled which paths are real. It costs one stat per row, and a full read of
/// every file that is new or has been replaced, which on the first run is
/// every file there is. Each row is written as soon as its file is hashed, so
/// a check cut short when the app closes keeps what it did.
pub fn check_library(conn: &Connection) -> crate::Result<usize> {
    let wads: Vec<WadRecord> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM wads WHERE COALESCE(cached_path, '') <> ''",
            db::WAD_COLUMNS
        ))?;
        stmt.query_map([], WadRecord::from_row)?
            .collect::<std::result::Result<_, _>>()?
    };

    let mut hashed = 0;
    for wad in &wads {
        let Some(path) = wad.cached_path.as_deref() else {
            continue;
        };
        if let Some(Checked::Hashed(_)) = verify(conn, wad, Path::new(path))?
            && wad.deleted_at.is_none()
        {
            hashed += 1;
        }
    }
    Ok(hashed)
}

// ---------------------------------------------------------------------------
// Finding a file by what it is
// ---------------------------------------------------------------------------

/// Link every WAD that knows what its file is and has no path for it, to a file that is
/// here. Returns how many were linked.
///
/// A row that came from another machine's log says what its file is and nothing of where,
/// since that is only true over there. The file may well be here anyway: in the keep dir,
/// which is on the side that travels, or in a cache left over from before the library was
/// rebuilt. It goes by the name a store dir would have given it, and then by its hash,
/// which is not optional. Unrelated WADs share names, `castle.zip` is on idgames seven
/// times, and a WAD linked to some other WAD's file would play the wrong game.
///
/// Off the UI thread, since it reads whatever it finds with the right name and size.
pub fn adopt_files(conn: &Connection, paths: &crate::presence::StorePaths) -> crate::Result<usize> {
    let wads: Vec<WadRecord> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM wads \
             WHERE COALESCE(cached_path, '') = '' AND file_sha256 IS NOT NULL \
               AND (COALESCE(filename, '') <> '' OR source_type = 'local')",
            db::WAD_COLUMNS
        ))?;
        stmt.query_map([], WadRecord::from_row)?
            .collect::<std::result::Result<_, _>>()?
    };

    let mut adopted = 0;
    for wad in &wads {
        let Some((path, found)) = find_by_identity(wad, paths) else {
            continue;
        };
        let linked = db::with_transaction(conn, |tx| {
            // Not if it was given a file while this one was being read.
            let free = tx.execute(
                "UPDATE wads SET cached_path = ?1, last_known_path = NULL \
                 WHERE id = ?2 AND COALESCE(cached_path, '') = ''",
                rusqlite::params![path.to_string_lossy(), wad.id],
            )? > 0;
            if free {
                record(tx, wad.id, &path, &found)?;
            }
            Ok(free)
        })?;
        if linked {
            adopted += 1;
        }
    }
    Ok(adopted)
}

/// A file here that is the one `wad` says it has.
fn find_by_identity(
    wad: &WadRecord,
    paths: &crate::presence::StorePaths,
) -> Option<(std::path::PathBuf, Identified)> {
    let identity = wad.identity.as_ref()?;
    candidates(wad, identity, paths)
        .into_iter()
        .find_map(|path| {
            // Most of what this finds is the file, so the read is rarely wasted, and the size
            // keeps it that way for a `map01.wad` that is some other WAD's.
            let same_size = fs::metadata(&path)
                .is_ok_and(|meta| meta.is_file() && meta.len() == identity.file.size);
            if !same_size {
                return None;
            }
            match identify(&path) {
                Ok(found)
                    if found.identity.file.sha256 == identity.file.sha256
                        || found.identity.content_sha256 == identity.content_sha256 =>
                {
                    Some((path, found))
                }
                Ok(_) => None,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    None
                }
            }
        })
}

/// Where `wad`'s file could be: in a store dir under its own name, or the one
/// [`crate::placement`] gives a file whose name was taken, and for a local import, where it
/// was imported from, which is where that stays. That last is only true of the machine it
/// was imported on, and this is what brings such a WAD's file back when `library.db` goes.
fn candidates(
    wad: &WadRecord,
    identity: &FileIdentity,
    paths: &crate::presence::StorePaths,
) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    // A row can say anything here, and a name with a directory in it is not one.
    if let Some(filename) = wad
        .filename
        .as_deref()
        .filter(|name| Path::new(name).file_name().and_then(|n| n.to_str()) == Some(*name))
    {
        let hashed = crate::placement::name_with_hash(filename, &identity.file.sha256);
        for dir in paths.search_order(wad.retrievability()) {
            found.push(dir.join(filename));
            found.push(dir.join(&hashed));
        }
    }
    if wad.source_type == db::SourceType::Local
        && let Some(source) = wad.source_url.as_deref().map(Path::new)
        && source.is_absolute()
    {
        found.push(source.to_path_buf());
    }
    found
}

// ---------------------------------------------------------------------------
// What was played
// ---------------------------------------------------------------------------

/// The newest session of a WAD that recorded what it loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastPlayed {
    /// When it started, as RFC 3339.
    pub started_at: String,
    pub content_sha256: String,
}

/// See [`LastPlayed`]. `None` when no session of the WAD recorded one.
pub fn last_played(conn: &Connection, wad_id: i64) -> crate::Result<Option<LastPlayed>> {
    let last = conn
        .query_row(
            "SELECT started_at, content_sha256 FROM sessions \
             WHERE wad_id = ?1 AND content_sha256 IS NOT NULL \
             ORDER BY started_at DESC, id DESC LIMIT 1",
            [wad_id],
            |row| {
                Ok(LastPlayed {
                    started_at: row.get(0)?,
                    content_sha256: row.get(1)?,
                })
            },
        )
        .optional()?;
    Ok(last)
}

/// A WAD's file plays differently from the one it was last played with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentChange {
    /// When the session that played the old file started, as RFC 3339.
    pub last_played_at: String,
    /// What that session loaded.
    pub played: String,
    /// What the file holds now.
    pub current: String,
}

/// Whether `current` plays differently from what `wad_id` was last played
/// with.
///
/// Against the newest session rather than the one that began the playthrough,
/// for two reasons. A playthrough only begins at its first map exit, so saves
/// made before then belong to none. And a user who was told the file changed
/// and played it anyway is on the new file from then on, so asking again at
/// every launch would be nagging.
pub fn change_since_last_played(
    conn: &Connection,
    wad_id: i64,
    current: &FileIdentity,
) -> crate::Result<Option<ContentChange>> {
    Ok(last_played(conn, wad_id)?
        .filter(|last| last.content_sha256 != current.content_sha256)
        .map(|last| ContentChange {
            last_played_at: last.started_at,
            played: last.content_sha256,
            current: current.content_sha256.clone(),
        }))
}

// ---------------------------------------------------------------------------
// Looking files up
// ---------------------------------------------------------------------------

/// The loadable files caco found inside a WAD's file, by path.
pub fn inner_files(conn: &Connection, wad_id: i64) -> crate::Result<Vec<InnerFile>> {
    let mut stmt = conn.prepare(
        "SELECT inner_path, size, sha256, sha1 FROM wad_files \
         WHERE wad_id = ?1 ORDER BY inner_path",
    )?;
    let files = stmt
        .query_map([wad_id], |row| {
            Ok(InnerFile {
                path: row.get(0)?,
                hashes: Hashes {
                    size: u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
                    sha256: row.get(2)?,
                    sha1: row.get(3)?,
                },
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(files)
}

/// SQL predicate matching WADs whose file plays the same as another live WAD's:
/// the same WAD in the library twice, imported from two places.
///
/// Must stay in lockstep with [`same_content`];
/// `test_duplicate_sql_matches_rust` asserts they agree.
pub const DUPLICATE_SQL: &str = "EXISTS (SELECT 1 FROM wads AS other \
     WHERE other.id <> wads.id AND other.deleted_at IS NULL \
     AND other.content_sha256 = wads.content_sha256)";

/// The live WADs other than `wad_id` whose files play the same as its own, as
/// `(id, title)`.
pub fn same_content(conn: &Connection, wad_id: i64) -> crate::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT other.id, other.title FROM wads AS this \
         JOIN wads AS other ON other.content_sha256 = this.content_sha256 \
         WHERE this.id = ?1 AND other.id <> this.id AND other.deleted_at IS NULL \
         ORDER BY other.id",
    )?;
    let wads = stmt
        .query_map([wad_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(wads)
}

/// A WAD whose file plays `content_sha256`, as `(id, title)`, trashed or not:
/// an import's duplicate check, which counts the trash as [`db::find_duplicate`]
/// does.
pub fn find_by_content(
    conn: &Connection,
    content_sha256: &str,
) -> crate::Result<Option<(i64, String)>> {
    let found = conn
        .query_row(
            "SELECT id, title FROM wads WHERE content_sha256 = ?1 ORDER BY id LIMIT 1",
            [content_sha256],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(found)
}

#[cfg(test)]
mod adopt_tests {
    use super::*;
    use crate::db::{NewWad, SourceType};
    use crate::presence::StorePaths;

    const BYTES: &[u8] = b"PWAD the one this row means";

    struct Fixture {
        conn: Connection,
        paths: StorePaths,
        _root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let paths = StorePaths {
                cache_dir: root.path().join("cache"),
                keep_dir: root.path().join("keep"),
            };
            fs::create_dir_all(&paths.cache_dir).unwrap();
            fs::create_dir_all(&paths.keep_dir).unwrap();
            let conn = db::open_memory().unwrap();
            db::init_db(&conn).unwrap();
            Self {
                conn,
                paths,
                _root: root,
            }
        }

        /// A row as it comes from another machine: what its file is, and not where.
        fn row(&self, filename: &str, bytes: &[u8]) -> i64 {
            let id = db::add_wad(
                &self.conn,
                &NewWad::new("Road", SourceType::Doomworld).filename(filename),
            )
            .unwrap();
            let hashes = hash_reader(bytes).unwrap();
            self.conn
                .execute(
                    "UPDATE wads SET file_size = ?1, file_sha256 = ?2, file_sha1 = ?3, \
                     content_sha256 = ?2 WHERE id = ?4",
                    rusqlite::params![hashes.size as i64, hashes.sha256, hashes.sha1, id],
                )
                .unwrap();
            id
        }

        fn cached(&self, id: i64) -> Option<String> {
            db::get_wad(&self.conn, id, false)
                .unwrap()
                .unwrap()
                .cached_path
        }

        fn adopt(&self) -> usize {
            adopt_files(&self.conn, &self.paths).unwrap()
        }
    }

    #[test]
    fn test_a_file_with_the_right_name_and_hash_is_this_wads() {
        let f = Fixture::new();
        let id = f.row("road.wad", BYTES);
        let file = f.paths.keep_dir.join("road.wad");
        fs::write(&file, BYTES).unwrap();

        assert_eq!(f.adopt(), 1);
        assert_eq!(f.cached(id).as_deref(), file.to_str());
        // With its stamp, so that the next check does not read it all again.
        let wad = db::get_wad(&f.conn, id, false).unwrap().unwrap();
        assert!(wad.file_mtime.is_some());
        assert_eq!(f.adopt(), 0);
    }

    /// `castle.zip` is on idgames seven times. The whole reason a hash is asked for.
    #[test]
    fn test_a_file_with_the_right_name_and_the_wrong_hash_is_someone_elses() {
        let f = Fixture::new();
        let id = f.row("map01.wad", BYTES);
        let other: Vec<u8> = BYTES.iter().map(|b| b.wrapping_add(1)).collect();
        fs::write(f.paths.keep_dir.join("map01.wad"), &other).unwrap();
        fs::write(f.paths.cache_dir.join("map01.wad"), b"different size").unwrap();

        assert_eq!(f.adopt(), 0);
        assert_eq!(f.cached(id), None);
    }

    /// Where `placement` puts a second file of the same name.
    #[test]
    fn test_a_file_is_found_under_the_name_placement_gives_it() {
        let f = Fixture::new();
        let id = f.row("map01.wad", BYTES);
        let other: Vec<u8> = BYTES.iter().map(|b| b.wrapping_add(1)).collect();
        fs::write(f.paths.cache_dir.join("map01.wad"), &other).unwrap();
        let sha256 = hash_reader(BYTES).unwrap().sha256;
        let name = crate::placement::name_with_hash("map01.wad", &sha256);
        let file = f.paths.cache_dir.join(name);
        fs::write(&file, BYTES).unwrap();

        assert_eq!(f.adopt(), 1);
        assert_eq!(f.cached(id).as_deref(), file.to_str());
    }

    #[test]
    fn test_nothing_outside_the_store_dirs_is_ever_looked_at() {
        let f = Fixture::new();
        let root = f.paths.cache_dir.parent().unwrap();
        fs::write(root.join("escaped.wad"), BYTES).unwrap();
        let id = f.row("../escaped.wad", BYTES);
        let absolute = root.join("escaped.wad").display().to_string();
        let other = f.row(&absolute, BYTES);

        assert_eq!(f.adopt(), 0);
        assert_eq!(f.cached(id), None);
        assert_eq!(f.cached(other), None);
    }

    /// A row that already has a file, however it got one, keeps it.
    #[test]
    fn test_a_row_with_a_file_is_not_asked_about() {
        let f = Fixture::new();
        let id = f.row("road.wad", BYTES);
        f.conn
            .execute(
                "UPDATE wads SET cached_path = '/elsewhere/road.wad' WHERE id = ?1",
                [id],
            )
            .unwrap();
        fs::write(f.paths.keep_dir.join("road.wad"), BYTES).unwrap();

        assert_eq!(f.adopt(), 0);
        assert_eq!(f.cached(id).as_deref(), Some("/elsewhere/road.wad"));
    }

    /// A row that knows nothing about its file has nothing to go by but the name, which
    /// is not enough, so it is left for whoever links one by hand.
    #[test]
    fn test_a_row_with_no_hash_is_not_given_a_file_on_its_name_alone() {
        let f = Fixture::new();
        let id = db::add_wad(
            &f.conn,
            &NewWad::new("Road", SourceType::Doomworld).filename("road.wad"),
        )
        .unwrap();
        fs::write(f.paths.keep_dir.join("road.wad"), BYTES).unwrap();

        assert_eq!(f.adopt(), 0);
        assert_eq!(f.cached(id), None);
    }

    /// A local import stays where it was. With `library.db` gone, the row comes back from
    /// the log, and that is the only place that file could be.
    #[test]
    fn test_a_local_import_is_found_where_it_was_imported_from() {
        let f = Fixture::new();
        let elsewhere = f.paths.cache_dir.parent().unwrap().join("my-wads");
        fs::create_dir_all(&elsewhere).unwrap();
        let file = elsewhere.join("mine.wad");
        fs::write(&file, BYTES).unwrap();

        let id = db::add_wad(
            &f.conn,
            &NewWad::new("Mine", SourceType::Local).source_url(file.display().to_string()),
        )
        .unwrap();
        let hashes = hash_reader(BYTES).unwrap();
        f.conn
            .execute(
                "UPDATE wads SET file_size = ?1, file_sha256 = ?2, file_sha1 = ?3, \
                 content_sha256 = ?2 WHERE id = ?4",
                rusqlite::params![hashes.size as i64, hashes.sha256, hashes.sha1, id],
            )
            .unwrap();

        assert_eq!(f.adopt(), 1);
        assert_eq!(f.cached(id).as_deref(), file.to_str());
    }

    /// Only a local import's `source_url` is a place on this machine, and even then, a file
    /// that has since been replaced is not the one the row means.
    #[test]
    fn test_where_it_was_imported_from_still_has_to_be_the_file() {
        let f = Fixture::new();
        let elsewhere = f.paths.cache_dir.parent().unwrap().join("my-wads");
        fs::create_dir_all(&elsewhere).unwrap();
        let file = elsewhere.join("mine.wad");
        let other: Vec<u8> = BYTES.iter().map(|b| b.wrapping_add(1)).collect();
        fs::write(&file, &other).unwrap();

        let local = db::add_wad(
            &f.conn,
            &NewWad::new("Mine", SourceType::Local).source_url(file.display().to_string()),
        )
        .unwrap();
        let hashes = hash_reader(BYTES).unwrap();
        f.conn
            .execute(
                "UPDATE wads SET file_size = ?1, file_sha256 = ?2, file_sha1 = ?3, \
                 content_sha256 = ?2",
                rusqlite::params![hashes.size as i64, hashes.sha256, hashes.sha1],
            )
            .unwrap();
        assert_eq!(f.adopt(), 0);
        assert_eq!(f.cached(local), None);

        // Nor does a page on the web become a place to look, for any other kind of row.
        fs::write(&file, BYTES).unwrap();
        let url = db::add_wad(
            &f.conn,
            &NewWad::new("Theirs", SourceType::Url).source_url(file.display().to_string()),
        )
        .unwrap();
        f.conn
            .execute(
                "UPDATE wads SET file_size = ?1, file_sha256 = ?2, file_sha1 = ?3, \
                 content_sha256 = ?2 WHERE id = ?4",
                rusqlite::params![hashes.size as i64, hashes.sha256, hashes.sha1, url],
            )
            .unwrap();
        f.conn
            .execute("DELETE FROM wads WHERE id = ?1", [local])
            .unwrap();
        assert_eq!(f.adopt(), 0);
        assert_eq!(f.cached(url), None);
    }

    /// `presence` would otherwise go on looking in the old place too.
    #[test]
    fn test_being_found_clears_where_it_was_last_seen() {
        let f = Fixture::new();
        let id = f.row("road.wad", BYTES);
        f.conn
            .execute(
                "UPDATE wads SET last_known_path = '/gone/road.wad' WHERE id = ?1",
                [id],
            )
            .unwrap();
        fs::write(f.paths.keep_dir.join("road.wad"), BYTES).unwrap();

        assert_eq!(f.adopt(), 1);
        let wad = db::get_wad(&f.conn, id, false).unwrap().unwrap();
        assert_eq!(wad.last_known_path, None);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::db::{NewWad, SourceType, WadUpdate};

    /// A PWAD-looking byte string, distinct per `tag`.
    fn wad_bytes(tag: &str) -> Vec<u8> {
        let mut bytes = b"PWAD".to_vec();
        bytes.extend_from_slice(tag.as_bytes());
        bytes
    }

    fn sha256_of(bytes: &[u8]) -> String {
        hash_reader(bytes).unwrap().sha256
    }

    /// A zip entry: its name and its bytes.
    type Entry<'a> = (&'a str, &'a [u8]);

    /// A zip of `entries`, deflated unless `stored`.
    fn zip_bytes(entries: &[Entry<'_>], stored: bool) -> Vec<u8> {
        let method = if stored {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(method),
                )
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    struct Fixture {
        conn: Connection,
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let conn = db::open_memory().unwrap();
            db::init_db(&conn).unwrap();
            Self {
                conn,
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            path
        }

        /// A WAD recording `path` as its file.
        fn wad(&self, path: &Path) -> i64 {
            let id = db::add_wad(&self.conn, &NewWad::new("Test", SourceType::Idgames)).unwrap();
            let update =
                WadUpdate::new().set_text("cached_path", Some(path.to_string_lossy().into()));
            db::update_wad(&self.conn, id, &update).unwrap();
            id
        }

        fn row(&self, id: i64) -> WadRecord {
            db::get_wad(&self.conn, id, true).unwrap().unwrap()
        }

        fn verify(&self, id: i64) -> Checked {
            let wad = self.row(id);
            let path = PathBuf::from(wad.cached_path.clone().unwrap());
            verify(&self.conn, &wad, &path).unwrap().unwrap()
        }

        fn session(&self, id: i64, started_at: &str, content: Option<&str>) -> i64 {
            self.conn
                .execute(
                    "INSERT INTO sessions (wad_id, started_at, content_sha256) VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, started_at, content],
                )
                .unwrap();
            self.conn.last_insert_rowid()
        }

        fn session_content(&self, session: i64) -> Option<String> {
            self.conn
                .query_row(
                    "SELECT content_sha256 FROM sessions WHERE id = ?1",
                    [session],
                    |row| row.get(0),
                )
                .unwrap()
        }
    }

    fn set_mtime(path: &Path, secs: u64) {
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    }

    // -----------------------------------------------------------------------
    // Hashing
    // -----------------------------------------------------------------------

    #[test]
    fn test_hashes_match_the_standard_vectors() {
        let hashes = hash_reader(&b"abc"[..]).unwrap();
        assert_eq!(hashes.size, 3);
        assert_eq!(
            hashes.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hashes.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn test_a_bare_wad_plays_itself() {
        let f = Fixture::new();
        let found = identify(&f.file("scythe2.wad", &wad_bytes("scythe2"))).unwrap();

        assert_eq!(found.identity.file.sha256, sha256_of(&wad_bytes("scythe2")));
        assert_eq!(found.identity.content_sha256, found.identity.file.sha256);
        assert!(!found.identity.is_container());
        assert!(found.inner.is_empty());
        assert!(found.mtime.is_some());
    }

    /// The idgames shape: what plays is the WAD, so it is what the zip's
    /// content is, and what a WAD extracted from it by hand agrees with.
    #[test]
    fn test_an_idgames_zip_plays_the_wad_inside() {
        let f = Fixture::new();
        let wad = wad_bytes("scythe2");
        let zip = f.file(
            "scythe2.zip",
            &zip_bytes(
                &[("scythe2.wad", &wad), ("scythe2.txt", b"Title: Scythe 2")],
                false,
            ),
        );
        let bare = identify(&f.file("extracted.wad", &wad)).unwrap();

        let found = identify(&zip).unwrap();

        assert_eq!(found.identity.file.size, fs::metadata(&zip).unwrap().len());
        assert_ne!(found.identity.file.sha256, sha256_of(&wad));
        assert_eq!(found.identity.content_sha256, sha256_of(&wad));
        assert_eq!(found.identity.content_sha256, bare.identity.content_sha256);
        assert!(found.identity.is_container());
        assert_eq!(found.inner.len(), 1);
        assert_eq!(found.inner[0].path, "scythe2.wad");
        assert_eq!(found.inner[0].hashes, hash_reader(&wad[..]).unwrap());
    }

    /// A corrected readme, another compression method, another entry order:
    /// a different zip, the same game.
    #[test]
    fn test_a_rezipped_upload_has_the_same_content() {
        let f = Fixture::new();
        let wad = wad_bytes("av");
        let first = identify(&f.file(
            "av.zip",
            &zip_bytes(&[("av.wad", &wad), ("av.txt", b"v1")], false),
        ))
        .unwrap();
        let second = identify(&f.file(
            "av2.zip",
            &zip_bytes(&[("AV.TXT", b"v1, typo fixed"), ("av.wad", &wad)], true),
        ))
        .unwrap();

        assert_ne!(first.identity.file.sha256, second.identity.file.sha256);
        assert_eq!(
            first.identity.content_sha256,
            second.identity.content_sha256
        );
    }

    /// A WAD and its DEH play together, so the content names both, whichever
    /// order the zip lists them in, and is neither one's own hash.
    #[test]
    fn test_several_loadable_files_are_one_content() {
        let f = Fixture::new();
        let (wad, deh) = (wad_bytes("btsx"), b"Patch File for DeHackEd v3.0".to_vec());
        let one = identify(&f.file(
            "a.zip",
            &zip_bytes(&[("btsx.wad", &wad), ("btsx.deh", &deh)], false),
        ))
        .unwrap();
        let other = identify(&f.file(
            "b.zip",
            &zip_bytes(
                &[("btsx.deh", &deh), ("btsx.wad", &wad), ("x.txt", b"")],
                false,
            ),
        ))
        .unwrap();

        assert_eq!(one.identity.content_sha256, other.identity.content_sha256);
        assert_ne!(one.identity.content_sha256, sha256_of(&wad));
        assert_ne!(one.identity.content_sha256, sha256_of(&deh));
        assert_eq!(one.inner.len(), 2);

        let different = identify(&f.file(
            "c.zip",
            &zip_bytes(&[("btsx.wad", &wad), ("btsx.deh", b"Patch v2")], false),
        ))
        .unwrap();
        assert_ne!(
            one.identity.content_sha256,
            different.identity.content_sha256
        );
    }

    /// Anything a sourceport could read more out of than its WADs is played
    /// whole, so a change anywhere in it is a change. Its WADs are still
    /// listed: they identify it.
    #[test]
    fn test_a_resource_archive_plays_itself() {
        let f = Fixture::new();
        let map = wad_bytes("map01");
        let cases: &[(&str, &[Entry<'_>])] = &[
            (
                "maps-dir.pk3",
                &[("maps/map01.wad", &map), ("zmapinfo.txt", b"map MAP01")],
            ),
            (
                "mapinfo.zip",
                &[("pack.wad", &map), ("MAPINFO.txt", b"map MAP01")],
            ),
            (
                "filtered.zip",
                &[("pack.wad", &map), ("filter/doom.doom2/mapinfo.txt", b"")],
            ),
            (
                "decorate.zip",
                &[("pack.wad", &map), ("decorate.monsters.txt", b"")],
            ),
            (
                "sounds.zip",
                &[("pack.wad", &map), ("sounds/dspistol.ogg", b"OggS")],
            ),
            (
                "bare-lump.zip",
                &[("pack.wad", &map), ("ZSCRIPT", b"version \"4.10\"")],
            ),
            (
                "wrapped.zip",
                &[("mod/mywad.wad", &map), ("mod/sprites/trooa1.png", b"PNG")],
            ),
        ];
        for (name, entries) in cases {
            let zip = zip_bytes(entries, false);
            let found = identify(&f.file(name, &zip)).unwrap();
            assert_eq!(
                found.identity.content_sha256,
                sha256_of(&zip),
                "{name} should play as a whole"
            );
            assert!(!found.identity.is_container(), "{name}");
            assert_eq!(found.inner.len(), 1, "{name} lists its WAD");
        }
    }

    /// A zip made by hand usually has its WAD inside a folder, as two in the
    /// author's library do. Still only a wrapper.
    #[test]
    fn test_a_wad_in_a_folder_is_still_wrapped() {
        let f = Fixture::new();
        let wad = wad_bytes("brownian");
        let found = identify(&f.file(
            "be2.zip",
            &zip_bytes(
                &[
                    ("BE/", b""),
                    ("BE/BE.txt", b"Brownian Emotions"),
                    ("BE/BrownianEmotions.wad", &wad),
                    ("BE/BrownianEmotionsinfo.TXT", b"info"),
                ],
                false,
            ),
        ))
        .unwrap();
        assert_eq!(found.identity.content_sha256, sha256_of(&wad));
        assert_eq!(found.inner.len(), 1);
        assert_eq!(found.inner[0].path, "BE/BrownianEmotions.wad");
    }

    #[test]
    fn test_a_zip_with_nothing_to_load_plays_itself() {
        let f = Fixture::new();
        let zip = zip_bytes(&[("readme.txt", b"coming soon")], false);
        let found = identify(&f.file("empty.zip", &zip)).unwrap();
        assert_eq!(found.identity.content_sha256, sha256_of(&zip));
        assert!(found.inner.is_empty());
    }

    /// What a Mac or Windows adds to a zip is not part of the WAD.
    #[test]
    fn test_archiver_junk_is_ignored() {
        let f = Fixture::new();
        let wad = wad_bytes("sunlust");
        let found = identify(&f.file(
            "sunlust.zip",
            &zip_bytes(
                &[
                    ("sunlust.wad", &wad),
                    ("__MACOSX/._sunlust.wad", b"\0\x05\x16\x07"),
                    (".DS_Store", b"Bud1"),
                    ("Thumbs.db", b""),
                ],
                false,
            ),
        ))
        .unwrap();
        assert_eq!(found.identity.content_sha256, sha256_of(&wad));
        assert_eq!(found.inner.len(), 1);
    }

    /// A name says nothing: it is a zip by its first bytes or not at all.
    #[test]
    fn test_a_file_named_zip_is_not_opened_as_one() {
        let f = Fixture::new();
        let found = identify(&f.file("liar.zip", &wad_bytes("liar"))).unwrap();
        assert_eq!(found.identity.content_sha256, sha256_of(&wad_bytes("liar")));
        assert!(found.inner.is_empty());
    }

    /// A zip caco cannot fully read into is taken whole, rather than given a
    /// content hash that leaves out part of what plays.
    #[test]
    fn test_an_unreadable_entry_takes_the_zip_whole() {
        let f = Fixture::new();
        let wad: Vec<u8> = (0..4096u32).flat_map(|i| i.to_le_bytes()).collect();
        let mut zip = zip_bytes(&[("big.wad", &wad), ("big.txt", b"")], false);
        // Inside big.wad's compressed data, well clear of every header.
        zip[200] ^= 0xff;
        zip[201] ^= 0xff;
        let found = identify(&f.file("broken.zip", &zip)).unwrap();
        assert_eq!(found.identity.content_sha256, sha256_of(&zip));
        assert!(found.inner.is_empty());
    }

    #[test]
    fn test_container_rules() {
        assert!(is_container([
            "a.wad",
            "a.txt",
            "screens/",
            "screens/1.png"
        ]));
        assert!(is_container(["A.WAD", "A.DEH", "file_id.diz", "demos.lmp"]));
        assert!(!is_container(["a.txt"]));
        assert!(is_container(["sub/a.wad", "sub/a.txt"]));
        assert!(!is_container(["maps/map01.wad"]));
        assert!(!is_container(["a.wad", "sprites/trooa1.png"]));
        assert!(!is_container([
            "wrapper/a.wad",
            "wrapper/Graphics/titlepic.png"
        ]));
        assert!(!is_container(["a.wad", "dehacked.txt"]));
        assert!(!is_container(["a.wad", "music.mid"]));
        assert!(!is_container(["a.wad", "LANGUAGE"]));
        assert!(!is_container(["a.wad", "inner.zip"]));
        // A loadable file is what plays even when its name is a lump's.
        assert!(is_container(["dehacked.deh", "mapinfo.wad"]));
    }

    // -----------------------------------------------------------------------
    // Recording and verifying
    // -----------------------------------------------------------------------

    #[test]
    fn test_a_new_file_is_hashed_and_recorded() {
        let f = Fixture::new();
        let wad = wad_bytes("ramp");
        let path = f.file("ramp.zip", &zip_bytes(&[("ramp.wad", &wad)], false));
        let id = f.wad(&path);

        let Checked::Hashed(identity) = f.verify(id) else {
            panic!("a file with no identity is hashed");
        };
        assert_eq!(f.row(id).identity, Some(identity));
        assert!(f.row(id).file_mtime.is_some());
        let inner = inner_files(&f.conn, id).unwrap();
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].hashes.sha256, sha256_of(&wad));
    }

    /// The stamp is the whole test: a file with the recorded size and
    /// modification time is not read again, and one without is.
    #[test]
    fn test_the_stamp_decides_whether_a_file_is_read_again() {
        let f = Fixture::new();
        let path = f.file("w.wad", b"PWAD v1");
        set_mtime(&path, 1_700_000_000);
        let id = f.wad(&path);
        let first = f.verify(id).into_identity();

        // Same size, same mtime, different bytes: trusted, so not noticed.
        fs::write(&path, b"PWAD v2").unwrap();
        set_mtime(&path, 1_700_000_000);
        assert_eq!(f.verify(id), Checked::Unchanged(first.clone()));

        // A new mtime is a new copy.
        set_mtime(&path, 1_700_000_100);
        let Checked::Hashed(identity) = f.verify(id) else {
            panic!("a file with a new stamp is hashed");
        };
        assert_ne!(identity, first);
        assert_eq!(f.verify(id), Checked::Unchanged(identity));
    }

    #[test]
    fn test_a_file_that_plays_differently_drops_its_analysis() {
        let f = Fixture::new();
        let wad = wad_bytes("v1");
        let path = f.file(
            "w.zip",
            &zip_bytes(&[("w.wad", &wad), ("w.txt", b"a")], false),
        );
        let id = f.wad(&path);
        f.verify(id);
        let analysis = |f: &Fixture| -> i64 {
            f.conn
                .query_row(
                    "SELECT COUNT(*) FROM wad_analysis WHERE wad_id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .unwrap()
        };
        let save = |f: &Fixture| {
            f.conn
                .execute(
                    "INSERT OR REPLACE INTO wad_analysis (wad_id, analysis_json) VALUES (?1, '{}')",
                    [id],
                )
                .unwrap();
        };

        // Re-zipped around the same WAD: the analysis still describes it.
        save(&f);
        fs::write(&path, zip_bytes(&[("w.wad", &wad), ("w.txt", b"b")], true)).unwrap();
        set_mtime(&path, 1_700_000_000);
        assert!(matches!(f.verify(id), Checked::Hashed(_)));
        assert_eq!(analysis(&f), 1);

        // An author's fix: it does not.
        fs::write(&path, zip_bytes(&[("w.wad", &wad_bytes("v2"))], false)).unwrap();
        set_mtime(&path, 1_700_000_100);
        assert!(matches!(f.verify(id), Checked::Hashed(_)));
        assert_eq!(analysis(&f), 0);
    }

    /// The upgrade case: a WAD in progress before caco recorded identities
    /// gets its newest session attributed to the file found in place, so a
    /// later change is noticed. Only that session, and only the first time.
    #[test]
    fn test_the_first_identity_is_what_the_newest_session_played() {
        let f = Fixture::new();
        let path = f.file("w.wad", b"PWAD v1");
        let id = f.wad(&path);
        let older = f.session(id, "2026-10-01T10:00:00+00:00", None);
        let newest = f.session(id, "2026-10-02T10:00:00+00:00", None);

        let first = f.verify(id).into_identity();

        assert_eq!(
            f.session_content(newest),
            Some(first.content_sha256.clone())
        );
        assert_eq!(f.session_content(older), None);

        fs::write(&path, b"PWAD v2, longer").unwrap();
        let second = f.verify(id).into_identity();
        assert_ne!(second, first);
        assert_eq!(
            f.session_content(newest),
            Some(first.content_sha256),
            "only a first identity is attributed"
        );
    }

    #[test]
    fn test_a_session_that_recorded_its_content_keeps_it() {
        let f = Fixture::new();
        let id = f.wad(&f.file("w.wad", b"PWAD"));
        let session = f.session(id, "2026-10-02T10:00:00+00:00", Some("abc123"));

        f.verify(id);

        assert_eq!(f.session_content(session).as_deref(), Some("abc123"));
    }

    /// Hashing reads a whole file, which on a slow mount takes long enough for
    /// the row to be relinked meanwhile. That link must survive.
    #[test]
    fn test_a_row_that_moved_on_is_not_overwritten() {
        let f = Fixture::new();
        let old = f.file("old.wad", b"PWAD old");
        let id = f.wad(&old);
        let read = f.row(id);
        let relinked = f.file("new.wad", b"PWAD new one");
        db::update_wad(
            &f.conn,
            id,
            &WadUpdate::new().set_text("cached_path", Some(relinked.to_string_lossy().into())),
        )
        .unwrap();

        let checked = verify(&f.conn, &read, &old).unwrap().unwrap();

        assert!(matches!(checked, Checked::Superseded(_)));
        assert_eq!(f.row(id).identity, None);
        assert!(inner_files(&f.conn, id).unwrap().is_empty());
    }

    /// The hash of a file that is gone is what names it to get it back.
    #[test]
    fn test_the_identity_outlives_the_file() {
        let f = Fixture::new();
        let path = f.file("gone.wad", b"PWAD");
        let id = f.wad(&path);
        let identity = f.verify(id).into_identity();

        fs::remove_file(&path).unwrap();
        db::clear_cached_path(&f.conn, id).unwrap();

        assert_eq!(f.row(id).identity, Some(identity));
        assert_eq!(
            verify(&f.conn, &f.row(id), &path).unwrap(),
            None,
            "an unreadable file is no identity, not an error"
        );
    }

    #[test]
    fn test_record_joins_an_open_transaction() {
        let f = Fixture::new();
        let path = f.file("w.wad", b"PWAD");
        let id = f.wad(&path);
        let found = identify(&path).unwrap();

        let recorded = db::with_transaction(&f.conn, |tx| record(tx, id, &path, &found)).unwrap();

        assert!(recorded);
        assert_eq!(f.row(id).identity, Some(found.identity));
    }

    #[test]
    fn test_check_library_reports_live_rows_once() {
        let f = Fixture::new();
        let live = f.wad(&f.file("live.wad", b"PWAD live"));
        let trashed = f.wad(&f.file("trashed.wad", b"PWAD trashed"));
        db::delete_wad(&f.conn, trashed, false).unwrap();
        let gone = f.wad(&f.dir.path().join("never-there.wad"));

        assert_eq!(check_library(&f.conn).unwrap(), 1);
        assert!(f.row(live).identity.is_some());
        assert!(
            f.row(trashed).identity.is_some(),
            "trashed rows can come back"
        );
        assert!(f.row(gone).identity.is_none());

        assert_eq!(check_library(&f.conn).unwrap(), 0);

        let path = PathBuf::from(f.row(live).cached_path.unwrap());
        let before = f.row(live).identity;
        fs::write(&path, b"PWAD live, replaced by a sync").unwrap();
        assert_eq!(check_library(&f.conn).unwrap(), 1);
        assert_ne!(f.row(live).identity, before);
    }

    // -----------------------------------------------------------------------
    // What was played
    // -----------------------------------------------------------------------

    #[test]
    fn test_change_since_last_played() {
        let f = Fixture::new();
        let id = f.wad(&f.file("w.wad", b"PWAD"));
        let current = f.verify(id).into_identity();
        let change = |f: &Fixture| change_since_last_played(&f.conn, id, &current).unwrap();

        // `verify` attributed nothing: the WAD had never been played.
        assert_eq!(change(&f), None);

        f.session(id, "2026-10-01T10:00:00+00:00", Some("old-content"));
        assert_eq!(
            change(&f),
            Some(ContentChange {
                last_played_at: "2026-10-01T10:00:00+00:00".into(),
                played: "old-content".into(),
                current: current.content_sha256.clone(),
            })
        );

        // A newer session that could not hash its file says nothing either way.
        f.session(id, "2026-10-02T10:00:00+00:00", None);
        assert_eq!(change(&f).map(|c| c.played).as_deref(), Some("old-content"));

        // Played anyway: the newest session is on the new file.
        f.session(
            id,
            "2026-10-03T10:00:00+00:00",
            Some(&current.content_sha256),
        );
        assert_eq!(change(&f), None);
    }

    // -----------------------------------------------------------------------
    // Duplicates
    // -----------------------------------------------------------------------

    /// `DUPLICATE_SQL` and `same_content` are two spellings of one rule.
    #[test]
    fn test_duplicate_sql_matches_rust() {
        let f = Fixture::new();
        let wad = wad_bytes("dup");
        let zipped = f.wad(&f.file("dup.zip", &zip_bytes(&[("dup.wad", &wad)], false)));
        let bare = f.wad(&f.file("dup.wad", &wad));
        let trashed = f.wad(&f.file("copy.wad", &wad));
        let lone_trashed_twin =
            f.wad(&f.file("other.zip", &zip_bytes(&[("o.wad", b"PWAD o")], false)));
        let other_twin = f.wad(&f.file("o.wad", b"PWAD o"));
        let unique = f.wad(&f.file("unique.wad", b"PWAD unique"));
        let unhashed = db::add_wad(&f.conn, &NewWad::new("Unhashed", SourceType::Url)).unwrap();
        check_library(&f.conn).unwrap();
        db::delete_wad(&f.conn, trashed, false).unwrap();
        db::delete_wad(&f.conn, lone_trashed_twin, false).unwrap();

        let ids = |query: &str| -> Vec<i64> {
            let mut ids: Vec<i64> = db::search_wads(&f.conn, Some(query), None, false, false, 0)
                .unwrap()
                .iter()
                .map(|w| w.id)
                .collect();
            ids.sort_unstable();
            ids
        };
        let rust: Vec<i64> = [zipped, bare, other_twin, unique, unhashed]
            .into_iter()
            .filter(|&id| !same_content(&f.conn, id).unwrap().is_empty())
            .collect();

        // A live WAD with a twin in the trash is a duplicate only of the trash,
        // which is not the library.
        assert_eq!(rust, vec![zipped, bare]);
        assert_eq!(ids("dup:yes"), rust);
        assert_eq!(ids("dup:no"), vec![other_twin, unique, unhashed]);
        assert_eq!(ids("^dup:yes"), ids("dup:no"));
        assert_eq!(
            same_content(&f.conn, zipped).unwrap(),
            vec![(bare, "Test".to_string())]
        );
    }

    /// Imports count the trash, as `find_duplicate` does.
    #[test]
    fn test_find_by_content_includes_the_trash() {
        let f = Fixture::new();
        let id = f.wad(&f.file("w.wad", b"PWAD"));
        let content = f.verify(id).into_identity().content_sha256;
        db::delete_wad(&f.conn, id, false).unwrap();

        assert_eq!(
            find_by_content(&f.conn, &content).unwrap(),
            Some((id, "Test".to_string()))
        );
        assert_eq!(find_by_content(&f.conn, "0000").unwrap(), None);
    }
}
