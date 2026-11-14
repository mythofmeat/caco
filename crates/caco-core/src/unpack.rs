//! A zip that only wraps what plays, unpacked for a sourceport.
//!
//! [`crate::identity`] already reads past such a zip: its content is the files
//! inside it, and that is what decides whether a WAD has changed. A launch has
//! to do the same, because no sourceport can be relied on to look inside one.
//! uzdoom loads a `.wad` at a zip's root and nothing else, so a `.pk3` in a zip
//! played as the IWAD alone, and a `.deh` beside the WAD was applied only with
//! its `dehload` setting turned on. nyan-doom loads the WADs at a zip's root and
//! not one inside a folder. So a launch hands the sourceport the files
//! themselves, unpacked into [`config::unpacked_dir`], and never the zip.
//!
//! # Once per content
//!
//! A folder is named after the content it holds, and is written under a
//! temporary name and renamed into place only once what was written hashes to
//! that content. So a folder of that name is a whole one, and a later launch
//! uses it as it is, after checking each file's size against the zip's
//! listing, which costs no decompression. It is a second copy of what is in the
//! zip, so it lasts only as long as a WAD with that content has its file on
//! this machine ([`prune`]).

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::Connection;
use zip::ZipArchive;

use crate::companion_service::is_deh_bex;
use crate::config;
use crate::identity::{self, MAX_INNER_BYTES, MAX_INNER_FILES};

/// How old a half-written folder has to be before [`prune`] takes it for one a
/// launch left behind, rather than one being written right now.
const ABANDONED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// The files to hand a sourceport for the zip at `zip`, whose content is
/// `content_sha256`, unpacked under `root` if they are not there already.
///
/// In the order to load them: by file name, ignoring case, wherever in the zip
/// they sit, then DEH and BEX patches the same way, which is the order
/// nyan-doom loads them from a zip itself.
///
/// Fails, leaving nothing behind, on a zip with nothing loadable in it, an
/// entry whose name would climb out of its folder, or files that do not hash to
/// `content_sha256`, which means the zip is not the one that was checked.
pub fn unpack(zip: &Path, content_sha256: &str, root: &Path) -> io::Result<Vec<PathBuf>> {
    if !is_content_hash(content_sha256) {
        return Err(invalid(format!("'{content_sha256}' is not a content hash")));
    }
    let mut archive = ZipArchive::new(File::open(zip)?)?;
    let files = listing(&mut archive)?;
    let folder = root.join(content_sha256);

    if !is_whole(&folder, &files) {
        fs::create_dir_all(root)?;
        let part = root.join(format!(".{content_sha256}.{}.part", std::process::id()));
        if let Err(e) = write_part(&mut archive, &files, &part, content_sha256)
            .and_then(|()| put_in_place(&part, &folder, &files))
        {
            let _ = fs::remove_dir_all(&part);
            return Err(e);
        }
    }
    Ok(files
        .iter()
        .map(|file| folder.join(&file.relative))
        .collect())
}

/// [`prune`] [`config::unpacked_dir`], for a caller with nothing to do about a
/// failure but note it: all that is left over is space.
pub fn tidy(conn: &Connection) {
    let root = config::unpacked_dir();
    if let Err(e) = prune(conn, &root) {
        tracing::warn!("cannot tidy {}: {e}", root.display());
    }
}

/// Remove what was unpacked for content that no WAD whose file is on this
/// machine has any more, and folders a launch died while writing. Returns how
/// many went.
///
/// Leaves anything in `root` it did not make. A WAD in the Trash keeps nothing,
/// since it cannot be played until it is restored, and then it is unpacked
/// again.
pub fn prune(conn: &Connection, root: &Path) -> crate::Result<usize> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let keep: HashSet<String> = conn
        .prepare(
            "SELECT content_sha256 FROM wads WHERE deleted_at IS NULL \
             AND COALESCE(cached_path, '') <> '' AND content_sha256 IS NOT NULL",
        )?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let unwanted = if is_content_hash(name) {
            !keep.contains(name)
        } else if name.starts_with('.') && name.ends_with(".part") {
            entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age > ABANDONED_AFTER)
        } else {
            false
        };
        if unwanted {
            match fs::remove_dir_all(entry.path()) {
                Ok(()) => removed += 1,
                Err(e) => tracing::warn!("cannot remove {}: {e}", entry.path().display()),
            }
        }
    }
    Ok(removed)
}

/// A loadable file in the zip, and where it unpacks to.
struct Listed {
    index: usize,
    name: String,
    /// Under the content's folder: the entry's own path in the zip.
    relative: PathBuf,
    /// Uncompressed, as the zip's central directory gives it.
    size: u64,
}

/// The zip's loadable files in the order to load them, without decompressing
/// any of them.
fn listing<R: Read + Seek>(archive: &mut ZipArchive<R>) -> io::Result<Vec<Listed>> {
    let loadable = identity::loadable_entries(archive);
    if loadable.is_empty() {
        return Err(invalid("it holds nothing a sourceport loads".to_string()));
    }
    if loadable.len() > MAX_INNER_FILES {
        return Err(invalid(format!("it holds over {MAX_INNER_FILES} files")));
    }

    let mut files = Vec::with_capacity(loadable.len());
    for (index, name) in loadable {
        let relative = relative_path(&name)
            .ok_or_else(|| invalid(format!("'{name}' is not a path inside the zip")))?;
        let size = archive.by_index_raw(index)?.size();
        files.push(Listed {
            index,
            name,
            relative,
            size,
        });
    }
    files.sort_by_cached_key(|file| {
        (
            is_deh_bex(&file.relative),
            identity::base_name(&file.name).to_lowercase(),
            file.name.clone(),
        )
    });
    Ok(files)
}

/// An entry's name as a path that stays inside the folder it is unpacked into,
/// or `None` for one that would not. Zips made on Windows sometimes separate
/// with backslashes.
fn relative_path(name: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" => {}
            "." | ".." => return None,
            part => path.push(part),
        }
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

/// Whether `folder` holds every file in `files`, each the size the zip says.
fn is_whole(folder: &Path, files: &[Listed]) -> bool {
    files.iter().all(|file| {
        fs::metadata(folder.join(&file.relative))
            .is_ok_and(|meta| meta.is_file() && meta.len() == file.size)
    })
}

/// Write `files` out under `part`, and check that together they are
/// `content_sha256`.
fn write_part<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    files: &[Listed],
    part: &Path,
    content_sha256: &str,
) -> io::Result<()> {
    // One this process left, if it had this id before.
    let _ = fs::remove_dir_all(part);
    fs::create_dir(part)?;

    let mut hashes = Vec::with_capacity(files.len());
    for file in files {
        let target = part.join(&file.relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let entry = archive.by_index(file.index)?;
        let written = identity::hash_reader(Tee {
            reader: entry.take(MAX_INNER_BYTES),
            writer: File::create(&target)?,
        })?;
        if written.size >= MAX_INNER_BYTES {
            return Err(invalid(format!("'{}' is too large", file.name)));
        }
        hashes.push(written.sha256);
    }

    if identity::content_digest(hashes.iter().map(String::as_str)) == content_sha256 {
        Ok(())
    } else {
        Err(invalid(
            "what it holds is not what caco recorded, so the file has changed since it was checked"
                .to_string(),
        ))
    }
}

/// Rename a finished `part` to `folder`, unless another launch finished the
/// same folder first, whose sourceport may be reading it.
fn put_in_place(part: &Path, folder: &Path, files: &[Listed]) -> io::Result<()> {
    if is_whole(folder, files) {
        return fs::remove_dir_all(part);
    }
    // Something incomplete, which a launch can never have left there, since a
    // folder only gets its name once it is whole: someone else's doing.
    match fs::remove_dir_all(folder) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    fs::rename(part, folder)
}

fn is_content_hash(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// A reader that writes everything read through it to `writer`, so that a file
/// is hashed in the same pass that writes it out.
struct Tee<R, W> {
    reader: R,
    writer: W,
}

impl<R: Read, W: Write> Read for Tee<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.reader.read(buf)?;
        self.writer.write_all(&buf[..n])?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::db::{self, NewWad, SourceType};

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn root(&self) -> PathBuf {
            self.dir.path().join("unpacked")
        }

        /// A zip of `entries`, and its content as `identity` reads it.
        fn zip(&self, entries: &[(&str, &[u8])]) -> (PathBuf, String) {
            let path = self.dir.path().join("wad.zip");
            fs::write(&path, zip_of(entries)).unwrap();
            let found = identity::identify(&path).unwrap();
            assert!(
                found.identity.is_container(),
                "{entries:?} wraps what plays"
            );
            (path, found.identity.content_sha256)
        }

        /// What is in `root`, by name.
        fn left(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.root())
                .map(|entries| {
                    entries
                        .map(|e| e.unwrap().file_name().into_string().unwrap())
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            names
        }
    }

    /// The Warrens: Burrow of the Were, which uzdoom played as Doom 2.
    #[test]
    fn test_a_pk3_in_a_zip_is_unpacked_without_its_readme() {
        let f = Fixture::new();
        let (zip, content) = f.zip(&[
            ("README.txt", b"read me"),
            ("BurrowOfTheWere_1.4.3.pk3", b"PK3 bytes"),
            ("CREDITS.txt", b"thanks"),
        ]);

        let files = unpack(&zip, &content, &f.root()).unwrap();

        assert_eq!(
            files,
            [f.root().join(&content).join("BurrowOfTheWere_1.4.3.pk3")]
        );
        assert_eq!(fs::read(&files[0]).unwrap(), b"PK3 bytes");
        assert_eq!(f.left(), [content]);
    }

    #[test]
    fn test_files_load_by_name_with_patches_last() {
        let f = Fixture::new();
        let (zip, content) = f.zip(&[
            ("Pack/zz.bex", b"bex"),
            ("Pack/Map.wad", b"map"),
            ("Pack/maps.deh", b"deh"),
            ("Pack/music.WAD", b"music"),
            ("Pack/readme.txt", b"text"),
            ("Pack/a_res.wad", b"resources"),
        ]);

        let files = unpack(&zip, &content, &f.root()).unwrap();

        let folder = f.root().join(&content).join("Pack");
        assert_eq!(
            files,
            ["a_res.wad", "Map.wad", "music.WAD", "maps.deh", "zz.bex"]
                .map(|name| folder.join(name))
        );
    }

    #[test]
    fn test_a_second_launch_uses_what_the_first_unpacked() {
        let f = Fixture::new();
        let (zip, content) = f.zip(&[("map.wad", b"PWAD map"), ("map.txt", b"text")]);
        let files = unpack(&zip, &content, &f.root()).unwrap();
        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        File::options()
            .write(true)
            .open(&files[0])
            .unwrap()
            .set_modified(long_ago)
            .unwrap();

        assert_eq!(unpack(&zip, &content, &f.root()).unwrap(), files);
        let modified = fs::metadata(&files[0]).unwrap().modified().unwrap();
        assert_eq!(modified, long_ago, "not written again");
    }

    #[test]
    fn test_a_damaged_folder_is_unpacked_again() {
        let f = Fixture::new();
        let (zip, content) = f.zip(&[("a.wad", b"PWAD a"), ("b.wad", b"PWAD b")]);
        let files = unpack(&zip, &content, &f.root()).unwrap();
        fs::write(&files[0], b"PW").unwrap();
        fs::remove_file(&files[1]).unwrap();

        assert_eq!(unpack(&zip, &content, &f.root()).unwrap(), files);
        assert_eq!(fs::read(&files[0]).unwrap(), b"PWAD a");
        assert_eq!(fs::read(&files[1]).unwrap(), b"PWAD b");
        assert_eq!(f.left(), [content]);
    }

    /// A zip replaced between the check and the launch.
    #[test]
    fn test_files_that_are_not_the_content_are_refused_and_left_nowhere() {
        let f = Fixture::new();
        let (zip, _) = f.zip(&[("map.wad", b"PWAD map")]);
        let other = "0".repeat(64);

        let e = unpack(&zip, &other, &f.root()).unwrap_err();

        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(f.left().is_empty(), "{:?}", f.left());
    }

    #[test]
    fn test_a_name_that_climbs_out_of_its_folder_is_refused() {
        let f = Fixture::new();
        let zip = f.dir.path().join("evil.zip");
        fs::write(&zip, zip_of(&[("../../evil.wad", b"PWAD")])).unwrap();
        let content = identity::identify(&zip).unwrap().identity.content_sha256;

        let e = unpack(&zip, &content, &f.root()).unwrap_err();

        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(!f.dir.path().join("evil.wad").exists());
        assert!(f.left().is_empty());
    }

    #[test]
    fn test_a_content_hash_names_a_folder_and_nothing_else_does() {
        let f = Fixture::new();
        let (zip, _) = f.zip(&[("map.wad", b"PWAD map")]);
        for content in ["../escape", "", &"A".repeat(64)] {
            let e = unpack(&zip, content, &f.root()).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{content:?}");
        }
        assert!(f.left().is_empty());
    }

    #[test]
    fn test_relative_paths_stay_inside() {
        assert_eq!(relative_path("a.wad"), Some(PathBuf::from("a.wad")));
        assert_eq!(
            relative_path("Pack\\a.wad"),
            Some(PathBuf::from("Pack/a.wad"))
        );
        assert_eq!(
            relative_path("/abs/a.wad"),
            Some(PathBuf::from("abs/a.wad"))
        );
        assert_eq!(relative_path("Pack/../../a.wad"), None);
        assert_eq!(relative_path("./a.wad"), None);
        assert_eq!(relative_path("//"), None);
    }

    #[test]
    fn test_prune_keeps_what_a_wad_here_plays_and_nothing_else() {
        let f = Fixture::new();
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        let wad = |title: &str, content: &str, cached: Option<&str>, trashed: bool| {
            let mut new = NewWad::new(title, SourceType::Idgames);
            if let Some(path) = cached {
                new = new.cached_path(path);
            }
            let id = db::add_wad(&conn, &new).unwrap();
            conn.execute(
                "UPDATE wads SET content_sha256 = ?1, \
                 deleted_at = CASE WHEN ?2 THEN CURRENT_TIMESTAMP END WHERE id = ?3",
                rusqlite::params![content, trashed, id],
            )
            .unwrap();
        };
        let [here, gone, trashed, nobody] = ["a", "b", "c", "d"].map(|c| c.repeat(64));
        wad("Here", &here, Some("/wads/here.zip"), false);
        wad("Gone", &gone, None, false);
        wad("Trashed", &trashed, Some("/wads/trashed.zip"), true);
        let old_part = format!(".{here}.1.part");
        let new_part = format!(".{here}.2.part");
        for name in [
            &here, &gone, &trashed, &nobody, &old_part, &new_part, "notes",
        ] {
            fs::create_dir_all(f.root().join(name).join("inside")).unwrap();
        }
        File::open(f.root().join(&old_part))
            .unwrap()
            .set_modified(SystemTime::now() - ABANDONED_AFTER * 2)
            .unwrap();

        assert_eq!(prune(&conn, &f.root()).unwrap(), 4);

        let mut kept = vec![here, new_part, "notes".to_string()];
        kept.sort();
        assert_eq!(f.left(), kept);
    }

    #[test]
    fn test_prune_without_a_folder_is_nothing() {
        let f = Fixture::new();
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        assert_eq!(prune(&conn, &f.root()).unwrap(), 0);
    }
}
