use std::path::Path;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use rusqlite::Connection;
use unicode_normalization::UnicodeNormalization;

static PUNCTUATION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9\s]").unwrap());

use crate::doomwiki::DoomwikiClient;
use crate::doomworld::ForumThread;
use crate::idgames::{
    FileEntry, IdgamesClient, extract_idgames_file_path_from_url, extract_idgames_id_from_url,
};

use caco_core::db::{self, NewWad, SourceType, WadUpdate};
use caco_core::sourceports;

/// Result of an import attempt.
///
/// Callers check `is_duplicate` first. If true and `force` was not set,
/// the import was skipped. Otherwise `wad_id` contains the new WAD ID.
#[derive(Debug, Clone)]
pub struct ImportResult {
    pub wad_id: Option<i64>,
    pub is_duplicate: bool,
    pub duplicate_id: Option<i64>,
    pub duplicate_title: Option<String>,
    pub error: Option<String>,
}

impl ImportResult {
    pub fn ok(&self) -> bool {
        self.wad_id.is_some() && self.error.is_none()
    }

    fn success(wad_id: i64) -> Self {
        Self {
            wad_id: Some(wad_id),
            is_duplicate: false,
            duplicate_id: None,
            duplicate_title: None,
            error: None,
        }
    }

    fn duplicate(id: i64, title: &str) -> Self {
        Self {
            wad_id: None,
            is_duplicate: true,
            duplicate_id: Some(id),
            duplicate_title: Some(title.to_string()),
            error: None,
        }
    }

    fn error(msg: impl Into<String>) -> Self {
        Self {
            wad_id: None,
            is_duplicate: false,
            duplicate_id: None,
            duplicate_title: None,
            error: Some(msg.into()),
        }
    }
}

pub use caco_core::utils::normalize_tags;

/// Normalize a title for fuzzy comparison.
///
/// Lowercase, strip accents/diacritics (NFD decomposition), remove
/// punctuation, collapse whitespace.
pub fn normalize_title(title: &str) -> String {
    let lower = title.to_lowercase();
    // Decompose unicode and strip combining marks (accents)
    let stripped: String = lower
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect();
    // Remove punctuation (keep alphanumeric and spaces)
    let cleaned = PUNCTUATION_RE.replace_all(&stripped, "");
    // Collapse whitespace
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Check if two titles match after normalization.
pub fn titles_match(a: &str, b: &str) -> bool {
    normalize_title(a) == normalize_title(b)
}

/// Handles duplicate checking and WAD import for all source types.
///
/// Holds shared HTTP clients so auto-enrichment and auto-link passes
/// don't re-construct a `reqwest::Client` (and its TLS state) on every
/// import. Clone is cheap — the clients are Arc'd.
#[derive(Clone)]
pub struct ImportService {
    doomwiki: Arc<DoomwikiClient>,
    idgames: Arc<IdgamesClient>,
}

impl Default for ImportService {
    fn default() -> Self {
        Self::new()
    }
}

impl ImportService {
    /// Construct a fresh service with newly-built HTTP clients.
    pub fn new() -> Self {
        Self {
            doomwiki: Arc::new(DoomwikiClient::new()),
            idgames: Arc::new(IdgamesClient::new()),
        }
    }

    /// Import from idgames archive.
    ///
    /// Duplicate detection: source_id + filename + author.
    pub fn import_idgames(
        &self,
        conn: &Connection,
        entry: &FileEntry,
        tags: Option<Vec<String>>,
        force: bool,
    ) -> ImportResult {
        // Check for duplicates
        if let Ok(Some(existing)) = db::find_duplicate(
            conn,
            SourceType::Idgames,
            Some(&entry.id.to_string()),
            None,
            Some(&entry.filename),
            Some(&entry.author),
        ) && !force
        {
            return ImportResult::duplicate(existing.id, &existing.title);
        }

        // Build and insert the WAD
        let mut wad =
            NewWad::new(&entry.title, SourceType::Idgames).source_id(entry.id.to_string());

        if !entry.author.is_empty() {
            wad = wad.author(&entry.author);
        }
        if !entry.date.is_empty()
            && let Some(y) = caco_core::utils::extract_year(&entry.date)
        {
            wad = wad.year(y);
        }
        if !entry.description.is_empty() {
            wad = wad.description(&entry.description);
        }
        if !entry.filename.is_empty() {
            wad = wad.filename(&entry.filename);
        }
        let url = format!(
            "https://www.doomworld.com/idgames/{}{}",
            entry.dir.trim_matches('/'),
            if entry.dir.is_empty() || entry.dir.ends_with('/') {
                ""
            } else {
                "/"
            }
        );
        // Use the idgamesurl or construct from dir
        if !entry.url.is_empty() {
            wad = wad.source_url(&entry.url);
        } else if !entry.dir.is_empty() {
            wad = wad.source_url(url);
        }
        if let Some(t) = tags {
            wad = wad.tags(t);
        }

        let result = db::with_transaction(conn, |tx| {
            let wad_id = db::add_wad(tx, &wad)?;
            // Auto-enrich with Doom Wiki metadata inside the same transaction
            // so a mid-enrich failure rolls the WAD insert back.
            self.auto_enrich_doomwiki(tx, wad_id, &entry.title);
            Ok(wad_id)
        });

        match result {
            Ok(wad_id) => ImportResult::success(wad_id),
            Err(e) => ImportResult::error(e.to_string()),
        }
    }

    /// Import from Doom Wiki.
    ///
    /// Duplicate detection: source_id (page_id).
    pub fn import_doomwiki(
        &self,
        conn: &Connection,
        entry: &crate::doomwiki::WikiEntry,
        tags: Option<Vec<String>>,
        force: bool,
    ) -> ImportResult {
        if let Ok(Some(existing)) = db::find_duplicate(
            conn,
            SourceType::Doomwiki,
            Some(&entry.page_id.to_string()),
            None,
            None,
            None,
        ) && !force
        {
            return ImportResult::duplicate(existing.id, &existing.title);
        }

        let display = entry.display_name().to_string();
        let mut wad =
            NewWad::new(&display, SourceType::Doomwiki).source_id(entry.page_id.to_string());

        if !entry.author.is_empty() {
            wad = wad.author(&entry.author);
        }
        if let Some(y) = entry.year {
            wad = wad.year(y);
        }
        if !entry.description.is_empty() {
            wad = wad.description(&entry.description);
        }
        if !entry.wiki_url.is_empty() {
            wad = wad.source_url(&entry.wiki_url);
        }
        if let Some(t) = tags {
            wad = wad.tags(t);
        }

        let result = db::with_transaction(conn, |tx| {
            let wad_id = db::add_wad(tx, &wad)?;
            if !entry.iwad.is_empty() {
                auto_link_iwad(tx, wad_id, &entry.iwad);
            }
            if !entry.port.is_empty() {
                auto_link_complevel(tx, wad_id, &entry.port);
                auto_link_sourceport_family(tx, wad_id, &entry.port);
                auto_link_zdoom_required(tx, wad_id, &entry.port);
            }
            if !entry.link.is_empty() {
                self.auto_link_idgames_from_url(tx, wad_id, &entry.link);
            }
            Ok(wad_id)
        });

        match result {
            Ok(wad_id) => ImportResult::success(wad_id),
            Err(e) => ImportResult::error(e.to_string()),
        }
    }

    /// Import from a Doomworld forum thread.
    ///
    /// Duplicate detection: source_id (thread_id).
    #[allow(clippy::too_many_arguments)]
    pub fn import_doomworld(
        &self,
        conn: &Connection,
        thread: &ForumThread,
        tags: Option<Vec<String>>,
        title: Option<&str>,
        author: Option<&str>,
        year: Option<i32>,
        force: bool,
    ) -> ImportResult {
        // Check for duplicates
        if let Ok(Some(existing)) = db::find_duplicate(
            conn,
            SourceType::Doomworld,
            Some(&thread.thread_id.to_string()),
            None,
            None,
            None,
        ) && !force
        {
            return ImportResult::duplicate(existing.id, &existing.title);
        }

        // Use provided values or fall back to thread data
        let final_title = title.unwrap_or(&thread.title);
        let final_author = author
            .filter(|a| !a.is_empty())
            .or(if thread.author.is_empty() {
                None
            } else {
                Some(thread.author.as_str())
            });
        let final_year = year.or_else(|| caco_core::utils::extract_year(&thread.posted_date));

        // Use first post text as description, truncated on a paragraph break
        // (or word boundary) so we don't cut mid-sentence.
        let description = if thread.first_post_text.is_empty() {
            None
        } else if thread.first_post_text.len() > 2000 {
            Some(truncate_description(&thread.first_post_text, 2000))
        } else {
            Some(thread.first_post_text.clone())
        };

        let mut wad =
            NewWad::new(final_title, SourceType::Doomworld).source_id(thread.thread_id.to_string());

        if let Some(a) = final_author {
            wad = wad.author(a);
        }
        if let Some(y) = final_year {
            wad = wad.year(y);
        }
        if let Some(ref d) = description {
            wad = wad.description(d);
        }
        if !thread.thread_url.is_empty() {
            wad = wad.source_url(&thread.thread_url);
        }
        if let Some(v) = thread.version.as_deref() {
            wad = wad.version(v);
        }
        if let Some(t) = tags {
            wad = wad.tags(t);
        }

        let result = db::with_transaction(conn, |tx| {
            let wad_id = db::add_wad(tx, &wad)?;

            if let Some(cl) = thread.complevel {
                let update = WadUpdate::new().set_int("complevel", Some(cl as i64));
                db::update_wad(tx, wad_id, &update)?;
            }

            // Persist thread-extracted IWAD and compatibility metadata.
            // Detected ports describe what family can run the WAD; they are
            // not user-selected executable overrides.
            if let Some(iwad) = thread.iwad.as_deref() {
                auto_link_iwad(tx, wad_id, iwad);
            }
            if let Some(port) = thread.sourceport.as_deref() {
                auto_link_sourceport_family(tx, wad_id, port);
                auto_link_zdoom_required(tx, wad_id, port);
            }

            // Persist download links. Previously these were counted in the
            // CLI output and thrown away, leaving the user no way to recover
            // them if auto-caching wasn't wired up.
            if !thread.download_links.is_empty()
                && let Ok(json) = serde_json::to_string(&thread.download_links)
            {
                let update = WadUpdate::new().set_text("download_urls", Some(json));
                db::update_wad(tx, wad_id, &update)?;
            }

            // Auto-enrich with Doom Wiki metadata
            self.auto_enrich_doomwiki(tx, wad_id, final_title);
            Ok(wad_id)
        });

        match result {
            Ok(wad_id) => ImportResult::success(wad_id),
            Err(e) => ImportResult::error(e.to_string()),
        }
    }

    /// Import from a direct URL.
    ///
    /// Duplicate detection: source_url.
    #[allow(clippy::too_many_arguments)]
    pub fn import_url(
        &self,
        conn: &Connection,
        title: &str,
        url: &str,
        author: Option<&str>,
        year: Option<i32>,
        description: Option<&str>,
        tags: Option<Vec<String>>,
        force: bool,
    ) -> ImportResult {
        if let Ok(Some(existing)) =
            db::find_duplicate(conn, SourceType::Url, None, Some(url), None, None)
            && !force
        {
            return ImportResult::duplicate(existing.id, &existing.title);
        }

        let mut wad = NewWad::new(title, SourceType::Url).source_url(url);
        if let Some(a) = author {
            wad = wad.author(a);
        }
        if let Some(y) = year {
            wad = wad.year(y);
        }
        if let Some(d) = description {
            wad = wad.description(d);
        }
        if let Some(t) = tags {
            wad = wad.tags(t);
        }

        let result = db::with_transaction(conn, |tx| {
            let wad_id = db::add_wad(tx, &wad)?;
            self.auto_enrich_doomwiki(tx, wad_id, title);
            Ok(wad_id)
        });

        match result {
            Ok(wad_id) => ImportResult::success(wad_id),
            Err(e) => ImportResult::error(e.to_string()),
        }
    }

    /// Import a local file.
    ///
    /// Duplicate detection: source_url (the resolved file path).
    #[allow(clippy::too_many_arguments)]
    pub fn import_local(
        &self,
        conn: &Connection,
        title: &str,
        path: &Path,
        author: Option<&str>,
        year: Option<i32>,
        description: Option<&str>,
        tags: Option<Vec<String>>,
        force: bool,
    ) -> ImportResult {
        let resolved = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => return ImportResult::error(format!("cannot resolve path: {e}")),
        };
        let source_url = resolved.to_string_lossy().to_string();

        if let Ok(Some(existing)) =
            db::find_duplicate(conn, SourceType::Local, None, Some(&source_url), None, None)
            && !force
        {
            return ImportResult::duplicate(existing.id, &existing.title);
        }

        let mut wad = NewWad::new(title, SourceType::Local).source_url(&source_url);

        if let Some(filename) = resolved.file_name().and_then(|f| f.to_str())
            && resolved.extension().is_some()
        {
            wad = wad.filename(filename);
        }
        if resolved.exists() {
            wad = wad.cached_path(source_url.clone());
        }
        if let Some(a) = author {
            wad = wad.author(a);
        }
        if let Some(y) = year {
            wad = wad.year(y);
        }
        if let Some(d) = description {
            wad = wad.description(d);
        }
        if let Some(t) = tags {
            wad = wad.tags(t);
        }

        let result = db::with_transaction(conn, |tx| {
            let wad_id = db::add_wad(tx, &wad)?;
            self.auto_enrich_doomwiki(tx, wad_id, title);
            Ok(wad_id)
        });

        match result {
            Ok(wad_id) => ImportResult::success(wad_id),
            Err(e) => ImportResult::error(e.to_string()),
        }
    }

    /// Auto-enrich a WAD with Doom Wiki metadata after import.
    ///
    /// Searches Doom Wiki for a matching title and backfills missing fields.
    /// Never overwrites existing author/year/custom_iwad.
    /// Silently ignores all errors.
    fn auto_enrich_doomwiki(&self, conn: &Connection, wad_id: i64, title: &str) {
        // Check config flag
        let cfg = caco_core::config::load_config();
        if !cfg.auto_doomwiki_enrich {
            return;
        }

        let result: std::result::Result<(), Box<dyn std::error::Error>> = (|| {
            let results = self.doomwiki.search_wads(title, 5)?;
            if results.is_empty() {
                return Ok(());
            }

            // Find first result with matching title
            let entry = results
                .iter()
                .find(|r| titles_match(title, r.display_name()));
            let entry = match entry {
                Some(e) => e,
                None => return Ok(()),
            };

            let wad = match db::get_wad(conn, wad_id, false)? {
                Some(w) => w,
                None => return Ok(()),
            };

            let mut update = WadUpdate::new();
            let mut has_changes = false;

            // Fill missing fields (never overwrite)
            if wad.author.is_none() && !entry.author.is_empty() {
                update = update.set_text("author", Some(entry.author.clone()));
                has_changes = true;
            }
            if wad.year.is_none() && entry.year.is_some() {
                update = update.set_int("year", entry.year.map(|y| y as i64));
                has_changes = true;
            }

            // Append wiki description
            if !entry.description.is_empty() {
                let existing = wad.description.as_deref().unwrap_or("");
                let new_desc = if existing.is_empty() {
                    entry.description.clone()
                } else {
                    format!("{existing}\n\n---\nFrom Doom Wiki:\n{}", entry.description)
                };
                update = update.set_text("description", Some(new_desc));
                has_changes = true;
            }

            if has_changes {
                db::update_wad(conn, wad_id, &update)?;
            }

            // Auto-link IWAD if wiki entry has one
            if !entry.iwad.is_empty() {
                auto_link_iwad(conn, wad_id, &entry.iwad);
            }

            // Auto-set compatibility metadata from port field
            if !entry.port.is_empty() {
                auto_link_sourceport_family(conn, wad_id, &entry.port);
                auto_link_zdoom_required(conn, wad_id, &entry.port);
            }

            Ok(())
        })();

        // Silently ignore errors
        let _ = result;
    }
}

/// Map detected sourceport text to a known sourceport family.
pub fn port_to_sourceport_family(port_text: &str) -> Option<&'static str> {
    let text = port_text.trim();
    if text.is_empty() {
        return None;
    }

    if let Some(family) = sourceports::family_name(text) {
        return Some(family);
    }

    let lower = text.to_lowercase();
    if let Some(family) = sourceports::family_name(&lower) {
        return Some(family);
    }

    for family in sourceports::FAMILIES {
        if lower == family.name {
            return Some(family.name);
        }
        if family.executables.iter().any(|exe| lower.contains(exe)) {
            return Some(family.name);
        }
    }

    if port_to_zdoom_required(&lower) == Some(true) {
        return Some("zdoom");
    }

    None
}

/// Truncate description text on the nearest paragraph/word boundary at or
/// before `max_len` bytes, suffixed with `...` when content is trimmed.
fn truncate_description(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    let budget = max_len.saturating_sub(3); // reserve room for the ellipsis
    let slice = &text[..text.floor_char_boundary(budget)];
    let cut = slice
        .rfind("\n\n")
        .or_else(|| slice.rfind('\n'))
        .or_else(|| slice.rfind(". "))
        .or_else(|| slice.rfind(' '))
        .unwrap_or(slice.len());
    format!("{}...", slice[..cut].trim_end())
}

/// Map a Doom Wiki "port" field string to a complevel integer.
///
/// Uses substring matching against known port requirement keywords.
/// Returns `None` if the port text doesn't match any known pattern.
pub fn port_to_complevel(port_text: &str) -> Option<i32> {
    // When a wiki infobox lists multiple ports (e.g. Ancient Aliens'
    // `port=Boom-compatible, port2=MBF21-compatible`), prefer the highest
    // compat level — running MBF21 content at Boom complevel loses features.
    let mapping = [
        ("mbf21", 21),
        ("mbf", 11),
        ("boom", 9),
        ("vanilla", 2),
        ("limit-removing", 2),
        ("limit removing", 2),
    ];

    let text = port_text.to_lowercase();
    let mut best: Option<i32> = None;
    for (key, cl) in &mapping {
        if text.contains(key) {
            best = Some(best.map_or(*cl, |b: i32| b.max(*cl)));
        }
    }
    best
}

/// Map a Doom Wiki "port" field string to a zdoom_required boolean.
///
/// Returns `Some(true)` if the port text indicates a ZDoom-family sourceport,
/// `None` if inconclusive.
pub fn port_to_zdoom_required(port_text: &str) -> Option<bool> {
    let text = port_text.to_lowercase();
    let zdoom_keywords = [
        "zdoom",
        "gzdoom",
        "uzdoom",
        "lzdoom",
        "vkdoom",
        "qzdoom",
        "zandronum",
        "skulltag",
    ];
    for kw in &zdoom_keywords {
        if text.contains(kw) {
            return Some(true);
        }
    }
    None
}

/// Auto-set required sourceport family from detected sourceport metadata.
fn auto_link_sourceport_family(conn: &Connection, wad_id: i64, port_text: &str) {
    if let Some(family) = port_to_sourceport_family(port_text)
        && let Ok(Some(wad)) = db::get_wad(conn, wad_id, false)
        && wad.required_sourceport_family.is_none()
    {
        let update =
            WadUpdate::new().set_text("required_sourceport_family", Some(family.to_string()));
        let _ = db::update_wad(conn, wad_id, &update);
    }
}

/// Auto-set zdoom_required based on Doom Wiki "port" field heuristic.
fn auto_link_zdoom_required(conn: &Connection, wad_id: i64, port_text: &str) {
    if let Some(true) = port_to_zdoom_required(port_text)
        && let Ok(Some(wad)) = db::get_wad(conn, wad_id, false)
    {
        let mut update = WadUpdate::new();
        let mut has_changes = false;
        if wad.zdoom_required.is_none() {
            update = update.set_int("zdoom_required", Some(1));
            has_changes = true;
        }
        if wad.required_sourceport_family.is_none() {
            update = update.set_text("required_sourceport_family", Some("zdoom".to_string()));
            has_changes = true;
        }
        if has_changes {
            let _ = db::update_wad(conn, wad_id, &update);
        }
    }
}

/// Auto-set complevel based on Doom Wiki "port" field heuristic.
fn auto_link_complevel(conn: &Connection, wad_id: i64, port_text: &str) {
    if let Some(cl) = port_to_complevel(port_text)
        && let Ok(Some(wad)) = db::get_wad(conn, wad_id, false)
        && wad.complevel.is_none()
    {
        let update = WadUpdate::new().set_int("complevel", Some(cl as i64));
        let _ = db::update_wad(conn, wad_id, &update);
    }
}

/// Apply a resolved idgames id (and optional filename) to a WAD record.
///
/// Skips the update when the WAD already carries an `idgames_id`. Backfills
/// the `filename` column only when it's currently empty.
fn apply_idgames_link(conn: &Connection, wad_id: i64, idgames_id: i64, filename: Option<&str>) {
    let Ok(Some(wad)) = db::get_wad(conn, wad_id, false) else {
        return;
    };
    if wad.idgames_id.is_some() {
        return;
    }

    let mut update = WadUpdate::new().set_text("idgames_id", Some(idgames_id.to_string()));

    if let Some(name) = filename
        && !name.is_empty()
        && wad.filename.is_none()
    {
        update = update.set_text("filename", Some(name.to_string()));
    }

    let _ = db::update_wad(conn, wad_id, &update);
}

impl ImportService {
    /// Resolve an idgames link parsed off a Doom Wiki page and stamp it onto a WAD.
    ///
    /// Two URL shapes are accepted: query-string ids (`/idgames/?id=N`) yield the
    /// id directly without a network round-trip; path-style URLs (`/idgames/<path>`,
    /// produced by the wiki's `{{ig|file=...}}` template) trigger an idgames API
    /// lookup so the resolved id and filename can be persisted. Network and
    /// parser errors are silently swallowed so a flaky idgames API never blocks
    /// the wiki import.
    fn auto_link_idgames_from_url(&self, conn: &Connection, wad_id: i64, link: &str) {
        if let Some(id) = extract_idgames_id_from_url(link) {
            // Pull the full idgames entry so we can backfill filename too —
            // otherwise `{{ig|id=N}}` wiki links leave `filename` null even
            // though the archive knows it. On API error, persist id only.
            let filename = self
                .idgames
                .get(Some(id), None)
                .ok()
                .filter(|e| !e.filename.is_empty())
                .map(|e| e.filename);
            apply_idgames_link(conn, wad_id, id, filename.as_deref());
            return;
        }

        let Some(file_path) = extract_idgames_file_path_from_url(link) else {
            return;
        };

        if let Ok(entry) = self.idgames.get_by_path(&file_path) {
            let filename = if entry.filename.is_empty() {
                None
            } else {
                Some(entry.filename.as_str())
            };
            apply_idgames_link(conn, wad_id, entry.id, filename);
        }
    }
}

/// Auto-set custom_iwad on a WAD if the IWAD name is registered.
fn auto_link_iwad(conn: &Connection, wad_id: i64, iwad_text: &str) {
    let short_name = match db::normalize_iwad_name(iwad_text) {
        Some(name) => name,
        None => return,
    };

    // Only set if the IWAD is registered in the database
    if db::get_iwad(conn, short_name, None)
        .ok()
        .flatten()
        .is_none()
    {
        return;
    }

    // Only set if the WAD doesn't already have a custom_iwad
    if let Ok(Some(wad)) = db::get_wad(conn, wad_id, false)
        && wad.custom_iwad.is_none()
    {
        let update = WadUpdate::new().set_text("custom_iwad", Some(short_name.to_string()));
        let _ = db::update_wad(conn, wad_id, &update);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{init_db, open_memory};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_normalize_tags_comma_separated() {
        let result = normalize_tags(Some("cacoward, megawad, doom")).unwrap();
        assert_eq!(result, vec!["cacoward", "megawad", "doom"]);
    }

    #[test]
    fn test_port_to_sourceport_family() {
        assert_eq!(port_to_sourceport_family("DSDA-Doom"), Some("dsda"));
        assert_eq!(port_to_sourceport_family("nyan-doom"), Some("dsda"));
        assert_eq!(port_to_sourceport_family("GZDoom"), Some("zdoom"));
        assert_eq!(
            port_to_sourceport_family("UZDoom-compatible"),
            Some("zdoom")
        );
        assert_eq!(port_to_sourceport_family("Boom-compatible"), None);
    }

    #[test]
    fn test_port_to_complevel_single_keyword() {
        assert_eq!(port_to_complevel("Boom-compatible"), Some(9));
        assert_eq!(port_to_complevel("MBF-compatible"), Some(11));
        assert_eq!(port_to_complevel("MBF21-compatible"), Some(21));
        assert_eq!(port_to_complevel("vanilla"), Some(2));
        assert_eq!(port_to_complevel("limit-removing"), Some(2));
        assert_eq!(port_to_complevel("limit removing"), Some(2));
        assert_eq!(port_to_complevel("GZDoom"), None);
    }

    #[test]
    fn test_port_to_complevel_picks_highest() {
        // Combined port+port2 string. The higher compat level wins so
        // content requiring MBF21 features still plays correctly.
        assert_eq!(
            port_to_complevel("Boom-compatible, MBF21-compatible"),
            Some(21)
        );
        assert_eq!(
            port_to_complevel("Boom-compatible, MBF-compatible"),
            Some(11)
        );
        // "mbf21" is a superstring of "mbf"; make sure we return 21, not 11.
        assert_eq!(port_to_complevel("MBF21-compatible"), Some(21));
    }

    #[test]
    fn test_normalize_tags_whitespace() {
        let result = normalize_tags(Some("  tag1 , TAG2 , ")).unwrap();
        assert_eq!(result, vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_normalize_tags_none() {
        assert!(normalize_tags(None).is_none());
    }

    #[test]
    fn test_normalize_tags_empty() {
        assert!(normalize_tags(Some("")).is_none());
        assert!(normalize_tags(Some(" , , ")).is_none());
    }

    #[test]
    fn test_normalize_title_basic() {
        assert_eq!(normalize_title("Scythe"), "scythe");
        assert_eq!(normalize_title("SCYTHE"), "scythe");
    }

    #[test]
    fn test_normalize_title_accents() {
        assert_eq!(normalize_title("Café"), "cafe");
        assert_eq!(normalize_title("über"), "uber");
    }

    #[test]
    fn test_normalize_title_punctuation() {
        assert_eq!(
            normalize_title("Doom II: Hell on Earth"),
            "doom ii hell on earth"
        );
        assert_eq!(normalize_title("TNT: Evilution"), "tnt evilution");
    }

    #[test]
    fn test_normalize_title_whitespace() {
        assert_eq!(normalize_title("  Too   Much   Space  "), "too much space");
    }

    #[test]
    fn test_titles_match() {
        assert!(titles_match("Scythe", "scythe"));
        assert!(titles_match(
            "Doom II: Hell on Earth",
            "doom ii hell on earth"
        ));
        assert!(!titles_match("Scythe", "Scythe 2"));
    }

    #[test]
    fn test_import_url() {
        let conn = setup();
        let svc = ImportService::new();

        let result = svc.import_url(
            &conn,
            "Test WAD",
            "https://example.com/test.zip",
            Some("Author"),
            Some(2023),
            Some("A test WAD"),
            Some(vec!["test".to_string()]),
            false,
        );

        assert!(result.ok());
        assert!(result.wad_id.is_some());

        // Verify it was inserted
        let wad = db::get_wad(&conn, result.wad_id.unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(wad.title, "Test WAD");
        assert_eq!(wad.author.as_deref(), Some("Author"));
        assert_eq!(wad.year, Some(2023));
        assert_eq!(wad.source_type, SourceType::Url);
        assert_eq!(wad.tags, vec!["test"]);
    }

    #[test]
    fn test_import_url_duplicate() {
        let conn = setup();
        let svc = ImportService::new();

        let r1 = svc.import_url(
            &conn,
            "Test",
            "https://example.com/test.zip",
            None,
            None,
            None,
            None,
            false,
        );
        assert!(r1.ok());

        // Second import of same URL should be duplicate
        let r2 = svc.import_url(
            &conn,
            "Test",
            "https://example.com/test.zip",
            None,
            None,
            None,
            None,
            false,
        );
        assert!(r2.is_duplicate);
        assert_eq!(r2.duplicate_id, r1.wad_id);
    }

    #[test]
    fn test_import_url_duplicate_force() {
        let conn = setup();
        let svc = ImportService::new();

        svc.import_url(
            &conn,
            "Test",
            "https://example.com/test.zip",
            None,
            None,
            None,
            None,
            false,
        );

        // Force should bypass duplicate check
        let r2 = svc.import_url(
            &conn,
            "Test 2",
            "https://example.com/test.zip",
            None,
            None,
            None,
            None,
            true,
        );
        assert!(r2.ok());
    }

    #[test]
    fn test_import_idgames() {
        let conn = setup();
        let svc = ImportService::new();

        let entry = FileEntry {
            id: 19312,
            title: "Sunlust".to_string(),
            dir: "levels/doom2/Ports/megawads/".to_string(),
            filename: "sunlust.zip".to_string(),
            size: 14237696,
            age: 0,
            date: "2015-09-01".to_string(),
            author: "Ribbiks & Dannebubinga".to_string(),
            email: String::new(),
            description: "A set of 32 maps.".to_string(),
            credits: String::new(),
            base: String::new(),
            buildtime: String::new(),
            editors: String::new(),
            bugs: String::new(),
            textfile: String::new(),
            rating: 4.7,
            votes: 19,
            url: "https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/sunlust"
                .to_string(),
            idgamesurl: String::new(),
            reviews: Vec::new(),
        };

        let result = svc.import_idgames(&conn, &entry, None, false);
        assert!(result.ok());

        let wad = db::get_wad(&conn, result.wad_id.unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(wad.title, "Sunlust");
        assert_eq!(wad.author.as_deref(), Some("Ribbiks & Dannebubinga"));
        assert_eq!(wad.year, Some(2015));
        assert_eq!(wad.source_type, SourceType::Idgames);
        assert_eq!(wad.source_id.as_deref(), Some("19312"));
    }

    #[test]
    fn test_import_idgames_duplicate() {
        let conn = setup();
        let svc = ImportService::new();

        let entry = FileEntry {
            id: 100,
            title: "Test".to_string(),
            dir: "levels/".to_string(),
            filename: "test.wad".to_string(),
            size: 0,
            age: 0,
            date: String::new(),
            author: "Me".to_string(),
            email: String::new(),
            description: String::new(),
            credits: String::new(),
            base: String::new(),
            buildtime: String::new(),
            editors: String::new(),
            bugs: String::new(),
            textfile: String::new(),
            rating: 0.0,
            votes: 0,
            url: String::new(),
            idgamesurl: String::new(),
            reviews: Vec::new(),
        };

        let r1 = svc.import_idgames(&conn, &entry, None, false);
        assert!(r1.ok());

        let r2 = svc.import_idgames(&conn, &entry, None, false);
        assert!(r2.is_duplicate);
    }

    #[test]
    fn test_import_doomwiki() {
        let conn = setup();
        let svc = ImportService::new();

        let entry = crate::doomwiki::WikiEntry {
            page_id: 5678,
            title: "Scythe".to_string(),
            name: "Scythe".to_string(),
            author: "Erik Alm".to_string(),
            year: Some(2003),
            iwad: "Doom II".to_string(),
            port: "Boom-compatible".to_string(),
            link: String::new(),
            description: "A popular megawad.".to_string(),
            wiki_url: "https://doomwiki.org/wiki/Scythe".to_string(),
        };

        let result = svc.import_doomwiki(&conn, &entry, None, false);
        assert!(result.ok());

        let wad = db::get_wad(&conn, result.wad_id.unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(wad.title, "Scythe");
        assert_eq!(wad.author.as_deref(), Some("Erik Alm"));
        assert_eq!(wad.year, Some(2003));
        assert_eq!(wad.source_type, SourceType::Doomwiki);
        assert_eq!(wad.source_id.as_deref(), Some("5678"));
    }

    #[test]
    fn test_auto_link_complevel() {
        let conn = setup();
        let wad = NewWad::new("Test", SourceType::Local);
        let wad_id = db::add_wad(&conn, &wad).unwrap();

        auto_link_complevel(&conn, wad_id, "Boom-compatible");

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.complevel, Some(9));
    }

    #[test]
    fn test_auto_link_complevel_mbf21() {
        let conn = setup();
        let wad = NewWad::new("Test", SourceType::Local);
        let wad_id = db::add_wad(&conn, &wad).unwrap();

        auto_link_complevel(&conn, wad_id, "MBF21");

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.complevel, Some(21));
    }

    #[test]
    fn test_auto_link_complevel_no_overwrite() {
        let conn = setup();
        let wad = NewWad::new("Test", SourceType::Local);
        let wad_id = db::add_wad(&conn, &wad).unwrap();

        // Set complevel manually first
        let update = WadUpdate::new().set_int("complevel", Some(2));
        db::update_wad(&conn, wad_id, &update).unwrap();

        // Should NOT overwrite
        auto_link_complevel(&conn, wad_id, "Boom-compatible");

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.complevel, Some(2)); // unchanged
    }

    #[test]
    fn test_apply_idgames_link_sets_id_and_filename() {
        let conn = setup();
        let new = NewWad::new("Test", SourceType::Doomwiki);
        let wad_id = db::add_wad(&conn, &new).unwrap();

        apply_idgames_link(&conn, wad_id, 18184, Some("scythe.zip"));

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.idgames_id.as_deref(), Some("18184"));
        assert_eq!(wad.filename.as_deref(), Some("scythe.zip"));
    }

    #[test]
    fn test_apply_idgames_link_does_not_overwrite_id() {
        let conn = setup();
        let new = NewWad::new("Test", SourceType::Doomwiki);
        let wad_id = db::add_wad(&conn, &new).unwrap();

        // Pre-populate idgames_id
        let update = WadUpdate::new().set_text("idgames_id", Some("99999".to_string()));
        db::update_wad(&conn, wad_id, &update).unwrap();

        apply_idgames_link(&conn, wad_id, 18184, Some("other.zip"));

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.idgames_id.as_deref(), Some("99999")); // unchanged
        assert!(wad.filename.is_none()); // also untouched
    }

    #[test]
    fn test_apply_idgames_link_preserves_existing_filename() {
        let conn = setup();
        let new = NewWad::new("Test", SourceType::Doomwiki).filename("manual.wad");
        let wad_id = db::add_wad(&conn, &new).unwrap();

        apply_idgames_link(&conn, wad_id, 18184, Some("scythe.zip"));

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.idgames_id.as_deref(), Some("18184"));
        assert_eq!(wad.filename.as_deref(), Some("manual.wad")); // preserved
    }

    #[test]
    fn test_auto_link_idgames_from_query_url() {
        // `?id=N` URLs are resolved without touching the network.
        let conn = setup();
        let new = NewWad::new("Test", SourceType::Doomwiki);
        let wad_id = db::add_wad(&conn, &new).unwrap();

        let svc = ImportService::new();
        svc.auto_link_idgames_from_url(
            &conn,
            wad_id,
            "https://www.doomworld.com/idgames/?id=18184",
        );

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.idgames_id.as_deref(), Some("18184"));
    }

    #[test]
    fn test_auto_link_idgames_ignores_non_idgames_url() {
        let conn = setup();
        let new = NewWad::new("Test", SourceType::Doomwiki);
        let wad_id = db::add_wad(&conn, &new).unwrap();

        let svc = ImportService::new();
        svc.auto_link_idgames_from_url(&conn, wad_id, "https://example.com/downloads/scythe.zip");

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert!(wad.idgames_id.is_none());
    }

    #[test]
    fn test_auto_link_iwad() {
        let conn = setup();
        // Register doom2 IWAD
        db::add_iwad(&conn, "doom2", "v1.9", "/doom2.wad", None, None).unwrap();

        let wad = NewWad::new("Test", SourceType::Local);
        let wad_id = db::add_wad(&conn, &wad).unwrap();

        auto_link_iwad(&conn, wad_id, "Doom II");

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.custom_iwad.as_deref(), Some("doom2"));
    }

    #[test]
    fn test_auto_link_iwad_not_registered() {
        let conn = setup();
        // No IWADs registered

        let wad = NewWad::new("Test", SourceType::Local);
        let wad_id = db::add_wad(&conn, &wad).unwrap();

        auto_link_iwad(&conn, wad_id, "Doom II");

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert!(wad.custom_iwad.is_none()); // should not be set
    }

    #[test]
    fn test_import_doomworld() {
        let conn = setup();
        let svc = ImportService::new();

        let thread = crate::doomworld::ForumThread {
            thread_id: 134292,
            title: "MyHouse.wad".to_string(),
            author: "MyHouseMapper".to_string(),
            posted_date: "2023-03-01".to_string(),
            first_post_html: String::new(),
            first_post_text: "A spooky Doom 2 map.".to_string(),
            thread_url: "https://www.doomworld.com/forum/topic/134292-myhousewad/".to_string(),
            download_links: vec!["https://example.com/myhouse.zip".to_string()],
            complevel: Some(9),
            iwad: Some("doom2".to_string()),
            sourceport: Some("gzdoom".to_string()),
            version: None,
        };

        let result = svc.import_doomworld(&conn, &thread, None, None, None, None, false);
        assert!(result.ok());

        let wad = db::get_wad(&conn, result.wad_id.unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(wad.title, "MyHouse.wad");
        assert_eq!(wad.author.as_deref(), Some("MyHouseMapper"));
        assert_eq!(wad.year, Some(2023));
        assert_eq!(wad.source_type, SourceType::Doomworld);
        assert_eq!(wad.source_id.as_deref(), Some("134292"));
        assert_eq!(wad.complevel, Some(9));
        assert_eq!(wad.custom_sourceport, None);
        assert_eq!(wad.required_sourceport_family.as_deref(), Some("zdoom"));
        assert_eq!(wad.zdoom_required, Some(1));
        assert_eq!(wad.description.as_deref(), Some("A spooky Doom 2 map."));
    }

    #[test]
    fn test_import_doomworld_detected_dsda_sets_required_family_not_override() {
        let conn = setup();
        let svc = ImportService::new();

        let thread = crate::doomworld::ForumThread {
            thread_id: 134293,
            title: "DSDA Map".to_string(),
            author: "Mapper".to_string(),
            posted_date: "2024-01-01".to_string(),
            first_post_html: String::new(),
            first_post_text: String::new(),
            thread_url: "https://www.doomworld.com/forum/topic/134293-dsda-map/".to_string(),
            download_links: Vec::new(),
            complevel: None,
            iwad: None,
            sourceport: Some("dsda-doom".to_string()),
            version: None,
        };

        let result = svc.import_doomworld(&conn, &thread, None, None, None, None, false);
        assert!(result.ok());

        let wad = db::get_wad(&conn, result.wad_id.unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(wad.custom_sourceport, None);
        assert_eq!(wad.required_sourceport_family.as_deref(), Some("dsda"));
    }

    #[test]
    fn test_import_doomworld_with_overrides() {
        let conn = setup();
        let svc = ImportService::new();

        let thread = crate::doomworld::ForumThread {
            thread_id: 99999,
            title: "Original Title".to_string(),
            author: "OrigAuthor".to_string(),
            posted_date: "2020-01-01".to_string(),
            first_post_html: String::new(),
            first_post_text: "Post text.".to_string(),
            thread_url: "https://www.doomworld.com/forum/topic/99999-test/".to_string(),
            download_links: Vec::new(),
            complevel: Some(21),
            iwad: None,
            sourceport: None,
            version: Some("v1.5".to_string()),
        };

        let result = svc.import_doomworld(
            &conn,
            &thread,
            Some(vec!["cacoward".to_string()]),
            Some("Override Title"),
            Some("Override Author"),
            Some(2024),
            false,
        );
        assert!(result.ok());

        let wad = db::get_wad(&conn, result.wad_id.unwrap(), false)
            .unwrap()
            .unwrap();
        // CLI overrides win for title/author/year.
        assert_eq!(wad.title, "Override Title");
        assert_eq!(wad.author.as_deref(), Some("Override Author"));
        assert_eq!(wad.year, Some(2024));
        // Version/complevel come straight from the parsed thread.
        assert_eq!(wad.version.as_deref(), Some("v1.5"));
        assert_eq!(wad.complevel, Some(21));
        assert_eq!(wad.tags, vec!["cacoward"]);
    }

    #[test]
    fn test_import_doomworld_duplicate() {
        let conn = setup();
        let svc = ImportService::new();

        let thread = crate::doomworld::ForumThread {
            thread_id: 55555,
            title: "Dup Test".to_string(),
            author: String::new(),
            posted_date: String::new(),
            first_post_html: String::new(),
            first_post_text: String::new(),
            thread_url: "https://www.doomworld.com/forum/topic/55555-dup/".to_string(),
            download_links: Vec::new(),
            complevel: None,
            iwad: None,
            sourceport: None,
            version: None,
        };

        let r1 = svc.import_doomworld(&conn, &thread, None, None, None, None, false);
        assert!(r1.ok());

        let r2 = svc.import_doomworld(&conn, &thread, None, None, None, None, false);
        assert!(r2.is_duplicate);
        assert_eq!(r2.duplicate_id, r1.wad_id);
    }

    #[test]
    fn test_import_doomworld_duplicate_force() {
        let conn = setup();
        let svc = ImportService::new();

        let thread = crate::doomworld::ForumThread {
            thread_id: 44444,
            title: "Force Test".to_string(),
            author: String::new(),
            posted_date: String::new(),
            first_post_html: String::new(),
            first_post_text: String::new(),
            thread_url: "https://www.doomworld.com/forum/topic/44444-force/".to_string(),
            download_links: Vec::new(),
            complevel: None,
            iwad: None,
            sourceport: None,
            version: None,
        };

        svc.import_doomworld(&conn, &thread, None, None, None, None, false);

        let r2 = svc.import_doomworld(&conn, &thread, None, None, None, None, true);
        assert!(r2.ok()); // force bypasses duplicate check
    }

    #[test]
    fn test_import_doomworld_long_description() {
        let conn = setup();
        let svc = ImportService::new();

        let long_text = "a".repeat(3000);
        let thread = crate::doomworld::ForumThread {
            thread_id: 33333,
            title: "Long Desc".to_string(),
            author: String::new(),
            posted_date: String::new(),
            first_post_html: String::new(),
            first_post_text: long_text,
            thread_url: "https://www.doomworld.com/forum/topic/33333-long/".to_string(),
            download_links: Vec::new(),
            complevel: None,
            iwad: None,
            sourceport: None,
            version: None,
        };

        let result = svc.import_doomworld(&conn, &thread, None, None, None, None, false);
        assert!(result.ok());

        let wad = db::get_wad(&conn, result.wad_id.unwrap(), false)
            .unwrap()
            .unwrap();
        let desc = wad.description.unwrap();
        assert!(desc.len() <= 2003); // 1997 + "..."
        assert!(desc.ends_with("..."));
    }

    #[test]
    fn test_import_result_ok() {
        let r = ImportResult::success(1);
        assert!(r.ok());
        assert!(!r.is_duplicate);
    }

    #[test]
    fn test_import_result_duplicate() {
        let r = ImportResult::duplicate(1, "Test");
        assert!(!r.ok());
        assert!(r.is_duplicate);
        assert_eq!(r.duplicate_id, Some(1));
    }

    #[test]
    fn test_import_result_error() {
        let r = ImportResult::error("something went wrong");
        assert!(!r.ok());
        assert!(!r.is_duplicate);
        assert_eq!(r.error.as_deref(), Some("something went wrong"));
    }
}
