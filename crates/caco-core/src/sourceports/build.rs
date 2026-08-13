//! Turn a recipe into an install prefix: clone, patch, configure, build, install.
//!
//! Every command caco runs is assembled by a pure function in this module and
//! only then handed to a process, so the argument lists — including flags that
//! are load-bearing and silently fatal if dropped, like uzdoom's
//! `-DINSTALL_PK3_PATH=bin` — are testable without a network or a compiler.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use crate::error::Error;

use super::manifest::{InstalledSourceport, SourceportManifest, write_manifest};
use super::recipe::SourceportRecipe;

/// Filesystem roots a build reads and writes.
///
/// Passed explicitly rather than read from config inside the driver: every
/// path in here is one a build creates and `remove` deletes from, and a test
/// that could reach the real roots is a test that can compile 74M of C++ into
/// a user's cache — or delete out of it. [`SourceportPaths::from_config`] is for
/// frontends; tests build these from a tempdir.
#[derive(Debug, Clone)]
pub struct SourceportPaths {
    /// Where user recipes and their patch files live.
    pub recipe_dir: PathBuf,
    /// Where checkouts and build trees go.
    pub src_root: PathBuf,
    /// Where install prefixes go.
    pub prefix_root: PathBuf,
}

impl SourceportPaths {
    pub fn from_config() -> Self {
        Self {
            recipe_dir: crate::config::sourceport_recipe_dir(),
            src_root: crate::config::sourceport_src_root(),
            prefix_root: crate::config::sourceport_prefix_root(),
        }
    }

    /// Git checkout for `name`.
    pub fn checkout_dir(&self, name: &str) -> PathBuf {
        self.src_root.join(name)
    }

    /// Build tree for `name`, kept beside the checkout rather than inside it
    /// so `git reset --hard` and a re-clone leave it alone.
    pub fn build_dir(&self, name: &str) -> PathBuf {
        self.src_root.join(format!("{name}.build"))
    }

    /// Install prefix for a recipe at its current ref.
    pub fn prefix(&self, recipe: &SourceportRecipe) -> PathBuf {
        self.prefix_root.join(&recipe.name).join(recipe.ref_slug())
    }
}

/// Stage of a build, for progress reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStep {
    Fetch,
    Patch,
    Configure,
    Compile,
    Install,
}

impl BuildStep {
    pub fn label(self) -> &'static str {
        match self {
            BuildStep::Fetch => "fetch",
            BuildStep::Patch => "patch",
            BuildStep::Configure => "configure",
            BuildStep::Compile => "compile",
            BuildStep::Install => "install",
        }
    }
}

/// Streamed while a build runs.
pub enum BuildProgress<'a> {
    /// A new stage started.
    Step(BuildStep),
    /// One line of output from the running command.
    Line(&'a str),
}

#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// Parallel jobs. `None` lets the generator decide, which for Ninja
    /// already means every core.
    pub jobs: Option<usize>,
    /// Discard the checkout and build tree first. The slow, always-correct
    /// path for when an incremental rebuild has gone wrong.
    pub clean: bool,
}

/// How many trailing output lines an error carries.
const ERROR_TAIL_LINES: usize = 25;

// ---------------------------------------------------------------------------
// Command construction
// ---------------------------------------------------------------------------

/// `git clone` for a fresh checkout.
///
/// Shallow: the full uzdoom history is 221M and no part of caco reads it.
/// `--branch` accepts a branch or a tag but not a bare commit SHA, which is
/// the one ref form a recipe cannot use.
pub fn clone_args(recipe: &SourceportRecipe, dest: &Path) -> Vec<String> {
    vec![
        "clone".into(),
        "--depth".into(),
        "1".into(),
        "--recurse-submodules".into(),
        "--shallow-submodules".into(),
        "--branch".into(),
        recipe.git_ref.clone(),
        recipe.repo.clone(),
        dest.display().to_string(),
    ]
}

/// `git fetch` to move an existing checkout to the recipe's ref.
pub fn fetch_args(recipe: &SourceportRecipe) -> Vec<String> {
    vec![
        "fetch".into(),
        "--depth".into(),
        "1".into(),
        "--tags".into(),
        "--force".into(),
        "origin".into(),
        recipe.git_ref.clone(),
    ]
}

/// Hard-reset onto whatever [`fetch_args`] just brought down, discarding any
/// patches a previous build applied so patching is never cumulative.
pub fn reset_args() -> Vec<String> {
    vec!["reset".into(), "--hard".into(), "FETCH_HEAD".into()]
}

/// Update submodules after a reset, for repos that have them.
pub fn submodule_args() -> Vec<String> {
    vec![
        "submodule".into(),
        "update".into(),
        "--init".into(),
        "--recursive".into(),
        "--depth".into(),
        "1".into(),
    ]
}

/// `git apply` for one patch file.
pub fn patch_args(patch: &Path) -> Vec<String> {
    vec!["apply".into(), "--3way".into(), patch.display().to_string()]
}

/// The cmake configure line.
///
/// The recipe's own args go last so a user override can win over anything
/// caco supplies.
pub fn configure_args(
    recipe: &SourceportRecipe,
    checkout: &Path,
    build_dir: &Path,
    prefix: &Path,
) -> Vec<String> {
    let source = match &recipe.source_subdir {
        Some(sub) => checkout.join(sub),
        None => checkout.to_path_buf(),
    };
    let mut args = vec![
        "-S".into(),
        source.display().to_string(),
        "-B".into(),
        build_dir.display().to_string(),
        format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display()),
    ];
    if let Some(generator) = &recipe.build.generator {
        args.push("-G".into());
        args.push(generator.clone());
    }
    args.extend(recipe.build.args.iter().cloned());
    args
}

/// The cmake build line.
pub fn compile_args(build_dir: &Path, jobs: Option<usize>) -> Vec<String> {
    let mut args = vec!["--build".into(), build_dir.display().to_string()];
    if let Some(jobs) = jobs {
        args.push("--parallel".into());
        args.push(jobs.to_string());
    }
    args
}

/// The cmake install line. The prefix was baked in at configure time.
pub fn install_args(build_dir: &Path) -> Vec<String> {
    vec!["--install".into(), build_dir.display().to_string()]
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Build `recipe` and install it into its prefix.
///
/// The install prefix is not touched until the compile has succeeded, so a
/// failed rebuild leaves the previously working sourceport in place.
pub fn install(
    recipe: &SourceportRecipe,
    paths: &SourceportPaths,
    opts: &BuildOptions,
    progress: &mut dyn FnMut(BuildProgress<'_>),
    cancel: &dyn Fn() -> bool,
) -> crate::Result<InstalledSourceport> {
    require_tools(recipe)?;

    let checkout = paths.checkout_dir(&recipe.name);
    let build_dir = paths.build_dir(&recipe.name);
    let prefix = paths.prefix(recipe);

    if opts.clean {
        let _ = std::fs::remove_dir_all(&checkout);
        let _ = std::fs::remove_dir_all(&build_dir);
    }

    // --- fetch ---
    progress(BuildProgress::Step(BuildStep::Fetch));
    std::fs::create_dir_all(&paths.src_root)?;
    if checkout.join(".git").is_dir() {
        git(
            recipe,
            BuildStep::Fetch,
            &checkout,
            fetch_args(recipe),
            progress,
            cancel,
        )?;
        git(
            recipe,
            BuildStep::Fetch,
            &checkout,
            reset_args(),
            progress,
            cancel,
        )?;
        git(
            recipe,
            BuildStep::Fetch,
            &checkout,
            submodule_args(),
            progress,
            cancel,
        )?;
    } else {
        let _ = std::fs::remove_dir_all(&checkout);
        git(
            recipe,
            BuildStep::Fetch,
            &paths.src_root,
            clone_args(recipe, &checkout),
            progress,
            cancel,
        )?;
    }

    // --- patch ---
    if !recipe.patches.is_empty() {
        progress(BuildProgress::Step(BuildStep::Patch));
        for patch in &recipe.patches {
            let path = resolve_patch(&paths.recipe_dir, patch);
            if !path.is_file() {
                return Err(Error::PortBuild {
                    sourceport: recipe.name.clone(),
                    step: BuildStep::Patch.label(),
                    detail: format!("patch not found: {}", path.display()),
                });
            }
            git(
                recipe,
                BuildStep::Patch,
                &checkout,
                patch_args(&path),
                progress,
                cancel,
            )?;
        }
    }

    // --- configure ---
    progress(BuildProgress::Step(BuildStep::Configure));
    run(
        recipe,
        BuildStep::Configure,
        cmake(configure_args(recipe, &checkout, &build_dir, &prefix), None),
        progress,
        cancel,
    )?;

    // --- compile ---
    progress(BuildProgress::Step(BuildStep::Compile));
    run(
        recipe,
        BuildStep::Compile,
        cmake(compile_args(&build_dir, opts.jobs), None),
        progress,
        cancel,
    )?;

    // --- install ---
    // Only now is the old prefix disturbed: everything above can fail without
    // costing the user a sourceport that currently works.
    if recipe.build.install {
        progress(BuildProgress::Step(BuildStep::Install));
        let _ = std::fs::remove_dir_all(&prefix);
        run(
            recipe,
            BuildStep::Install,
            cmake(install_args(&build_dir), None),
            progress,
            cancel,
        )?;
    }

    let commit = resolve_commit(&checkout);
    let manifest = SourceportManifest {
        name: recipe.name.clone(),
        repo: recipe.repo.clone(),
        git_ref: recipe.git_ref.clone(),
        commit,
        binary: recipe.binary.clone(),
        built_at: chrono::Local::now().to_rfc3339(),
    };
    write_manifest(&prefix, &manifest)?;
    // The commit just recorded is the current answer, so a cached "behind"
    // verdict from before the build would keep the badge lit until the
    // check interval expired.
    super::update::invalidate(&paths.prefix_root, &recipe.name);

    let installed = InstalledSourceport { manifest, prefix };
    if !installed.is_usable() {
        return Err(Error::PortBuild {
            sourceport: recipe.name.clone(),
            step: BuildStep::Install.label(),
            detail: format!(
                "build succeeded but {} is missing — check `binary` in the recipe",
                installed.binary_path().display()
            ),
        });
    }
    Ok(installed)
}

/// The tools a recipe needs before anything is downloaded.
///
/// Checked up front so a missing generator fails in a second rather than
/// after a 221M clone.
fn require_tools(recipe: &SourceportRecipe) -> crate::Result<()> {
    match missing_tool(&required_tools(recipe)) {
        Some(tool) => Err(Error::MissingTool(tool)),
        None => Ok(()),
    }
}

/// The first of `tools` that is not on PATH.
fn missing_tool(tools: &[String]) -> Option<String> {
    tools
        .iter()
        .find(|t| crate::config::which(t).is_none())
        .cloned()
}

/// Executables that must be on PATH to build `recipe`.
pub fn required_tools(recipe: &SourceportRecipe) -> Vec<String> {
    let mut tools = vec!["git".to_string(), "cmake".to_string()];
    if let Some(generator) = &recipe.build.generator {
        // "Ninja", "Unix Makefiles" — only the single-word generators name a
        // binary, and Makefiles are guaranteed present wherever cmake is.
        let lowered = generator.to_ascii_lowercase();
        if lowered == "ninja" {
            tools.push("ninja".to_string());
        }
    }
    tools
}

/// Resolve a patch path from a recipe against the recipe directory.
fn resolve_patch(recipe_dir: &Path, patch: &str) -> PathBuf {
    let path = Path::new(patch);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        recipe_dir.join(path)
    }
}

fn cmake(args: Vec<String>, cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new("cmake");
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd
}

fn git(
    recipe: &SourceportRecipe,
    step: BuildStep,
    cwd: &Path,
    args: Vec<String>,
    progress: &mut dyn FnMut(BuildProgress<'_>),
    cancel: &dyn Fn() -> bool,
) -> crate::Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd);
    run(recipe, step, cmd, progress, cancel)
}

/// The commit the checkout ended up on, or `"unknown"` if git will not say.
///
/// Recorded because `ref = "master"` is not reproducible on its own: the
/// manifest is what lets a user see which build they are actually running.
fn resolve_commit(checkout: &Path) -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(checkout)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Run a command, streaming merged stdout/stderr to `progress`.
///
/// Cancellation kills the child and then drains the pipes to completion: the
/// reader threads own the pipe ends, and returning while they are still
/// blocked would leak a thread per cancelled build.
fn run(
    recipe: &SourceportRecipe,
    step: BuildStep,
    mut cmd: Command,
    progress: &mut dyn FnMut(BuildProgress<'_>),
    cancel: &dyn Fn() -> bool,
) -> crate::Result<()> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| Error::PortBuild {
        sourceport: recipe.name.clone(),
        step: step.label(),
        detail: e.to_string(),
    })?;

    let (tx, rx) = mpsc::channel::<String>();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let readers: Vec<_> = [
        stdout.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        stderr.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|pipe| {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        })
    })
    .collect();
    drop(tx);

    let mut tail: Vec<String> = Vec::new();
    let mut cancelled = false;
    for line in rx {
        if tail.len() == ERROR_TAIL_LINES {
            tail.remove(0);
        }
        tail.push(line.clone());
        progress(BuildProgress::Line(&line));
        if !cancelled && cancel() {
            cancelled = true;
            let _ = child.kill();
        }
    }
    for reader in readers {
        let _ = reader.join();
    }

    let status = child.wait().map_err(|e| Error::PortBuild {
        sourceport: recipe.name.clone(),
        step: step.label(),
        detail: e.to_string(),
    })?;

    if cancelled {
        return Err(Error::PortBuildCancelled(recipe.name.clone()));
    }
    if !status.success() {
        let detail = if tail.is_empty() {
            format!("exited with {status}")
        } else {
            format!("exited with {status}\n{}", tail.join("\n"))
        };
        return Err(Error::PortBuild {
            sourceport: recipe.name.clone(),
            step: step.label(),
            detail,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sourceports::recipe::builtin_recipes;

    fn recipe(name: &str) -> SourceportRecipe {
        builtin_recipes()
            .into_iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no {name} recipe"))
    }

    /// Roots under a tempdir. Nothing in this module's tests may run a build
    /// or touch a path outside one of these.
    fn paths(root: &Path) -> SourceportPaths {
        SourceportPaths {
            recipe_dir: root.join("recipes"),
            src_root: root.join("src"),
            prefix_root: root.join("prefix"),
        }
    }

    #[test]
    fn prefix_is_namespaced_by_name_and_ref() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let mut r = recipe("uzdoom");
        r.git_ref = "origin/master".into();
        assert_eq!(
            p.prefix(&r),
            p.prefix_root.join("uzdoom").join("origin-master")
        );
    }

    #[test]
    fn build_dir_is_outside_the_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        assert!(!p.build_dir("uzdoom").starts_with(p.checkout_dir("uzdoom")));
    }

    #[test]
    fn clone_is_shallow_and_pinned_to_the_ref() {
        let dir = tempfile::tempdir().unwrap();
        let r = recipe("nyan-doom");
        let args = clone_args(&r, &dir.path().join("nyan-doom"));
        assert!(args.windows(2).any(|w| w == ["--depth", "1"]));
        assert!(args.windows(2).any(|w| w == ["--branch", "master"]));
        assert!(args.contains(&r.repo));
    }

    #[test]
    fn configure_points_cmake_at_the_source_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let r = recipe("nyan-doom");
        let args = configure_args(
            &r,
            &p.checkout_dir("nyan-doom"),
            &p.build_dir("nyan-doom"),
            &p.prefix(&r),
        );

        let src = args[args.iter().position(|a| a == "-S").unwrap() + 1].clone();
        assert!(src.ends_with("nyan-doom/prboom2"), "got {src}");
    }

    #[test]
    fn configure_uses_the_checkout_root_when_there_is_no_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let r = recipe("uzdoom");
        let args = configure_args(
            &r,
            &p.checkout_dir("uzdoom"),
            &p.build_dir("uzdoom"),
            &p.prefix(&r),
        );

        let src = args[args.iter().position(|a| a == "-S").unwrap() + 1].clone();
        assert!(src.ends_with("uzdoom"), "got {src}");
    }

    #[test]
    fn configure_bakes_the_install_prefix_and_keeps_recipe_flags() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let r = recipe("uzdoom");
        let prefix = p.prefix(&r);
        let args = configure_args(
            &r,
            &p.checkout_dir("uzdoom"),
            &p.build_dir("uzdoom"),
            &prefix,
        );

        assert!(args.contains(&format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display())));
        // Losing this yields a build that succeeds and aborts at launch.
        assert!(args.contains(&"-DINSTALL_PK3_PATH=bin".to_string()));
        assert!(args.windows(2).any(|w| w == ["-G", "Ninja"]));
    }

    #[test]
    fn recipe_args_come_after_cacos_own() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let mut r = recipe("uzdoom");
        r.build.args = vec!["-DCMAKE_INSTALL_PREFIX=/somewhere/else".into()];
        let args = configure_args(
            &r,
            &p.checkout_dir("uzdoom"),
            &p.build_dir("uzdoom"),
            &p.prefix(&r),
        );

        let last = args
            .iter()
            .rposition(|a| a.starts_with("-DCMAKE_INSTALL_PREFIX="));
        assert_eq!(
            args[last.unwrap()],
            "-DCMAKE_INSTALL_PREFIX=/somewhere/else"
        );
    }

    #[test]
    fn compile_only_passes_parallel_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let build = dir.path().join("b");
        assert!(!compile_args(&build, None).contains(&"--parallel".to_string()));
        assert!(
            compile_args(&build, Some(4))
                .windows(2)
                .any(|w| w == ["--parallel", "4"])
        );
    }

    #[test]
    fn install_targets_the_build_dir() {
        let dir = tempfile::tempdir().unwrap();
        let build = dir.path().join("b");
        assert_eq!(
            install_args(&build),
            vec!["--install".to_string(), build.display().to_string()]
        );
    }

    #[test]
    fn ninja_generator_is_a_required_tool() {
        assert!(required_tools(&recipe("uzdoom")).contains(&"ninja".to_string()));
        let mut r = recipe("uzdoom");
        r.build.generator = Some("Unix Makefiles".into());
        assert!(!required_tools(&r).contains(&"ninja".to_string()));
        assert!(required_tools(&r).contains(&"cmake".to_string()));
    }

    #[test]
    fn patches_resolve_against_the_recipe_dir() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        assert_eq!(
            resolve_patch(&p.recipe_dir, "fix.patch"),
            p.recipe_dir.join("fix.patch")
        );
        assert_eq!(
            resolve_patch(&p.recipe_dir, "/abs/fix.patch"),
            PathBuf::from("/abs/fix.patch")
        );
    }

    #[test]
    fn missing_tool_names_the_first_absent_one() {
        let tools = vec![
            "caco-no-such-tool-a".to_string(),
            "caco-no-such-tool-b".to_string(),
        ];
        assert_eq!(missing_tool(&tools).as_deref(), Some("caco-no-such-tool-a"));
        assert_eq!(missing_tool(&[]), None);
    }
}
