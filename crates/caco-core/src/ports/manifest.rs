//! The record a built port leaves in its own install prefix.
//!
//! Kept inside the prefix rather than in the database on purpose: the prefix
//! lives on the cache side and can be deleted wholesale, and a database row
//! describing a directory that is no longer there would have to be reconciled
//! on every launch. A manifest beside the binary cannot go stale — if the
//! directory exists, so does its description.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Filename of the manifest inside an install prefix.
pub const MANIFEST_NAME: &str = "caco-port.toml";

/// What was built, from where, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortManifest {
    pub name: String,
    pub repo: String,
    /// The ref as the recipe asked for it — a branch name is not reproducible.
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// The commit that ref actually resolved to, which is.
    pub commit: String,
    /// Executable name inside `bin/`.
    pub binary: String,
    /// RFC 3339 timestamp of the successful install.
    pub built_at: String,
}

/// A port present on disk.
#[derive(Debug, Clone)]
pub struct InstalledPort {
    pub manifest: PortManifest,
    pub prefix: PathBuf,
}

impl InstalledPort {
    /// Absolute path to the executable.
    pub fn binary_path(&self) -> PathBuf {
        self.prefix.join("bin").join(&self.manifest.binary)
    }

    /// Whether the executable this manifest promises is actually there.
    ///
    /// A prefix can outlive its contents (an interrupted `rm`, a cache wipe
    /// that caught the binary but not the manifest), and a launch against a
    /// missing path fails far from the cause.
    pub fn is_usable(&self) -> bool {
        self.binary_path().is_file()
    }

    /// Total size of the install tree in bytes.
    pub fn size_bytes(&self) -> u64 {
        dir_size(&self.prefix)
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            Ok(t) if t.is_file() => entry.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

pub fn write_manifest(prefix: &Path, manifest: &PortManifest) -> crate::Result<()> {
    std::fs::create_dir_all(prefix)?;
    let text = toml::to_string_pretty(manifest)?;
    std::fs::write(prefix.join(MANIFEST_NAME), text)?;
    Ok(())
}

/// Read the manifest out of a prefix, or `None` if there isn't a readable one.
pub fn read_manifest(prefix: &Path) -> Option<PortManifest> {
    let text = std::fs::read_to_string(prefix.join(MANIFEST_NAME)).ok()?;
    toml::from_str(&text).ok()
}

/// Every port installed under `prefix_root`, newest build first.
///
/// The layout is `<prefix_root>/<name>/<ref-slug>/`, so a name can have
/// several refs installed side by side.
pub fn list_installed(prefix_root: &Path) -> Vec<InstalledPort> {
    let mut found = Vec::new();
    let Ok(names) = std::fs::read_dir(prefix_root) else {
        return found;
    };
    for name_entry in names.flatten() {
        let Ok(refs) = std::fs::read_dir(name_entry.path()) else {
            continue;
        };
        for ref_entry in refs.flatten() {
            let prefix = ref_entry.path();
            if let Some(manifest) = read_manifest(&prefix) {
                found.push(InstalledPort { manifest, prefix });
            }
        }
    }
    // Descending by timestamp: RFC 3339 with a fixed offset sorts lexically.
    found.sort_by(|a, b| b.manifest.built_at.cmp(&a.manifest.built_at));
    found
}

/// The most recently built usable install of `name`, if any.
pub fn find_installed(prefix_root: &Path, name: &str) -> Option<InstalledPort> {
    list_installed(prefix_root)
        .into_iter()
        .find(|p| p.manifest.name == name && p.is_usable())
}

/// Delete an install prefix.
///
/// Refuses any directory that does not carry a manifest. The prefix root is a
/// path assembled from config, and `execute`-style deletion against a
/// mis-resolved root is exactly the failure mode that costs a user their
/// data — requiring caco's own marker file means the worst a wrong root can
/// do is nothing.
pub fn remove_installed(prefix: &Path) -> crate::Result<()> {
    if read_manifest(prefix).is_none() {
        return Err(crate::error::Error::Config(format!(
            "{} is not a caco port install (no {MANIFEST_NAME})",
            prefix.display()
        )));
    }
    std::fs::remove_dir_all(prefix)?;
    // Drop the now-empty per-name directory so `ls` does not show a port with
    // no builds under it.
    if let Some(parent) = prefix.parent()
        && parent.read_dir().is_ok_and(|mut d| d.next().is_none())
    {
        let _ = std::fs::remove_dir(parent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str, built_at: &str) -> PortManifest {
        PortManifest {
            name: name.to_string(),
            repo: "https://example.invalid/x".to_string(),
            git_ref: "master".to_string(),
            commit: "abc123".to_string(),
            binary: name.to_string(),
            built_at: built_at.to_string(),
        }
    }

    /// Build a prefix root under a tempdir. Nothing here may reach real paths.
    fn install(root: &Path, name: &str, slug: &str, built_at: &str, with_binary: bool) -> PathBuf {
        let prefix = root.join(name).join(slug);
        write_manifest(&prefix, &manifest(name, built_at)).unwrap();
        if with_binary {
            std::fs::create_dir_all(prefix.join("bin")).unwrap();
            std::fs::write(prefix.join("bin").join(name), b"#!/bin/true\n").unwrap();
        }
        prefix
    }

    #[test]
    fn manifest_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let m = manifest("uzdoom", "2026-08-10T12:00:00+00:00");
        write_manifest(dir.path(), &m).unwrap();
        assert_eq!(read_manifest(dir.path()).unwrap(), m);
    }

    #[test]
    fn list_installed_is_newest_first() {
        let root = tempfile::tempdir().unwrap();
        install(
            root.path(),
            "uzdoom",
            "master",
            "2026-01-01T00:00:00+00:00",
            true,
        );
        install(
            root.path(),
            "nyan-doom",
            "master",
            "2026-06-01T00:00:00+00:00",
            true,
        );

        let names: Vec<_> = list_installed(root.path())
            .into_iter()
            .map(|p| p.manifest.name)
            .collect();
        assert_eq!(names, vec!["nyan-doom", "uzdoom"]);
    }

    #[test]
    fn find_installed_skips_a_prefix_whose_binary_is_gone() {
        let root = tempfile::tempdir().unwrap();
        // Newer, but the executable was deleted out from under the manifest.
        install(
            root.path(),
            "uzdoom",
            "v2",
            "2026-06-01T00:00:00+00:00",
            false,
        );
        install(
            root.path(),
            "uzdoom",
            "v1",
            "2026-01-01T00:00:00+00:00",
            true,
        );

        let found = find_installed(root.path(), "uzdoom").unwrap();
        assert!(found.prefix.ends_with("v1"));
        assert!(found.is_usable());
    }

    #[test]
    fn find_installed_returns_none_for_an_unknown_name() {
        let root = tempfile::tempdir().unwrap();
        install(
            root.path(),
            "uzdoom",
            "master",
            "2026-01-01T00:00:00+00:00",
            true,
        );
        assert!(find_installed(root.path(), "gzdoom").is_none());
    }

    #[test]
    fn a_missing_prefix_root_lists_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_installed(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn remove_deletes_the_prefix_and_its_empty_parent() {
        let root = tempfile::tempdir().unwrap();
        let prefix = install(
            root.path(),
            "uzdoom",
            "master",
            "2026-01-01T00:00:00+00:00",
            true,
        );
        remove_installed(&prefix).unwrap();
        assert!(!prefix.exists());
        assert!(!root.path().join("uzdoom").exists());
    }

    #[test]
    fn remove_keeps_the_parent_when_another_ref_remains() {
        let root = tempfile::tempdir().unwrap();
        let v1 = install(
            root.path(),
            "uzdoom",
            "v1",
            "2026-01-01T00:00:00+00:00",
            true,
        );
        install(
            root.path(),
            "uzdoom",
            "v2",
            "2026-02-01T00:00:00+00:00",
            true,
        );
        remove_installed(&v1).unwrap();
        assert!(root.path().join("uzdoom").join("v2").exists());
    }

    #[test]
    fn remove_refuses_a_directory_that_is_not_a_port_install() {
        let root = tempfile::tempdir().unwrap();
        let victim = root.path().join("important");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("data.db"), b"precious").unwrap();

        assert!(remove_installed(&victim).is_err());
        assert!(victim.join("data.db").exists());
    }
}
