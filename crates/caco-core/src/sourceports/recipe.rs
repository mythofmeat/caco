//! Sourceport build recipes: what to clone, how to configure it, what it needs.
//!
//! A recipe is the portable artifact. Binaries are ABI- and OS-specific and
//! live on the cache side with the WAD downloads; the recipe is a few hundred
//! bytes of TOML that travels with the library and rebuilds the binary on
//! whatever machine it lands on.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// The built-in recipe set, compiled into the binary.
const BUILTIN: &str = include_str!("recipes.toml");

/// Which build system drives a recipe.
///
/// Only cmake exists today. It is an enum rather than a bare string so an
/// unknown value fails at parse time, where the file name is still in hand,
/// rather than at build time halfway through a clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildSystem {
    Cmake,
}

/// How to configure and build a sourceport once it is checked out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSpec {
    pub system: BuildSystem,
    /// cmake `-G` generator. `None` uses cmake's platform default.
    #[serde(default)]
    pub generator: Option<String>,
    /// Extra configure flags, appended after the ones caco supplies.
    #[serde(default)]
    pub args: Vec<String>,
    /// Whether `cmake --install` runs. Effectively always true: every sourceport
    /// caco supports needs its data wad or pk3s next to the binary, and the
    /// bare executable out of the build tree is useless.
    #[serde(default = "default_true")]
    pub install: bool,
}

fn default_true() -> bool {
    true
}

/// System packages a recipe needs, per platform package manager.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepSpec {
    #[serde(default)]
    pub arch: Vec<String>,
}

impl DepSpec {
    /// The dependency list for the platform caco is running on, or an empty
    /// slice where caco has no package list to offer.
    pub fn for_host(&self) -> &[String] {
        if cfg!(target_os = "linux") {
            &self.arch
        } else {
            &[]
        }
    }
}

/// Everything needed to turn a git URL into a runnable sourceport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceportRecipe {
    /// Key the recipe was filed under. Also the sourceport name caco matches
    /// against `config.sourceport`, so it must equal an executable name known
    /// to [`crate::sourceports`] for family features to apply.
    #[serde(skip)]
    pub name: String,
    pub repo: String,
    /// Branch, tag or commit to check out.
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// Subdirectory holding the cmake root, when it is not the repo root.
    #[serde(default)]
    pub source_subdir: Option<String>,
    /// Executable name inside the install prefix's `bin/`.
    pub binary: String,
    pub build: BuildSpec,
    #[serde(default)]
    pub deps: DepSpec,
    /// Patch files applied after checkout and before configure. Relative
    /// paths resolve against the recipe directory, which is why patches
    /// belong on the portable side with the recipes themselves.
    #[serde(default)]
    pub patches: Vec<String>,
}

impl SourceportRecipe {
    /// Directory name for this recipe's install prefix.
    ///
    /// Refs can contain path separators (`origin/master`), so they cannot be
    /// used as a path component unchanged.
    pub fn ref_slug(&self) -> String {
        slugify_ref(&self.git_ref)
    }
}

/// Make a git ref safe to use as a single path component.
pub fn slugify_ref(git_ref: &str) -> String {
    let slug: String = git_ref
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // A component of "." or ".." would escape the prefix root.
    if slug.chars().all(|c| c == '.') {
        return "ref".to_string();
    }
    slug
}

/// Parse a TOML document holding one or more `[name]` recipe tables.
pub fn parse_recipes(toml_str: &str) -> crate::Result<Vec<SourceportRecipe>> {
    let map: BTreeMap<String, SourceportRecipe> = toml::from_str(toml_str)?;
    Ok(map
        .into_iter()
        .map(|(name, mut recipe)| {
            recipe.name = name;
            recipe
        })
        .collect())
}

/// The recipes compiled into this binary.
///
/// Panics only if `recipes.toml` is malformed, which is a build-time mistake
/// caught by the test at the bottom of this file.
pub fn builtin_recipes() -> Vec<SourceportRecipe> {
    parse_recipes(BUILTIN).expect("built-in recipes.toml is malformed")
}

/// Every recipe caco knows about: the built-in set with any user files in
/// `recipe_dir` merged over it by name.
///
/// A user file may define several recipes and may be named anything ending in
/// `.toml`; files are read in sorted order so a merge conflict between two of
/// them resolves predictably. A malformed file is reported rather than
/// skipped — silently ignoring it would look identical to the override not
/// being picked up.
pub fn load_recipes(recipe_dir: &Path) -> crate::Result<Vec<SourceportRecipe>> {
    let mut merged: BTreeMap<String, SourceportRecipe> = builtin_recipes()
        .into_iter()
        .map(|r| (r.name.clone(), r))
        .collect();

    let mut files: Vec<_> = match std::fs::read_dir(recipe_dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect(),
        // No recipe directory just means no overrides.
        Err(_) => Vec::new(),
    };
    files.sort();

    for path in files {
        let text = std::fs::read_to_string(&path)?;
        let recipes = parse_recipes(&text).map_err(|e| {
            Error::Config(format!(
                "{}: {}",
                path.display(),
                strip_prefix(&e.to_string())
            ))
        })?;
        for recipe in recipes {
            merged.insert(recipe.name.clone(), recipe);
        }
    }

    Ok(merged.into_values().collect())
}

/// Drop the `TOML parse error: ` prefix so a wrapped message does not read
/// `config error: TOML parse error: ...`.
fn strip_prefix(msg: &str) -> String {
    msg.trim_start_matches("TOML parse error: ").to_string()
}

/// Look up a single recipe by name.
pub fn find_recipe(recipe_dir: &Path, name: &str) -> crate::Result<SourceportRecipe> {
    load_recipes(recipe_dir)?
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| Error::PortNotFound(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_recipes_parse() {
        let recipes = builtin_recipes();
        let names: Vec<_> = recipes.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"nyan-doom"));
        assert!(names.contains(&"uzdoom"));
    }

    #[test]
    fn builtin_names_are_known_sourceports() {
        // A recipe whose name is not a known executable would build fine and
        // then silently lose complevel args, save dirs and config profiles.
        for recipe in builtin_recipes() {
            assert!(
                crate::sourceports::identify_family(&recipe.name).is_some(),
                "recipe '{}' is not mapped to a sourceport family",
                recipe.name
            );
        }
    }

    #[test]
    fn uzdoom_keeps_the_pk3_install_path() {
        // Dropping this flag produces a build that succeeds and only fails at
        // launch, so it is worth a test rather than only a comment.
        let uzdoom = builtin_recipes()
            .into_iter()
            .find(|r| r.name == "uzdoom")
            .expect("uzdoom recipe");
        assert!(
            uzdoom
                .build
                .args
                .iter()
                .any(|a| a == "-DINSTALL_PK3_PATH=bin"),
            "uzdoom must install its pk3s beside the binary"
        );
    }

    #[test]
    fn nyan_doom_builds_from_its_subdir() {
        let nyan = builtin_recipes()
            .into_iter()
            .find(|r| r.name == "nyan-doom")
            .expect("nyan-doom recipe");
        assert_eq!(nyan.source_subdir.as_deref(), Some("prboom2"));
    }

    #[test]
    fn unknown_build_system_is_rejected() {
        let err = parse_recipes(
            r#"
[foo]
repo = "https://example.invalid/foo"
ref = "master"
binary = "foo"
[foo.build]
system = "meson"
"#,
        );
        assert!(err.is_err());
    }

    #[test]
    fn ref_slug_cannot_escape_the_prefix_root() {
        assert_eq!(slugify_ref("master"), "master");
        assert_eq!(slugify_ref("v1.2.3"), "v1.2.3");
        assert_eq!(slugify_ref("origin/master"), "origin-master");
        assert_eq!(slugify_ref(".."), "ref");
        assert_eq!(slugify_ref("."), "ref");
        // Dots survive, separators do not — the result is one component that
        // cannot traverse, which is the property that matters.
        assert_eq!(slugify_ref("../../etc"), "..-..-etc");
        assert!(!slugify_ref("../../etc").contains(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn user_files_override_builtins_by_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pin.toml"),
            r#"
[uzdoom]
repo = "https://github.com/UZDoom/uzdoom"
ref = "v1.0.0"
binary = "uzdoom"
[uzdoom.build]
system = "cmake"
args = ["-DCMAKE_BUILD_TYPE=Release", "-DINSTALL_PK3_PATH=bin"]
"#,
        )
        .unwrap();

        let recipes = load_recipes(dir.path()).unwrap();
        let uzdoom = recipes.iter().find(|r| r.name == "uzdoom").unwrap();
        assert_eq!(uzdoom.git_ref, "v1.0.0");
        // Overriding one recipe must not drop the others.
        assert!(recipes.iter().any(|r| r.name == "nyan-doom"));
    }

    #[test]
    fn user_files_can_add_new_recipes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("extra.toml"),
            r#"
[woof]
repo = "https://github.com/fabiangreffrath/woof"
ref = "master"
binary = "woof"
[woof.build]
system = "cmake"
"#,
        )
        .unwrap();

        let recipes = load_recipes(dir.path()).unwrap();
        assert!(recipes.iter().any(|r| r.name == "woof"));
        assert_eq!(recipes.len(), builtin_recipes().len() + 1);
    }

    #[test]
    fn a_malformed_user_file_is_an_error_not_a_silent_skip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.toml"), "this is not toml =").unwrap();
        let err = load_recipes(dir.path()).unwrap_err();
        assert!(err.to_string().contains("bad.toml"), "got: {err}");
    }

    #[test]
    fn a_missing_recipe_dir_yields_the_builtins() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert_eq!(
            load_recipes(&missing).unwrap().len(),
            builtin_recipes().len()
        );
    }

    #[test]
    fn find_recipe_reports_the_name_it_could_not_find() {
        let dir = tempfile::tempdir().unwrap();
        let err = find_recipe(dir.path(), "nope").unwrap_err();
        assert!(err.to_string().contains("nope"));
    }
}
