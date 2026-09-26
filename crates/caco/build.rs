//! Build-time derivations: the version string and the window icon.
//!
//! **Version.** `Cargo.toml` carries a permanent `version = "0.0.0"` placeholder, so
//! `CARGO_PKG_VERSION` says nothing useful. The real version is the git tag:
//! `git describe --tags` resolves to `4.0.5` on a tagged commit and
//! `4.0.5-9-g2243dac` nine commits later, which is exactly the distinction
//! worth seeing in a bug report.
//!
//! Emitted as `CACO_VERSION`. Three fallbacks, in order: an explicit
//! `CACO_VERSION` in the environment (for a packager building outside a
//! checkout), whatever `git describe` produces, then `CARGO_PKG_VERSION` when
//! there is no git, no repository, or no tag reachable from HEAD.
//!
//! **Icon.** `assets/caco.svg` is the only icon in the repository. Every
//! window API underneath eframe takes raw RGBA, not SVG, so it is rasterised
//! here and embedded as pixels — a checked-in PNG beside the SVG would be a
//! second copy free to drift from the first. The SVG itself is what a Linux
//! desktop install uses, next to `assets/caco.desktop`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Edge length of the embedded window icon. Large enough for a HiDPI taskbar;
/// the window system downsamples for anything smaller.
const ICON_SIZE: u32 = 256;

fn main() {
    rasterize_icon();
    version();
}

fn version() {
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

/// Render `assets/caco.svg` to `$OUT_DIR/icon.rgba`: `ICON_SIZE`² straight
/// (not premultiplied) RGBA, the layout `egui::IconData` expects. Panics on a
/// bad SVG — a broken icon should fail the build, not ship blank.
fn rasterize_icon() {
    let svg = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/caco.svg");
    println!("cargo:rerun-if-changed={}", svg.display());

    let data = std::fs::read(&svg).unwrap_or_else(|e| panic!("reading {}: {e}", svg.display()));
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default())
        .unwrap_or_else(|e| panic!("parsing {}: {e}", svg.display()));

    let mut pixmap = resvg::tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE).expect("non-zero size");
    let size = tree.size();
    let transform = resvg::tiny_skia::Transform::from_scale(
        ICON_SIZE as f32 / size.width(),
        ICON_SIZE as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("icon.rgba"), rgba).expect("writing icon.rgba");
}
