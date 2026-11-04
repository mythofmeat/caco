//! The local index of the idgames archive: every upload, and what is known about it.
//!
//! Its own SQLite file on the cache side (`config::idgames_index_path`), because all of
//! it can be fetched again. So there are no migrations: a file from another version, or
//! one that is damaged, is deleted and built again, which costs a few minutes in the
//! background and nothing else.
//!
//! A row comes into being when the listing names an upload, with no more than its path,
//! size and date, which is already enough to find it by file name and to fetch it. What it
//! is and who made it arrives later, from the API or from the text file beside it, and
//! `fresh` says whether what is here describes the file the listing names now. See
//! [`super::sync`] for how it gets filled.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use caco_core::config;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use unicode_normalization::UnicodeNormalization;

use super::listing::ListedUpload;
use super::models::FileEntry;
use super::textfile::TextFile;

/// Bumped whenever the tables change. A file at any other version is rebuilt.
const SCHEMA_VERSION: i32 = 1;

/// Where the archive's own pages are, which every WAD from it has as its `source_url`.
const ARCHIVE_PAGES: &str = "https://www.doomworld.com/idgames/";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS uploads (
        path        TEXT PRIMARY KEY,
        dir         TEXT NOT NULL,
        filename    TEXT NOT NULL,
        size        INTEGER NOT NULL,
        date        TEXT NOT NULL,
        text_name   TEXT,
        text_size   INTEGER,
        text_date   TEXT,
        -- Whether what follows describes the file the listing names now.
        fresh       INTEGER NOT NULL DEFAULT 0,
        id          INTEGER,
        -- Whether the API has been asked about it since it appeared.
        asked       INTEGER NOT NULL DEFAULT 0,
        title       TEXT NOT NULL DEFAULT '',
        author      TEXT NOT NULL DEFAULT '',
        email       TEXT NOT NULL DEFAULT '',
        description TEXT NOT NULL DEFAULT '',
        credits     TEXT NOT NULL DEFAULT '',
        base        TEXT NOT NULL DEFAULT '',
        buildtime   TEXT NOT NULL DEFAULT '',
        editors     TEXT NOT NULL DEFAULT '',
        bugs        TEXT NOT NULL DEFAULT '',
        rating      REAL NOT NULL DEFAULT 0,
        votes       INTEGER NOT NULL DEFAULT 0,
        -- search_key() of the file name, title, author and description.
        search_text TEXT NOT NULL DEFAULT ''
    );
    CREATE UNIQUE INDEX IF NOT EXISTS uploads_id ON uploads(id) WHERE id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS uploads_date ON uploads(date DESC, path);
    CREATE INDEX IF NOT EXISTS uploads_dir ON uploads(dir);

    -- When the API was last asked about a whole directory, as YYYY-MM-DD.
    CREATE TABLE IF NOT EXISTS dirs (
        dir   TEXT PRIMARY KEY,
        asked TEXT NOT NULL
    ) WITHOUT ROWID;

    CREATE TABLE IF NOT EXISTS state (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) WITHOUT ROWID;
";

const COLUMNS: &str = "path, dir, filename, size, date, id, title, author, email, description, \
                       credits, base, buildtime, editors, bugs, rating, votes";

/// A listing that names less than this share of what is already here is not believed:
/// a mirror partway through something, rather than the archive losing half of itself.
const SHORTEST_BELIEVABLE_LISTING: f64 = 0.5;

/// How little of the archive can still be waiting to be described for a search to go by
/// the index alone. An index that has just read a listing always has a few new uploads
/// it is about to read, and a search should not go to the API for them.
const COMPLETE_ENOUGH: f64 = 0.01;

/// The local index. Open one per thread: a connection cannot be shared, and opening one
/// costs next to nothing.
pub struct Index {
    conn: Connection,
}

/// Which listing the index holds, and how to ask whether there is a newer one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListingState {
    /// The mirror it came from.
    pub source: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// When it was made, as `YYYY-MM-DD`.
    pub generated: String,
    /// When a mirror last answered about it, as RFC 3339.
    pub checked: String,
}

/// What reading a listing changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListingChanges {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
}

/// How far along the index is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexStatus {
    pub uploads: usize,
    /// How many have a text file nobody has read yet, or the API has not described.
    pub waiting_for_text: usize,
    /// How many have an id.
    pub with_id: usize,
    pub listing: Option<ListingState>,
}

impl IndexStatus {
    /// Whether the index knows nothing yet, not even what is in the archive.
    pub fn is_empty(&self) -> bool {
        self.uploads == 0
    }

    /// Whether a search can go by the index alone.
    pub fn is_complete(&self) -> bool {
        !self.is_empty() && self.waiting_for_text as f64 <= self.uploads as f64 * COMPLETE_ENOUGH
    }
}

/// A text file the index wants to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextJob {
    /// The upload it describes.
    pub upload: String,
    /// Its own path under the archive's root.
    pub path: String,
}

/// A directory with uploads the API has not been asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryJob {
    pub dir: String,
    /// The uploads in it it has not been asked about.
    pub waiting: Vec<String>,
    /// How many uploads it has, asked about or not.
    pub uploads: usize,
}

impl Index {
    /// The index at the usual place, for the app. Anything testable takes a path.
    pub fn open_default() -> caco_core::Result<Self> {
        Self::open(&config::idgames_index_path())
    }

    /// Open the index at `path`, making it if it is not there. One that is damaged, or
    /// that another version of caco made, is replaced by an empty one.
    pub fn open(path: &Path) -> caco_core::Result<Self> {
        match Self::open_as_it_is(path) {
            Ok(index) => Ok(index),
            Err(OpenError::Unusable) => {
                for suffix in ["", "-wal", "-shm"] {
                    let mut name = path.as_os_str().to_owned();
                    name.push(suffix);
                    let _ = std::fs::remove_file(name);
                }
                Self::open_as_it_is(path).map_err(|error| match error {
                    OpenError::Failed(e) => e,
                    OpenError::Unusable => {
                        caco_core::Error::Config("the idgames index could not be rebuilt".into())
                    }
                })
            }
            Err(OpenError::Failed(e)) => Err(e),
        }
    }

    fn open_as_it_is(path: &Path) -> Result<Self, OpenError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| OpenError::Failed(e.into()))?;
        }
        let conn = Connection::open(path).map_err(OpenError::from_sql)?;
        // Durable enough for a cache, with a commit per text file not costing a sync.
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
            .map_err(OpenError::from_sql)?;
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(OpenError::from_sql)?;
        match version {
            SCHEMA_VERSION => {}
            0 => {
                conn.execute_batch(&format!(
                    "BEGIN IMMEDIATE;
                     {SCHEMA}
                     PRAGMA user_version = {SCHEMA_VERSION};
                     COMMIT;"
                ))
                .map_err(OpenError::from_sql)?;
            }
            _ => return Err(OpenError::Unusable),
        }
        Ok(Self { conn })
    }

    // -----------------------------------------------------------------------
    // Reading
    // -----------------------------------------------------------------------

    /// How far along the index is.
    pub fn status(&self) -> caco_core::Result<IndexStatus> {
        let (uploads, waiting_for_text, with_id) = self.conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(fresh = 0 AND text_size IS NOT NULL), 0),
                    COUNT(id)
             FROM uploads",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        Ok(IndexStatus {
            uploads: uploads as usize,
            waiting_for_text: waiting_for_text as usize,
            with_id: with_id as usize,
            listing: self.listing_state()?,
        })
    }

    /// Which listing the index holds, if it holds one.
    pub fn listing_state(&self) -> caco_core::Result<Option<ListingState>> {
        let Some(source) = self.state("listing_source")? else {
            return Ok(None);
        };
        Ok(Some(ListingState {
            source,
            etag: self.state("listing_etag")?,
            last_modified: self.state("listing_last_modified")?,
            generated: self.state("listing_generated")?.unwrap_or_default(),
            checked: self.state("listing_checked")?.unwrap_or_default(),
        }))
    }

    /// Uploads matching `query`, best first.
    ///
    /// Every word has to be somewhere in the file name, title, author or description, as
    /// a substring: the archive's own search takes part of a name, and `sunl` finding
    /// nothing because the word is `sunlust` would be worse than what this replaces. It
    /// reads the whole table to do that, which at 21,000 rows takes a few tens of
    /// milliseconds on a worker.
    pub fn search(&self, query: &str, limit: usize) -> caco_core::Result<Vec<FileEntry>> {
        let key = search_key(query);
        let words: Vec<&str> = key.split_whitespace().collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let condition = vec!["search_text LIKE ?"; words.len()].join(" AND ");
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM uploads WHERE {condition}"))?;
        // `search_key` leaves nothing that LIKE would take for a wildcard.
        let patterns = words.iter().map(|word| format!("%{word}%"));
        let mut found: Vec<FileEntry> = statement
            .query_map(rusqlite::params_from_iter(patterns), entry_from_row)?
            .collect::<rusqlite::Result<_>>()?;

        found.sort_by_cached_key(|entry| {
            (
                relevance(entry, &key, &words),
                !entry.dir.starts_with("levels/"),
                std::cmp::Reverse(entry.date.clone()),
                entry.filename.clone(),
            )
        });
        found.truncate(limit);
        Ok(found)
    }

    /// The most recent uploads, newest first. Which is the date on the file, so one that
    /// has been replaced counts from when it was.
    pub fn newest(&self, limit: usize) -> caco_core::Result<Vec<FileEntry>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM uploads ORDER BY date DESC, path LIMIT ?1"
        ))?;
        let found = statement
            .query_map(params![limit as i64], entry_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(found)
    }

    /// The upload with this id, if the index knows of one.
    pub fn by_id(&self, id: i64) -> caco_core::Result<Option<FileEntry>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM uploads WHERE id = ?1"),
                params![id],
                entry_from_row,
            )
            .optional()?)
    }

    /// The upload at this path under the archive's root.
    pub fn by_path(&self, path: &str) -> caco_core::Result<Option<FileEntry>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM uploads WHERE path = ?1"),
                params![path],
                entry_from_row,
            )
            .optional()?)
    }

    // -----------------------------------------------------------------------
    // What the sync wants next
    // -----------------------------------------------------------------------

    /// The text files to read, what is likeliest to be looked for first: WADs before the
    /// rest of the archive, and new ones before old ones.
    pub(super) fn text_jobs(&self) -> caco_core::Result<Vec<TextJob>> {
        let mut statement = self.conn.prepare(
            "SELECT path, dir || text_name FROM uploads
             WHERE fresh = 0 AND text_size IS NOT NULL
             ORDER BY dir NOT LIKE 'levels/%', date DESC, path",
        )?;
        let jobs = statement
            .query_map([], |row| {
                Ok(TextJob {
                    upload: row.get(0)?,
                    path: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(jobs)
    }

    /// The directories with uploads the API has not been asked about, WADs first.
    pub(super) fn directory_jobs(&self) -> caco_core::Result<Vec<DirectoryJob>> {
        let mut sizes = self
            .conn
            .prepare("SELECT dir, COUNT(*) FROM uploads GROUP BY dir")?;
        let sizes: HashMap<String, i64> = sizes
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;

        let mut statement = self.conn.prepare(
            "SELECT dir, path FROM uploads WHERE id IS NULL AND asked = 0
             ORDER BY dir NOT LIKE 'levels/%', dir, path",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut jobs: Vec<DirectoryJob> = Vec::new();
        for row in rows {
            let (dir, path) = row?;
            match jobs.last_mut() {
                Some(job) if job.dir == dir => job.waiting.push(path),
                _ => jobs.push(DirectoryJob {
                    uploads: sizes.get(&dir).copied().unwrap_or(0) as usize,
                    dir,
                    waiting: vec![path],
                }),
            }
        }
        Ok(jobs)
    }

    /// The directories asked about longest ago, up to `limit`, if that was before
    /// `before`. Which is how ratings keep up, since nothing else would ever ask about
    /// an upload again: a new one has none yet on the day it is first asked about.
    pub(super) fn directories_to_refresh(
        &self,
        before: &str,
        limit: usize,
    ) -> caco_core::Result<Vec<String>> {
        let mut statement = self
            .conn
            .prepare("SELECT dir FROM dirs WHERE asked < ?1 ORDER BY asked, dir LIMIT ?2")?;
        let found = statement
            .query_map(params![before, limit as i64], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(found)
    }

    // -----------------------------------------------------------------------
    // Writing
    // -----------------------------------------------------------------------

    /// Make the index say what a listing says, and record where it came from.
    ///
    /// Only what has changed is touched, and each change costs what it described: an
    /// upload whose file or text file is different is no longer `fresh`, so its text file
    /// gets read again. It keeps what it knew until then, which is nearly always still
    /// true, and is better than a title turning back into a file name for a minute.
    pub(crate) fn apply_listing(
        &self,
        uploads: &[ListedUpload],
        state: &ListingState,
    ) -> caco_core::Result<ListingChanges> {
        let tx = self.transaction()?;
        let known = self.known_stamps()?;

        let mut changes = ListingChanges::default();
        let mut listed = HashSet::with_capacity(uploads.len());
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO uploads
                     (path, dir, filename, size, date, text_name, text_size, text_date, search_text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut update = tx.prepare_cached(
                "UPDATE uploads SET size = ?2, date = ?3, text_name = ?4, text_size = ?5,
                                    text_date = ?6, fresh = 0
                 WHERE path = ?1",
            )?;
            for upload in uploads {
                let path = upload.path();
                let stamp = Stamp::of(upload);
                let text = upload.text.as_ref();
                let text_name = text.map(|text| text.name.as_str());
                match known.get(&path) {
                    None => {
                        insert.execute(params![
                            path,
                            upload.dir,
                            upload.filename,
                            upload.size,
                            upload.date,
                            text_name,
                            stamp.text_size,
                            stamp.text_date,
                            search_key(&upload.filename),
                        ])?;
                        changes.added += 1;
                    }
                    Some(old) if *old != stamp => {
                        update.execute(params![
                            path,
                            upload.size,
                            upload.date,
                            text_name,
                            stamp.text_size,
                            stamp.text_date,
                        ])?;
                        changes.changed += 1;
                    }
                    Some(_) => {}
                }
                listed.insert(path);
            }
            let mut remove = tx.prepare_cached("DELETE FROM uploads WHERE path = ?1")?;
            for path in known.keys().filter(|path| !listed.contains(*path)) {
                remove.execute(params![path])?;
                changes.removed += 1;
            }
        }
        tx.execute(
            "DELETE FROM dirs WHERE dir NOT IN (SELECT dir FROM uploads)",
            [],
        )?;
        self.write_listing_state(&tx, state)?;
        tx.commit()?;
        Ok(changes)
    }

    /// Whether a listing of this many uploads is to be believed, given what is here.
    pub(super) fn is_believable(&self, listed: usize) -> caco_core::Result<bool> {
        let known = self.status()?.uploads;
        Ok(listed as f64 >= known as f64 * SHORTEST_BELIEVABLE_LISTING)
    }

    /// Record that a mirror answered and the listing is still current.
    pub(super) fn listing_unchanged(&self, checked: &str) -> caco_core::Result<()> {
        self.set_state("listing_checked", Some(checked))
    }

    /// What a text file said about its upload.
    ///
    /// What it does not say is left alone: a file with no `Title` line takes nothing away
    /// from a title the API gave.
    pub(super) fn record_text(&self, job: &TextJob, found: &TextFile) -> caco_core::Result<()> {
        let tx = self.transaction()?;
        tx.execute(
            "UPDATE uploads SET
                 title       = COALESCE(NULLIF(?2, ''), title),
                 author      = COALESCE(NULLIF(?3, ''), author),
                 email       = COALESCE(NULLIF(?4, ''), email),
                 description = COALESCE(NULLIF(?5, ''), description),
                 credits     = COALESCE(NULLIF(?6, ''), credits),
                 base        = COALESCE(NULLIF(?7, ''), base),
                 buildtime   = COALESCE(NULLIF(?8, ''), buildtime),
                 editors     = COALESCE(NULLIF(?9, ''), editors),
                 bugs        = COALESCE(NULLIF(?10, ''), bugs),
                 fresh       = 1
             WHERE path = ?1",
            params![
                job.upload,
                found.title,
                found.author,
                found.email,
                found.description,
                found.credits,
                found.base,
                found.buildtime,
                found.editors,
                found.bugs,
            ],
        )?;
        refresh_search_text(&tx, &job.upload)?;
        tx.commit()?;
        Ok(())
    }

    /// What the API said about one upload. Whether the index learned its id from this.
    pub fn record_entry(&self, entry: &FileEntry) -> caco_core::Result<bool> {
        let tx = self.transaction()?;
        let learned = write_entry(&tx, entry)?;
        tx.commit()?;
        Ok(learned)
    }

    /// Record that the API has been asked about an upload and did not know it.
    pub(super) fn record_unknown_to_api(&self, path: &str) -> caco_core::Result<()> {
        self.conn.execute(
            "UPDATE uploads SET asked = 1 WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }

    /// What the API said about a whole directory, asked on `today`. How many ids that
    /// taught the index.
    pub(super) fn record_directory(
        &self,
        dir: &str,
        entries: &[FileEntry],
        today: &str,
    ) -> caco_core::Result<usize> {
        let tx = self.transaction()?;
        let mut learned = 0;
        for entry in entries {
            learned += usize::from(write_entry(&tx, entry)?);
        }
        tx.execute("UPDATE uploads SET asked = 1 WHERE dir = ?1", params![dir])?;
        tx.execute(
            "INSERT INTO dirs (dir, asked) VALUES (?1, ?2)
             ON CONFLICT(dir) DO UPDATE SET asked = excluded.asked",
            params![dir, today],
        )?;
        tx.commit()?;
        Ok(learned)
    }

    fn known_stamps(&self) -> caco_core::Result<HashMap<String, Stamp>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT path, size, date, text_size, text_date FROM uploads")?;
        let known = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    Stamp {
                        size: row.get(1)?,
                        date: row.get(2)?,
                        text_size: row.get(3)?,
                        text_date: row.get(4)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(known)
    }

    fn write_listing_state(
        &self,
        tx: &rusqlite::Transaction<'_>,
        state: &ListingState,
    ) -> caco_core::Result<()> {
        let values = [
            ("listing_source", Some(state.source.as_str())),
            ("listing_etag", state.etag.as_deref()),
            ("listing_last_modified", state.last_modified.as_deref()),
            ("listing_generated", Some(state.generated.as_str())),
            ("listing_checked", Some(state.checked.as_str())),
        ];
        for (key, value) in values {
            write_state(tx, key, value)?;
        }
        Ok(())
    }

    fn state(&self, key: &str) -> caco_core::Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM state WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn set_state(&self, key: &str, value: Option<&str>) -> caco_core::Result<()> {
        write_state(&self.conn, key, value)
    }

    /// With the write lock taken up front, as `db::with_transaction` does and for the same
    /// reason: a transaction that reads and then writes under WAL cannot wait for the lock,
    /// and an import can be recording an id while a sync is working through a directory.
    fn transaction(&self) -> rusqlite::Result<rusqlite::Transaction<'_>> {
        rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
    }
}

enum OpenError {
    /// Nothing that deleting the file would fix.
    Failed(caco_core::Error),
    /// Something it would.
    Unusable,
}

impl OpenError {
    fn from_sql(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt) => {
                Self::Unusable
            }
            _ => Self::Failed(error.into()),
        }
    }
}

/// What the listing said about an upload, to compare with the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    size: i64,
    date: String,
    text_size: Option<i64>,
    text_date: Option<String>,
}

impl Stamp {
    fn of(upload: &ListedUpload) -> Self {
        Self {
            size: upload.size,
            date: upload.date.clone(),
            text_size: upload.text.as_ref().map(|text| text.size),
            text_date: upload.text.as_ref().map(|text| text.date.clone()),
        }
    }
}

fn write_state(conn: &Connection, key: &str, value: Option<&str>) -> caco_core::Result<()> {
    match value {
        Some(value) => conn.execute(
            "INSERT INTO state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?,
        None => conn.execute("DELETE FROM state WHERE key = ?1", params![key])?,
    };
    Ok(())
}

/// Put an entry from the API on its row. Whether that was the row's first id.
///
/// The id and rating are about the upload whichever copy of it the API has seen. What it
/// says about the rest only counts if it is about this file, since the API lags the
/// archive: a file replaced today still reads as the old one there, under the old size,
/// and then the text file beside it is the one to believe.
///
/// For one upload in twenty it has nothing to say at all, from 1994 to 1998 mostly, as their
/// text files are not the template and gave its own parser nothing. Those files are still
/// what the upload is, so an answer with no title or description leaves them to be read.
fn write_entry(conn: &Connection, entry: &FileEntry) -> caco_core::Result<bool> {
    let path = entry.path();
    let row: Option<(i64, Option<i64>)> = conn
        .query_row(
            "SELECT size, id FROM uploads WHERE path = ?1",
            params![path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((size, known_id)) = row else {
        return Ok(false);
    };

    // A moved file keeps its id, and the listing may not have dropped its old row yet.
    conn.execute(
        "UPDATE uploads SET id = NULL WHERE id = ?1 AND path != ?2",
        params![entry.id, path],
    )?;
    conn.execute(
        "UPDATE uploads SET id = ?2, asked = 1, rating = ?3, votes = ?4 WHERE path = ?1",
        params![path, entry.id, entry.rating, entry.votes],
    )?;
    if size == entry.size {
        let describes_it = !entry.title.is_empty() || !entry.description.is_empty();
        conn.execute(
            "UPDATE uploads SET
                 title       = COALESCE(NULLIF(?2, ''), title),
                 author      = COALESCE(NULLIF(?3, ''), author),
                 email       = COALESCE(NULLIF(?4, ''), email),
                 description = COALESCE(NULLIF(?5, ''), description),
                 credits     = COALESCE(NULLIF(?6, ''), credits),
                 base        = COALESCE(NULLIF(?7, ''), base),
                 buildtime   = COALESCE(NULLIF(?8, ''), buildtime),
                 editors     = COALESCE(NULLIF(?9, ''), editors),
                 bugs        = COALESCE(NULLIF(?10, ''), bugs),
                 fresh       = MAX(fresh, ?11)
             WHERE path = ?1",
            params![
                path,
                entry.title,
                entry.author,
                entry.email,
                entry.description,
                entry.credits,
                entry.base,
                entry.buildtime,
                entry.editors,
                entry.bugs,
                describes_it,
            ],
        )?;
        refresh_search_text(conn, &path)?;
    }
    Ok(known_id.is_none())
}

/// Bring a row's `search_text` up to date with what it now says.
fn refresh_search_text(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    let (filename, title, author, description): (String, String, String, String) = conn.query_row(
        "SELECT filename, title, author, description FROM uploads WHERE path = ?1",
        params![path],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let key = search_key(&format!("{filename} {title} {author} {description}"));
    conn.execute(
        "UPDATE uploads SET search_text = ?2 WHERE path = ?1",
        params![path, key],
    )?;
    Ok(())
}

fn stem_of(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => filename.to_string(),
    }
}

/// A row as the API would have answered, near enough that nothing after this can tell. A
/// row nobody has described yet goes by its file name, since a blank row in a results
/// table is no use to anyone.
fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<FileEntry> {
    let dir: String = row.get(1)?;
    let filename: String = row.get(2)?;
    let stem = stem_of(&filename);
    let title: String = row.get(6)?;
    Ok(FileEntry {
        id: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
        title: if title.is_empty() {
            stem.clone()
        } else {
            title
        },
        url: format!("{ARCHIVE_PAGES}{dir}{stem}"),
        idgamesurl: format!("idgames://{dir}{filename}"),
        dir,
        filename,
        size: row.get(3)?,
        age: 0,
        date: row.get(4)?,
        author: row.get(7)?,
        email: row.get(8)?,
        description: row.get(9)?,
        credits: row.get(10)?,
        base: row.get(11)?,
        buildtime: row.get(12)?,
        editors: row.get(13)?,
        bugs: row.get(14)?,
        textfile: String::new(),
        rating: row.get(15)?,
        votes: row.get(16)?,
        reviews: Vec::new(),
    })
}

/// How a query and a row's text are compared: lower case, with no accents and no
/// punctuation, so that `Sófalvi` finds `Sofalvi`, `ad_mortem` finds `Ad Mortem`'s file and
/// `santa's` finds `Santas`. Letters and digits of any script stay, unlike in the
/// library's title comparison, because a title in Cyrillic has to be able to find itself.
fn search_key(text: &str) -> String {
    let stripped: String = text
        .to_lowercase()
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How well an entry answers a query: lower is better. A name is what people search by,
/// and most things mention `hell` somewhere in their description.
fn relevance(entry: &FileEntry, query: &str, words: &[&str]) -> u8 {
    let title = search_key(&entry.title);
    let stem = search_key(&stem_of(&entry.filename));
    let filename = search_key(&entry.filename);
    let has_all = |text: &str| words.iter().all(|word| text.contains(word));

    if title == query || stem == query || filename == query {
        0
    } else if title.starts_with(query) || stem.starts_with(query) {
        1
    } else if has_all(&title) {
        2
    } else if has_all(&filename) {
        3
    } else if has_all(&search_key(&entry.author)) {
        4
    } else {
        5
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idgames::listing::TextStamp;

    fn index() -> (Index, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Index::open(&dir.path().join("index.db")).unwrap(), dir)
    }

    fn state() -> ListingState {
        ListingState {
            source: "https://mirror.example/".to_string(),
            etag: Some("\"abc\"".to_string()),
            last_modified: None,
            generated: "2026-11-03".to_string(),
            checked: "2026-11-03T00:00:00Z".to_string(),
        }
    }

    fn upload(path: &str, size: i64, date: &str) -> ListedUpload {
        let (dir, filename) = path.rsplit_once('/').unwrap();
        ListedUpload {
            dir: format!("{dir}/"),
            filename: filename.to_string(),
            size,
            date: date.to_string(),
            text: Some(TextStamp {
                name: format!("{}.txt", stem_of(filename)),
                size: 100,
                date: date.to_string(),
            }),
        }
    }

    fn api_entry(path: &str, id: i64, size: i64, title: &str) -> FileEntry {
        let (dir, filename) = path.rsplit_once('/').unwrap();
        serde_json::from_value(serde_json::json!({
            "id": id,
            "dir": format!("{dir}/"),
            "filename": filename,
            "size": size,
            "title": title,
            "author": "Someone",
            "description": "From the API",
            "rating": 4.5,
            "votes": 10,
        }))
        .unwrap()
    }

    fn text(title: &str) -> TextFile {
        TextFile {
            title: title.to_string(),
            author: "From a text file".to_string(),
            ..Default::default()
        }
    }

    fn job(upload: &str) -> TextJob {
        TextJob {
            upload: upload.to_string(),
            path: format!("{}.txt", stem_of(upload)),
        }
    }

    const SUNLUST: &str = "levels/doom2/Ports/megawads/sunlust.zip";

    #[test]
    fn test_a_new_index_is_empty_and_not_complete() {
        let (index, _dir) = index();
        let status = index.status().unwrap();
        assert!(status.is_empty());
        assert!(!status.is_complete());
        assert_eq!(status.listing, None);
    }

    #[test]
    fn test_a_listing_is_searchable_by_file_name_straight_away() {
        let (index, _dir) = index();
        index
            .apply_listing(&[upload(SUNLUST, 1, "2015-08-09")], &state())
            .unwrap();

        let found = index.search("sunlust", 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].filename, "sunlust.zip");
        assert_eq!(found[0].dir, "levels/doom2/Ports/megawads/");
        // What would otherwise be a blank row.
        assert_eq!(found[0].title, "sunlust");
        assert_eq!(found[0].id, 0);
        assert_eq!(index.status().unwrap().listing, Some(state()));
    }

    /// An import writes `url` as the WAD's `source_url`, which has to come out as the
    /// API's would, or the same WAD from the two routes is two WADs.
    #[test]
    fn test_a_row_has_the_urls_the_api_would_give_it() {
        let (index, _dir) = index();
        index
            .apply_listing(&[upload(SUNLUST, 1, "2015-08-09")], &state())
            .unwrap();
        let entry = index.by_path(SUNLUST).unwrap().unwrap();
        assert_eq!(
            entry.url,
            "https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/sunlust"
        );
        assert_eq!(
            entry.idgamesurl,
            "idgames://levels/doom2/Ports/megawads/sunlust.zip"
        );
    }

    #[test]
    fn test_reading_a_listing_says_what_changed_and_only_touches_that() {
        let (index, _dir) = index();
        let a = "levels/doom2/a.zip";
        let b = "levels/doom2/b.zip";
        let c = "levels/doom2/c.zip";
        let first = [upload(a, 1, "2020-01-01"), upload(b, 1, "2020-01-01")];
        assert_eq!(
            index.apply_listing(&first, &state()).unwrap(),
            ListingChanges {
                added: 2,
                changed: 0,
                removed: 0,
            }
        );
        index.record_text(&job(a), &text("Title A")).unwrap();
        index.record_text(&job(b), &text("Title B")).unwrap();
        assert_eq!(index.text_jobs().unwrap(), []);

        // `a` is replaced, `b` is taken down, and `c` is new.
        let second = [upload(a, 2, "2026-01-01"), upload(c, 1, "2026-01-01")];
        assert_eq!(
            index.apply_listing(&second, &state()).unwrap(),
            ListingChanges {
                added: 1,
                changed: 1,
                removed: 1,
            }
        );

        let wanted: Vec<String> = index
            .text_jobs()
            .unwrap()
            .into_iter()
            .map(|job| job.upload)
            .collect();
        assert_eq!(wanted, [a.to_string(), c.to_string()]);
        // Keeps what it knew until the text file has been read again.
        let kept = index.by_path(a).unwrap().unwrap();
        assert_eq!((kept.title.as_str(), kept.size), ("Title A", 2));
        assert!(index.by_path(b).unwrap().is_none());
    }

    #[test]
    fn test_a_text_file_that_changes_alone_is_read_again() {
        let (index, _dir) = index();
        let mut listed = upload(SUNLUST, 1, "2020-01-01");
        index.apply_listing(&[listed.clone()], &state()).unwrap();
        index.record_text(&job(SUNLUST), &text("Sunlust")).unwrap();

        listed.text.as_mut().unwrap().size = 200;
        let changes = index.apply_listing(&[listed], &state()).unwrap();

        assert_eq!(changes.changed, 1);
        assert_eq!(index.text_jobs().unwrap().len(), 1);
    }

    #[test]
    fn test_a_listing_with_nothing_new_changes_nothing() {
        let (index, _dir) = index();
        let listed = [upload(SUNLUST, 1, "2020-01-01")];
        index.apply_listing(&listed, &state()).unwrap();
        index.record_text(&job(SUNLUST), &text("Sunlust")).unwrap();
        assert_eq!(
            index.apply_listing(&listed, &state()).unwrap(),
            ListingChanges::default()
        );
        assert_eq!(index.text_jobs().unwrap(), []);
    }

    /// A mirror partway through a sync, or a listing cut short some way the gzip did not
    /// catch, must not read as most of the archive having been taken down.
    #[test]
    fn test_a_listing_with_most_of_the_archive_missing_is_not_believed() {
        let (index, _dir) = index();
        let whole: Vec<ListedUpload> = (0..100)
            .map(|n| upload(&format!("levels/doom2/{n}.zip"), 1, "2020-01-01"))
            .collect();
        assert!(index.is_believable(0).unwrap());
        index.apply_listing(&whole, &state()).unwrap();

        assert!(!index.is_believable(10).unwrap());
        assert!(!index.is_believable(49).unwrap());
        assert!(index.is_believable(50).unwrap());
        assert!(index.is_believable(21_000).unwrap());
    }

    #[test]
    fn test_the_order_of_text_files_is_wads_first_then_newest() {
        let (index, _dir) = index();
        index
            .apply_listing(
                &[
                    upload("levels/doom2/old.zip", 1, "2001-01-01"),
                    upload("levels/doom2/new.zip", 1, "2026-01-01"),
                    upload("utils/newest.zip", 1, "2026-06-01"),
                ],
                &state(),
            )
            .unwrap();
        let order: Vec<String> = index
            .text_jobs()
            .unwrap()
            .into_iter()
            .map(|job| job.path)
            .collect();
        assert_eq!(
            order,
            [
                "levels/doom2/new.txt",
                "levels/doom2/old.txt",
                "utils/newest.txt",
            ]
        );
    }

    #[test]
    fn test_a_zip_with_no_text_file_waits_for_nothing() {
        let (index, _dir) = index();
        let mut listed = upload(SUNLUST, 1, "2020-01-01");
        listed.text = None;
        index.apply_listing(&[listed], &state()).unwrap();

        assert_eq!(index.text_jobs().unwrap(), []);
        let status = index.status().unwrap();
        assert_eq!(status.waiting_for_text, 0);
        assert!(status.is_complete());
    }

    #[test]
    fn test_a_text_file_fills_in_a_row_and_is_searchable() {
        let (index, _dir) = index();
        index
            .apply_listing(&[upload(SUNLUST, 1, "2020-01-01")], &state())
            .unwrap();
        index
            .record_text(
                &job(SUNLUST),
                &TextFile {
                    title: "Sunlust: The Megawad".to_string(),
                    author: "Ribbiks".to_string(),
                    description: "Thirty-two maps of pain.".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();

        for query in ["megawad", "ribbiks", "thirty two maps"] {
            assert_eq!(index.search(query, 10).unwrap().len(), 1, "{query}");
        }
        assert_eq!(index.status().unwrap().waiting_for_text, 0);
    }

    /// The whole of why the API is asked before anything is read off a mirror.
    #[test]
    fn test_an_entry_about_this_file_means_its_text_file_need_not_be_read() {
        let (index, _dir) = index();
        index
            .apply_listing(&[upload(SUNLUST, 1000, "2015-08-09")], &state())
            .unwrap();

        let learned = index
            .record_directory(
                "levels/doom2/Ports/megawads/",
                &[api_entry(SUNLUST, 18168, 1000, "Sunlust")],
                "2026-11-03",
            )
            .unwrap();

        assert_eq!(learned, 1);
        assert_eq!(index.text_jobs().unwrap(), []);
        let entry = index.by_id(18168).unwrap().unwrap();
        assert_eq!(entry.title, "Sunlust");
        assert_eq!(entry.author, "Someone");
        assert_eq!((entry.rating, entry.votes), (4.5, 10));
    }

    /// `utakata.zip` was replaced on 2026-11-01. A day later the API still gave the July
    /// file's size and description.
    #[test]
    fn test_an_entry_about_an_older_file_gives_only_its_id() {
        let (index, _dir) = index();
        index
            .apply_listing(&[upload(SUNLUST, 2000, "2026-11-01")], &state())
            .unwrap();

        index
            .record_entry(&api_entry(SUNLUST, 18168, 1000, "An older title"))
            .unwrap();

        let entry = index.by_id(18168).unwrap().unwrap();
        assert_eq!(entry.title, "sunlust");
        assert_eq!(entry.rating, 4.5);
        assert_eq!(index.text_jobs().unwrap().len(), 1);
    }

    /// `levels/doom/0-9/23castle.zip`, from 1994. The API knows who made it and nothing
    /// about it, and its text file is a paragraph saying what it is.
    #[test]
    fn test_an_entry_with_no_title_or_description_leaves_the_text_file_to_be_read() {
        let (index, _dir) = index();
        index
            .apply_listing(&[upload(SUNLUST, 1000, "1994-08-09")], &state())
            .unwrap();
        let mut says_little = api_entry(SUNLUST, 18168, 1000, "");
        says_little.description = String::new();
        index.record_entry(&says_little).unwrap();

        assert_eq!(index.text_jobs().unwrap().len(), 1);
        let entry = index.by_id(18168).unwrap().unwrap();
        assert_eq!(entry.author, "Someone");

        index
            .record_text(
                &job(SUNLUST),
                &TextFile {
                    description: "HEY! It's an original level.".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(index.text_jobs().unwrap(), []);
        let entry = index.by_id(18168).unwrap().unwrap();
        assert_eq!(entry.author, "Someone");
        assert_eq!(entry.description, "HEY! It's an original level.");
        assert_eq!(index.search("original level", 10).unwrap().len(), 1);
    }

    /// A description alone is enough, as a title alone is: the two together are rarer than
    /// they should be on anything from the first few years.
    #[test]
    fn test_an_entry_with_a_description_and_no_title_is_a_description() {
        let (index, _dir) = index();
        index
            .apply_listing(&[upload(SUNLUST, 1000, "1994-08-09")], &state())
            .unwrap();
        index
            .record_entry(&api_entry(SUNLUST, 18168, 1000, ""))
            .unwrap();
        assert_eq!(index.text_jobs().unwrap(), []);
    }

    #[test]
    fn test_a_text_file_does_not_take_away_what_the_api_said() {
        let (index, _dir) = index();
        let mut listed = upload(SUNLUST, 1000, "2015-08-09");
        index.apply_listing(&[listed.clone()], &state()).unwrap();
        index
            .record_entry(&api_entry(SUNLUST, 18168, 1000, "Sunlust"))
            .unwrap();
        // The text file changes, and says less than the API did.
        listed.text.as_mut().unwrap().size = 200;
        index.apply_listing(&[listed], &state()).unwrap();

        index
            .record_text(&job(SUNLUST), &TextFile::default())
            .unwrap();

        let entry = index.by_id(18168).unwrap().unwrap();
        assert_eq!(entry.title, "Sunlust");
        assert_eq!(entry.author, "Someone");
    }

    /// An entry for a file this mirror has not got yet, or that it has dropped.
    #[test]
    fn test_an_entry_for_nothing_in_the_listing_is_ignored() {
        let (index, _dir) = index();
        assert!(
            !index
                .record_entry(&api_entry(SUNLUST, 18168, 1000, "Sunlust"))
                .unwrap()
        );
        assert!(index.by_id(18168).unwrap().is_none());
    }

    #[test]
    fn test_an_id_that_moves_with_its_file_is_not_a_conflict() {
        let (index, _dir) = index();
        let moved_to = "levels/doom2/Ports/d-f/sunlust.zip";
        index
            .apply_listing(
                &[
                    upload(SUNLUST, 1000, "2015-08-09"),
                    upload(moved_to, 1000, "2015-08-09"),
                ],
                &state(),
            )
            .unwrap();
        index
            .record_entry(&api_entry(SUNLUST, 18168, 1000, "Sunlust"))
            .unwrap();

        index
            .record_entry(&api_entry(moved_to, 18168, 1000, "Sunlust"))
            .unwrap();

        assert_eq!(
            index.by_id(18168).unwrap().unwrap().dir,
            "levels/doom2/Ports/d-f/"
        );
        assert_eq!(index.by_path(SUNLUST).unwrap().unwrap().id, 0);
    }

    #[test]
    fn test_directories_are_asked_about_only_while_something_in_them_is_new() {
        let (index, _dir) = index();
        let dir = "levels/doom2/Ports/megawads/";
        index
            .apply_listing(
                &[
                    upload(SUNLUST, 1000, "2015-08-09"),
                    upload("levels/doom2/Ports/megawads/other.zip", 1, "2015-08-09"),
                    upload("utils/tool.zip", 1, "2015-08-09"),
                ],
                &state(),
            )
            .unwrap();

        let jobs = index.directory_jobs().unwrap();
        assert_eq!(
            jobs.iter().map(|job| job.dir.as_str()).collect::<Vec<_>>(),
            [dir, "utils/"]
        );
        assert_eq!(jobs[0].waiting.len(), 2);
        assert_eq!(jobs[0].uploads, 2);

        // Answered, and it had nothing to say about `other.zip`: still done with.
        index
            .record_directory(dir, &[api_entry(SUNLUST, 18168, 1000, "S")], "2026-11-03")
            .unwrap();
        index.record_unknown_to_api("utils/tool.zip").unwrap();
        assert_eq!(index.directory_jobs().unwrap(), []);

        // Until something new turns up in it.
        index
            .apply_listing(
                &[
                    upload(SUNLUST, 1000, "2015-08-09"),
                    upload("levels/doom2/Ports/megawads/other.zip", 1, "2015-08-09"),
                    upload("levels/doom2/Ports/megawads/third.zip", 1, "2026-11-03"),
                    upload("utils/tool.zip", 1, "2015-08-09"),
                ],
                &state(),
            )
            .unwrap();
        let jobs = index.directory_jobs().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].waiting, ["levels/doom2/Ports/megawads/third.zip"]);
        assert_eq!(jobs[0].uploads, 3);
    }

    /// A new upload has no votes on the day it is first asked about.
    #[test]
    fn test_directories_come_round_again_oldest_first() {
        let (index, _dir) = index();
        index
            .apply_listing(
                &[
                    upload("levels/a/x.zip", 1, "2015-08-09"),
                    upload("levels/b/y.zip", 1, "2015-08-09"),
                ],
                &state(),
            )
            .unwrap();
        index
            .record_directory("levels/a/", &[], "2026-05-01")
            .unwrap();
        index
            .record_directory("levels/b/", &[], "2026-01-01")
            .unwrap();

        assert_eq!(
            index.directories_to_refresh("2026-10-01", 1).unwrap(),
            ["levels/b/"]
        );
        assert_eq!(
            index.directories_to_refresh("2026-10-01", 5).unwrap(),
            ["levels/b/", "levels/a/"]
        );
        assert_eq!(
            index.directories_to_refresh("2026-02-01", 5).unwrap(),
            ["levels/b/"]
        );
    }

    fn searchable(index: &Index, path: &str, date: &str, title: &str, author: &str, more: &str) {
        let (dir, filename) = path.rsplit_once('/').unwrap();
        let mut existing: Vec<ListedUpload> = index
            .conn
            .prepare("SELECT path, size, date FROM uploads")
            .unwrap()
            .query_map([], |row| {
                Ok(upload(
                    &row.get::<_, String>(0)?,
                    row.get(1)?,
                    &row.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        existing.push(ListedUpload {
            dir: format!("{dir}/"),
            filename: filename.to_string(),
            size: 1,
            date: date.to_string(),
            text: None,
        });
        index.apply_listing(&existing, &state()).unwrap();
        index
            .record_text(
                &job(path),
                &TextFile {
                    title: title.to_string(),
                    author: author.to_string(),
                    description: more.to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
    }

    fn found(index: &Index, query: &str) -> Vec<String> {
        index
            .search(query, 50)
            .unwrap()
            .into_iter()
            .map(|entry| entry.filename)
            .collect()
    }

    #[test]
    fn test_a_name_outranks_a_mention() {
        let (index, _dir) = index();
        searchable(
            &index,
            "levels/doom2/mention.zip",
            "2026-01-01",
            "Other",
            "A",
            "a hell of a time",
        );
        searchable(
            &index,
            "levels/doom2/byauthor.zip",
            "2026-01-01",
            "Other",
            "Hell Fan",
            "",
        );
        searchable(
            &index,
            "levels/doom2/hellish.zip",
            "2001-01-01",
            "Other",
            "A",
            "",
        );
        searchable(
            &index,
            "levels/doom2/intitle.zip",
            "2001-01-01",
            "Gates of Hell",
            "A",
            "",
        );
        searchable(
            &index,
            "levels/doom2/starts.zip",
            "2001-01-01",
            "Hell Revealed",
            "A",
            "",
        );
        searchable(
            &index,
            "levels/doom2/exact.zip",
            "2001-01-01",
            "Hell",
            "A",
            "",
        );

        assert_eq!(
            found(&index, "hell"),
            [
                "exact.zip",
                "hellish.zip",
                "starts.zip",
                "intitle.zip",
                "byauthor.zip",
                "mention.zip",
            ]
        );
    }

    #[test]
    fn test_all_the_words_have_to_be_there() {
        let (index, _dir) = index();
        searchable(
            &index,
            "levels/doom2/a.zip",
            "2001-01-01",
            "Ancient Aliens",
            "A",
            "",
        );
        searchable(
            &index,
            "levels/doom2/b.zip",
            "2001-01-01",
            "Ancient Hatred",
            "A",
            "",
        );

        assert_eq!(found(&index, "ancient aliens"), ["a.zip"]);
        assert_eq!(found(&index, "aliens ancient"), ["a.zip"]);
        assert_eq!(found(&index, "anc"), ["a.zip", "b.zip"]);
        assert!(found(&index, "ancient mountain").is_empty());
    }

    #[test]
    fn test_wads_come_before_the_rest_of_the_archive_and_then_newest() {
        let (index, _dir) = index();
        searchable(
            &index,
            "lmps/doom2/scythe-a.zip",
            "2026-01-01",
            "A",
            "Scythe Fan",
            "",
        );
        searchable(
            &index,
            "levels/doom2/scythe-old.zip",
            "2001-01-01",
            "B",
            "Scythe Fan",
            "",
        );
        searchable(
            &index,
            "levels/doom2/scythe-new.zip",
            "2010-01-01",
            "C",
            "Scythe Fan",
            "",
        );

        assert_eq!(
            found(&index, "scythe"),
            ["scythe-new.zip", "scythe-old.zip", "scythe-a.zip"]
        );
    }

    #[test]
    fn test_a_search_ignores_case_accents_and_punctuation() {
        let (index, _dir) = index();
        searchable(
            &index,
            "levels/doom2/ad_mortem.zip",
            "2001-01-01",
            "Santa's Bender",
            "Zoltán Sófalvi",
            "",
        );
        for query in [
            "SANTA'S",
            "santas",
            "sofalvi",
            "Zoltán",
            "ZOLTAN",
            "ad_mortem",
            "admortem",
            "ad_mortem.zip",
        ] {
            assert_eq!(found(&index, query), ["ad_mortem.zip"], "{query}");
        }
    }

    /// `normalize_title` strips these, which is right for comparing two titles and would
    /// be wrong here: a query of nothing but Cyrillic would have nothing left to look for.
    #[test]
    fn test_a_title_in_another_script_can_be_searched_for() {
        let (index, _dir) = index();
        searchable(
            &index,
            "levels/doom2/a.zip",
            "2001-01-01",
            "Ад на Земле",
            "A",
            "",
        );
        assert_eq!(found(&index, "ад"), ["a.zip"]);
        assert_eq!(found(&index, "АД ЗЕМЛЕ"), ["a.zip"]);
    }

    #[test]
    fn test_nothing_to_search_for_finds_nothing() {
        let (index, _dir) = index();
        searchable(&index, "levels/doom2/a.zip", "2001-01-01", "Hell", "A", "");
        assert!(found(&index, "").is_empty());
        assert!(found(&index, "   ").is_empty());
        // Would be every row, if `%` got through to LIKE.
        assert!(found(&index, "%").is_empty());
        assert!(found(&index, "_").is_empty());
    }

    #[test]
    fn test_a_search_has_a_limit() {
        let (index, _dir) = index();
        for n in 0..5 {
            searchable(
                &index,
                &format!("levels/doom2/m{n}.zip"),
                "2001-01-01",
                "Map",
                "A",
                "",
            );
        }
        assert_eq!(index.search("map", 3).unwrap().len(), 3);
    }

    #[test]
    fn test_the_newest_uploads_come_first() {
        let (index, _dir) = index();
        index
            .apply_listing(
                &[
                    upload("levels/doom2/old.zip", 1, "2001-01-01"),
                    upload("levels/doom2/new.zip", 1, "2026-11-01"),
                    upload("combos/mid.zip", 1, "2015-06-01"),
                ],
                &state(),
            )
            .unwrap();
        let names: Vec<String> = index
            .newest(2)
            .unwrap()
            .into_iter()
            .map(|entry| entry.filename)
            .collect();
        assert_eq!(names, ["new.zip", "mid.zip"]);
    }

    #[test]
    fn test_being_mostly_described_is_complete_enough() {
        let status = |uploads, waiting_for_text| IndexStatus {
            uploads,
            waiting_for_text,
            ..Default::default()
        };
        assert!(!status(0, 0).is_complete());
        assert!(status(21_000, 0).is_complete());
        // A day's new uploads, about to be read.
        assert!(status(21_000, 15).is_complete());
        assert!(!status(21_000, 20_000).is_complete());
    }

    #[test]
    fn test_an_index_comes_back_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        {
            let index = Index::open(&path).unwrap();
            index
                .apply_listing(&[upload(SUNLUST, 1, "2020-01-01")], &state())
                .unwrap();
        }
        let index = Index::open(&path).unwrap();
        assert_eq!(index.status().unwrap().uploads, 1);
    }

    #[test]
    fn test_a_damaged_index_is_replaced_by_an_empty_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        std::fs::write(&path, "this has never been a database, or not for long").unwrap();

        let index = Index::open(&path).unwrap();

        assert!(index.status().unwrap().is_empty());
    }

    #[test]
    fn test_an_index_from_another_version_is_replaced_by_an_empty_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        {
            let index = Index::open(&path).unwrap();
            index
                .apply_listing(&[upload(SUNLUST, 1, "2020-01-01")], &state())
                .unwrap();
            index
                .conn
                .execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 1))
                .unwrap();
        }

        let index = Index::open(&path).unwrap();

        assert!(index.status().unwrap().is_empty());
    }

    #[test]
    fn test_opening_makes_the_directory_it_goes_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not/there/yet/index.db");
        assert!(Index::open(&path).is_ok());
    }
}
