//! Derives the version string the app reports at runtime.
//!
//! `Cargo.toml` carries a permanent `version = "0.0.0"` placeholder, so
//! `CARGO_PKG_VERSION` says nothing useful. The real version is the git tag:
//! `git describe --tags` resolves to `4.0.5` on a tagged commit and
//! `4.0.5-9-g2243dac` nine commits later, which is exactly the distinction
//! worth seeing in a bug report.
//!
//! Emitted as `CACO_VERSION`. Three fallbacks, in order: an explicit
//! `CACO_VERSION` in the environment (for a packager building outside a
//! checkout), whatever `git describe` produces, then `CARGO_PKG_VERSION` when
//! there is no git, no repository, or no tag reachable from HEAD.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    // An explicit override wins: a build from a source tarball has no git
    // history to describe, and guessing is worse than being told.
    println!("cargo:rerun-if-env-changed=CACO_VERSION");
    if let Ok(version) = std::env::var("CACO_VERSION")
        && !version.trim().is_empty()
    {
        emit(version.trim());
        return;
    }

    watch_git_refs();

    match describe() {
        Some(version) => emit(&version),
        None => emit(env!("CARGO_PKG_VERSION")),
    }
}

fn emit(version: &str) {
    println!("cargo:rustc-env=CACO_VERSION={version}");
}

/// `git describe --tags --dirty`, with the conventional leading `v` stripped so
/// call sites can format the prefix themselves.
fn describe() -> Option<String> {
    let out = git(&["describe", "--tags", "--dirty", "--always"])?;
    let described = out.trim();
    if described.is_empty() {
        return None;
    }
    Some(described.strip_prefix('v').unwrap_or(described).to_string())
}

/// Tell cargo to re-run when HEAD moves or a tag is written. Without this the
/// version is baked in at the first compile and a later `git tag` is invisible
/// until something else forces a rebuild.
fn watch_git_refs() {
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let git_dir = PathBuf::from(git_dir.trim());
    for path in ["HEAD", "packed-refs", "refs/tags"] {
        let path = git_dir.join(path);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// Run git in the crate's own directory. `None` for any failure at all —
/// git missing, not a repository, non-zero exit — since every one of them
/// means the same thing here: no version to read.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
