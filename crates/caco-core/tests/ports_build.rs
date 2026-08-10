//! End-to-end build of a real sourceport.
//!
//! `#[ignore]`d: it clones from the network and compiles a C project, so it
//! has no business running in `cargo test --workspace`. Run it deliberately
//! after touching the build driver:
//!
//! ```bash
//! cargo test -p caco-core --test ports_build -- --ignored --nocapture
//! ```
//!
//! Every path it uses comes from a tempdir. It must never call
//! `PortPaths::from_config()`.

use std::sync::atomic::{AtomicBool, Ordering};

use caco_core::ports::{BuildOptions, BuildProgress, PortPaths, build, recipe};

fn sandbox(root: &std::path::Path) -> PortPaths {
    PortPaths {
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

#[test]
#[ignore = "clones a sourceport"]
fn cancel_stops_a_build_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sandbox(dir.path());
    let recipe = recipe::find_recipe(&paths.recipe_dir, "nyan-doom").unwrap();

    let flag = AtomicBool::new(false);
    let err = build::install(
        &recipe,
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
    assert!(caco_core::ports::find_installed(&paths.prefix_root, "nyan-doom").is_none());
}

/// Print what this machine is missing for every built-in recipe.
///
/// Not an assertion — a machine with everything installed and a machine with
/// nothing are both valid. It exists so `doctor` can be eyeballed against a
/// real package manager, which is the only way to know the `pacman -T` and
/// `brew list` parsing is right.
#[test]
#[ignore = "reports on the host machine"]
fn doctor_report_for_this_machine() {
    for recipe in recipe::builtin_recipes() {
        let report = caco_core::ports::doctor(&recipe);
        eprintln!("== {}", report.port);
        eprintln!("   can build:        {}", report.can_build());
        eprintln!("   missing tools:    {:?}", report.missing_tools);
        eprintln!("   package check:    {:?}", report.package_check);
        eprintln!("   missing packages: {:?}", report.missing_packages);
        eprintln!("   hint:             {:?}", report.install_hint());
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
