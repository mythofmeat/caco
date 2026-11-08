//! End-to-end build of a real sourceport.
//!
//! `#[ignore]`d: it clones from the network and compiles a C project, so it
//! has no business running in `cargo test --workspace`. Run it deliberately
//! after touching the build driver:
//!
//! ```bash
//! cargo test -p caco-core --test sourceports_build -- --ignored --nocapture
//! ```
//!
//! Every path it uses comes from a tempdir. It must never call
//! `SourceportPaths::from_config()`.

use std::sync::atomic::{AtomicBool, Ordering};

use caco_core::sourceports::{BuildOptions, BuildProgress, SourceportPaths, build, recipe};

fn sandbox(root: &std::path::Path) -> SourceportPaths {
    SourceportPaths {
        recipe_dir: root.join("recipes"),
        src_root: root.join("src"),
        prefix_root: root.join("prefix"),
    }
}

#[test]
#[ignore = "clones and compiles a sourceport"]
fn nyan_doom_builds_and_installs() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sandbox(dir.path());
    let recipe = recipe::find_recipe(&paths.recipe_dir, "nyan-doom").unwrap();

    let mut steps = Vec::new();
    let installed = build::install(
        &recipe,
        &recipe.default_version(),
        &paths,
        &BuildOptions::default(),
        &mut |p| match p {
            BuildProgress::Step(s) => {
                eprintln!("== {}", s.label());
                steps.push(s);
            }
            BuildProgress::Line(l) => eprintln!("   {l}"),
        },
        &|| false,
    )
    .expect("build failed");

    // The install prefix, not the build tree, is what a launch resolves to.
    assert!(
        installed.is_usable(),
        "{:?} missing",
        installed.binary_path()
    );
    assert!(installed.prefix.starts_with(&paths.prefix_root));
    assert_ne!(installed.manifest.commit, "unknown");

    // The bare binary is useless without its data wad; the point of running
    // `cmake --install` at all is that this file lands beside it.
    let data_wad = walk(&installed.prefix)
        .into_iter()
        .any(|p| p.file_name().is_some_and(|n| n == "nyan-doom.wad"));
    assert!(data_wad, "nyan-doom.wad was not installed");

    // Relocatability: the prefix must still run after being renamed, which is
    // what lets it sit in a cache directory the user may move.
    let moved = dir.path().join("moved-prefix");
    std::fs::rename(&installed.prefix, &moved).unwrap();
    let binary = moved.join("bin").join(&installed.manifest.binary);
    let out = std::process::Command::new(&binary)
        .arg("-help")
        .output()
        .expect("moved binary did not run");
    let text = String::from_utf8_lossy(&out.stderr).to_lowercase()
        + &String::from_utf8_lossy(&out.stdout).to_lowercase();
    assert!(
        !text.contains("cannot find") && !text.contains("not found"),
        "moved prefix could not find its data: {text}"
    );
}

/// uzdoom is the expensive case and the one with a silently-fatal flag.
///
/// Its stock install puts the pk3s in `share/games/uzdoom` while progdir — with
/// `SYSTEMINSTALL` off, the default — looks beside the executable, so the build
/// succeeds and only aborts at launch with `Cannot find uzdoom.pk3`.
/// `-DINSTALL_PK3_PATH=bin` is what fixes that while keeping the prefix
/// relocatable, and this test is what stops it being "cleaned up" later.
///
/// Deliberately does not execute the binary: uzdoom has no batch mode that
/// exits, so a launch would open a window. Where the pk3s land *is* the
/// property, since progdir is the executable's own directory by definition.
#[test]
#[ignore = "clones and compiles a large C++ sourceport (~4 min)"]
fn uzdoom_installs_its_pk3s_beside_the_binary() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sandbox(dir.path());
    let recipe = recipe::find_recipe(&paths.recipe_dir, "uzdoom").unwrap();

    let installed = build::install(
        &recipe,
        &recipe.default_version(),
        &paths,
        &BuildOptions::default(),
        &mut |p| {
            if let BuildProgress::Step(s) = p {
                eprintln!("== {}", s.label());
            }
        },
        &|| false,
    )
    .expect("build failed");

    assert!(installed.is_usable());
    let bin = installed.prefix.join("bin");
    assert!(
        bin.join("uzdoom.pk3").is_file(),
        "uzdoom.pk3 is not beside the binary — has -DINSTALL_PK3_PATH=bin been dropped? \
         bin/ holds: {:?}",
        std::fs::read_dir(&bin)
            .map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
            .unwrap_or_default()
    );

    // The other four support pk3s ride along with the same flag.
    let pk3s = std::fs::read_dir(&bin)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "pk3"))
        .count();
    assert!(pk3s >= 5, "expected 5 pk3s beside the binary, found {pk3s}");
}

#[test]
#[ignore = "clones a sourceport"]
fn cancel_stops_a_build_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sandbox(dir.path());
    let recipe = recipe::find_recipe(&paths.recipe_dir, "nyan-doom").unwrap();

    let flag = AtomicBool::new(false);
    let err = build::install(
        &recipe,
        &recipe.default_version(),
        &paths,
        &BuildOptions::default(),
        &mut |p| {
            // Cancel on the first line of output the clone produces.
            if matches!(p, BuildProgress::Line(_)) {
                flag.store(true, Ordering::Relaxed);
            }
        },
        &|| flag.load(Ordering::Relaxed),
    )
    .unwrap_err();

    assert!(
        matches!(err, caco_core::error::Error::PortBuildCancelled(_)),
        "got {err}"
    );
    // A cancelled build must not leave a prefix a launch would resolve to.
    assert!(caco_core::sourceports::find_installed(&paths.prefix_root, "nyan-doom").is_none());
}

/// Run git in `cwd` with nobody's settings, so a developer's signing setup
/// cannot stop a commit.
fn git(cwd: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=caco",
            "-c",
            "user.email=caco@example.invalid",
        ])
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

/// Commit a version of the fake sourceport, whose "binary" says what it is.
fn commit_version(repo: &std::path::Path, says: &str) {
    std::fs::write(repo.join("fake-doom"), format!("#!/bin/sh\necho {says}\n")).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "--quiet", "-m", says]);
}

fn installed_says(installed: &caco_core::sourceports::InstalledSourceport) -> String {
    std::fs::read_to_string(installed.binary_path()).unwrap()
}

/// Everything a build does with versions, on a sourceport small enough to build
/// in a second from a repository on disk: what the latest release is, that an
/// update to it replaces the release it updates, and that a name which is both
/// a tag and a branch builds the tag, in a new checkout as in an old one. uzdoom
/// has a `4.14.3` of each, and `git clone --branch` used to take the branch.
#[test]
#[ignore = "runs git and cmake"]
fn versions_resolve_and_an_update_replaces_the_release_it_updates() {
    use caco_core::sourceports::{Version, list_installed};

    let dir = tempfile::tempdir().unwrap();
    let paths = sandbox(dir.path());
    let repo = dir.path().join("fake-doom");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=trunk"]);
    std::fs::write(
        repo.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.10)\nproject(fake NONE)\n\
         install(PROGRAMS fake-doom DESTINATION bin)\n",
    )
    .unwrap();
    commit_version(&repo, "release 1.0");
    git(&repo, &["tag", "-a", "-m", "1.0", "1.0"]);
    commit_version(&repo, "work on trunk");
    // A branch with the same name as the release, somewhere else.
    git(&repo, &["branch", "1.0"]);

    let recipe = recipe::parse_recipes(&format!(
        "[fake-doom]\nrepo = {:?}\nbinary = \"fake-doom\"\n[fake-doom.build]\nsystem = \"cmake\"\n",
        repo.display().to_string()
    ))
    .unwrap()
    .remove(0);
    let build = |version: &Version, clean: bool| {
        build::install(
            &recipe,
            version,
            &paths,
            &BuildOptions { jobs: None, clean },
            &mut |p| {
                if let BuildProgress::Line(l) = p {
                    eprintln!("   {l}");
                }
            },
            &|| false,
        )
        .expect("build failed")
    };

    let first = build(&Version::LatestRelease, false);
    assert_eq!(first.manifest.git_ref, "1.0");
    assert_eq!(first.manifest.follows_releases, Some(true));
    assert!(installed_says(&first).contains("release 1.0"));

    commit_version(&repo, "release 2.0");
    git(&repo, &["tag", "2.0"]);
    let update = build(&Version::LatestRelease, false);
    assert_eq!(update.manifest.git_ref, "2.0");
    assert!(installed_says(&update).contains("release 2.0"));
    assert!(
        !first.prefix.exists(),
        "the release it updated is still there"
    );

    // In a new checkout, where this used to be a clone of the branch.
    let older = build(&Version::Named("1.0".into()), true);
    assert!(
        installed_says(&older).contains("release 1.0"),
        "built the branch"
    );
    assert_eq!(older.manifest.follows_releases, Some(false));
    // Going back to an older release is a choice, not an update: what it was on stays.
    assert!(update.prefix.exists());
    assert_eq!(list_installed(&paths.prefix_root).len(), 2);

    let trunk = build(&Version::Named("trunk".into()), false);
    assert!(installed_says(&trunk).contains("release 2.0"));
}

/// Print what this machine is missing for every built-in recipe.
///
/// Not an assertion — a machine with everything installed and a machine with
/// nothing are both valid. It exists so `doctor` can be eyeballed against a
/// real package manager, which is the only way to know the `pacman -T`,
/// `rpm -q --whatprovides` and `brew list` parsing is right.
#[test]
#[ignore = "reports on the host machine"]
fn doctor_report_for_this_machine() {
    for recipe in recipe::builtin_recipes() {
        let report = caco_core::sourceports::doctor(&recipe);
        eprintln!("== {}", report.sourceport);
        eprintln!("   can build:        {}", report.can_build());
        eprintln!("   missing tools:    {:?}", report.missing_tools);
        eprintln!("   package check:    {:?}", report.package_check);
        eprintln!("   missing packages: {:?}", report.missing_packages);
        eprintln!("   hint:             {:?}", report.install_hint());
    }
}

/// Ask every built-in recipe's real repository what there is to build.
///
/// Catches what is otherwise invisible until someone presses Build: a project
/// that changes how it tags releases, starts a new kind of tag that reads as a
/// version, or stops saying which branch it is developed on.
#[test]
#[ignore = "hits the network"]
fn every_builtin_recipe_has_a_latest_release_and_a_development_branch() {
    use caco_core::sourceports::{Version, remote_versions, versions::is_release};

    for recipe in recipe::builtin_recipes() {
        let versions = remote_versions(&recipe).unwrap_or_else(|e| panic!("{}: {e}", recipe.name));
        let latest = versions
            .resolve(&Version::LatestRelease)
            .unwrap_or_else(|| panic!("{} has no latest release", recipe.name));
        eprintln!(
            "{}: latest {} at {}, development {:?}, older {:?}",
            recipe.name,
            latest.name,
            latest.commit,
            versions.development(),
            versions.older_releases()
        );
        assert!(latest.follows_releases, "{} has no releases", recipe.name);
        assert_eq!(latest.commit.len(), 40, "not a sha: {}", latest.commit);
        assert!(
            versions.development().is_some(),
            "{} does not say where it is developed",
            recipe.name
        );
        for release in versions.older_releases() {
            assert!(is_release(release, recipe.first_release.as_ref()));
        }
    }
}

fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}
