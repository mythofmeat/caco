use std::process::Command;

fn main() {
    // Embed git hash into the binary for --version output
    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let dirty = Command::new("git")
        .args(["diff", "--quiet", "HEAD"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false);

    let version = if dirty {
        format!("{hash}-dirty")
    } else {
        hash
    };

    println!("cargo:rustc-env=CACO_GIT_HASH={version}");
    // Rebuild when git HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
}
