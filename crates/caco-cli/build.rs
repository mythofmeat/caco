fn main() {
    // `--version` embeds a short build identifier. We deliberately do NOT watch
    // `.git/*` with `cargo:rerun-if-changed`: cargo compares those paths by mtime,
    // and git rewrites HEAD/refs mtimes on fetch/pull/gc even when the commit is
    // unchanged. That forced a full (LTO, ~50s) rebuild of this crate on every
    // no-op `git pull` in install.sh — a rebuild for a hash that never changed.
    //
    // Instead the hash is injected via $CACO_GIT_HASH (install.sh and the Arch
    // PKGBUILD export it), and `rerun-if-env-changed` rebuilds only when that value
    // actually changes — never on a spurious mtime bump. Plain local builds show
    // "dev": no git dependency, no spurious rebuilds.
    println!("cargo:rerun-if-env-changed=CACO_GIT_HASH");

    let version = std::env::var("CACO_GIT_HASH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "dev".to_string());

    println!("cargo:rustc-env=CACO_GIT_HASH={version}");
}
