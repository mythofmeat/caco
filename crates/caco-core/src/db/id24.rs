use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use rusqlite::Connection;

use crate::utils::compute_md5;
use crate::Result;

// =============================================================================
// Known id24 MD5 checksums -> (name, version, title)
// =============================================================================

pub static KNOWN_ID24_WADS: LazyLock<HashMap<&'static str, (&'static str, &'static str, &'static str)>> =
    LazyLock::new(|| {
        HashMap::from([
            // id1.wad — Legacy of Rust
            ("713c5a3c1734b1d55b2813a3dd0136d9", ("id1", "update2", "Legacy of Rust")),
            ("681bcea18c1286e8b9986c335034bdd1", ("id1", "initial", "Legacy of Rust")),
            // id24res.wad — id24 resource WAD
            ("4f0651accebc007b853943ac12aa95b8", ("id24res", "all", "id24 Resource WAD")),
            // id1-res.wad — Legacy of Rust resources
            ("f8fbab472230bfa090d6a9234d65fae6", ("id1-res", "update2", "Legacy of Rust Resources")),
            ("b6b2370ae8733aaf1377b0ef12351572", ("id1-res", "initial", "Legacy of Rust Resources")),
            // id1-tex.wad — Legacy of Rust textures
            ("187bfe543f8328b379e46957976e800d", ("id1-tex", "update2", "Legacy of Rust Textures")),
            // id1-weap.wad — Legacy of Rust weapons
            ("85d25c8c3d06a05a1283ae4afe749c9f", ("id1-weap", "update2", "Legacy of Rust Weapons")),
            ("b50da800b17db51fa06b5191becad82d", ("id1-weap", "initial", "Legacy of Rust Weapons")),
            // id1-mus.wad — Legacy of Rust music
            ("436c83dd83a47f8dd251ba15108e9459", ("id1-mus", "update2", "Legacy of Rust Music")),
            // iddm1.wad — id Deathmatch 1
            ("5670fd8fe8eb6910ec28f9e27969d84f", ("iddm1", "initial", "id Deathmatch 1")),
        ])
    });

// =============================================================================
// Filename fallback for unrecognized MD5s
// =============================================================================

pub static KNOWN_ID24_FILENAMES: LazyLock<HashMap<&'static str, (&'static str, &'static str, &'static str)>> =
    LazyLock::new(|| {
        HashMap::from([
            ("id1.wad", ("id1", "unknown", "Legacy of Rust")),
            ("id24res.wad", ("id24res", "unknown", "id24 Resource WAD")),
            ("id1-res.wad", ("id1-res", "unknown", "Legacy of Rust Resources")),
            ("id1-tex.wad", ("id1-tex", "unknown", "Legacy of Rust Textures")),
            ("id1-weap.wad", ("id1-weap", "unknown", "Legacy of Rust Weapons")),
            ("id1-mus.wad", ("id1-mus", "unknown", "Legacy of Rust Music")),
            ("iddm1.wad", ("iddm1", "unknown", "id Deathmatch 1")),
        ])
    });

// =============================================================================
// Identification helpers
// =============================================================================

/// Identify an id24 WAD file by MD5 hash, falling back to filename.
///
/// Returns `(name, version, display_title)` or `None` if unrecognized.
pub fn identify_id24(path: &Path) -> Result<Option<(&'static str, &'static str, &'static str)>> {
    if !path.exists() {
        return Ok(None);
    }

    let md5 = compute_md5(path)?;
    if let Some(&info) = KNOWN_ID24_WADS.get(md5.as_str()) {
        return Ok(Some(info));
    }

    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
        let lower = filename.to_lowercase();
        if let Some(&info) = KNOWN_ID24_FILENAMES.get(lower.as_str()) {
            return Ok(Some(info));
        }
    }

    Ok(None)
}

// =============================================================================
// id24 record
// =============================================================================

/// An id24 WAD record from the database.
#[derive(Debug, Clone)]
pub struct Id24Record {
    pub id: i64,
    pub name: String,
    pub version: Option<String>,
    pub title: Option<String>,
    pub path: String,
    pub md5: Option<String>,
}

impl Id24Record {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            version: row.get("version")?,
            title: row.get("title")?,
            path: row.get("path")?,
            md5: row.get("md5")?,
        })
    }
}

// =============================================================================
// Database CRUD
// =============================================================================

/// Register an id24 WAD in the database. Returns the new ID.
pub fn add_id24(
    conn: &Connection,
    name: &str,
    path: &str,
    version: Option<&str>,
    title: Option<&str>,
    md5: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO id24_wads (name, version, title, path, md5) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![name, version, title, path, md5],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get a registered id24 WAD by name.
pub fn get_id24(conn: &Connection, name: &str) -> Result<Option<Id24Record>> {
    let mut stmt = conn.prepare("SELECT * FROM id24_wads WHERE name = ?")?;
    match stmt.query_row([name], Id24Record::from_row) {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all registered id24 WADs, ordered by name.
pub fn get_all_id24(conn: &Connection) -> Result<Vec<Id24Record>> {
    let mut stmt = conn.prepare("SELECT * FROM id24_wads ORDER BY name")?;
    let rows = stmt
        .query_map([], Id24Record::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get a registered id24 WAD by file path.
pub fn get_id24_by_path(conn: &Connection, path: &str) -> Result<Option<Id24Record>> {
    let mut stmt = conn.prepare("SELECT * FROM id24_wads WHERE path = ?")?;
    match stmt.query_row([path], Id24Record::from_row) {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Remove a registered id24 WAD by name. Returns number of rows removed.
pub fn remove_id24(conn: &Connection, name: &str) -> Result<usize> {
    let count = conn.execute("DELETE FROM id24_wads WHERE name = ?", [name])?;
    Ok(count)
}

/// Remove a registered id24 WAD and return the path of the removed entry.
pub fn remove_id24_with_paths(conn: &Connection, name: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM id24_wads WHERE name = ?")?;
    let paths: Vec<String> = stmt
        .query_map([name], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    conn.execute("DELETE FROM id24_wads WHERE name = ?", [name])?;
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::open_memory;
    use crate::db::schema::init_db;

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_add_and_get_id24() {
        let conn = setup();
        let id = add_id24(
            &conn,
            "id1",
            "/path/id1.wad",
            Some("update2"),
            Some("Legacy of Rust"),
            Some("abc123"),
        )
        .unwrap();
        assert!(id > 0);

        let record = get_id24(&conn, "id1").unwrap().unwrap();
        assert_eq!(record.name, "id1");
        assert_eq!(record.version.as_deref(), Some("update2"));
        assert_eq!(record.title.as_deref(), Some("Legacy of Rust"));
        assert_eq!(record.path, "/path/id1.wad");
    }

    #[test]
    fn test_get_id24_not_found() {
        let conn = setup();
        assert!(get_id24(&conn, "nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_get_all_id24() {
        let conn = setup();
        add_id24(&conn, "id24res", "/id24res.wad", None, None, None).unwrap();
        add_id24(&conn, "id1", "/id1.wad", None, None, None).unwrap();

        let all = get_all_id24(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "id1"); // Ordered by name
        assert_eq!(all[1].name, "id24res");
    }

    #[test]
    fn test_get_id24_by_path() {
        let conn = setup();
        add_id24(&conn, "id1", "/path/id1.wad", None, None, None).unwrap();

        let result = get_id24_by_path(&conn, "/path/id1.wad").unwrap();
        assert!(result.is_some());

        let result = get_id24_by_path(&conn, "/other.wad").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_remove_id24() {
        let conn = setup();
        add_id24(&conn, "id1", "/id1.wad", None, None, None).unwrap();

        let count = remove_id24(&conn, "id1").unwrap();
        assert_eq!(count, 1);
        assert!(get_id24(&conn, "id1").unwrap().is_none());

        // Remove nonexistent
        let count = remove_id24(&conn, "id1").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_remove_id24_with_paths() {
        let conn = setup();
        add_id24(&conn, "id1", "/path/id1.wad", None, None, None).unwrap();

        let paths = remove_id24_with_paths(&conn, "id1").unwrap();
        assert_eq!(paths, vec!["/path/id1.wad"]);
        assert!(get_id24(&conn, "id1").unwrap().is_none());
    }

    #[test]
    fn test_known_id24_wads() {
        let id1 = KNOWN_ID24_WADS.get("713c5a3c1734b1d55b2813a3dd0136d9");
        assert_eq!(id1, Some(&("id1", "update2", "Legacy of Rust")));

        let res = KNOWN_ID24_WADS.get("4f0651accebc007b853943ac12aa95b8");
        assert_eq!(res, Some(&("id24res", "all", "id24 Resource WAD")));
    }

    #[test]
    fn test_known_id24_filenames() {
        let id1 = KNOWN_ID24_FILENAMES.get("id1.wad");
        assert_eq!(id1, Some(&("id1", "unknown", "Legacy of Rust")));
    }

    #[test]
    fn test_duplicate_name_fails() {
        let conn = setup();
        add_id24(&conn, "id1", "/id1a.wad", None, None, None).unwrap();

        let result = add_id24(&conn, "id1", "/id1b.wad", None, None, None);
        assert!(result.is_err()); // UNIQUE constraint on name
    }
}
