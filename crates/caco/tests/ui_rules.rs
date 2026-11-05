//! The rules in `docs/ui.md` that a machine can check by reading the source.
//!
//! Crude on purpose. A rule that needs a parser would not get written, and each of these
//! has a clear way to be wrong: a line in the wrong file. No GPU, so `cargo test` runs them.

use std::path::{Path, PathBuf};

fn source_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read source dir") {
        let path = entry.expect("read source entry").path();
        if path.is_dir() {
            source_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// `(path relative to src, line number, line)` for every line of code containing one of
/// `needles`, outside `allowed` and outside comments.
fn lines_containing(needles: &[&str], allowed: &[&str]) -> Vec<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);

    let mut hits = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&src)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        if allowed.contains(&relative.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("read source file");
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if needles.iter().any(|needle| code.contains(needle)) {
                hits.push(format!("{relative}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    hits
}

/// Colour tokens live in one file, or the status palette has a second copy within a month:
/// that is how the Cacowards view came to have its own unplayed blue.
#[test]
fn every_colour_is_a_token_in_the_theme() {
    let hits: Vec<String> = lines_containing(&["Color32::"], &["theme.rs"])
        .into_iter()
        .filter(|line| !line.contains("Color32::TRANSPARENT"))
        .collect();
    assert_eq!(hits, Vec::<String>::new(), "name these in theme.rs");
}

/// A window opened anywhere else has its own idea of where the buttons go.
#[test]
fn only_the_dialog_skeleton_opens_a_window() {
    let hits = lines_containing(
        &["Window::new", ".title_bar(", "Modal::new"],
        &["dialogs/skeleton.rs"],
    );
    assert_eq!(hits, Vec::<String>::new(), "use dialogs::Dialog");
}
