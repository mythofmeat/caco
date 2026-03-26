use std::fs;
use std::path::Path;

use rusqlite::Connection;

use crate::config::{get_id24_dir, get_iwad_dir};
use crate::db::{
    add_id24, add_iwad, get_id24, get_iwad_variant, identify_id24, identify_iwad,
    managed_iwad_filename,
};
use crate::utils::compute_md5;
use crate::Result;

/// Identify and register an IWAD file.
///
/// Identifies the file by MD5/filename, copies to the managed IWAD
/// directory, and adds to the DB.
///
/// Returns `(family, variant, title)` on success, or `None` if the file
/// is not a recognized IWAD or is already registered.
pub fn register_iwad(
    conn: &Connection,
    path: &Path,
) -> Result<Option<(String, String, String)>> {
    let info = identify_iwad(path)?;
    let (family, variant, title) = match info {
        Some(i) => i,
        None => return Ok(None),
    };

    // Check if already registered
    if get_iwad_variant(conn, family, variant)?.is_some() {
        return Ok(None);
    }

    // Copy to managed directory
    let iwad_directory = get_iwad_dir();
    let managed_rel = managed_iwad_filename(family, variant);
    let dest = iwad_directory.join(&managed_rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(path, &dest)?;

    // Register in DB
    let md5 = compute_md5(path)?;
    let dest_str = dest.to_string_lossy();
    add_iwad(conn, family, variant, &dest_str, Some(title), Some(&md5))?;

    Ok(Some((
        family.to_string(),
        variant.to_string(),
        title.to_string(),
    )))
}

/// Identify and register an id24 WAD file.
///
/// Identifies the file by MD5/filename, copies to the managed id24
/// directory, and adds to the DB.
///
/// Returns `(name, version, title)` on success, or `None` if the file
/// is not a recognized id24 WAD or is already registered.
pub fn register_id24(
    conn: &Connection,
    path: &Path,
) -> Result<Option<(String, String, String)>> {
    let info = identify_id24(path)?;
    let (name, version, title) = match info {
        Some(i) => i,
        None => return Ok(None),
    };

    // Check if already registered
    if get_id24(conn, name)?.is_some() {
        return Ok(None);
    }

    // Copy to managed directory
    let id24_directory = get_id24_dir();
    fs::create_dir_all(&id24_directory)?;
    let dest = id24_directory.join(format!("{name}.wad"));
    fs::copy(path, &dest)?;

    // Register in DB
    let md5 = compute_md5(path)?;
    let dest_str = dest.to_string_lossy();
    add_id24(conn, name, &dest_str, Some(version), Some(title), Some(&md5))?;

    Ok(Some((
        name.to_string(),
        version.to_string(),
        title.to_string(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{get_all_id24, get_all_iwads, open_memory, init_db};
    use std::io::Write;

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_register_iwad_unrecognized() {
        let conn = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("random.wad");
        // Write junk data that won't match any known IWAD
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"not a real wad file").unwrap();

        let result = register_iwad(&conn, &path).unwrap();
        assert!(result.is_none());
        assert!(get_all_iwads(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_register_id24_unrecognized() {
        let conn = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("random.wad");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"not a real wad file").unwrap();

        let result = register_id24(&conn, &path).unwrap();
        assert!(result.is_none());
        assert!(get_all_id24(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_register_iwad_nonexistent_file() {
        let conn = setup();
        let result = register_iwad(&conn, Path::new("/nonexistent/doom2.wad")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_register_id24_nonexistent_file() {
        let conn = setup();
        let result = register_id24(&conn, Path::new("/nonexistent/id1.wad")).unwrap();
        assert!(result.is_none());
    }
}
