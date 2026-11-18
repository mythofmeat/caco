//! What a demo was recorded with, kept beside it, so it can be played back that way.
//!
//! A demo is a list of inputs, and plays back as recorded only with the same sourceport, the
//! same files in the same order, the same complevel and the same settings. Playing it under
//! whatever the WAD is set to today desyncs it the moment any of that has changed, with no word
//! of why. So the session that records a demo writes this down, and playback launches from it
//! rather than from the WAD's row: see `crate::player::play_demo`.
//!
//! It is a file beside the demo (`run.lmp` and `run.lmp.toml`), not a row. The demo is a file
//! that travels however the data dir does, and so does this, in the same move, so the two cannot
//! arrive apart as a row and a file could. Backups take it along for the same reason. It names
//! its session by uid, which is the same on every machine, so Sessions can find its demo.
//!
//! Nothing here is a path. Every file is named by its hash, and found on whichever machine plays
//! it back: the WAD by its content, a companion in the registry, an id24 WAD by name. And what
//! comes from a file the user's sync tool brought is checked before it is used, as the sync log
//! is (`crate::sync::merge::decode`): the sourceport has to be one a family knows, and a profile a
//! plain name, since one names what gets run and the other a file the sourceport writes to.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What this build writes. A record from a later one is still read, for whatever it has that
/// this one understands.
pub const RECORD_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemoRecord {
    pub version: u32,
    /// The session that recorded it, by uid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// When the session started, RFC 3339. A file's time is whenever a sync tool last
    /// wrote it.
    pub recorded_at: String,
    /// The sourceport as the user knows it: `nyan-doom`, never where it was.
    pub sourceport: String,
    /// What it was built from, when caco built it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<RecordedBuild>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iwad: Option<RecordedIwad>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complevel: Option<i32>,
    /// The config profile, by name.
    pub profile: String,
    /// Every sourceport's arguments and this one's, from the settings.
    #[serde(default)]
    pub args: Vec<String>,
    /// The WAD's own arguments.
    #[serde(default)]
    pub custom_args: Vec<String>,
    /// Everything loaded after the IWAD, in the order it was.
    #[serde(default, rename = "file")]
    pub files: Vec<RecordedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedBuild {
    /// The release or branch, as the Sourceports tool shows it.
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedIwad {
    /// What `-iwad` was given, before it was looked for: `doom2`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedFile {
    pub role: FileRole,
    /// Its file name, for saying which one when it is missing.
    pub name: String,
    /// The WAD's is its content hash (`crate::identity`), the same whether it came zipped or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// What the companion registry and the id24 registry know files by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
}

/// Why a file was loaded, which is where to look for it on playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileRole {
    /// An id24 resource WAD, from the IWADs tool.
    Id24,
    /// A file from the WAD's Companions tab.
    Companion,
    /// What caco loads into a ZDoom-family sourceport to hear which maps were finished.
    StatsReporter,
    /// The WAD's own files: the WAD, or what its zip wraps.
    Wad,
}

/// Something playback cannot do as the demo was recorded, which is likely to desync it. Each says
/// what playing anyway would do instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DemoDifference {
    /// The sourceport is not on this machine, and `instead` would play it.
    SourceportMissing {
        name: String,
        instead: String,
    },
    /// caco built the sourceport from one version and has a different one now, or none of its
    /// own, in which case `current` is `None`.
    SourceportVersion {
        name: String,
        recorded: String,
        current: Option<String>,
    },
    /// It was recorded with a sourceport caco did not build, which is not here, and caco's own
    /// build of `current` is.
    SourceportNotBuilt {
        name: String,
        current: String,
    },
    IwadMissing {
        name: String,
    },
    IwadChanged {
        name: String,
    },
    WadChanged,
    FileMissing {
        role: FileRole,
        name: String,
    },
    FileChanged {
        role: FileRole,
        name: String,
    },
    ProfileMissing {
        name: String,
    },
}

impl std::fmt::Display for DemoDifference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DemoDifference::SourceportMissing { name, instead } => {
                write!(
                    f,
                    "{name} isn't on this machine, so {instead} would play it."
                )
            }
            DemoDifference::SourceportVersion {
                name,
                recorded,
                current: Some(current),
            } => write!(
                f,
                "It was recorded with {name} {recorded}, and the {name} here is {current}."
            ),
            DemoDifference::SourceportVersion {
                name,
                recorded,
                current: None,
            } => write!(
                f,
                "It was recorded with {name} {recorded}, which caco built, and the {name} here \
                 isn't one caco built."
            ),
            DemoDifference::SourceportNotBuilt { name, current } => write!(
                f,
                "It was recorded with a {name} caco didn't build, and the only one here is the \
                 {current} caco built."
            ),
            DemoDifference::IwadMissing { name } => {
                write!(f, "The IWAD it was recorded with, {name}, isn't here.")
            }
            DemoDifference::IwadChanged { name } => {
                write!(
                    f,
                    "The IWAD {name} is a different file from the one it was recorded with."
                )
            }
            DemoDifference::WadChanged => {
                write!(f, "The WAD's file has changed since it was recorded.")
            }
            DemoDifference::FileMissing { role, name } => match role {
                FileRole::Companion => {
                    write!(
                        f,
                        "The companion file {name} isn't here, so it would be left out."
                    )
                }
                _ => write!(f, "{name} isn't here, so it would be left out."),
            },
            DemoDifference::FileChanged { role, name } => match role {
                FileRole::StatsReporter => write!(
                    f,
                    "caco's stats reporter has changed since, and ZDoom demos can tell."
                ),
                _ => write!(
                    f,
                    "{name} is a different file from the one it was recorded with."
                ),
            },
            DemoDifference::ProfileMissing { name } => write!(
                f,
                "The config profile '{name}' it was recorded with is gone, so it would start \
                 from an empty one."
            ),
        }
    }
}

impl DemoRecord {
    /// Whether what it says can be acted on here. A sourceport no family knows, or a name with
    /// a path in it, is refused whole, since the record came from a file and these say what gets
    /// run and where it writes.
    pub fn is_safe(&self) -> bool {
        crate::sourceports::family_name(&self.sourceport).is_some()
            && is_plain_name(&self.sourceport)
            && is_plain_name(&self.profile)
    }
}

/// A name that cannot be taken for a path.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".."
}

/// Where a demo's record is: beside it, under its whole name and `.toml`.
pub fn record_path(demo: &Path) -> PathBuf {
    let mut name = demo.file_name().unwrap_or_default().to_os_string();
    name.push(".toml");
    demo.with_file_name(name)
}

/// Whether a file is a demo's record, by its name.
pub fn is_record_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".lmp.toml"))
}

/// A demo's record, if it has one that can be read and acted on.
pub fn read_record(demo: &Path) -> Option<DemoRecord> {
    let path = record_path(demo);
    let text = std::fs::read_to_string(&path).ok()?;
    match toml::from_str::<DemoRecord>(&text) {
        Ok(record) if record.is_safe() => Some(record),
        Ok(_) => {
            tracing::warn!("ignoring {path:?}: it names a sourceport or profile caco won't use");
            None
        }
        Err(e) => {
            tracing::warn!("cannot read {path:?}: {e}");
            None
        }
    }
}

/// Write a demo's record beside it.
pub fn write_record(demo: &Path, record: &DemoRecord) -> crate::Result<()> {
    let text = toml::to_string(record)
        .map_err(|e| crate::Error::Config(format!("cannot write a demo's record: {e}")))?;
    let header = "# What caco recorded this demo with, to play it back the same way.\n";
    std::fs::write(record_path(demo), format!("{header}{text}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DemoRecord {
        DemoRecord {
            version: RECORD_VERSION,
            session: Some("0123456789abcdef0123456789abcdef".to_string()),
            recorded_at: "2026-11-18T09:30:00+00:00".to_string(),
            sourceport: "nyan-doom".to_string(),
            build: Some(RecordedBuild {
                git_ref: "v1.2.1".to_string(),
                commit: "1111111111111111111111111111111111111111".to_string(),
            }),
            iwad: Some(RecordedIwad {
                name: "doom2".to_string(),
                sha256: Some("ab".repeat(32)),
            }),
            complevel: Some(9),
            profile: "default".to_string(),
            args: vec!["-nomusic".to_string()],
            custom_args: vec!["-fast".to_string()],
            files: vec![
                RecordedFile {
                    role: FileRole::Companion,
                    name: "fix.deh".to_string(),
                    sha256: None,
                    md5: Some("cd".repeat(16)),
                },
                RecordedFile {
                    role: FileRole::Wad,
                    name: "scythe.zip".to_string(),
                    sha256: Some("ef".repeat(32)),
                    md5: None,
                },
            ],
        }
    }

    #[test]
    fn test_a_record_reads_back_as_it_was_written() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("run.lmp");
        std::fs::write(&demo, b"demo").unwrap();
        write_record(&demo, &sample()).unwrap();
        assert!(dir.path().join("run.lmp.toml").is_file());
        assert_eq!(read_record(&demo), Some(sample()));
    }

    #[test]
    fn test_a_record_that_would_run_something_else_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("run.lmp");
        for (sourceport, profile) in [
            ("/bin/sh", "default"),
            ("not-a-sourceport", "default"),
            ("nyan-doom", "../../.bashrc"),
            ("nyan-doom", ""),
        ] {
            let record = DemoRecord {
                sourceport: sourceport.to_string(),
                profile: profile.to_string(),
                ..sample()
            };
            write_record(&demo, &record).unwrap();
            assert_eq!(read_record(&demo), None, "{sourceport} {profile}");
        }
    }

    #[test]
    fn test_a_record_from_a_later_build_is_read_for_what_this_one_knows() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("run.lmp");
        let text = toml::to_string(&sample()).unwrap();
        std::fs::write(
            record_path(&demo),
            format!("{text}\nsomething_new = 3\n").replace("version = 1", "version = 7"),
        )
        .unwrap();
        let read = read_record(&demo).unwrap();
        assert_eq!(read.version, 7);
        assert_eq!(read.files, sample().files);
    }

    #[test]
    fn test_a_broken_record_is_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("run.lmp");
        std::fs::write(record_path(&demo), "sourceport = [").unwrap();
        assert_eq!(read_record(&demo), None);
    }

    #[test]
    fn test_record_files_are_told_apart_by_name() {
        assert!(is_record_file(Path::new("/d/demos/run.lmp.toml")));
        assert!(is_record_file(Path::new("RUN.LMP.TOML")));
        assert!(!is_record_file(Path::new("run.lmp")));
        assert!(!is_record_file(Path::new("config.toml")));
        assert_eq!(
            record_path(Path::new("/d/demos/run.lmp")),
            PathBuf::from("/d/demos/run.lmp.toml")
        );
    }

    #[test]
    fn test_every_difference_says_what_playing_anyway_would_do() {
        let said = DemoDifference::SourceportMissing {
            name: "nyan-doom".to_string(),
            instead: "dsda-doom".to_string(),
        }
        .to_string();
        assert_eq!(
            said,
            "nyan-doom isn't on this machine, so dsda-doom would play it."
        );
        let said = DemoDifference::SourceportVersion {
            name: "uzdoom".to_string(),
            recorded: "4.14.3".to_string(),
            current: Some("4.15.0".to_string()),
        }
        .to_string();
        assert_eq!(
            said,
            "It was recorded with uzdoom 4.14.3, and the uzdoom here is 4.15.0."
        );
        let said = DemoDifference::FileMissing {
            role: FileRole::Companion,
            name: "fix.deh".to_string(),
        }
        .to_string();
        assert_eq!(
            said,
            "The companion file fix.deh isn't here, so it would be left out."
        );
    }
}
