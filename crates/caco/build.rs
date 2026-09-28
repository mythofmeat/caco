//! Rasterise `assets/caco.svg` into the window icon.
//!
//! `main.rs` embeds the result as square straight (non-premultiplied) RGBA and
//! derives the side length from its byte count, so the output must stay square.

use std::path::PathBuf;

use resvg::{tiny_skia, usvg};

/// Largest size a window manager asks for; smaller ones are downscaled by it.
const SIDE: u32 = 256;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let svg_path = manifest.join("../../assets/caco.svg");
    println!("cargo::rerun-if-changed={}", svg_path.display());

    let svg = std::fs::read(&svg_path).expect("read assets/caco.svg");
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).expect("parse caco.svg");

    let size = tree.size();
    let transform =
        tiny_skia::Transform::from_scale(SIDE as f32 / size.width(), SIDE as f32 / size.height());
    let mut pixmap = tiny_skia::Pixmap::new(SIDE, SIDE).expect("non-zero pixmap");
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // tiny-skia stores premultiplied alpha; winit expects straight RGBA.
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("icon.rgba");
    std::fs::write(out, rgba).expect("write icon.rgba");
}
