//! Everything caco knows about sourceports: which families exist and how they
//! are spelled on a command line ([`registry`]), and how to build the ones no
//! distro packages ([`build`]).
//!
//! The two halves used to be `sourceports.rs` and `sourceports/`, which put the
//! word "sourceport" in the tree for something that is never called that outside
//! caco's own source — every Doom engine is a *sourceport*, and a "sourceport" is a
//! different thing entirely. They also belong together: a built binary is only
//! useful because the registry already knows `nyan-doom` is dsda-flavoured and
//! `uzdoom` zdoom-flavoured, so complevel args, save directories and config
//! profiles work the moment the binary exists.
//!
//! On the build half: not every sourceport is packaged — nyan-doom and uzdoom
//! are in no distro repo — and requiring a global install makes a library
//! non-portable in the way that matters, since copying the data dir to another
//! machine should be enough to play. It is, because the thing that travels is
//! the *recipe*. A few hundred bytes of TOML rebuild the binary on whatever
//! machine and OS it lands on, which a binary itself could never do.
//!
//! That split is why the pieces live where they do: recipes and their patches
//! sit beside the database on the portable side, while checkouts, build trees
//! and install prefixes sit in the cache with the WAD downloads. Deleting the
//! cache costs a rebuild, never a reconfiguration.
//!
//! The single integration point for a managed build is
//! [`crate::config::resolve_sourceport`], which every launch already goes
//! through.

pub mod build;
pub mod doctor;
pub mod manifest;
pub mod recipe;
pub mod registry;
pub mod update;

pub use build::{BuildOptions, BuildProgress, BuildStep, SourceportPaths};
pub use doctor::{DoctorReport, PackageCheck, doctor};
pub use manifest::{
    InstalledSourceport, SourceportManifest, find_installed, list_installed, remove_installed,
};
pub use recipe::{SourceportRecipe, builtin_recipes, find_recipe, load_recipes};
pub use update::{RemoteRefs, UpdateStatus, check_updates, remote_refs};

// Flat, because every caller has always said `sourceports::identify_family`
// and there is no reason for the file split to show up at the call site.
pub use registry::*;

/// A recipe paired with whatever is installed for it.
#[derive(Debug, Clone)]
pub struct SourceportStatus {
    pub recipe: SourceportRecipe,
    /// Most recent usable install, if there is one.
    pub installed: Option<InstalledSourceport>,
    /// Commit the ref pointed at as of the last update check, if one has
    /// run. Read from the cache, so building this list stays offline.
    pub remote_commit: Option<String>,
}

impl SourceportStatus {
    /// Whether rebuilding from the currently selected ref would switch refs.
    ///
    /// The selection belongs to the frontend rather than the recipe: a user
    /// can intentionally build a stable release while the recipe defaults to
    /// a development branch. Comparing only with the recipe would report that
    /// perfectly intentional release build as needing attention forever.
    pub fn selected_ref_changed(&self, selected_ref: &str) -> bool {
        self.installed
            .as_ref()
            .is_some_and(|installed| installed.manifest.git_ref != selected_ref)
    }

    /// Whether the remote has moved past the installed build.
    pub fn update_available(&self) -> bool {
        let (Some(installed), Some(remote)) = (&self.installed, &self.remote_commit) else {
            return false;
        };
        installed.manifest.commit != "unknown" && &installed.manifest.commit != remote
    }
}

/// Every known recipe with its install state, for the sourceports UI.
/// Never reaches the network — remote state comes from whatever the last
/// update check cached, so opening the dialog is instant.
pub fn status(paths: &SourceportPaths) -> crate::Result<Vec<SourceportStatus>> {
    let installed = list_installed(&paths.prefix_root);
    let cache = update::load_cache(&paths.prefix_root);
    let mut out = Vec::new();
    for recipe in load_recipes(&paths.recipe_dir)? {
        let current = installed
            .iter()
            .find(|p| p.manifest.name == recipe.name && p.is_usable())
            .cloned();
        let remote_commit = cache
            .sourceports
            .get(&recipe.name)
            .map(|c| c.remote_commit.clone());
        out.push(SourceportStatus {
            recipe,
            installed: current,
            remote_commit,
        });
    }
    Ok(out)
}

/// Path to a managed build of `name`, if one is installed and usable.
///
/// Consulted by [`crate::config::resolve_sourceport`] ahead of `PATH`: a sourceport
/// the user asked caco to build is the one they meant, even when a distro
/// package of the same name happens to exist.
pub fn managed_binary(name: &str) -> Option<String> {
    let installed = find_installed(&crate::config::sourceport_prefix_root(), name)?;
    Some(installed.binary_path().display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &std::path::Path) -> SourceportPaths {
        SourceportPaths {
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
        assert!(all.iter().all(|s| !s.selected_ref_changed("master")));
    }

    #[test]
    fn status_compares_an_install_with_the_selected_ref_not_the_recipe_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());

        // Installed from a deliberately selected stable release...
        let prefix = p.prefix_root.join("uzdoom").join("v1.0.0");
        manifest::write_manifest(
            &prefix,
            &SourceportManifest {
                name: "uzdoom".into(),
                repo: "https://github.com/UZDoom/uzdoom".into(),
                git_ref: "v1.0.0".into(),
                commit: "deadbeef".into(),
                binary: "uzdoom".into(),
                built_at: "2026-01-01T00:00:00+00:00".into(),
            },
        )
        .unwrap();
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::write(prefix.join("bin").join("uzdoom"), b"x").unwrap();

        // ...while the recipe still defaults to the development branch.
        std::fs::create_dir_all(&p.recipe_dir).unwrap();
        std::fs::write(
            p.recipe_dir.join("pin.toml"),
            r#"
[uzdoom]
repo = "https://github.com/UZDoom/uzdoom"
ref = "master"
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
        assert!(!uzdoom.selected_ref_changed("v1.0.0"));
        assert!(uzdoom.selected_ref_changed("master"));
    }
}
