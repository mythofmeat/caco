use std::process::Command;

fn main() {
    // Only embed the git hash in release builds. In dev, watching .git/ would force
    // a full recompile of this crate on every git operation (commit, branch switch,
    // `git pull` touching refs/), so dev builds get a constant placeholder instead.
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        println!("cargo:rustc-env=CACO_GIT_HASH=dev");
        return;
    }

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

    let version = if dirty { format!("{hash}-dirty") } else { hash };

    println!("cargo:rustc-env=CACO_GIT_HASH={version}");
    // Rebuild when git HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
}
