//! Interactive WAD selection via fzf or numbered menu.

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use caco_core::db::WadRecord;

/// Check if fzf is available on PATH.
fn fzf_available() -> bool {
    Command::new("fzf")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Select WADs interactively using fzf.
fn fzf_select(wads: &[WadRecord], multi: bool) -> Vec<usize> {
    let mut cmd = Command::new("fzf");
    cmd.arg("--ansi")
        .arg("--no-sort")
        .arg("--prompt")
        .arg("Select WAD> ")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    if multi {
        cmd.arg("--multi");
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Write WAD lines to fzf's stdin
    if let Some(mut stdin) = child.stdin.take() {
        for (i, wad) in wads.iter().enumerate() {
            let author = wad.author.as_deref().unwrap_or("");
            let line = format!("{}: {} - {} [{}]\n", i + 1, wad.title, author, wad.status);
            let _ = stdin.write_all(line.as_bytes());
        }
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            // Parse the index from the "N: Title..." format
            line.split(':').next()?.trim().parse::<usize>().ok().map(|n| n - 1)
        })
        .filter(|&idx| idx < wads.len())
        .collect()
}

/// Select a WAD via numbered menu (fallback when fzf is not available).
fn numbered_select(wads: &[WadRecord]) -> Option<usize> {
    for (i, wad) in wads.iter().enumerate() {
        let author = wad.author.as_deref().unwrap_or("");
        eprintln!("  {}: {} - {} [{}]", i + 1, wad.title, author, wad.status);
    }
    eprint!("Select (1-{}): ", wads.len());
    let _ = io::stderr().flush();

    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_err() {
        return None;
    }

    let choice: usize = line.trim().parse().ok()?;
    if choice >= 1 && choice <= wads.len() {
        Some(choice - 1)
    } else {
        None
    }
}

/// Pick one or more WADs interactively.
///
/// Uses fzf if available, falls back to numbered menu.
/// Returns indices into the `wads` slice.
pub fn pick_wads(wads: &[WadRecord], multi: bool) -> Vec<usize> {
    if wads.is_empty() {
        return Vec::new();
    }

    if fzf_available() {
        fzf_select(wads, multi)
    } else if multi {
        // Without fzf, multi-select is not supported
        eprintln!("Multi-select requires fzf. Install fzf for multi-select support.");
        Vec::new()
    } else {
        numbered_select(wads).into_iter().collect()
    }
}
