use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use rusqlite::Connection;

use crate::utils::compute_md5;
use crate::Result;

// =============================================================================
// Known IWAD MD5 checksums -> (family, variant, display_title)
// =============================================================================

pub static KNOWN_IWADS: LazyLock<HashMap<&'static str, (&'static str, &'static str, &'static str)>> =
    LazyLock::new(|| {
        HashMap::from([
            // doom family
            ("1cd63c5ddff1bf8ce844237f580e9cf3", ("doom", "v1.9", "Doom (Registered)")),
            ("c4fe9fd920207691a9f493668e0a2083", ("doom", "v1.9ud", "The Ultimate Doom")),
            ("fb35c4a5a9fd49ec29ab6e900572c524", ("doom", "bfg", "The Ultimate Doom (BFG Edition)")),
            ("8517c4e8f0eef90b82852667d345eb86", ("doom", "enhanced", "The Ultimate Doom (Enhanced)")),
            ("4461d4511386518e784c647e3128e7bc", ("doom", "kex", "The Ultimate Doom (KEX)")),
            ("3b37188f6337f15718b617c16e6e7a9c", ("doom", "kex", "The Ultimate Doom (KEX)")),
            // doom1 (shareware)
            ("f0cefca49926d00903cf57551d901abe", ("doom1", "v1.0", "Doom (Shareware)")),
            // doom2 family
            ("25e1459ca71d321525f84628f45ca8cd", ("doom2", "v1.9", "Doom II: Hell on Earth")),
            ("c3bea40570c23e511a7ed3ebcd9865f7", ("doom2", "bfg", "Doom II: Hell on Earth (BFG Edition)")),
            ("8ab6d0527a29efdc1ef200e5687b5cae", ("doom2", "enhanced", "Doom II: Hell on Earth (Enhanced)")),
            ("9aa3cbf65b961d0bdac98ec403b832e1", ("doom2", "kex", "Doom II: Hell on Earth (KEX)")),
            ("64a4c88a871da67492aaa2020a068cd8", ("doom2", "kex", "Doom II: Hell on Earth (KEX)")),
            // plutonia family
            ("75c8cf89566741fa9d22447604053bd7", ("plutonia", "v1.9", "The Plutonia Experiment")),
            ("3493be7e1e2588bc9c8b31eab2587a04", ("plutonia", "v1.9alt", "The Plutonia Experiment")),
            ("0b381ff7bae93bde6496f9547463619d", ("plutonia", "unity", "The Plutonia Experiment (Unity)")),
            ("ae76c20366ff685d3bb9fab11b148b84", ("plutonia", "unity", "The Plutonia Experiment (Unity)")),
            ("24037397056e919961005e08611623f4", ("plutonia", "kex", "The Plutonia Experiment (KEX)")),
            ("e47cf6d82a0ccedf8c1c16a284bb5937", ("plutonia", "kex", "The Plutonia Experiment (KEX)")),
            // tnt family
            ("4e158d9953c79ccf97bd0663244cc6b6", ("tnt", "v1.9", "TNT: Evilution")),
            ("1d39e405bf6ee3df69a8d2646c8d5c49", ("tnt", "v1.9alt", "TNT: Evilution")),
            ("a6685de59ddf2c07f45deeec95296d98", ("tnt", "unity", "TNT: Evilution (Unity)")),
            ("f5528f6fd55cf9629141d79eda169630", ("tnt", "unity", "TNT: Evilution (Unity)")),
            ("8974e3117ed4a1839c752d5e11ab1b7b", ("tnt", "kex", "TNT: Evilution (KEX)")),
            ("ad7885c17a6b9b79b09d7a7634dd7e2c", ("tnt", "kex", "TNT: Evilution (KEX)")),
            // other families
            ("66d686b1ed6d35ff103f15dbd30e0341", ("heretic", "v1.3", "Heretic")),
            ("ae779722390ec32fa37b0d361f7d82f8", ("heretic1", "v1.0", "Heretic (Shareware)")),
            ("abb033caf81e26f12a2103e1fa25453f", ("hexen", "v1.1", "Hexen")),
            ("78d5898e99e220e4de64edaa0e479593", ("hexdd", "v1.0", "Hexen: Deathkings")),
            ("2fed2031a5b03892106e0f117f17901f", ("strife", "v1.2", "Strife")),
            ("25485721882b050afa96a56e5758dd52", ("chex", "v1.0", "Chex Quest")),
            ("bce163d06521f9d15f9686786e64df13", ("chex3", "v1.0", "Chex Quest 3")),
        ])
    });

// =============================================================================
// Filename fallback for when MD5 doesn't match
// =============================================================================

pub static KNOWN_IWAD_FILENAMES: LazyLock<HashMap<&'static str, (&'static str, &'static str, &'static str)>> =
    LazyLock::new(|| {
        HashMap::from([
            ("doom2.wad", ("doom2", "unknown", "Doom II: Hell on Earth")),
            ("doom.wad", ("doom", "unknown", "The Ultimate Doom")),
            ("doomu.wad", ("doom", "unknown", "The Ultimate Doom")),
            ("doom1.wad", ("doom1", "unknown", "Doom (Shareware)")),
            ("plutonia.wad", ("plutonia", "unknown", "The Plutonia Experiment")),
            ("tnt.wad", ("tnt", "unknown", "TNT: Evilution")),
            ("heretic.wad", ("heretic", "unknown", "Heretic")),
            ("hexen.wad", ("hexen", "unknown", "Hexen")),
            ("hexdd.wad", ("hexdd", "unknown", "Hexen: Deathkings")),
            ("strife1.wad", ("strife", "unknown", "Strife")),
            ("chex.wad", ("chex", "unknown", "Chex Quest")),
            ("chex3.wad", ("chex3", "unknown", "Chex Quest 3")),
            ("freedoom2.wad", ("freedoom2", "unknown", "Freedoom: Phase 2")),
            ("freedoom1.wad", ("freedoom1", "unknown", "Freedoom: Phase 1")),
            ("hacx.wad", ("hacx", "unknown", "HacX")),
        ])
    });

// =============================================================================
// Alias mapping: free-text IWAD strings -> family names
// =============================================================================

pub static IWAD_ALIASES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("doom ii", "doom2"),
        ("doom 2", "doom2"),
        ("doom2", "doom2"),
        ("doom ii: hell on earth", "doom2"),
        ("hell on earth", "doom2"),
        ("the ultimate doom", "doom"),
        ("ultimate doom", "doom"),
        ("doom", "doom"),
        ("doom 1", "doom"),
        ("doom (shareware)", "doom1"),
        ("doom shareware", "doom1"),
        ("plutonia", "plutonia"),
        ("the plutonia experiment", "plutonia"),
        ("plutonia experiment", "plutonia"),
        ("tnt", "tnt"),
        ("tnt: evilution", "tnt"),
        ("tnt evilution", "tnt"),
        ("evilution", "tnt"),
        ("final doom", "doom2"),
        ("heretic", "heretic"),
        ("heretic (shareware)", "heretic1"),
        ("hexen", "hexen"),
        ("hexen: deathkings", "hexdd"),
        ("hexen deathkings", "hexdd"),
        ("strife", "strife"),
        ("chex quest", "chex"),
        ("chex quest 3", "chex3"),
        ("freedoom", "freedoom2"),
        ("freedoom phase 1", "freedoom1"),
        ("freedoom phase 2", "freedoom2"),
        ("freedoom: phase 1", "freedoom1"),
        ("freedoom: phase 2", "freedoom2"),
        ("hacx", "hacx"),
    ])
});

// =============================================================================
// Variant priority: preferred variant order per family
// =============================================================================

pub static DEFAULT_IWAD_PRIORITY: LazyLock<HashMap<&'static str, Vec<&'static str>>> =
    LazyLock::new(|| {
        HashMap::from([
            ("doom", vec!["v1.9ud", "v1.9", "bfg", "enhanced", "kex"]),
            ("doom1", vec!["v1.0"]),
            ("doom2", vec!["v1.9", "bfg", "enhanced", "kex"]),
            ("plutonia", vec!["v1.9", "v1.9alt", "unity", "kex"]),
            ("tnt", vec!["v1.9", "v1.9alt", "unity", "kex"]),
            ("freedoom1", vec!["latest"]),
            ("freedoom2", vec!["latest"]),
            ("heretic", vec!["v1.3"]),
            ("heretic1", vec!["v1.0"]),
            ("hexen", vec!["v1.1"]),
            ("hexdd", vec!["v1.0"]),
            ("strife", vec!["v1.2"]),
            ("chex", vec!["v1.0"]),
            ("chex3", vec!["v1.0"]),
        ])
    });

// =============================================================================
// Cross-family fallbacks (freedoom as last resort)
// =============================================================================

pub static FAMILY_FALLBACKS: LazyLock<HashMap<&'static str, Vec<&'static str>>> =
    LazyLock::new(|| {
        HashMap::from([
            ("doom", vec!["freedoom1"]),
            ("doom2", vec!["freedoom2"]),
            ("plutonia", vec!["freedoom2"]),
            ("tnt", vec!["freedoom2"]),
        ])
    });

// =============================================================================
// Identification helpers
// =============================================================================

/// Identify an IWAD file by MD5 hash, falling back to filename.
///
/// Returns `(family, variant, display_title)` or `None` if unrecognized.
pub fn identify_iwad(path: &Path) -> Result<Option<(&'static str, &'static str, &'static str)>> {
    if !path.exists() {
        return Ok(None);
    }

    let md5 = compute_md5(path)?;
    if let Some(&info) = KNOWN_IWADS.get(md5.as_str()) {
        return Ok(Some(info));
    }

    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
        let lower = filename.to_lowercase();
        if let Some(&info) = KNOWN_IWAD_FILENAMES.get(lower.as_str()) {
            return Ok(Some(info));
        }
    }

    Ok(None)
}

/// Normalize free text to a known IWAD family name.
pub fn normalize_iwad_name(text: &str) -> Option<&'static str> {
    let key = text.trim().to_lowercase();
    IWAD_ALIASES.get(key.as_str()).copied()
}

// =============================================================================
// Priority resolution
// =============================================================================

/// Get variant priority list for a family.
///
/// Checks the user's `iwad_priority` config first, then falls back to
/// `DEFAULT_IWAD_PRIORITY`.
pub fn get_iwad_priority(
    family: &str,
    user_priority: Option<&HashMap<String, Vec<String>>>,
) -> Vec<String> {
    if let Some(user) = user_priority
        && let Some(variants) = user.get(family)
    {
        return variants.clone();
    }

    DEFAULT_IWAD_PRIORITY
        .get(family)
        .map(|v| v.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

// =============================================================================
// IWAD record
// =============================================================================

/// An IWAD record from the database.
#[derive(Debug, Clone)]
pub struct IwadRecord {
    pub id: i64,
    pub family: String,
    pub variant: String,
    pub path: String,
    pub title: Option<String>,
    pub md5: Option<String>,
}

impl IwadRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            family: row.get("family")?,
            variant: row.get("variant")?,
            path: row.get("path")?,
            title: row.get("title")?,
            md5: row.get("md5")?,
        })
    }
}

// =============================================================================
// Database CRUD
// =============================================================================

/// Register an IWAD variant in the database. Returns the new ID.
pub fn add_iwad(
    conn: &Connection,
    family: &str,
    variant: &str,
    path: &str,
    title: Option<&str>,
    md5: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO iwads (family, variant, path, title, md5) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![family, variant, path, title, md5],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get the preferred variant of an IWAD family.
///
/// Walks the priority list and returns the first registered variant.
/// Falls back to any registered variant, then tries cross-family fallbacks.
pub fn get_iwad(
    conn: &Connection,
    family: &str,
    user_priority: Option<&HashMap<String, Vec<String>>>,
) -> Result<Option<IwadRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM iwads WHERE family = ?")?;
    let rows: Vec<IwadRecord> = stmt
        .query_map([family], IwadRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    if rows.is_empty() {
        // Try cross-family fallback
        if let Some(fallbacks) = FAMILY_FALLBACKS.get(family) {
            for fallback_family in fallbacks {
                if let Some(result) = get_iwad(conn, fallback_family, user_priority)? {
                    return Ok(Some(result));
                }
            }
        }
        return Ok(None);
    }

    let variants: HashMap<&str, &IwadRecord> =
        rows.iter().map(|r| (r.variant.as_str(), r)).collect();

    // Walk priority list
    for v in get_iwad_priority(family, user_priority) {
        if let Some(&record) = variants.get(v.as_str()) {
            return Ok(Some(record.clone()));
        }
    }

    // Fallback: "unknown" variant or first registered
    if let Some(&record) = variants.get("unknown") {
        return Ok(Some(record.clone()));
    }
    Ok(Some(rows.into_iter().next().unwrap()))
}

/// Get a specific IWAD variant.
pub fn get_iwad_variant(
    conn: &Connection,
    family: &str,
    variant: &str,
) -> Result<Option<IwadRecord>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM iwads WHERE family = ? AND variant = ?",
    )?;
    match stmt.query_row(rusqlite::params![family, variant], IwadRecord::from_row) {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all variants of an IWAD family, sorted by priority.
pub fn get_family_iwads(
    conn: &Connection,
    family: &str,
    user_priority: Option<&HashMap<String, Vec<String>>>,
) -> Result<Vec<IwadRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM iwads WHERE family = ?")?;
    let mut rows: Vec<IwadRecord> = stmt
        .query_map([family], IwadRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    if rows.is_empty() {
        return Ok(rows);
    }

    let priority = get_iwad_priority(family, user_priority);
    let priority_map: HashMap<&str, usize> = priority
        .iter()
        .enumerate()
        .map(|(i, v)| (v.as_str(), i))
        .collect();
    let fallback = priority.len();

    rows.sort_by_key(|r| *priority_map.get(r.variant.as_str()).unwrap_or(&fallback));
    Ok(rows)
}

/// Get a registered IWAD by file path.
pub fn get_iwad_by_path(conn: &Connection, path: &str) -> Result<Option<IwadRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM iwads WHERE path = ?")?;
    match stmt.query_row([path], IwadRecord::from_row) {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all registered IWADs, ordered by family then variant.
pub fn get_all_iwads(conn: &Connection) -> Result<Vec<IwadRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM iwads ORDER BY family, variant")?;
    let rows = stmt
        .query_map([], IwadRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Remove registered IWAD(s). Returns number of rows removed.
pub fn remove_iwad(
    conn: &Connection,
    family: &str,
    variant: Option<&str>,
) -> Result<usize> {
    let count = if let Some(v) = variant {
        conn.execute(
            "DELETE FROM iwads WHERE family = ? AND variant = ?",
            rusqlite::params![family, v],
        )?
    } else {
        conn.execute("DELETE FROM iwads WHERE family = ?", [family])?
    };
    Ok(count)
}

/// Return the canonical path for a managed IWAD: `{variant}/{family}.wad`.
pub fn managed_iwad_filename(family: &str, variant: &str) -> String {
    format!("{variant}/{family}.wad")
}

/// Remove registered IWAD(s) and return the paths of removed entries.
pub fn remove_iwad_with_paths(
    conn: &Connection,
    family: &str,
    variant: Option<&str>,
) -> Result<Vec<String>> {
    let paths: Vec<String> = if let Some(v) = variant {
        let mut stmt = conn.prepare(
            "SELECT path FROM iwads WHERE family = ? AND variant = ?",
        )?;
        stmt.query_map(rusqlite::params![family, v], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        let mut stmt = conn.prepare("SELECT path FROM iwads WHERE family = ?")?;
        stmt.query_map([family], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };

    if let Some(v) = variant {
        conn.execute(
            "DELETE FROM iwads WHERE family = ? AND variant = ?",
            rusqlite::params![family, v],
        )?;
    } else {
        conn.execute("DELETE FROM iwads WHERE family = ?", [family])?;
    }

    Ok(paths)
}

/// Look up a family name in the IWAD registry and return its path.
///
/// Gracefully returns `None` if the iwads table doesn't exist yet.
pub fn resolve_iwad_from_db(
    conn: &Connection,
    name: &str,
    user_priority: Option<&HashMap<String, Vec<String>>>,
) -> Option<String> {
    match get_iwad(conn, name, user_priority) {
        Ok(Some(iwad)) => Some(iwad.path),
        Ok(None) => None,
        Err(_) => None, // Table may not exist yet
    }
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
    fn test_add_and_get_iwad() {
        let conn = setup();
        let id = add_iwad(&conn, "doom2", "v1.9", "/path/doom2.wad", Some("Doom II"), Some("abc123")).unwrap();
        assert!(id > 0);

        let iwad = get_iwad(&conn, "doom2", None).unwrap().unwrap();
        assert_eq!(iwad.family, "doom2");
        assert_eq!(iwad.variant, "v1.9");
        assert_eq!(iwad.path, "/path/doom2.wad");
        assert_eq!(iwad.title.as_deref(), Some("Doom II"));
    }

    #[test]
    fn test_get_iwad_priority_resolution() {
        let conn = setup();
        // Add BFG first, then v1.9
        add_iwad(&conn, "doom2", "bfg", "/bfg/doom2.wad", None, None).unwrap();
        add_iwad(&conn, "doom2", "v1.9", "/v19/doom2.wad", None, None).unwrap();

        // v1.9 should be preferred (higher priority in DEFAULT_IWAD_PRIORITY)
        let iwad = get_iwad(&conn, "doom2", None).unwrap().unwrap();
        assert_eq!(iwad.variant, "v1.9");
    }

    #[test]
    fn test_get_iwad_user_priority() {
        let conn = setup();
        add_iwad(&conn, "doom2", "v1.9", "/v19/doom2.wad", None, None).unwrap();
        add_iwad(&conn, "doom2", "bfg", "/bfg/doom2.wad", None, None).unwrap();

        // Override priority to prefer BFG
        let mut user_prio = HashMap::new();
        user_prio.insert("doom2".to_string(), vec!["bfg".to_string(), "v1.9".to_string()]);

        let iwad = get_iwad(&conn, "doom2", Some(&user_prio)).unwrap().unwrap();
        assert_eq!(iwad.variant, "bfg");
    }

    #[test]
    fn test_get_iwad_fallback() {
        let conn = setup();
        // Register freedoom2 but not doom2
        add_iwad(&conn, "freedoom2", "latest", "/freedoom2.wad", None, None).unwrap();

        // doom2 should fall back to freedoom2
        let iwad = get_iwad(&conn, "doom2", None).unwrap().unwrap();
        assert_eq!(iwad.family, "freedoom2");
    }

    #[test]
    fn test_get_iwad_not_found() {
        let conn = setup();
        let result = get_iwad(&conn, "nonexistent", None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_iwad_variant() {
        let conn = setup();
        add_iwad(&conn, "doom2", "v1.9", "/v19/doom2.wad", None, None).unwrap();
        add_iwad(&conn, "doom2", "bfg", "/bfg/doom2.wad", None, None).unwrap();

        let result = get_iwad_variant(&conn, "doom2", "bfg").unwrap().unwrap();
        assert_eq!(result.variant, "bfg");

        let result = get_iwad_variant(&conn, "doom2", "kex").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_family_iwads() {
        let conn = setup();
        add_iwad(&conn, "doom2", "kex", "/kex/doom2.wad", None, None).unwrap();
        add_iwad(&conn, "doom2", "v1.9", "/v19/doom2.wad", None, None).unwrap();

        let iwads = get_family_iwads(&conn, "doom2", None).unwrap();
        assert_eq!(iwads.len(), 2);
        assert_eq!(iwads[0].variant, "v1.9"); // Higher priority
        assert_eq!(iwads[1].variant, "kex");
    }

    #[test]
    fn test_get_all_iwads() {
        let conn = setup();
        add_iwad(&conn, "doom", "v1.9ud", "/doom.wad", None, None).unwrap();
        add_iwad(&conn, "doom2", "v1.9", "/doom2.wad", None, None).unwrap();

        let iwads = get_all_iwads(&conn).unwrap();
        assert_eq!(iwads.len(), 2);
        assert_eq!(iwads[0].family, "doom");
        assert_eq!(iwads[1].family, "doom2");
    }

    #[test]
    fn test_remove_iwad() {
        let conn = setup();
        add_iwad(&conn, "doom2", "v1.9", "/v19/doom2.wad", None, None).unwrap();
        add_iwad(&conn, "doom2", "bfg", "/bfg/doom2.wad", None, None).unwrap();

        // Remove specific variant
        let count = remove_iwad(&conn, "doom2", Some("bfg")).unwrap();
        assert_eq!(count, 1);
        assert_eq!(get_all_iwads(&conn).unwrap().len(), 1);

        // Remove all variants
        let count = remove_iwad(&conn, "doom2", None).unwrap();
        assert_eq!(count, 1);
        assert!(get_all_iwads(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_remove_iwad_with_paths() {
        let conn = setup();
        add_iwad(&conn, "doom2", "v1.9", "/v19/doom2.wad", None, None).unwrap();
        add_iwad(&conn, "doom2", "bfg", "/bfg/doom2.wad", None, None).unwrap();

        let paths = remove_iwad_with_paths(&conn, "doom2", None).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(get_all_iwads(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_managed_iwad_filename() {
        assert_eq!(managed_iwad_filename("doom2", "v1.9"), "v1.9/doom2.wad");
        assert_eq!(managed_iwad_filename("tnt", "unity"), "unity/tnt.wad");
    }

    #[test]
    fn test_normalize_iwad_name() {
        assert_eq!(normalize_iwad_name("Doom II"), Some("doom2"));
        assert_eq!(normalize_iwad_name("the ultimate doom"), Some("doom"));
        assert_eq!(normalize_iwad_name("TNT: Evilution"), Some("tnt"));
        assert_eq!(normalize_iwad_name("freedoom"), Some("freedoom2"));
        assert_eq!(normalize_iwad_name("unknown"), None);
    }

    #[test]
    fn test_get_iwad_by_path() {
        let conn = setup();
        add_iwad(&conn, "doom2", "v1.9", "/path/doom2.wad", None, None).unwrap();

        let result = get_iwad_by_path(&conn, "/path/doom2.wad").unwrap();
        assert!(result.is_some());

        let result = get_iwad_by_path(&conn, "/other/path.wad").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_iwad_from_db() {
        let conn = setup();
        add_iwad(&conn, "doom2", "v1.9", "/path/doom2.wad", None, None).unwrap();

        assert_eq!(
            resolve_iwad_from_db(&conn, "doom2", None),
            Some("/path/doom2.wad".to_string())
        );
        assert_eq!(resolve_iwad_from_db(&conn, "nonexistent", None), None);
    }

    #[test]
    fn test_get_iwad_unknown_variant_fallback() {
        let conn = setup();
        // Only an "unknown" variant registered (filename-detected)
        add_iwad(&conn, "doom2", "unknown", "/doom2.wad", None, None).unwrap();

        let iwad = get_iwad(&conn, "doom2", None).unwrap().unwrap();
        assert_eq!(iwad.variant, "unknown");
    }

    #[test]
    fn test_known_iwads_data() {
        // Verify some well-known hashes
        let doom2 = KNOWN_IWADS.get("25e1459ca71d321525f84628f45ca8cd");
        assert_eq!(doom2, Some(&("doom2", "v1.9", "Doom II: Hell on Earth")));

        let doom = KNOWN_IWADS.get("c4fe9fd920207691a9f493668e0a2083");
        assert_eq!(doom, Some(&("doom", "v1.9ud", "The Ultimate Doom")));
    }

    #[test]
    fn test_iwad_aliases() {
        assert_eq!(IWAD_ALIASES.get("doom ii"), Some(&"doom2"));
        assert_eq!(IWAD_ALIASES.get("the ultimate doom"), Some(&"doom"));
        assert_eq!(IWAD_ALIASES.get("final doom"), Some(&"doom2"));
    }

    #[test]
    fn test_default_priority() {
        let doom2_prio = DEFAULT_IWAD_PRIORITY.get("doom2").unwrap();
        assert_eq!(doom2_prio[0], "v1.9");
        assert!(doom2_prio.contains(&"bfg"));
    }

    #[test]
    fn test_family_fallbacks() {
        assert_eq!(
            FAMILY_FALLBACKS.get("doom2"),
            Some(&vec!["freedoom2"])
        );
        assert_eq!(
            FAMILY_FALLBACKS.get("doom"),
            Some(&vec!["freedoom1"])
        );
    }
}
