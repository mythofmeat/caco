//! A WAD's demos: the files in `<data>/demos/`, what each says about itself
//! ([`header`]), and what caco recorded it with ([`record`]).
//!
//! Recording is a way to play (`crate::player::PlayOptions::record`): the sourceport is told to
//! write into this folder under a name made here ([`new_demo_base`]), and once the session ends
//! whatever it wrote under that name gets its record ([`new_demos`]). Every sourceport that
//! records adds `.lmp` to a name without one, and dsda's and crispy's number a name that is
//! taken (`-00002`), or that a restart in the same session made, which is why what a session
//! recorded is found by its name's start rather than taken to be one file.

pub mod export;
pub mod header;
pub mod record;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use header::DemoHeader;
pub use record::{DemoDifference, DemoRecord, FileRole, read_record, record_path, write_record};

/// Demo file extension.
pub const DEMO_EXTENSION: &str = ".lmp";

/// A demo file, and what is known about it.
#[derive(Debug, Clone)]
pub struct DemoFile {
    pub path: PathBuf,
    pub name: String,
    /// What its own bytes say, if they can be read.
    pub header: Option<DemoHeader>,
    /// What caco recorded it with, if caco recorded it.
    pub record: Option<DemoRecord>,
}

impl DemoFile {
    /// When it was recorded, from its record: a file's time is when a sync tool or a copy
    /// last wrote it.
    pub fn recorded_at(&self) -> Option<&str> {
        self.record.as_ref().map(|r| r.recorded_at.as_str())
    }

    /// Which sourceport recorded it, as far as anything says: caco's record, or the footer a
    /// dsda-family sourceport leaves.
    pub fn sourceport(&self) -> Option<String> {
        if let Some(record) = &self.record {
            return Some(match &record.build {
                Some(build) => format!("{} {}", record.sourceport, build.git_ref),
                None => record.sourceport.clone(),
            });
        }
        self.header
            .as_ref()
            .and_then(|h| h.footer.as_ref())
            .and_then(|f| f.sourceport.clone())
    }

    /// Whether it exited the map it started on, which is what makes a demo of a map.
    pub fn finished(&self) -> Finished {
        let start = self.header.as_ref().and_then(|h| h.start.as_ref());
        match (start, self.record.as_ref().and_then(|r| r.exited.as_ref())) {
            (Some(start), Some(exited)) => {
                if exited.iter().any(|m| start.is(m)) {
                    Finished::Yes
                } else {
                    Finished::No
                }
            }
            _ => Finished::Unknown,
        }
    }
}

/// Return the demos subdirectory within a WAD's data directory.
pub fn demos_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("demos")
}

/// Whether a path names a demo file, by its extension.
pub fn is_demo_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| format!(".{}", ext.to_lowercase()) == DEMO_EXTENSION)
}

/// Whether a file belongs to a demo: the demo, or its record. What Clean's Keep Demos keeps.
pub fn is_demo_or_record(path: &Path) -> bool {
    is_demo_file(path) || record::is_record_file(path)
}

/// The demo files in a WAD's demos directory, by name, without reading them.
fn demo_paths(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(demos_dir(data_dir)) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|path| path.is_file() && is_demo_file(path))
        .collect();
    paths.sort();
    paths
}

/// How many demos a WAD has, without reading any of them.
pub fn count_demos(data_dir: &Path) -> usize {
    demo_paths(data_dir).len()
}

/// Find all demo files in a WAD's demos directory, with their headers and records.
pub fn find_demo_files(data_dir: &Path) -> Vec<DemoFile> {
    demo_paths(data_dir)
        .into_iter()
        .map(|path| DemoFile {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            header: DemoHeader::read_file(&path),
            record: read_record(&path),
            path,
        })
        .collect()
}

/// Which demos each session recorded, by session uid, in name order, which is the order a
/// session's restarts were numbered in.
pub fn demos_by_session(data_dir: &Path) -> HashMap<String, Vec<String>> {
    let mut by_session: HashMap<String, Vec<String>> = HashMap::new();
    for path in demo_paths(data_dir) {
        let Some(session) = read_record(&path).and_then(|r| r.session) else {
            continue;
        };
        if let Some(name) = path.file_name() {
            by_session
                .entry(session)
                .or_default()
                .push(name.to_string_lossy().into_owned());
        }
    }
    by_session
}

/// Resolve a demo in a WAD's demos directory by name: verbatim, and then with
/// [`DEMO_EXTENSION`] appended.
pub fn resolve_demo_path(data_dir: &Path, name: &str) -> crate::Result<PathBuf> {
    let dir = demos_dir(data_dir);
    if is_plain_file_name(name) {
        let exact = dir.join(name);
        if exact.is_file() {
            return Ok(exact);
        }
        let with_ext = dir.join(format!("{name}{DEMO_EXTENSION}"));
        if with_ext.is_file() {
            return Ok(with_ext);
        }
    }
    Err(crate::Error::FileNotFound(format!(
        "Demo '{name}' not found in {}",
        dir.display()
    )))
}

/// What to call the demos a session records: the WAD, then when, as the CLI named them, with
/// nothing a sourceport could take for an extension or a folder.
pub fn new_demo_base(title: &str, at: chrono::DateTime<chrono::Local>) -> String {
    format!("{}_{}", slug(title), at.format("%Y%m%d_%H%M%S"))
}

/// A WAD's title as a file name: lower case, letters and digits, a dash for anything else, and
/// nothing a sourceport could take for an extension or a folder.
pub fn slug(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_end_matches('-').chars().take(40).collect();
    match slug.trim_end_matches('-') {
        "" => "demo".to_string(),
        slug => slug.to_string(),
    }
}

/// Whether a WAD's maps go by episode (E1M1) rather than number (MAP01), for reading the
/// numbers in a demo's header, which are the same for Doom's first episode and Doom 2: by its
/// maps when they have been read, else by its IWAD.
pub fn uses_episode_names(maps: &[String], iwad: Option<&str>) -> bool {
    let episode_map = |name: &str| {
        let b = name.as_bytes();
        b.len() == 4
            && b[0].eq_ignore_ascii_case(&b'E')
            && b[1].is_ascii_digit()
            && b[2].eq_ignore_ascii_case(&b'M')
            && b[3].is_ascii_digit()
    };
    if maps.iter().any(|m| episode_map(m)) {
        return true;
    }
    if maps
        .iter()
        .any(|m| m.len() >= 5 && m[..3].eq_ignore_ascii_case("MAP"))
    {
        return false;
    }
    iwad.map(|iwad| iwad.trim().to_ascii_lowercase())
        .is_some_and(|iwad| {
            let stem = iwad.trim_end_matches(".wad");
            let stem = stem.rsplit(['/', '\\']).next().unwrap_or(stem);
            matches!(
                stem,
                "doom"
                    | "doom1"
                    | "doomu"
                    | "udoom"
                    | "ultimate"
                    | "freedoom1"
                    | "heretic"
                    | "heretic1"
                    | "chex"
                    | "chex3"
            )
        })
}

/// The demo files in a folder, by name: what was there before a session, to tell what it wrote.
pub fn names_in(dir: &Path) -> HashSet<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter(|e| is_demo_file(&e.path()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// What a session recorded: demo files in `dir` under the name it was given, `base`, that were
/// not there before it, in the order they were recorded where the names say
/// ([`attempt_number`]).
pub fn new_demos(dir: &Path, base: &str, before: &HashSet<String>) -> Vec<PathBuf> {
    let lower = base.to_lowercase();
    let mut found: Vec<(Option<u32>, String)> = names_in(dir)
        .into_iter()
        .filter(|name| !before.contains(name) && name.to_lowercase().starts_with(&lower))
        .filter(|name| dir.join(name).metadata().is_ok_and(|m| m.len() > 0))
        .map(|name| (attempt_number(&name, base), name))
        .collect();
    // Unnumbered last, since nothing says when they were.
    found.sort_by(|a, b| (a.0.is_none(), a.0, &a.1).cmp(&(b.0.is_none(), b.0, &b.1)));
    found.into_iter().map(|(_, name)| dir.join(name)).collect()
}

/// Which attempt a session's demo was, from its name: `base.lmp` is the first, and dsda and
/// crispy number the rest `base-00002.lmp` on (`dsda_GenerateDemoName`), as a restart in one
/// session makes another. `None` for a name made some other way, such as dsda's names with an
/// IL's time in them.
pub fn attempt_number(name: &str, base: &str) -> Option<u32> {
    let name = name.to_ascii_lowercase();
    let stem = name
        .strip_prefix(&base.to_ascii_lowercase())?
        .strip_suffix(DEMO_EXTENSION)?;
    if stem.is_empty() {
        return Some(1);
    }
    let number = stem.strip_prefix('-')?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// Which of a session's `demos` exited which maps, from the maps the session `exited`. Only a
/// session's only demo can be credited with them. A restart while nyan-doom records ends that
/// demo and starts the next on the map the first started on (`dsda_WatchLevelReload`), so one
/// attempt can finish the map and the next start it again, and the session's stats can't say
/// which got there. So several demos and any exit is `None`, which is never guessed at, and
/// several with no exit exited nothing.
pub fn attribute_exits(demos: usize, exited: &[String]) -> Option<Vec<Vec<String>>> {
    match demos {
        _ if exited.is_empty() => Some(vec![Vec::new(); demos]),
        1 => Some(vec![exited.to_vec()]),
        _ => None,
    }
}

/// Whether a demo finished the map it started on, as far as anything says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finished {
    Yes,
    No,
    /// caco didn't record it, recorded it before it noted this, or couldn't tell.
    Unknown,
}

/// A name a demo can be given: one file name, with nothing that would climb out of the folder.
fn is_plain_file_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && !name.contains(['/', '\\', '\0']) && name != "." && name != ".."
}

/// A name with [`DEMO_EXTENSION`] on it, since only those are listed.
fn with_demo_extension(name: &str) -> String {
    let name = name.trim();
    if is_demo_file(Path::new(name)) {
        name.to_string()
    } else {
        format!("{name}{DEMO_EXTENSION}")
    }
}

/// Give a demo another name, and its record with it. Returns where it is now.
pub fn rename_demo(demo: &Path, new_name: &str) -> crate::Result<PathBuf> {
    if !is_plain_file_name(new_name) {
        return Err(crate::Error::Config(format!(
            "'{new_name}' can't be a file name."
        )));
    }
    let target = demo.with_file_name(with_demo_extension(new_name));
    if target == demo {
        return Ok(target);
    }
    // Asked first, since a rename would replace it. A name that differs only in case is the
    // same file on some filesystems, and that is a rename of this one.
    let same_file_by_case =
        target.to_string_lossy().to_lowercase() == demo.to_string_lossy().to_lowercase();
    if target.exists() && !same_file_by_case {
        return Err(crate::Error::Config(format!(
            "There is already a demo called {}.",
            target.file_name().unwrap_or_default().to_string_lossy()
        )));
    }
    fs::rename(demo, &target)?;
    let old_record = record_path(demo);
    if old_record.exists() {
        fs::rename(&old_record, record_path(&target))?;
    }
    Ok(target)
}

/// Delete a demo and its record.
pub fn delete_demo(demo: &Path) -> crate::Result<()> {
    fs::remove_file(demo)?;
    let record = record_path(demo);
    if record.exists() {
        fs::remove_file(record)?;
    }
    Ok(())
}

/// Copy a demo from somewhere else into a WAD's demos directory: one from a speedrun archive,
/// or one that came in the WAD's zip. Refuses what is not a demo, unless it says it is one by
/// its name, since a demo in a format nobody here reads is still a demo. Keeps its name, with a
/// number added if that is taken. A record beside it comes too, as one caco exported would
/// have, so it plays back as it was recorded. Returns where it went.
pub fn add_demo_from_file(data_dir: &Path, source: &Path) -> crate::Result<PathBuf> {
    if !is_demo_file(source) && DemoHeader::read_file(source).is_none() {
        return Err(crate::Error::Config(format!(
            "{} doesn't look like a demo.",
            source.file_name().unwrap_or_default().to_string_lossy()
        )));
    }
    let dir = demos_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| is_plain_file_name(s))
        .unwrap_or_else(|| "demo".to_string());
    let mut target = dir.join(format!("{stem}{DEMO_EXTENSION}"));
    let mut n = 2;
    while target.exists() {
        target = dir.join(format!("{stem}-{n}{DEMO_EXTENSION}"));
        n += 1;
    }
    fs::copy(source, &target)?;
    if let Some(record) = read_record(source) {
        write_record(&target, &record)?;
    }
    Ok(target)
}

/// Delete demo files from a WAD's demos directory, and their records.
///
/// Returns list of deleted file paths.
pub fn clean_demo_files(data_dir: &Path) -> Vec<PathBuf> {
    demo_paths(data_dir)
        .into_iter()
        .filter(|path| delete_demo(path).is_ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demos_in(dir: &Path, names: &[&str]) {
        let demos = demos_dir(dir);
        fs::create_dir_all(&demos).unwrap();
        for name in names {
            fs::write(demos.join(name), b"data").unwrap();
        }
    }

    #[test]
    fn test_find_demo_files_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_demo_files(dir.path()).is_empty());
        assert_eq!(count_demos(dir.path()), 0);
    }

    #[test]
    fn test_find_demo_files_lists_demos_and_not_their_records() {
        let dir = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["demo1.lmp", "demo2.LMP", "other.txt"]);
        write_record(&demos_dir(dir.path()).join("demo1.lmp"), &sample_record()).unwrap();

        let demos = find_demo_files(dir.path());
        assert_eq!(
            demos.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["demo1.lmp", "demo2.LMP"]
        );
        assert_eq!(count_demos(dir.path()), 2);
        assert!(demos[0].record.is_some());
        assert_eq!(demos[0].recorded_at(), Some("2026-11-18T09:30:00+00:00"));
        assert_eq!(demos[0].sourceport().as_deref(), Some("nyan-doom v1.2.1"));
        assert!(demos[1].record.is_none());
        assert_eq!(demos[1].recorded_at(), None);
    }

    #[test]
    fn test_clean_demo_files_takes_their_records_too() {
        let dir = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["demo1.lmp", "demo2.lmp"]);
        write_record(&demos_dir(dir.path()).join("demo1.lmp"), &sample_record()).unwrap();

        let deleted = clean_demo_files(dir.path());
        assert_eq!(deleted.len(), 2);
        assert!(find_demo_files(dir.path()).is_empty());
        assert!(!demos_dir(dir.path()).join("demo1.lmp.toml").exists());
    }

    #[test]
    fn test_resolve_demo_path_by_name() {
        let dir = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["run.lmp"]);

        // Both the bare name and the full filename resolve to the same file.
        let bare = resolve_demo_path(dir.path(), "run").unwrap();
        let full = resolve_demo_path(dir.path(), "run.lmp").unwrap();
        assert_eq!(bare, demos_dir(dir.path()).join("run.lmp"));
        assert_eq!(bare, full);
    }

    #[test]
    fn test_resolve_demo_path_refuses_what_is_not_there_or_not_a_name() {
        let dir = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["run.lmp"]);
        fs::write(dir.path().join("outside.lmp"), b"data").unwrap();
        assert!(resolve_demo_path(dir.path(), "nope").is_err());
        assert!(resolve_demo_path(dir.path(), "../outside.lmp").is_err());
    }

    #[test]
    fn test_a_new_demo_is_named_after_the_wad_and_when() {
        use chrono::TimeZone;
        let at = chrono::Local
            .with_ymd_and_hms(2026, 11, 18, 9, 5, 7)
            .unwrap();
        assert_eq!(new_demo_base("Scythe 2", at), "scythe-2_20261118_090507");
        assert_eq!(
            new_demo_base("Ancient Aliens v1.1: Remastered!", at),
            "ancient-aliens-v1-1-remastered_20261118_090507"
        );
        assert_eq!(new_demo_base("...", at), "demo_20261118_090507");
        assert!(!new_demo_base("a.b/c\\d", at).contains(['.', '/', '\\']));
    }

    #[test]
    fn test_what_a_session_recorded_is_found_by_the_start_of_its_name() {
        let dir = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["scythe_20261118_090507.lmp", "old.lmp"]);
        let demos = demos_dir(dir.path());
        let before = names_in(&demos);
        // A restart numbered by dsda, the session's own, another session's, and one left empty.
        fs::write(demos.join("scythe_20261118_100000.lmp"), b"x").unwrap();
        fs::write(demos.join("scythe_20261118_100000-00002.lmp"), b"x").unwrap();
        fs::write(demos.join("scythe_20261118_100001.lmp"), b"x").unwrap();
        fs::write(demos.join("scythe_20261118_100000-00003.lmp"), b"").unwrap();

        // A name with an IL's time in it, which says nothing of when.
        fs::write(demos.join("scythe_20261118_1000004712.lmp"), b"x").unwrap();
        fs::write(demos.join("scythe_20261118_100000-00010.lmp"), b"x").unwrap();

        let found = new_demos(&demos, "scythe_20261118_100000", &before);
        assert_eq!(
            found,
            [
                demos.join("scythe_20261118_100000.lmp"),
                demos.join("scythe_20261118_100000-00002.lmp"),
                demos.join("scythe_20261118_100000-00010.lmp"),
                demos.join("scythe_20261118_1000004712.lmp"),
            ]
        );
    }

    #[test]
    fn test_an_attempt_is_numbered_as_dsda_numbers_it() {
        let base = "scythe_20261118_100000";
        assert_eq!(attempt_number("scythe_20261118_100000.lmp", base), Some(1));
        assert_eq!(
            attempt_number("Scythe_20261118_100000-00002.LMP", base),
            Some(2)
        );
        assert_eq!(
            attempt_number("scythe_20261118_100000-00123.lmp", base),
            Some(123)
        );
        assert_eq!(attempt_number("scythe_20261118_1000004712.lmp", base), None);
        assert_eq!(attempt_number("scythe_20261118_100000-.lmp", base), None);
        assert_eq!(attempt_number("other.lmp", base), None);
    }

    fn numbers(episode: u8, map: u8) -> header::StartMap {
        header::StartMap::Numbers { episode, map }
    }

    fn maps(names: &[&str]) -> Vec<String> {
        names.iter().map(|m| m.to_string()).collect()
    }

    #[test]
    fn test_a_sessions_only_demo_exited_whatever_the_session_did() {
        assert_eq!(
            attribute_exits(1, &maps(&["MAP07", "MAP08"])),
            Some(vec![maps(&["MAP07", "MAP08"])])
        );
        // Exiting nothing is known too, however many attempts there were.
        assert_eq!(attribute_exits(3, &[]), Some(vec![Vec::new(); 3]));
    }

    #[test]
    fn test_of_several_attempts_nothing_says_which_got_to_the_exit() {
        // A restart goes back to where the recording started, whether or not the attempt
        // before it got there, so MAP07 could have been finished in either.
        assert_eq!(attribute_exits(2, &maps(&["MAP07"])), None);
    }

    #[test]
    fn test_a_start_is_the_map_a_stats_file_calls_it() {
        assert!(numbers(1, 7).is("MAP07"));
        assert!(numbers(1, 7).is("e1m7"));
        assert!(numbers(3, 2).is("E3M2"));
        assert!(!numbers(3, 2).is("MAP02"));
        assert!(!numbers(1, 7).is("MAP08"));
        assert!(header::StartMap::Named("BTW01".to_string()).is("btw01"));
        assert!(!numbers(1, 1).is("BTW01"));
    }

    #[test]
    fn test_demos_are_found_by_the_session_that_recorded_them() {
        let dir = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["a.lmp", "b.lmp", "c.lmp"]);
        let demos = demos_dir(dir.path());
        let record = sample_record();
        write_record(&demos.join("a.lmp"), &record).unwrap();
        write_record(&demos.join("b.lmp"), &record).unwrap();

        let by_session = demos_by_session(dir.path());
        assert_eq!(by_session.len(), 1);
        assert_eq!(
            by_session[record.session.as_deref().unwrap()],
            ["a.lmp", "b.lmp"]
        );
    }

    #[test]
    fn test_a_rename_takes_the_record_along_and_refuses_to_replace() {
        let dir = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["run.lmp", "taken.lmp"]);
        let demos = demos_dir(dir.path());
        write_record(&demos.join("run.lmp"), &sample_record()).unwrap();

        let renamed = rename_demo(&demos.join("run.lmp"), "map01 uv-max").unwrap();
        assert_eq!(renamed, demos.join("map01 uv-max.lmp"));
        assert!(read_record(&renamed).is_some());
        assert!(!demos.join("run.lmp.toml").exists());

        assert!(rename_demo(&renamed, "taken").is_err());
        assert!(rename_demo(&renamed, "../escape").is_err());
        assert!(rename_demo(&renamed, "  ").is_err());
        assert!(renamed.exists());
    }

    #[test]
    fn test_delete_demo_takes_its_record() {
        let dir = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["run.lmp"]);
        let demo = demos_dir(dir.path()).join("run.lmp");
        write_record(&demo, &sample_record()).unwrap();
        delete_demo(&demo).unwrap();
        assert!(!demo.exists());
        assert!(!record_path(&demo).exists());
    }

    #[test]
    fn test_a_demo_from_elsewhere_is_copied_in_under_a_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        demos_in(dir.path(), &["sc01-uv.lmp"]);
        let source = elsewhere.path().join("sc01-uv.lmp");
        fs::write(&source, b"anything").unwrap();

        let added = add_demo_from_file(dir.path(), &source).unwrap();
        assert_eq!(added, demos_dir(dir.path()).join("sc01-uv-2.lmp"));
        assert!(source.exists());

        // A file that is neither called a demo nor reads as one is refused.
        let notes = elsewhere.path().join("notes.txt");
        fs::write(&notes, b"hello").unwrap();
        assert!(add_demo_from_file(dir.path(), &notes).is_err());

        // One that reads as a demo is taken whatever it is called, and gets the extension.
        let mut vanilla = vec![109, 3, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0];
        vanilla.extend([0u8; 8]);
        vanilla.push(0x80);
        let unnamed = elsewhere.path().join("run.dem");
        fs::write(&unnamed, &vanilla).unwrap();
        let added = add_demo_from_file(dir.path(), &unnamed).unwrap();
        assert_eq!(added, demos_dir(dir.path()).join("run.lmp"));
    }

    #[test]
    fn test_a_demo_added_from_an_export_brings_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let unzipped = tempfile::tempdir().unwrap();
        let source = unzipped.path().join("map07-uv.lmp");
        fs::write(&source, b"demo").unwrap();
        write_record(&source, &sample_record()).unwrap();

        let added = add_demo_from_file(dir.path(), &source).unwrap();
        assert_eq!(read_record(&added), Some(sample_record()));
    }

    #[test]
    fn test_episode_names_come_from_the_maps_and_then_the_iwad() {
        let maps = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert!(uses_episode_names(&maps(&["E1M1", "E1M2"]), Some("doom2")));
        assert!(!uses_episode_names(&maps(&["MAP01"]), Some("doom")));
        assert!(uses_episode_names(&[], Some("doom")));
        assert!(uses_episode_names(&[], Some("/iwads/DOOM.WAD")));
        assert!(!uses_episode_names(&[], Some("doom2")));
        assert!(!uses_episode_names(&[], None));
    }

    #[test]
    fn test_keep_demos_covers_a_demo_and_its_record() {
        assert!(is_demo_or_record(Path::new("demos/run.lmp")));
        assert!(is_demo_or_record(Path::new("demos/run.lmp.toml")));
        assert!(!is_demo_or_record(Path::new("stats.txt")));
    }

    fn sample_record() -> DemoRecord {
        DemoRecord {
            version: record::RECORD_VERSION,
            session: Some("0123456789abcdef0123456789abcdef".to_string()),
            recorded_at: "2026-11-18T09:30:00+00:00".to_string(),
            sourceport: "nyan-doom".to_string(),
            build: Some(record::RecordedBuild {
                git_ref: "v1.2.1".to_string(),
                commit: "1".repeat(40),
            }),
            iwad: None,
            complevel: None,
            profile: "default".to_string(),
            args: Vec::new(),
            custom_args: Vec::new(),
            exited: None,
            files: Vec::new(),
        }
    }
}
