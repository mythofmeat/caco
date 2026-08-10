//! Build and manage sourceports from source into a caco-owned prefix.
//!
//! Not every port is packaged — nyan-doom and uzdoom are in no distro repo —
//! and requiring a global install makes a library non-portable in the way
//! that matters: copying `~/.local/share/caco` to another machine should be
//! enough to play. It is, because the thing that travels is the *recipe*. A
//! few hundred bytes of TOML rebuild the binary on whatever machine and OS it
//! lands on, which a binary itself could never do.
//!
//! That split is why the pieces live where they do: recipes and their patches
//! sit beside the database on the portable side, while checkouts, build trees
//! and install prefixes sit in the cache with the WAD downloads. Deleting the
//! cache costs a rebuild, never a reconfiguration.
//!
//! Nothing in [`crate::sourceports`] needs to know a port was built here.
//! `nyan-doom` and `uzdoom` were already mapped to the dsda and zdoom
//! families, so complevel args, save directories and config profiles start
//! working the moment a managed binary exists. The single integration point
//! is [`crate::config::resolve_sourceport`], which every launch already goes
//! through.

pub mod build;
pub mod doctor;
pub mod manifest;
pub mod recipe;
pub mod update;

pub use build::{BuildOptions, BuildProgress, BuildStep, PortPaths};
pub use doctor::{DoctorReport, PackageCheck, doctor};
pub use manifest::{InstalledPort, PortManifest, find_installed, list_installed, remove_installed};
pub use recipe::{PortRecipe, builtin_recipes, find_recipe, load_recipes};
pub use update::{UpdateStatus, check_updates};

/// A recipe paired with whatever is installed for it.
#[derive(Debug, Clone)]
pub struct PortStatus {
    pub recipe: PortRecipe,
    /// Most recent usable install, if there is one.
    pub installed: Option<InstalledPort>,
    /// True when something is installed but at a different ref than the
    /// recipe now asks for — the case a rebuild fixes.
    pub ref_changed: bool,
    /// Commit the ref pointed at as of the last update check, if one has
    /// run. Read from the cache, so building this list stays offline.
    pub remote_commit: Option<String>,
}

impl PortStatus {
    /// Whether the remote has moved past the installed build.
    ///
    /// Distinct from [`Self::ref_changed`]: that is the user repointing the
    /// recipe, this is upstream committing.
    pub fn update_available(&self) -> bool {
        let (Some(installed), Some(remote)) = (&self.installed, &self.remote_commit) else {
            return false;
        };
        installed.manifest.commit != "unknown" && &installed.manifest.commit != remote
    }
}

/// Every known recipe with its install state, for the ports UI.
/// Never reaches the network — remote state comes from whatever the last
/// update check cached, so opening the dialog is instant.
pub fn status(paths: &PortPaths) -> crate::Result<Vec<PortStatus>> {
    let installed = list_installed(&paths.prefix_root);
    let cache = update::load_cache(&paths.prefix_root);
    let mut out = Vec::new();
    for recipe in load_recipes(&paths.recipe_dir)? {
        let current = installed
            .iter()
            .find(|p| p.manifest.name == recipe.name && p.is_usable())
            .cloned();
        let ref_changed = current
            .as_ref()
            .is_some_and(|p| p.manifest.git_ref != recipe.git_ref);
        let remote_commit = cache
            .ports
            .get(&recipe.name)
            .map(|c| c.remote_commit.clone());
        out.push(PortStatus {
            recipe,
            installed: current,
            ref_changed,
            remote_commit,
        });
    }
    Ok(out)
}

/// Path to a managed build of `name`, if one is installed and usable.
///
/// Consulted by [`crate::config::resolve_sourceport`] ahead of `PATH`: a port
/// the user asked caco to build is the one they meant, even when a distro
/// package of the same name happens to exist.
pub fn managed_binary(name: &str) -> Option<String> {
    let installed = find_installed(&crate::config::port_prefix_root(), name)?;
    Some(installed.binary_path().display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &std::path::Path) -> PortPaths {
        PortPaths {
            recipe_dir: root.join("recipes"),
            src_root: root.join("src"),
            prefix_root: root.join("prefix"),
        }
    }

    #[test]
    fn status_lists_every_recipe_even_with_nothing_installed() {
        let dir = tempfile::tempdir().unwrap();
        let all = status(&paths(dir.path())).unwrap();
        assert_eq!(all.len(), builtin_recipes().len());
        assert!(all.iter().all(|s| s.installed.is_none()));
        assert!(all.iter().all(|s| !s.ref_changed));
    }

    #[test]
    fn status_flags_an_install_left_behind_by_a_ref_change() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());

        // Installed at master...
        let prefix = p.prefix_root.join("uzdoom").join("master");
        manifest::write_manifest(
            &prefix,
            &PortManifest {
                name: "uzdoom".into(),
                repo: "https://github.com/UZDoom/uzdoom".into(),
                git_ref: "master".into(),
                commit: "deadbeef".into(),
                binary: "uzdoom".into(),
                built_at: "2026-01-01T00:00:00+00:00".into(),
            },
        )
        .unwrap();
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::write(prefix.join("bin").join("uzdoom"), b"x").unwrap();

        // ...but the recipe now pins a tag.
        std::fs::create_dir_all(&p.recipe_dir).unwrap();
        std::fs::write(
            p.recipe_dir.join("pin.toml"),
            r#"
[uzdoom]
repo = "https://github.com/UZDoom/uzdoom"
ref = "v1.0.0"
binary = "uzdoom"
[uzdoom.build]
system = "cmake"
"#,
        )
        .unwrap();

        let uzdoom = status(&p)
            .unwrap()
            .into_iter()
            .find(|s| s.recipe.name == "uzdoom")
            .unwrap();
        assert!(uzdoom.installed.is_some());
        assert!(uzdoom.ref_changed);
    }
}
