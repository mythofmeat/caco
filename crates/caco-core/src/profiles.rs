//! Sourceport config profile management.
//!
//! A profile is a named sourceport config file living at
//! `{sourceport_dir}/{exe}/{profile}.{ext}` — see [`config::get_profile_path`].
//! WADs reference profiles by name through their `custom_config` column.
//!
//! This module owns the profile *operations*; how a profile is edited is left
//! to the caller, because that differs sharply per frontend (a CLI spawns
//! `$EDITOR`, a GUI opens a text buffer). Callers that want to edit should go
//! through [`read`] and [`write`].

use std::fs;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::config;
use crate::db::{self, WadRecord};
use crate::{Error, Result};

/// A profile and the sourceport it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub sourceport: String,
    pub name: String,
    pub path: PathBuf,
}

/// Resolve an explicit sourceport, falling back to the configured default.
///
/// Errors rather than returning an empty string, since every operation here
/// needs a real sourceport to build a path from.
pub fn resolve_sourceport(explicit: Option<&str>) -> Result<String> {
    let port = explicit
        .map(str::to_string)
        .unwrap_or_else(config::get_default_sourceport);
    if port.is_empty() {
        return Err(Error::NoSourceport);
    }
    Ok(port)
}

/// Every profile, flattened and sorted by sourceport then name.
///
/// Pass `sourceport` to restrict to one port. Returns an empty vec when the
/// profile directory does not exist yet.
pub fn list(sourceport: Option<&str>) -> Vec<Profile> {
    let grouped = config::list_profiles(sourceport);
    let mut ports: Vec<_> = grouped.into_iter().collect();
    ports.sort_by(|a, b| a.0.cmp(&b.0));

    ports
        .into_iter()
        .flat_map(|(port, names)| {
            names.into_iter().map(move |name| Profile {
                path: config::get_profile_path(&port, &name),
                sourceport: port.clone(),
                name,
            })
        })
        .collect()
}

/// Absolute path to a profile file, whether or not it exists.
pub fn path(sourceport: &str, name: &str) -> PathBuf {
    config::get_profile_path(sourceport, name)
}

pub fn exists(sourceport: &str, name: &str) -> bool {
    path(sourceport, name).exists()
}

/// Create an empty profile, or a copy of `from` when given.
pub fn create(sourceport: &str, name: &str, from: Option<&str>) -> Result<PathBuf> {
    let dest = path(sourceport, name);
    if dest.exists() {
        return Err(Error::ProfileExists {
            sourceport: sourceport.to_string(),
            name: name.to_string(),
        });
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    match from {
        Some(source_name) => {
            let source = path(sourceport, source_name);
            if !source.exists() {
                return Err(Error::ProfileNotFound {
                    sourceport: sourceport.to_string(),
                    name: source_name.to_string(),
                });
            }
            fs::copy(&source, &dest)?;
        }
        None => {
            fs::File::create(&dest)?;
        }
    }

    Ok(dest)
}

/// Copy an existing profile to a new name within the same sourceport.
pub fn copy(sourceport: &str, source: &str, dest: &str) -> Result<PathBuf> {
    let source_path = path(sourceport, source);
    if !source_path.exists() {
        return Err(Error::ProfileNotFound {
            sourceport: sourceport.to_string(),
            name: source.to_string(),
        });
    }

    let dest_path = path(sourceport, dest);
    if dest_path.exists() {
        return Err(Error::ProfileExists {
            sourceport: sourceport.to_string(),
            name: dest.to_string(),
        });
    }

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&source_path, &dest_path)?;
    Ok(dest_path)
}

/// Delete a profile file.
///
/// Does not check whether any WAD still references it — call
/// [`referencing_wads`] first and decide what to do about that, since a CLI
/// warns and prompts while a GUI wants to show the list up front.
pub fn remove(sourceport: &str, name: &str) -> Result<()> {
    let target = path(sourceport, name);
    if !target.exists() {
        return Err(Error::ProfileNotFound {
            sourceport: sourceport.to_string(),
            name: name.to_string(),
        });
    }
    fs::remove_file(&target)?;
    Ok(())
}

/// Read a profile's contents.
pub fn read(sourceport: &str, name: &str) -> Result<String> {
    let target = path(sourceport, name);
    if !target.exists() {
        return Err(Error::ProfileNotFound {
            sourceport: sourceport.to_string(),
            name: name.to_string(),
        });
    }
    Ok(fs::read_to_string(&target)?)
}

/// Overwrite a profile's contents, creating it if absent.
pub fn write(sourceport: &str, name: &str, contents: &str) -> Result<()> {
    let target = path(sourceport, name);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&target, contents)?;
    Ok(())
}

/// WADs whose `custom_config` points at this profile name.
///
/// Used to warn before deleting a profile that is still in use.
pub fn referencing_wads(conn: &Connection, name: &str) -> Result<Vec<WadRecord>> {
    db::search_wads(conn, Some(&format!("config:{name}")), None, true, false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Point the profile directory at a temp dir for the duration of a test.
    ///
    /// `config::get_sourceport_dir` reads the process environment, so these
    /// tests share one serialized guard rather than running in parallel.
    fn with_profile_dir<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        use std::sync::Mutex;
        static GUARD: Mutex<()> = Mutex::new(());
        let _lock = GUARD.lock().unwrap_or_else(|e| e.into_inner());

        let dir = tempfile::tempdir().unwrap();
        // SAFETY: serialized by GUARD; no other thread reads the env here.
        unsafe { std::env::set_var("CACO_HOME", dir.path()) };
        config::reload_config();
        let result = f(dir.path());
        unsafe { std::env::remove_var("CACO_HOME") };
        config::reload_config();
        result
    }

    #[test]
    fn test_create_then_read_write_roundtrip() {
        with_profile_dir(|_| {
            create("dsda-doom", "speed", None).unwrap();
            assert!(exists("dsda-doom", "speed"));

            write("dsda-doom", "speed", "screen_resolution 1920x1080\n").unwrap();
            assert_eq!(
                read("dsda-doom", "speed").unwrap(),
                "screen_resolution 1920x1080\n"
            );
        });
    }

    #[test]
    fn test_create_rejects_duplicate() {
        with_profile_dir(|_| {
            create("dsda-doom", "dup", None).unwrap();
            let err = create("dsda-doom", "dup", None).unwrap_err();
            assert!(matches!(err, Error::ProfileExists { .. }));
        });
    }

    #[test]
    fn test_create_from_copies_contents() {
        with_profile_dir(|_| {
            create("dsda-doom", "base", None).unwrap();
            write("dsda-doom", "base", "usemouse 0\n").unwrap();

            create("dsda-doom", "derived", Some("base")).unwrap();
            assert_eq!(read("dsda-doom", "derived").unwrap(), "usemouse 0\n");
        });
    }

    #[test]
    fn test_create_from_missing_source_errors() {
        with_profile_dir(|_| {
            let err = create("dsda-doom", "derived", Some("nope")).unwrap_err();
            assert!(matches!(err, Error::ProfileNotFound { .. }));
            // The destination must not be left behind after a failed copy.
            assert!(!exists("dsda-doom", "derived"));
        });
    }

    #[test]
    fn test_copy_rejects_existing_destination() {
        with_profile_dir(|_| {
            create("dsda-doom", "a", None).unwrap();
            create("dsda-doom", "b", None).unwrap();
            let err = copy("dsda-doom", "a", "b").unwrap_err();
            assert!(matches!(err, Error::ProfileExists { .. }));
        });
    }

    #[test]
    fn test_remove_missing_profile_errors() {
        with_profile_dir(|_| {
            let err = remove("dsda-doom", "ghost").unwrap_err();
            assert!(matches!(err, Error::ProfileNotFound { .. }));
        });
    }

    #[test]
    fn test_list_groups_and_sorts() {
        with_profile_dir(|_| {
            create("dsda-doom", "zebra", None).unwrap();
            create("dsda-doom", "alpha", None).unwrap();
            create("woof", "solo", None).unwrap();

            let found = list(None);
            let pairs: Vec<_> = found
                .iter()
                .map(|p| (p.sourceport.as_str(), p.name.as_str()))
                .collect();
            assert_eq!(
                pairs,
                vec![
                    ("dsda-doom", "alpha"),
                    ("dsda-doom", "zebra"),
                    ("woof", "solo"),
                ]
            );

            let only_woof = list(Some("woof"));
            assert_eq!(only_woof.len(), 1);
            assert_eq!(only_woof[0].name, "solo");
        });
    }

    #[test]
    fn test_list_empty_when_nothing_created() {
        with_profile_dir(|_| {
            assert!(list(None).is_empty());
        });
    }
}
