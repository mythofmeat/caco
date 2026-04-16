//! Sandbox reset implementation.

use std::path::Path;

use fs_extra::dir::{self, CopyOptions};

use crate::error::{CacoMcpError, Result};
use crate::sandbox::SandboxPaths;

#[derive(Debug, Default)]
pub struct ResetOptions {
    pub skip_wads: bool,
}

/// Wipe `sandbox` and deep-copy `source_home` (and the user config file) into it.
///
/// Entries under `source_home` are copied individually so we can skip `wads/`
/// when requested. The user config file is copied to `<sandbox>/config/config.toml`.
pub fn reset_sandbox(paths: &SandboxPaths, opts: &ResetOptions) -> Result<()> {
    // Sanity: refuse if somehow the safety guard was bypassed.
    crate::sandbox::validate_sandbox_path(&paths.sandbox)?;

    if !paths.source_home.is_dir() {
        return Err(CacoMcpError::SourceHomeMissing {
            path: paths.source_home.clone(),
        });
    }

    // Wipe sandbox if it exists.
    if paths.sandbox.exists() {
        std::fs::remove_dir_all(&paths.sandbox)?;
    }
    std::fs::create_dir_all(&paths.sandbox)?;

    let entries = std::fs::read_dir(&paths.source_home)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip thumbnails: they're in ~/.cache, not the source_home, so they
        // wouldn't be here anyway, but be defensive.
        if name_str == "thumbnails" {
            continue;
        }
        if opts.skip_wads && name_str == "wads" {
            continue;
        }

        let src = entry.path();
        let dst_parent = paths.sandbox.clone();
        copy_entry(&src, &dst_parent)?;
    }

    // Copy user config to <sandbox>/config/config.toml, sanitized so that
    // absolute path fields (db_path, cache_dir, data_dir, etc.) don't override
    // the CACO_HOME-derived sandbox defaults at runtime.
    let config_src = dirs::config_dir().map(|d| d.join("caco").join("config.toml"));
    let config_dst = paths.config_path();
    if let Some(src) = config_src
        && src.is_file()
    {
        std::fs::create_dir_all(config_dst.parent().unwrap())?;
        let raw = std::fs::read_to_string(&src)?;
        let sanitized = sanitize_sandbox_config(&raw);
        std::fs::write(&config_dst, sanitized)?;
    }

    Ok(())
}

/// Strip path fields from a config TOML so the sandbox falls back to the
/// `CACO_HOME`-derived defaults. Without this, an absolute `db_path` (or any
/// of the other absolute path keys) in the user's real config would point the
/// sandboxed CLI back at the user's real library.
///
/// If parsing fails, return a minimal valid config so the sandbox is still
/// usable.
fn sanitize_sandbox_config(raw: &str) -> String {
    const PATH_KEYS: &[&str] = &[
        "db_path",
        "cache_dir",
        "data_dir",
        "iwad_dir",
        "sourceport_dir",
        "iwad_dirs",
    ];

    match raw.parse::<toml::Table>() {
        Ok(mut table) => {
            for key in PATH_KEYS {
                table.remove(*key);
            }
            toml::to_string_pretty(&table).unwrap_or_default()
        }
        Err(_) => String::new(),
    }
}

fn copy_entry(src: &Path, dst_parent: &Path) -> Result<()> {
    if src.is_dir() {
        let opts = CopyOptions::new().overwrite(true).copy_inside(false);
        dir::copy(src, dst_parent, &opts)?;
    } else if src.is_file() {
        let dst = dst_parent.join(src.file_name().unwrap());
        std::fs::copy(src, &dst)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn seed_source(root: &Path) {
        std::fs::create_dir_all(root.join("wads")).unwrap();
        std::fs::write(root.join("library.db"), b"fake-db-bytes").unwrap();
        std::fs::write(root.join("wads/doom2.wad"), b"wad-bytes").unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/stats.txt"), b"stats").unwrap();
    }

    #[test]
    fn copies_db_and_directories() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        seed_source(src.path());
        // Ensure the sandbox path is safe (point forbidden paths elsewhere)
        temp_env::with_vars(
            [
                ("XDG_DATA_HOME", Some("/nonexistent/xdg")),
                ("CACO_HOME", Some("/nonexistent")),
            ],
            || {
                let paths =
                    SandboxPaths::new(dst.path().join("sb"), src.path().to_path_buf()).unwrap();
                reset_sandbox(&paths, &ResetOptions::default()).unwrap();
                assert!(paths.db_path().is_file());
                assert!(paths.sandbox.join("wads/doom2.wad").is_file());
                assert!(paths.sandbox.join("data/stats.txt").is_file());
            },
        );
    }

    #[test]
    fn skip_wads_omits_wads_dir() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        seed_source(src.path());
        temp_env::with_vars(
            [
                ("XDG_DATA_HOME", Some("/nonexistent/xdg")),
                ("CACO_HOME", Some("/nonexistent")),
            ],
            || {
                let paths =
                    SandboxPaths::new(dst.path().join("sb"), src.path().to_path_buf()).unwrap();
                reset_sandbox(&paths, &ResetOptions { skip_wads: true }).unwrap();
                assert!(paths.db_path().is_file());
                assert!(!paths.sandbox.join("wads").exists());
            },
        );
    }

    #[test]
    fn wipes_existing_sandbox() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        seed_source(src.path());
        let sb = dst.path().join("sb");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join("stale.txt"), b"stale").unwrap();
        temp_env::with_vars(
            [
                ("XDG_DATA_HOME", Some("/nonexistent/xdg")),
                ("CACO_HOME", Some("/nonexistent")),
            ],
            || {
                let paths = SandboxPaths::new(sb.clone(), src.path().to_path_buf()).unwrap();
                reset_sandbox(&paths, &ResetOptions::default()).unwrap();
                assert!(!sb.join("stale.txt").exists(), "stale file should be wiped");
                assert!(sb.join("library.db").is_file());
            },
        );
    }

    #[test]
    fn sanitize_strips_absolute_path_fields() {
        let input = r#"
sourceport = "dsda-doom"
db_path = "/home/eshen/.local/share/caco/library.db"
cache_dir = "/home/eshen/.local/share/caco/wads"
data_dir = "/home/eshen/.local/share/caco/data"
iwad_dir = "/home/eshen/.local/share/caco/iwads"
sourceport_dir = "/home/eshen/.local/share/caco/sourceports"
iwad_dirs = ["/home/eshen/Doom"]
auto_stats = true
"#;
        let out = sanitize_sandbox_config(input);
        let parsed: toml::Table = out.parse().unwrap();
        assert!(!parsed.contains_key("db_path"));
        assert!(!parsed.contains_key("cache_dir"));
        assert!(!parsed.contains_key("data_dir"));
        assert!(!parsed.contains_key("iwad_dir"));
        assert!(!parsed.contains_key("sourceport_dir"));
        assert!(!parsed.contains_key("iwad_dirs"));
        // Non-path keys should survive.
        assert_eq!(
            parsed.get("sourceport").and_then(|v| v.as_str()),
            Some("dsda-doom")
        );
        assert_eq!(
            parsed.get("auto_stats").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn sanitize_returns_empty_for_invalid_toml() {
        assert_eq!(sanitize_sandbox_config("[[[not toml"), "");
    }

    #[test]
    fn copies_and_sanitizes_user_config() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        let fake_config_home = TempDir::new().unwrap();
        seed_source(src.path());

        // Seed a fake user config with absolute paths pointing into the real
        // user data dir. After reset, the sandbox copy must NOT contain those
        // path keys.
        let user_config_dir = fake_config_home.path().join("caco");
        std::fs::create_dir_all(&user_config_dir).unwrap();
        std::fs::write(
            user_config_dir.join("config.toml"),
            "sourceport = \"dsda-doom\"\n\
             db_path = \"/home/eshen/.local/share/caco/library.db\"\n\
             cache_dir = \"/home/eshen/.local/share/caco/wads\"\n",
        )
        .unwrap();

        temp_env::with_vars(
            [
                (
                    "XDG_CONFIG_HOME",
                    Some(fake_config_home.path().to_str().unwrap()),
                ),
                ("XDG_DATA_HOME", Some("/nonexistent/xdg")),
                ("CACO_HOME", Some("/nonexistent")),
            ],
            || {
                let paths =
                    SandboxPaths::new(dst.path().join("sb"), src.path().to_path_buf()).unwrap();
                reset_sandbox(&paths, &ResetOptions::default()).unwrap();

                let copied = std::fs::read_to_string(paths.config_path()).unwrap();
                let parsed: toml::Table = copied.parse().unwrap();
                assert!(
                    !parsed.contains_key("db_path"),
                    "db_path must be stripped from sandbox config; got: {copied}"
                );
                assert!(!parsed.contains_key("cache_dir"));
                assert_eq!(
                    parsed.get("sourceport").and_then(|v| v.as_str()),
                    Some("dsda-doom")
                );
            },
        );
    }

    #[test]
    fn errors_when_source_home_missing() {
        let dst = TempDir::new().unwrap();
        temp_env::with_vars(
            [
                ("XDG_DATA_HOME", Some("/nonexistent/xdg")),
                ("CACO_HOME", Some("/nonexistent")),
            ],
            || {
                let paths =
                    SandboxPaths::new(dst.path().join("sb"), PathBuf::from("/nonexistent/source"))
                        .unwrap();
                let err = reset_sandbox(&paths, &ResetOptions::default()).unwrap_err();
                assert!(matches!(err, CacoMcpError::SourceHomeMissing { .. }));
            },
        );
    }
}
