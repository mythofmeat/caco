//! What each mirror holds, as of the last time its index was read, and which of
//! that is which WAD.
//!
//! A mirror holding a WAD's file makes that WAD fetchable, which is a fact about
//! the mirror and the WAD at once. It is kept derived, for the reason migration 38
//! dropped the stored availability column: a flag on `wads` would be a cached copy
//! of external state, and nothing would keep it honest. So each index is synced
//! into `mirror_files`, and [`mirror_file_matches`] joins that to `wads` every time
//! something asks. Remove a mirror, or a file from one, and every WAD that relied
//! on it reads as manual again with nothing to update.
//!
//! The rows live in `library.db`, though they could be read again from the mirrors.
//! They are small, like `wad_analysis`, and retrievability has to join them on
//! every connection, where an attached cache database would be one more thing to
//! go missing. They also say something true on a machine that cannot reach the
//! mirror right now.

use rusqlite::Connection;

use super::connection::with_transaction;
use crate::Result;
use crate::mirrors::MirrorEntry;
use crate::source_url::canonical_page;

/// The files on mirrors that are the WAD in the enclosing query's `wads` row, as
/// one `SELECT mirror, sha256, evidence` for each kind of evidence, strongest
/// first. A macro rather than a constant so that `concat!` can compose the
/// predicates in [`crate::db::models`] from it at compile time.
///
/// By hash if the row has one, and nothing else then counts. A row with a hash
/// names one file, and fetching another release because it shares a forum thread
/// would be a silent upgrade. The content hash counts as the same file because it
/// is what plays: a re-zip, or the WAD extracted from the zip.
///
/// A row with no hash has never had a file on any machine that recorded one, which
/// is most of what a mirror is for. It goes by identifier: idgames id, Cacoward key,
/// Doomworld topic, Doom Wiki page. As in `cacoward_link`, weaker evidence refuses a
/// file whose idgames id differs, because a series can share one wiki page across
/// releases and a wrong file is worse than a missing one. A row's topic and page need
/// no such check against each other, since both come from its one `source_url`.
///
/// One arm per kind, rather than one `OR`, so each probes its own index. A term the
/// planner cannot index would turn the whole `OR` into a scan of `mirror_files`, for
/// every row of a query that runs on the UI thread. [`crate::db::connection`]
/// registers `doomworld_topic` and `doomwiki_page`.
macro_rules! mirror_file_matches {
    () => {
        concat!(
            "SELECT m.mirror AS mirror, m.sha256 AS sha256, 0 AS evidence ",
            "FROM mirror_files AS m WHERE m.sha256 = wads.file_sha256 ",
            "UNION ALL SELECT m.mirror, m.sha256, 1 ",
            "FROM mirror_files AS m WHERE m.content_sha256 = wads.content_sha256 ",
            "UNION ALL SELECT m.mirror, m.sha256, 2 ",
            "FROM mirror_files AS m WHERE wads.file_sha256 IS NULL ",
            "AND m.idgames_id = TRIM(wads.idgames_id) ",
            "UNION ALL SELECT m.mirror, m.sha256, 3 ",
            "FROM cacowards AS c ",
            "JOIN mirror_file_cacowards AS k ON k.cacoward_key = c.key ",
            "JOIN mirror_files AS m ON m.mirror = k.mirror AND m.sha256 = k.sha256 ",
            "WHERE wads.file_sha256 IS NULL AND c.wad_id = wads.id ",
            "AND COALESCE(m.idgames_id <> NULLIF(TRIM(wads.idgames_id), ''), 0) = 0 ",
            "UNION ALL SELECT m.mirror, m.sha256, 4 ",
            "FROM mirror_files AS m WHERE wads.file_sha256 IS NULL ",
            "AND m.doomworld_topic = doomworld_topic(wads.source_url) ",
            "AND COALESCE(m.idgames_id <> NULLIF(TRIM(wads.idgames_id), ''), 0) = 0 ",
            "UNION ALL SELECT m.mirror, m.sha256, 5 ",
            "FROM mirror_files AS m WHERE wads.file_sha256 IS NULL ",
            "AND m.doomwiki_page = doomwiki_page(wads.source_url) ",
            "AND COALESCE(m.idgames_id <> NULLIF(TRIM(wads.idgames_id), ''), 0) = 0"
        )
    };
}
pub(crate) use mirror_file_matches;

/// A file on a mirror that is a given WAD's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorFile {
    /// The name of the mirror holding it, from `[[mirrors]]`.
    pub mirror: String,
    pub sha256: String,
    pub size: u64,
    pub filename: String,
    /// Its name on the mirror, when that is not `filename`.
    pub stored_as: Option<String>,
    /// How strongly it is known to be this WAD's: lower is stronger.
    pub evidence: i64,
}

impl MirrorFile {
    /// The file's path under the mirror's root.
    pub fn stored_path(&self) -> String {
        crate::mirrors::stored_path(
            &self.sha256,
            self.stored_as.as_deref().unwrap_or(&self.filename),
        )
    }
}

/// The files on mirrors that are this WAD's: the strongest evidence first, then the
/// newest upload, which is the release a row that names none of them wants.
///
/// The rule is the same SQL `WadRecord::mirrored` reads, so a WAD reading as
/// fetchable and there being something to fetch cannot disagree.
pub fn files_for_wad(conn: &Connection, wad_id: i64) -> Result<Vec<MirrorFile>> {
    let mut stmt = conn.prepare(concat!(
        "SELECT f.mirror, f.sha256, f.size, f.filename, f.stored_as, ",
        "(SELECT MIN(x.evidence) FROM (",
        mirror_file_matches!(),
        ") AS x WHERE x.mirror = f.mirror AND x.sha256 = f.sha256) AS evidence ",
        "FROM wads, mirror_files AS f WHERE wads.id = ?1 AND evidence IS NOT NULL ",
        "ORDER BY evidence, f.added DESC, f.mirror"
    ))?;
    let files = stmt
        .query_map([wad_id], |row| {
            Ok(MirrorFile {
                mirror: row.get(0)?,
                sha256: row.get(1)?,
                size: u64::try_from(row.get::<_, i64>(2)?).unwrap_or_default(),
                filename: row.get(3)?,
                stored_as: row.get(4)?,
                evidence: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(files)
}

/// One mirror file as the tables hold it, which is how two can be compared.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileRow {
    sha256: String,
    size: i64,
    filename: String,
    stored_as: Option<String>,
    content_sha256: String,
    idgames_id: Option<String>,
    doomworld_topic: Option<i64>,
    doomwiki_page: Option<String>,
    added: Option<String>,
    cacowards: Vec<String>,
}

impl FileRow {
    /// `entry` in the form the match compares: a content hash on every row, so the
    /// SQL has one column to ask, and the page in one spelling.
    fn from_entry(entry: &MirrorEntry) -> Self {
        let mut cacowards = entry.cacowards.clone();
        cacowards.sort_unstable();
        cacowards.dedup();
        Self {
            sha256: entry.sha256.clone(),
            size: i64::try_from(entry.size).unwrap_or(i64::MAX),
            filename: entry.filename.clone(),
            stored_as: entry.stored_as.clone(),
            content_sha256: entry
                .content_sha256
                .clone()
                .unwrap_or_else(|| entry.sha256.clone()),
            idgames_id: entry.idgames.map(|id| id.to_string()),
            doomworld_topic: entry.doomworld,
            doomwiki_page: entry.doomwiki.as_deref().map(canonical_page),
            added: entry.added.clone(),
            cacowards,
        }
    }
}

/// Make the tables say what `entries` says about `mirror`. `false`, without waiting
/// for the write lock, when they already do, which is what an ordinary launch finds.
pub fn sync_files(conn: &Connection, mirror: &str, entries: &[MirrorEntry]) -> Result<bool> {
    let mut wanted: Vec<FileRow> = entries.iter().map(FileRow::from_entry).collect();
    wanted.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    // The same file listed twice is one file. The last one listed wins.
    wanted.reverse();
    wanted.dedup_by(|a, b| a.sha256 == b.sha256);
    wanted.reverse();

    if read_rows(conn, mirror)? == wanted {
        return Ok(false);
    }

    with_transaction(conn, |tx| {
        tx.execute("DELETE FROM mirror_files WHERE mirror = ?1", [mirror])?;
        let mut file = tx.prepare(
            "INSERT INTO mirror_files (mirror, sha256, size, filename, stored_as, \
             content_sha256, idgames_id, doomworld_topic, doomwiki_page, added) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        let mut award = tx.prepare(
            "INSERT INTO mirror_file_cacowards (mirror, sha256, cacoward_key) VALUES (?1, ?2, ?3)",
        )?;
        for row in &wanted {
            file.execute(rusqlite::params![
                mirror,
                row.sha256,
                row.size,
                row.filename,
                row.stored_as,
                row.content_sha256,
                row.idgames_id,
                row.doomworld_topic,
                row.doomwiki_page,
                row.added,
            ])?;
            for key in &row.cacowards {
                award.execute(rusqlite::params![mirror, row.sha256, key])?;
            }
        }
        Ok(())
    })?;
    Ok(true)
}

fn read_rows(conn: &Connection, mirror: &str) -> Result<Vec<FileRow>> {
    let mut stmt = conn.prepare(
        "SELECT sha256, size, filename, stored_as, content_sha256, idgames_id, \
         doomworld_topic, doomwiki_page, added FROM mirror_files \
         WHERE mirror = ?1 ORDER BY sha256",
    )?;
    let mut rows: Vec<FileRow> = stmt
        .query_map([mirror], |row| {
            Ok(FileRow {
                sha256: row.get(0)?,
                size: row.get(1)?,
                filename: row.get(2)?,
                stored_as: row.get(3)?,
                content_sha256: row.get(4)?,
                idgames_id: row.get(5)?,
                doomworld_topic: row.get(6)?,
                doomwiki_page: row.get(7)?,
                added: row.get(8)?,
                cacowards: Vec::new(),
            })
        })?
        .collect::<std::result::Result<_, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT sha256, cacoward_key FROM mirror_file_cacowards \
         WHERE mirror = ?1 ORDER BY sha256, cacoward_key",
    )?;
    let awards = stmt.query_map([mirror], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for award in awards {
        let (sha256, key) = award?;
        if let Ok(position) = rows.binary_search_by(|row| row.sha256.cmp(&sha256)) {
            rows[position].cacowards.push(key);
        }
    }
    Ok(rows)
}

/// Forget every mirror but these, whose rows no longer say anything true: a mirror
/// taken out of the config cannot be fetched from. `false` when there was nothing
/// to forget, without waiting for the write lock.
pub fn remove_other_mirrors(conn: &Connection, keep: &[&str]) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT DISTINCT mirror FROM mirror_files")?;
    let known: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let stale: Vec<&String> = known
        .iter()
        .filter(|mirror| !keep.contains(&mirror.as_str()))
        .collect();
    if stale.is_empty() {
        return Ok(false);
    }
    with_transaction(conn, |tx| {
        for mirror in stale {
            tx.execute("DELETE FROM mirror_files WHERE mirror = ?1", [mirror])?;
        }
        Ok(())
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{Availability, Retrievability, SourceType};
    use crate::db::wads::{NewWad, WadUpdate};
    use crate::db::{self, NewCacoward};

    const HOME: &str = "home";
    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn setup() -> Connection {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        conn
    }

    fn file(sha256: &str) -> MirrorEntry {
        MirrorEntry {
            sha256: sha256.to_string(),
            size: 4,
            filename: "file.wad".to_string(),
            ..Default::default()
        }
    }

    fn holds(conn: &Connection, entries: &[MirrorEntry]) {
        sync_files(conn, HOME, entries).unwrap();
    }

    fn add(conn: &Connection, new: NewWad) -> i64 {
        db::add_wad(conn, &new).unwrap()
    }

    /// A row whose file was hashed once, whether or not it is still here.
    fn add_hashed(conn: &Connection, title: &str, file: &str, content: &str) -> i64 {
        let id = add(conn, NewWad::new(title, SourceType::Doomworld));
        conn.execute(
            "UPDATE wads SET file_size = 4, file_sha256 = ?1, file_sha1 = 'x', \
             content_sha256 = ?2 WHERE id = ?3",
            rusqlite::params![file, content, id],
        )
        .unwrap();
        id
    }

    fn mirrored(conn: &Connection, id: i64) -> bool {
        db::get_wad(conn, id, true).unwrap().unwrap().mirrored
    }

    /// What is on a mirror for this WAD, as a list of `(mirror, sha256, evidence)`.
    fn found(conn: &Connection, id: i64) -> Vec<(String, String, i64)> {
        files_for_wad(conn, id)
            .unwrap()
            .into_iter()
            .map(|file| (file.mirror, file.sha256, file.evidence))
            .collect()
    }

    fn one(sha256: &str, evidence: i64) -> Vec<(String, String, i64)> {
        vec![(HOME.to_string(), sha256.to_string(), evidence)]
    }

    #[test]
    fn test_nothing_on_a_mirror_means_nothing_found() {
        let conn = setup();
        let id = add(&conn, NewWad::new("Alone", SourceType::Doomworld));
        assert!(!mirrored(&conn, id));
        assert_eq!(found(&conn, id), []);
    }

    #[test]
    fn test_a_hash_finds_the_file() {
        let conn = setup();
        holds(&conn, &[file(HASH_A)]);
        let id = add_hashed(&conn, "Hashed", HASH_A, HASH_B);

        assert!(mirrored(&conn, id));
        assert_eq!(found(&conn, id), one(HASH_A, 0));
    }

    /// The mirror holds the WAD extracted from the row's zip, or a re-zip of it.
    #[test]
    fn test_the_same_content_counts_as_the_same_file() {
        let conn = setup();
        holds(&conn, &[file(HASH_B)]);
        let extracted = add_hashed(&conn, "Row's file is a zip", HASH_A, HASH_B);
        assert_eq!(found(&conn, extracted), one(HASH_B, 1));

        holds(
            &conn,
            &[MirrorEntry {
                content_sha256: Some(HASH_B.to_string()),
                ..file(HASH_C)
            }],
        );
        assert_eq!(found(&conn, extracted), one(HASH_C, 1));
    }

    /// A row with a hash names one file. Another release that happens to share its
    /// forum thread is not it, and fetching it would be a silent upgrade.
    #[test]
    fn test_a_row_with_a_hash_goes_by_nothing_else() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                doomworld: Some(151858),
                ..file(HASH_C)
            }],
        );
        let id = add_hashed(&conn, "Other release", HASH_A, HASH_A);
        conn.execute(
            "UPDATE wads SET source_url = 'https://www.doomworld.com/vb/thread/151858' \
             WHERE id = ?1",
            [id],
        )
        .unwrap();

        assert!(!mirrored(&conn, id));
        assert_eq!(found(&conn, id), []);
    }

    #[test]
    fn test_an_idgames_id_finds_the_file() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                idgames: Some(19951),
                ..file(HASH_A)
            }],
        );
        let id = add(
            &conn,
            NewWad::new("TNT", SourceType::Idgames).idgames_id("19951"),
        );
        assert_eq!(found(&conn, id), one(HASH_A, 2));

        let other = add(
            &conn,
            NewWad::new("Other", SourceType::Idgames).idgames_id("1"),
        );
        assert_eq!(found(&conn, other), []);
    }

    fn award(conn: &Connection, key: &str, wad_id: Option<i64>) {
        let row = db::upsert_cacoward(
            conn,
            &NewCacoward {
                key: key.to_string(),
                year: 2025,
                category: "winner".to_string(),
                page_order: 1,
                wad_title: key.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        if let Some(wad_id) = wad_id {
            db::link_wad(conn, row, wad_id, true).unwrap();
        }
    }

    #[test]
    fn test_a_cacoward_key_finds_the_file() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                cacowards: vec!["2025-winner-road".to_string()],
                ..file(HASH_A)
            }],
        );
        let linked = add(&conn, NewWad::new("Road", SourceType::Local));
        let unlinked = add(&conn, NewWad::new("Road", SourceType::Local));
        award(&conn, "2025-winner-road", Some(linked));

        assert_eq!(found(&conn, linked), one(HASH_A, 3));
        assert_eq!(found(&conn, unlinked), []);
    }

    #[test]
    fn test_a_doomworld_topic_finds_the_file_under_either_url() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                doomworld: Some(151858),
                ..file(HASH_A)
            }],
        );
        for url in [
            "https://www.doomworld.com/vb/thread/151858",
            "https://www.doomworld.com/forum/topic/151858-lonesome-road/?page=2",
        ] {
            let id = add(
                &conn,
                NewWad::new("Road", SourceType::Doomworld).source_url(url),
            );
            assert_eq!(found(&conn, id), one(HASH_A, 4), "{url}");
        }
    }

    /// Why this is not a `LIKE`: 15185 is the start of 151858.
    #[test]
    fn test_a_topic_is_compared_whole() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                doomworld: Some(15185),
                ..file(HASH_A)
            }],
        );
        let id = add(
            &conn,
            NewWad::new("Road", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/vb/thread/151858"),
        );
        assert_eq!(found(&conn, id), []);
    }

    /// Why this is an SQL function: no string function in SQLite decodes `%27`.
    #[test]
    fn test_a_doom_wiki_page_finds_the_file_however_it_is_spelled() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                doomwiki: Some("DBP31:_Santa's_Outback_Bender".to_string()),
                ..file(HASH_A)
            }],
        );
        for url in [
            "https://doomwiki.org/wiki/DBP31:_Santa%27s_Outback_Bender",
            "https://www.doomwiki.org/wiki/DBP31:_Santa's_Outback_Bender#Reception",
        ] {
            let id = add(
                &conn,
                NewWad::new("Bender", SourceType::Doomwiki).source_url(url),
            );
            assert_eq!(found(&conn, id), one(HASH_A, 5), "{url}");
        }
    }

    /// Two releases, one wiki page: a series shares them, and the wrong file is
    /// worse than none. An idgames id that differs says it is the wrong one.
    #[test]
    fn test_weaker_evidence_gives_way_to_an_idgames_id_that_differs() {
        let conn = setup();
        award(&conn, "2025-winner-road", None);
        holds(
            &conn,
            &[MirrorEntry {
                idgames: Some(100),
                doomworld: Some(1),
                doomwiki: Some("Series".to_string()),
                cacowards: vec!["2025-winner-road".to_string()],
                ..file(HASH_A)
            }],
        );

        let same_thread = add(
            &conn,
            NewWad::new("Other release", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/vb/thread/1")
                .idgames_id("200"),
        );
        conn.execute(
            "UPDATE cacowards SET wad_id = ?1 WHERE key = '2025-winner-road'",
            [same_thread],
        )
        .unwrap();
        assert_eq!(
            found(&conn, same_thread),
            [],
            "topic and key lose to idgames"
        );

        let same_page = add(
            &conn,
            NewWad::new("Other release", SourceType::Doomwiki)
                .source_url("https://doomwiki.org/wiki/Series")
                .idgames_id("200"),
        );
        assert_eq!(found(&conn, same_page), [], "page loses to idgames");
    }

    /// Identifiers of different kinds do not contradict each other: a release can
    /// have a forum thread and an idgames entry both, and only one side knows it.
    #[test]
    fn test_an_identifier_only_one_side_has_contradicts_nothing() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                idgames: Some(100),
                doomworld: Some(1),
                ..file(HASH_A)
            }],
        );
        let thread_only = add(
            &conn,
            NewWad::new("Road", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/vb/thread/1"),
        );
        assert_eq!(found(&conn, thread_only), one(HASH_A, 4));

        // The edit dialog stores a blank field as an empty string.
        let blank_id = add(
            &conn,
            NewWad::new("Road", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/vb/thread/1")
                .idgames_id(""),
        );
        assert_eq!(found(&conn, blank_id), one(HASH_A, 4));
    }

    #[test]
    fn test_a_blank_id_is_not_an_id() {
        let conn = setup();
        holds(&conn, &[file(HASH_A)]);
        let id = add(
            &conn,
            NewWad::new("Blank", SourceType::Doomworld).idgames_id(""),
        );
        assert_eq!(found(&conn, id), []);
    }

    #[test]
    fn test_the_strongest_evidence_comes_first() {
        let conn = setup();
        holds(
            &conn,
            &[
                MirrorEntry {
                    doomworld: Some(1),
                    ..file(HASH_A)
                },
                MirrorEntry {
                    idgames: Some(7),
                    ..file(HASH_B)
                },
            ],
        );
        let id = add(
            &conn,
            NewWad::new("Road", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/vb/thread/1")
                .idgames_id("7"),
        );
        let mut expected = one(HASH_B, 2);
        expected.extend(one(HASH_A, 4));
        assert_eq!(found(&conn, id), expected);
    }

    #[test]
    fn test_a_file_matching_twice_is_listed_once_at_its_best() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                idgames: Some(7),
                doomworld: Some(1),
                ..file(HASH_A)
            }],
        );
        let id = add(
            &conn,
            NewWad::new("Road", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/vb/thread/1")
                .idgames_id("7"),
        );
        assert_eq!(found(&conn, id), one(HASH_A, 2));
    }

    #[test]
    fn test_the_newest_of_several_releases_comes_first() {
        let conn = setup();
        let release = |sha256: &str, added: Option<&str>| MirrorEntry {
            doomworld: Some(1),
            added: added.map(str::to_string),
            ..file(sha256)
        };
        holds(
            &conn,
            &[
                release(HASH_A, Some("2026-01-01T00:00:00Z")),
                release(HASH_B, Some("2026-06-01T00:00:00Z")),
                release(HASH_C, None),
            ],
        );
        let id = add(
            &conn,
            NewWad::new("Road", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/vb/thread/1"),
        );
        let order: Vec<String> = found(&conn, id).into_iter().map(|file| file.1).collect();
        assert_eq!(order, [HASH_B, HASH_A, HASH_C]);
    }

    /// It is derived: nothing has to be written to a WAD for this to change.
    #[test]
    fn test_a_file_leaving_the_mirror_makes_the_wad_manual_again() {
        let conn = setup();
        let id = add_hashed(&conn, "Hashed", HASH_A, HASH_A);
        assert_eq!(wad_retrievability(&conn, id), Retrievability::Manual);

        holds(&conn, &[file(HASH_A)]);
        assert_eq!(wad_retrievability(&conn, id), Retrievability::Automatic);

        holds(&conn, &[]);
        assert_eq!(wad_retrievability(&conn, id), Retrievability::Manual);
    }

    #[test]
    fn test_a_mirror_taken_out_of_the_config_stops_counting() {
        let conn = setup();
        let id = add_hashed(&conn, "Hashed", HASH_A, HASH_A);
        holds(&conn, &[file(HASH_A)]);
        sync_files(&conn, "other", &[file(HASH_B)]).unwrap();

        assert!(!remove_other_mirrors(&conn, &[HOME, "other"]).unwrap());
        assert!(mirrored(&conn, id));

        assert!(remove_other_mirrors(&conn, &["other"]).unwrap());
        assert!(!mirrored(&conn, id));
        assert_eq!(read_rows(&conn, "other").unwrap().len(), 1);
    }

    fn wad_retrievability(conn: &Connection, id: i64) -> Retrievability {
        db::get_wad(conn, id, true)
            .unwrap()
            .unwrap()
            .retrievability()
    }

    #[test]
    fn test_a_mirrored_wad_with_no_file_is_not_lost_and_can_be_downloaded() {
        let conn = setup();
        // No source_url: the mirror is the only place there is to try.
        let id = add_hashed(&conn, "Hashed", HASH_A, HASH_A);

        let wad = db::get_wad(&conn, id, true).unwrap().unwrap();
        assert!(wad.is_lost());
        assert_eq!(wad.availability(), Availability::Unavailable);

        holds(&conn, &[file(HASH_A)]);

        let wad = db::get_wad(&conn, id, true).unwrap().unwrap();
        assert!(!wad.is_lost());
        assert_eq!(wad.availability(), Availability::Downloadable);
    }

    #[test]
    fn test_syncing_the_same_index_again_writes_nothing() {
        let conn = setup();
        let entries = [MirrorEntry {
            title: Some("Only in the index".to_string()),
            content_sha256: Some(HASH_B.to_string()),
            idgames: Some(1),
            doomworld: Some(2),
            doomwiki: Some("Some_Page".to_string()),
            cacowards: vec!["b".to_string(), "a".to_string(), "a".to_string()],
            added: Some("2026-01-01T00:00:00Z".to_string()),
            stored_as: Some("stored.wad".to_string()),
            ..file(HASH_A)
        }];

        assert!(sync_files(&conn, HOME, &entries).unwrap());
        assert!(!sync_files(&conn, HOME, &entries).unwrap());

        let mut changed = entries.clone();
        changed[0].cacowards.pop();
        changed[0].cacowards.pop();
        assert!(sync_files(&conn, HOME, &changed).unwrap());
    }

    #[test]
    fn test_a_file_listed_twice_is_one_file() {
        let conn = setup();
        let second = MirrorEntry {
            filename: "second.wad".to_string(),
            ..file(HASH_A)
        };
        assert!(sync_files(&conn, HOME, &[file(HASH_A), second]).unwrap());
        let rows = read_rows(&conn, HOME).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "second.wad");
    }

    #[test]
    fn test_a_row_says_where_its_file_is_stored() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                stored_as: Some("Santa_s.wad".to_string()),
                filename: "Santa's.wad".to_string(),
                ..file(HASH_A)
            }],
        );
        let id = add_hashed(&conn, "Hashed", HASH_A, HASH_A);

        let files = files_for_wad(&conn, id).unwrap();

        assert_eq!(files[0].filename, "Santa's.wad");
        assert_eq!(files[0].stored_path(), format!("{HASH_A}/Santa_s.wad"));
        assert_eq!(files[0].size, 4);
    }

    /// Dropping a file drops its keys with it, or they would outlive it and match
    /// nothing, and a new file with that hash would inherit them.
    #[test]
    fn test_a_files_cacoward_keys_go_with_it() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                cacowards: vec!["a".to_string()],
                ..file(HASH_A)
            }],
        );
        holds(&conn, &[file(HASH_A)]);
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM mirror_file_cacowards", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);

        remove_other_mirrors(&conn, &[]).unwrap();
        holds(
            &conn,
            &[MirrorEntry {
                cacowards: vec!["a".to_string()],
                ..file(HASH_A)
            }],
        );
        remove_other_mirrors(&conn, &[]).unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM mirror_file_cacowards", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);
    }

    /// The library query runs this for every row, on the UI thread. One term the
    /// planner cannot index and it becomes a scan of the whole mirror per WAD.
    #[test]
    fn test_every_kind_of_evidence_probes_an_index() {
        let conn = setup();
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT {} FROM wads",
            crate::db::WAD_COLUMNS
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        let plan: Vec<String> = stmt
            .query_map([], |row| row.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();

        let scans: Vec<&String> = plan
            .iter()
            .filter(|step| step.contains("SCAN") && *step != "SCAN wads")
            .collect();
        assert!(scans.is_empty(), "{plan:#?}");
        // One per arm, and the Cacoward arm's join is three.
        let searches = plan.iter().filter(|step| step.contains("SEARCH")).count();
        assert_eq!(searches, 8, "{plan:#?}");
    }

    /// A library row changing its URL changes what it matches, with nothing to sync.
    #[test]
    fn test_editing_a_wads_source_url_changes_what_it_matches() {
        let conn = setup();
        holds(
            &conn,
            &[MirrorEntry {
                doomworld: Some(1),
                ..file(HASH_A)
            }],
        );
        let id = add(&conn, NewWad::new("Road", SourceType::Url));
        assert!(!mirrored(&conn, id));

        let update = WadUpdate::new().set_text(
            "source_url",
            Some("https://doomworld.com/vb/thread/1".into()),
        );
        db::update_wad(&conn, id, &update).unwrap();

        assert!(mirrored(&conn, id));
    }
}
