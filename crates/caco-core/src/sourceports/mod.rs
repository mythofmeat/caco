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
pub mod versions;

pub use build::{BuildOptions, BuildProgress, BuildStep, SourceportPaths};
pub use doctor::{DoctorReport, Host, PackageCheck, doctor};
pub use manifest::{
    InstalledSourceport, SourceportManifest, find_installed, list_installed, remove_installed,
};
pub use recipe::{SourceportRecipe, builtin_recipes, find_recipe, load_recipes};
pub use update::{CachedCheck, Update, UpdateStatus, check_updates};
pub use versions::{ReleaseNumber, RemoteVersions, Target, Version, remote_versions};

// Flat, because every caller has always said `sourceports::identify_family`
// and there is no reason for the file split to show up at the call site.
pub use registry::*;

/// A recipe paired with whatever is installed for it.
#[derive(Debug, Clone)]
pub struct SourceportStatus {
    pub recipe: SourceportRecipe,
    /// Most recent usable install, if there is one.
    pub installed: Option<InstalledSourceport>,
    /// What the last update check found, if one has run. Read from the cache,
    /// so building this list stays offline.
    pub last_check: Option<CachedCheck>,
}

impl SourceportStatus {
    /// What the installed build was made as.
    pub fn installed_version(&self) -> Option<Version> {
        let installed = self.installed.as_ref()?;
        Some(Version::of(
            &installed.manifest,
            self.recipe.first_release.as_ref(),
        ))
    }

    /// Whether building `version` would be a different version from the one installed,
    /// rather than the same one again or an update to it.
    ///
    /// What to build belongs to the frontend rather than the recipe, so that an older
    /// release can be chosen without anybody's recipe being rewritten, and comparing the
    /// install with the recipe would report that choice as needing attention forever.
    pub fn switches_version(&self, version: &Version) -> bool {
        self.installed_version()
            .is_some_and(|installed| &installed != version)
    }

    /// What rebuilding the installed build as it was built would bring: from `versions`
    /// once the repository has been asked, and otherwise from the last update check.
    pub fn update(&self, versions: Option<&RemoteVersions>) -> Option<Update> {
        let installed = &self.installed.as_ref()?.manifest;
        match versions {
            Some(versions) => {
                let target = versions.resolve(&self.installed_version()?)?;
                let release = target.follows_releases.then_some(target.name.as_str());
                update::update_to(installed, &target.commit, release)
            }
            None => self.last_check.as_ref()?.update_for(installed),
        }
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
        let last_check = cache.sourceports.get(&recipe.name).cloned();
        out.push(SourceportStatus {
            recipe,
            installed: current,
            last_check,
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

    const REPOSITORY: &str = "\
ref: refs/heads/trunk\tHEAD
1111111111111111111111111111111111111111\trefs/heads/trunk
5025025025025025025025025025025025025025\trefs/tags/5.0.2
5035035035035035035035035035035035035035\trefs/tags/5.0.3
";

    fn versions() -> RemoteVersions {
        RemoteVersions::parse(REPOSITORY, None)
    }

    fn uzdoom_built(
        git_ref: &str,
        follows_releases: Option<bool>,
        commit: &str,
    ) -> SourceportStatus {
        let recipe = find_recipe(std::path::Path::new("/nonexistent"), "uzdoom").unwrap();
        SourceportStatus {
            installed: Some(InstalledSourceport {
                manifest: SourceportManifest {
                    name: recipe.name.clone(),
                    repo: recipe.repo.clone(),
                    git_ref: git_ref.into(),
                    commit: commit.into(),
                    follows_releases,
                    binary: recipe.binary.clone(),
                    built_at: "2026-01-01T00:00:00+00:00".into(),
                },
                prefix: std::path::PathBuf::from("/unused"),
            }),
            recipe,
            last_check: None,
        }
    }

    #[test]
    fn status_lists_every_recipe_even_with_nothing_installed() {
        let dir = tempfile::tempdir().unwrap();
        let all = status(&paths(dir.path())).unwrap();
        assert_eq!(all.len(), builtin_recipes().len());
        assert!(all.iter().all(|s| s.installed.is_none()));
        assert!(
            all.iter()
                .all(|s| !s.switches_version(&Version::LatestRelease))
        );
    }

    #[test]
    fn status_compares_an_install_with_the_chosen_version_not_the_recipe_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());

        // Installed from an older release, chosen to stay on...
        let prefix = p.prefix("uzdoom", "5.0.2");
        manifest::write_manifest(
            &prefix,
            &SourceportManifest {
                name: "uzdoom".into(),
                repo: "https://github.com/UZDoom/uzdoom".into(),
                git_ref: "5.0.2".into(),
                commit: "deadbeef".into(),
                follows_releases: Some(false),
                binary: "uzdoom".into(),
                built_at: "2026-01-01T00:00:00+00:00".into(),
            },
        )
        .unwrap();
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::write(prefix.join("bin").join("uzdoom"), b"x").unwrap();

        // ...while the recipe pins the development branch.
        std::fs::create_dir_all(&p.recipe_dir).unwrap();
        std::fs::write(
            p.recipe_dir.join("pin.toml"),
            r#"
[uzdoom]
repo = "https://github.com/UZDoom/uzdoom"
ref = "trunk"
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
        assert_eq!(
            uzdoom.recipe.default_version(),
            Version::Named("trunk".into())
        );
        assert!(!uzdoom.switches_version(&Version::Named("5.0.2".into())));
        assert!(uzdoom.switches_version(&Version::Named("trunk".into())));
        assert!(uzdoom.switches_version(&Version::LatestRelease));
    }

    #[test]
    fn a_build_that_follows_releases_is_updated_by_a_newer_one() {
        let uzdoom = uzdoom_built(
            "5.0.2",
            Some(true),
            "5025025025025025025025025025025025025025",
        );
        assert!(!uzdoom.switches_version(&Version::LatestRelease));
        assert_eq!(
            uzdoom.update(Some(&versions())),
            Some(Update::Release("5.0.3".into()))
        );
    }

    #[test]
    fn an_older_release_chosen_to_stay_on_has_nothing_newer() {
        let uzdoom = uzdoom_built(
            "5.0.2",
            Some(false),
            "5025025025025025025025025025025025025025",
        );
        assert_eq!(uzdoom.update(Some(&versions())), None);
    }

    #[test]
    fn a_branch_build_is_updated_by_new_commits() {
        let uzdoom = uzdoom_built(
            "trunk",
            Some(false),
            "0000000000000000000000000000000000000000",
        );
        assert_eq!(uzdoom.update(Some(&versions())), Some(Update::Commits));
    }

    #[test]
    fn until_the_repository_answers_the_last_check_does() {
        let mut uzdoom = uzdoom_built(
            "5.0.2",
            Some(true),
            "5025025025025025025025025025025025025025",
        );
        assert_eq!(uzdoom.update(None), None);
        uzdoom.last_check = Some(CachedCheck {
            checked_at: "2026-01-02T00:00:00+00:00".into(),
            remote_commit: "5035035035035035035035035035035035035035".into(),
            release: Some("5.0.3".into()),
        });
        assert_eq!(uzdoom.update(None), Some(Update::Release("5.0.3".into())));
    }

    #[test]
    fn a_build_with_no_known_commit_never_has_an_update() {
        let uzdoom = uzdoom_built("trunk", Some(false), "unknown");
        assert_eq!(uzdoom.update(Some(&versions())), None);
    }
}
