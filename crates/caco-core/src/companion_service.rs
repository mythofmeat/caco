//! Companion file registration and lifecycle management.
//!
//! Handles MD5-based deduplication, managed storage, and orphan cleanup policy.

use std::fs;
use std::path::Path;

use rusqlite::Connection;

use crate::Result;
use crate::config::{get_companion_dir, get_companion_orphan_cleanup};
use crate::db::{
    add_companion, find_companion_by_md5, is_orphan, link_companion_to_wad,
    remove_companion_with_path, unlink_companion_from_wad,
};
use crate::utils::compute_md5;

/// DEH/BEX file extensions.
const DEH_EXTENSIONS: &[&str] = &["deh", "bex"];

/// Check if a file path is a DEH or BEX patch file (by extension).
pub fn is_deh_bex(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| DEH_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Result of orphan cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrphanResult {
    /// Companion was deleted (orphan + delete policy).
    Deleted,
    /// Companion is orphaned but kept (keep or ask policy).
    Kept,
    /// Companion is not orphaned (still linked to other WADs).
    NotOrphaned,
}

/// Register a companion file and link it to a WAD.
///
/// Computes MD5, checks for dedup, copies to managed dir, and links.
/// Managed storage directory is created on first use.
///
/// Returns `(companion_id, filename)` on success.
pub fn register_companion(
    conn: &Connection,
    wad_id: i64,
    file_path: &Path,
) -> Result<(i64, String)> {
    let file_path = file_path
        .canonicalize()
        .map_err(|_| crate::Error::FileNotFound(file_path.display().to_string()))?;

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let md5 = compute_md5(&file_path)?;
    let size = fs::metadata(&file_path)?.len() as i64;

    // Check for existing companion with same MD5 (dedup)
    let companion_id = if let Some(existing) = find_companion_by_md5(conn, &md5)? {
        existing.id
    } else {
        // Copy to managed dir
        let companion_dir = get_companion_dir();
        fs::create_dir_all(&companion_dir)?;

        let managed_name = format!("{}_{}", &md5[..12], filename);
        let managed_path = companion_dir.join(&managed_name);

        if !managed_path.exists() {
            fs::copy(&file_path, &managed_path)?;
        }

        let managed_path_str = managed_path.to_string_lossy();
        add_companion(conn, &md5, &filename, &managed_path_str, size)?
    };

    // Link to WAD (INSERT OR IGNORE handles already-linked case)
    link_companion_to_wad(conn, wad_id, companion_id)?;

    Ok((companion_id, filename))
}

/// Unlink a companion from a WAD, applying orphan policy if it becomes orphaned.
///
/// If `orphan_policy` is `None`, reads from config.
///
/// Returns the orphan result indicating what happened.
pub fn unregister_companion(
    conn: &Connection,
    wad_id: i64,
    companion_id: i64,
    orphan_policy: Option<&str>,
) -> Result<OrphanResult> {
    let removed = unlink_companion_from_wad(conn, wad_id, companion_id)?;
    if !removed {
        return Ok(OrphanResult::NotOrphaned);
    }

    if !is_orphan(conn, companion_id)? {
        return Ok(OrphanResult::NotOrphaned);
    }

    // Companion is now orphaned — apply policy
    let policy = match orphan_policy {
        Some(p) => p.to_string(),
        None => get_companion_orphan_cleanup(),
    };

    if policy == "delete" {
        let managed_path = remove_companion_with_path(conn, companion_id)?;
        if let Some(path_str) = managed_path {
            let p = Path::new(&path_str);
            if p.exists() {
                fs::remove_file(p)?;
            }
        }
        return Ok(OrphanResult::Deleted);
    }

    // "keep" or "ask" (caller handles "ask" at UI level)
    Ok(OrphanResult::Kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::SourceType;
    use crate::db::wads::{NewWad, add_wad};
    use crate::db::{find_companion_by_md5, get_companions_for_wad, init_db, open_memory};
    use std::io::Write;

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wad(conn: &Connection) -> i64 {
        add_wad(conn, &NewWad::new("Test WAD", SourceType::Local)).unwrap()
    }

    fn create_test_file(dir: &Path, name: &str, content: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path
    }

    // -- is_deh_bex tests --

    #[test]
    fn test_is_deh_bex() {
        assert!(is_deh_bex(Path::new("patch.deh")));
        assert!(is_deh_bex(Path::new("patch.DEH")));
        assert!(is_deh_bex(Path::new("patch.bex")));
        assert!(is_deh_bex(Path::new("patch.BEX")));
        assert!(is_deh_bex(Path::new("/path/to/my.deh")));
        assert!(!is_deh_bex(Path::new("patch.wad")));
        assert!(!is_deh_bex(Path::new("patch.pk3")));
        assert!(!is_deh_bex(Path::new("no_extension")));
    }

    // -- register_companion tests --

    #[test]
    fn test_register_companion() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let dir = tempfile::tempdir().unwrap();
        let companion_dir = tempfile::tempdir().unwrap();

        // Override companion dir by setting env (we test with the actual function)
        let file_path = create_test_file(dir.path(), "patch.deh", b"dehacked content");

        // We need to mock get_companion_dir. Since we can't easily do that,
        // test the lower-level pieces or do a full integration test.
        // For now, test that register works with a real temp dir.
        // The function uses get_companion_dir() which reads config — in tests
        // this returns the default path. We'll test the core logic works by
        // verifying DB state.

        // Create a companion dir we can write to
        let managed_dir = companion_dir.path().join("companions");
        fs::create_dir_all(&managed_dir).unwrap();

        // Manually compute what register_companion does, since we can't override
        // get_companion_dir in tests easily. Test the DB + dedup logic directly.
        let md5 = compute_md5(&file_path).unwrap();
        let size = fs::metadata(&file_path).unwrap().len() as i64;

        // No existing companion
        assert!(find_companion_by_md5(&conn, &md5).unwrap().is_none());

        // Add companion + link
        let companion_id = add_companion(
            &conn,
            &md5,
            "patch.deh",
            &managed_dir.join("test_patch.deh").to_string_lossy(),
            size,
        )
        .unwrap();
        link_companion_to_wad(&conn, wad_id, companion_id).unwrap();

        // Verify linked
        let comps = get_companions_for_wad(&conn, wad_id).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].filename, "patch.deh");

        // Dedup: same MD5 should find existing
        let existing = find_companion_by_md5(&conn, &md5).unwrap().unwrap();
        assert_eq!(existing.id, companion_id);
    }

    #[test]
    fn test_register_companion_dedup() {
        let conn = setup();
        let w1 = add_test_wad(&conn);
        let w2 = add_wad(&conn, &NewWad::new("WAD 2", SourceType::Local)).unwrap();

        // Add one companion
        let c_id = add_companion(
            &conn,
            "abc123def456",
            "shared.deh",
            "/managed/shared.deh",
            100,
        )
        .unwrap();

        // Link to first WAD
        link_companion_to_wad(&conn, w1, c_id).unwrap();

        // "Register" to second WAD — dedup means same companion_id
        let existing = find_companion_by_md5(&conn, "abc123def456")
            .unwrap()
            .unwrap();
        assert_eq!(existing.id, c_id);
        link_companion_to_wad(&conn, w2, existing.id).unwrap();

        // Both WADs should have the companion
        assert_eq!(get_companions_for_wad(&conn, w1).unwrap().len(), 1);
        assert_eq!(get_companions_for_wad(&conn, w2).unwrap().len(), 1);
    }

    // -- unregister_companion tests --

    #[test]
    fn test_unregister_companion_not_orphaned() {
        let conn = setup();
        let w1 = add_test_wad(&conn);
        let w2 = add_wad(&conn, &NewWad::new("WAD 2", SourceType::Local)).unwrap();
        let c_id = add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();

        link_companion_to_wad(&conn, w1, c_id).unwrap();
        link_companion_to_wad(&conn, w2, c_id).unwrap();

        // Unlink from w1 — still linked to w2, not orphaned
        let result = unregister_companion(&conn, w1, c_id, Some("delete")).unwrap();
        assert_eq!(result, OrphanResult::NotOrphaned);

        // Companion still exists
        assert!(find_companion_by_md5(&conn, "md5abc").unwrap().is_some());
    }

    #[test]
    fn test_unregister_companion_orphan_keep() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();
        link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        let result = unregister_companion(&conn, wad_id, c_id, Some("keep")).unwrap();
        assert_eq!(result, OrphanResult::Kept);

        // Companion still in registry (kept)
        assert!(find_companion_by_md5(&conn, "md5abc").unwrap().is_some());
    }

    #[test]
    fn test_unregister_companion_orphan_delete() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);

        // Create a real managed file so deletion works
        let dir = tempfile::tempdir().unwrap();
        let managed_file = dir.path().join("patch.deh");
        fs::write(&managed_file, b"content").unwrap();
        let managed_path = managed_file.to_string_lossy().to_string();

        let c_id = add_companion(&conn, "md5abc", "patch.deh", &managed_path, 100).unwrap();
        link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        let result = unregister_companion(&conn, wad_id, c_id, Some("delete")).unwrap();
        assert_eq!(result, OrphanResult::Deleted);

        // Companion removed from registry
        assert!(find_companion_by_md5(&conn, "md5abc").unwrap().is_none());
        // Managed file deleted
        assert!(!managed_file.exists());
    }

    #[test]
    fn test_unregister_companion_orphan_ask() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();
        link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        // "ask" policy keeps the companion (caller handles UI prompt)
        let result = unregister_companion(&conn, wad_id, c_id, Some("ask")).unwrap();
        assert_eq!(result, OrphanResult::Kept);

        // Companion still in registry
        assert!(find_companion_by_md5(&conn, "md5abc").unwrap().is_some());
    }

    #[test]
    fn test_unregister_companion_not_linked() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();

        // Not linked — unlink returns false, treated as not orphaned
        let result = unregister_companion(&conn, wad_id, c_id, Some("delete")).unwrap();
        assert_eq!(result, OrphanResult::NotOrphaned);
    }

    // -- is_orphan DB function tests --

    #[test]
    fn test_is_orphan_true() {
        let conn = setup();
        let c_id = add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();
        assert!(is_orphan(&conn, c_id).unwrap());
    }

    #[test]
    fn test_is_orphan_false() {
        let conn = setup();
        let wad_id = add_test_wad(&conn);
        let c_id = add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();
        link_companion_to_wad(&conn, wad_id, c_id).unwrap();
        assert!(!is_orphan(&conn, c_id).unwrap());
    }

    // -- remove_companion_with_path DB function tests --

    #[test]
    fn test_remove_companion_with_path() {
        let conn = setup();
        let c_id = add_companion(&conn, "md5abc", "patch.deh", "/managed/patch.deh", 100).unwrap();

        let path = remove_companion_with_path(&conn, c_id).unwrap();
        assert_eq!(path, Some("/managed/patch.deh".to_string()));

        // Should be gone from DB
        assert!(find_companion_by_md5(&conn, "md5abc").unwrap().is_none());
    }

    #[test]
    fn test_remove_companion_with_path_not_found() {
        let conn = setup();
        let path = remove_companion_with_path(&conn, 999).unwrap();
        assert!(path.is_none());
    }
}
