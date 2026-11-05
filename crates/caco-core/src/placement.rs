//! Putting a file in one of caco's store directories without costing another WAD
//! its own.
//!
//! Which directory is [`crate::config::wad_store_dir`]'s answer. This is what comes
//! after that: which name the file takes there, and getting it there so that nothing
//! half-written ever sits under a name a launch could pick up. Mirrors and linking a
//! file both go through it, and they had different rules before: a link overwrote
//! whatever had the same name, which for a `map01.wad` was some other WAD's only copy.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::LinkMode;
use crate::identity;

/// Where a file belongs in a store directory.
#[derive(Debug, PartialEq, Eq)]
pub enum Destination {
    /// The file is already there, under this name.
    AlreadyHere(PathBuf),
    /// Nothing is in the way of writing it here.
    Write(PathBuf),
}

/// Where a file goes: under its own name, which saves and sourceports may depend on,
/// unless that belongs to something else. Unrelated WADs share names (`map01.wad`),
/// and overwriting one would cost that WAD its file, so the second gets its hash in
/// the name. Directly under the store dir either way, which is where the presence
/// check looks for a file by name.
pub fn destination(dir: &Path, filename: &str, sha256: &str) -> io::Result<Destination> {
    let plain = dir.join(filename);
    let hashed = dir.join(name_with_hash(filename, sha256));
    for candidate in [plain, hashed.clone()] {
        match identity::hash_file(&candidate) {
            Ok(hashes) if hashes.sha256 == sha256 => {
                return Ok(Destination::AlreadyHere(candidate));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(Destination::Write(candidate));
            }
            Err(e) => {
                let context = format!("could not read {}: {e}", candidate.display());
                return Err(io::Error::new(e.kind(), context));
            }
        }
    }
    // Something under the hashed name is not what its name says. Nothing relies on
    // that, so it can go.
    Ok(Destination::Write(hashed))
}

/// `road.zip` as `road-1a2b3c4d.zip`.
pub(crate) fn name_with_hash(filename: &str, sha256: &str) -> String {
    let short = sha256.get(..8).unwrap_or(sha256);
    match filename.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => format!("{stem}-{short}.{extension}"),
        _ => format!("{filename}-{short}"),
    }
}

/// A name beside `dest` for a file still being written, which nothing looks for. The
/// presence check goes by exact name, and a mirror's index never names it.
pub fn partial_beside(dest: &Path) -> PathBuf {
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    dest.with_file_name(format!("{name}.partial"))
}

/// Put the file at `source` in `dir`, and say where it ended up.
///
/// Hashes only when its name is taken, so that the ordinary case, on the UI thread
/// for a hand-picked file, costs what the copy costs and no more.
pub fn place_file(source: &Path, dir: &Path, mode: LinkMode) -> io::Result<PathBuf> {
    let Some(filename) = source.file_name().map(|name| name.to_string_lossy()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} does not name a file", source.display()),
        ));
    };
    fs::create_dir_all(dir)?;

    let plain = dir.join(filename.as_ref());
    let dest = if fs::symlink_metadata(&plain).is_err() {
        plain
    } else {
        let sha256 = identity::hash_file(source)?.sha256;
        match destination(dir, &filename, &sha256)? {
            Destination::Write(path) => path,
            Destination::AlreadyHere(path) => {
                // It may be the very file being placed, and then it must stay.
                if mode == LinkMode::Move && !is_same_file(source, &path) {
                    remove_source(source);
                }
                return Ok(path);
            }
        }
    };

    transfer(source, &dest, mode)?;
    Ok(dest)
}

fn transfer(source: &Path, dest: &Path, mode: LinkMode) -> io::Result<()> {
    // Fails across filesystems, which a Downloads folder and a data dir can be on,
    // and the copy below is then how it moves.
    if mode == LinkMode::Move && fs::rename(source, dest).is_ok() {
        return Ok(());
    }
    let partial = partial_beside(dest);
    if let Err(e) = fs::copy(source, &partial).and_then(|_| fs::rename(&partial, dest)) {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    if mode == LinkMode::Move {
        remove_source(source);
    }
    Ok(())
}

/// The file has been placed by now, so a source that will not go is not a failure.
/// It only stays where the user can see it.
fn remove_source(source: &Path) {
    if let Err(e) = fs::remove_file(source) {
        tracing::warn!("could not remove {}: {e}", source.display());
    }
}

fn is_same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        // Unknown, so nothing gets deleted on the strength of it.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        downloads: PathBuf,
        store: PathBuf,
        _root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let downloads = root.path().join("downloads");
            fs::create_dir_all(&downloads).unwrap();
            Self {
                downloads,
                store: root.path().join("store"),
                _root: root,
            }
        }

        fn download(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.downloads.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }

        fn in_store(&self, name: &str, bytes: &[u8]) -> PathBuf {
            fs::create_dir_all(&self.store).unwrap();
            let path = self.store.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }

        fn store_names(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.store)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    #[test]
    fn test_moving_takes_the_file_out_of_where_it_was() {
        let f = Fixture::new();
        let source = f.download("road.zip", b"road");

        let dest = place_file(&source, &f.store, LinkMode::Move).unwrap();

        assert_eq!(dest, f.store.join("road.zip"));
        assert_eq!(fs::read(&dest).unwrap(), b"road");
        assert!(!source.exists());
    }

    #[test]
    fn test_copying_leaves_the_file_where_it_was() {
        let f = Fixture::new();
        let source = f.download("road.zip", b"road");

        let dest = place_file(&source, &f.store, LinkMode::Copy).unwrap();

        assert_eq!(fs::read(&dest).unwrap(), b"road");
        assert_eq!(fs::read(&source).unwrap(), b"road");
        assert_eq!(f.store_names(), ["road.zip"]);
    }

    /// Unrelated WADs share names, and the second must not cost the first its file.
    #[test]
    fn test_a_name_another_file_has_is_not_overwritten() {
        for mode in [LinkMode::Copy, LinkMode::Move] {
            let f = Fixture::new();
            let theirs = f.in_store("map01.wad", b"another WAD's only copy");
            let source = f.download("map01.wad", b"this one");

            let dest = place_file(&source, &f.store, mode).unwrap();

            assert_eq!(fs::read(&theirs).unwrap(), b"another WAD's only copy");
            assert_ne!(dest, theirs);
            assert_eq!(fs::read(&dest).unwrap(), b"this one");
            let name = dest.file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.starts_with("map01-"), "{name}");
            assert!(name.ends_with(".wad"), "{name}");
            assert_eq!(dest.parent().unwrap(), f.store);
        }
    }

    #[test]
    fn test_a_file_already_there_is_not_written_twice() {
        let f = Fixture::new();
        let there = f.in_store("road.zip", b"road");

        let source = f.download("road.zip", b"road");
        let dest = place_file(&source, &f.store, LinkMode::Copy).unwrap();
        assert_eq!(dest, there);
        assert!(source.exists(), "copying leaves the source alone");
        assert_eq!(f.store_names(), ["road.zip"]);

        // Moved, it has arrived, so the spare goes.
        let dest = place_file(&source, &f.store, LinkMode::Move).unwrap();
        assert_eq!(dest, there);
        assert!(!source.exists());
        assert_eq!(f.store_names(), ["road.zip"]);
    }

    /// The Downloads folder can be the store dir, and a hand-picked file can already
    /// be in it. Moving a file onto itself must not delete it.
    #[test]
    fn test_a_file_placed_where_it_already_is_stays() {
        for mode in [LinkMode::Copy, LinkMode::Move] {
            let f = Fixture::new();
            let there = f.in_store("road.zip", b"road");

            let dest = place_file(&there, &f.store, mode).unwrap();

            assert_eq!(dest, there);
            assert_eq!(fs::read(&there).unwrap(), b"road");
        }
    }

    #[test]
    fn test_a_failure_leaves_nothing_behind() {
        let f = Fixture::new();
        let missing = f.downloads.join("gone.zip");

        for mode in [LinkMode::Copy, LinkMode::Move] {
            let failure = place_file(&missing, &f.store, mode).unwrap_err();
            assert_eq!(failure.kind(), io::ErrorKind::NotFound);
            assert_eq!(f.store_names(), Vec::<String>::new());
        }
    }

    /// A rename cannot cross filesystems, which is the usual case when a data dir is
    /// on a different drive from Downloads. Only testable with two filesystems to
    /// hand, so on anything else this has nothing to say.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_moving_across_filesystems_still_moves_it() {
        use std::os::unix::fs::MetadataExt;

        let f = Fixture::new();
        let Ok(other) = tempfile::tempdir_in("/dev/shm") else {
            return;
        };
        let device = |path: &Path| fs::metadata(path).unwrap().dev();
        if device(other.path()) == device(&f.downloads) {
            return;
        }
        let source = other.path().join("road.zip");
        fs::write(&source, b"road").unwrap();

        let dest = place_file(&source, &f.store, LinkMode::Move).unwrap();

        assert_eq!(fs::read(&dest).unwrap(), b"road");
        assert!(!source.exists());
        assert_eq!(f.store_names(), ["road.zip"]);
    }

    #[test]
    fn test_a_hash_goes_before_the_extension() {
        let hash = "1a2b3c4d5e6f";
        assert_eq!(name_with_hash("road.zip", hash), "road-1a2b3c4d.zip");
        assert_eq!(name_with_hash("road.v2.pk3", hash), "road.v2-1a2b3c4d.pk3");
        assert_eq!(name_with_hash("road", hash), "road-1a2b3c4d");
        assert_eq!(name_with_hash(".road", hash), ".road-1a2b3c4d");
    }
}
