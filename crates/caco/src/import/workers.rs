use std::path::PathBuf;
use std::thread;

use caco_sources::import_service::ImportService;

use crate::import::state::{FormKind, SearchResultEntry, SearchSource, SearchSourceData};
use crate::message::AppMessage;
use crate::workers::BackgroundSender;

/// Convert an empty string to None, non-empty to Some(String).
fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

// ---------------------------------------------------------------------------
// Search workers
// ---------------------------------------------------------------------------

pub fn spawn_search(sender: BackgroundSender, source: SearchSource, query: String) {
    thread::spawn(move || {
        let outcome = match source {
            SearchSource::Idgames => search_idgames(&query),
            SearchSource::Doomwiki => search_doomwiki(&query),
        };
        sender.send(AppMessage::SearchComplete(
            source,
            outcome.map_err(|e| e.to_string()),
        ));
    });
}

fn search_idgames(query: &str) -> caco_sources::Result<Vec<SearchResultEntry>> {
    let client = caco_sources::idgames::IdgamesClient::new();
    let entries = client.search(query, None, None, None)?;
    Ok(entries
        .into_iter()
        .map(|e| SearchResultEntry {
            title: e.title.clone(),
            author: non_empty(&e.author),
            description: non_empty(&e.description),
            source_data: SearchSourceData::Idgames {
                id: e.id,
                rating: if e.rating > 0.0 { Some(e.rating) } else { None },
                date: non_empty(&e.date),
                filename: non_empty(&e.filename),
            },
        })
        .collect())
}

fn search_doomwiki(query: &str) -> caco_sources::Result<Vec<SearchResultEntry>> {
    let client = caco_sources::doomwiki::DoomwikiClient::new();
    let entries = client.search_wads(query, 50)?;
    Ok(entries
        .into_iter()
        .map(|e| SearchResultEntry {
            title: e.title.clone(),
            author: non_empty(&e.author),
            description: non_empty(&e.description),
            source_data: SearchSourceData::Doomwiki {
                year: e.year,
                iwad: non_empty(&e.iwad),
                port: non_empty(&e.port),
            },
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Import workers
// ---------------------------------------------------------------------------

pub fn spawn_import_idgames(sender: BackgroundSender, db_path: PathBuf, source_id: String) {
    thread::spawn(move || {
        let result: Result<_, String> = (|| {
            let conn = caco_core::db::open_connection(&db_path).map_err(|e| e.to_string())?;
            let client = caco_sources::idgames::IdgamesClient::new();
            let id: i64 = source_id
                .parse()
                .map_err(|e: std::num::ParseIntError| e.to_string())?;
            let file_entry = client.get(Some(id), None).map_err(|e| e.to_string())?;
            let service = ImportService::new();
            let result = service.import_idgames(&conn, &file_entry, None, false);
            if let Some(err) = &result.error {
                return Err(err.clone());
            }
            Ok(result)
        })();
        sender.send(AppMessage::ImportComplete(result));
    });
}

pub fn spawn_import_doomwiki(sender: BackgroundSender, db_path: PathBuf, source_id: String) {
    thread::spawn(move || {
        let result: Result<_, String> = (|| {
            let conn = caco_core::db::open_connection(&db_path).map_err(|e| e.to_string())?;
            let client = caco_sources::doomwiki::DoomwikiClient::new();
            let wiki_entry = client
                .get_entry(&source_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "Wiki page not found".to_string())?;
            let service = ImportService::new();
            let result = service.import_doomwiki(&conn, &wiki_entry, None, false);
            if let Some(err) = &result.error {
                return Err(err.clone());
            }
            Ok(result)
        })();
        sender.send(AppMessage::ImportComplete(result));
    });
}

/// Import the WAD referenced by a Cacoward entry (DB pk), then link it to the
/// entry so the magazine view immediately reflects the new state.
pub fn spawn_import_cacoward(sender: BackgroundSender, db_path: PathBuf, cacoward_pk: i64) {
    thread::spawn(move || {
        let result: Result<_, String> = (|| {
            let conn = caco_core::db::open_connection(&db_path).map_err(|e| e.to_string())?;
            let record = caco_core::db::cacowards::get_cacoward(&conn, cacoward_pk)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no cacoward entry with pk {cacoward_pk}"))?;

            let import_result = import_from_any_source(&conn, &record)?;

            if let Some(err) = &import_result.error {
                return Err(err.clone());
            }

            // The import links whatever it can prove. This is for the row the
            // user actually clicked, which it can miss: a duplicate, or a page
            // whose title differs from the entry's. Never sets manual_override.
            let target_wad = import_result.wad_id.or(import_result.duplicate_id);
            if let Some(wad_id) = target_wad
                && record.wad_id != Some(wad_id)
            {
                caco_core::db::cacowards::link_wad(&conn, record.id, wad_id, false)
                    .map_err(|e| e.to_string())?;
            }

            Ok(import_result)
        })();
        sender.send(AppMessage::ImportComplete(result));
    });
}

/// Somewhere a Cacoward entry says its WAD can be found.
#[derive(Debug, PartialEq, Eq)]
enum CacowardSource {
    Idgames(i64),
    DoomworldTopic(i64),
    DoomwikiPage(String),
}

impl CacowardSource {
    /// The sources an entry names, best first. idgames, because caco can download
    /// from there unattended. Then the forum thread, which is where a Doomworld-only
    /// release's download links are. Then the wiki, which knows about the WAD but
    /// rarely where to get it.
    fn of(record: &caco_core::db::CacowardRecord) -> Vec<Self> {
        let idgames = record.idgames_id.map(Self::Idgames);
        let topic = record.doomworld_id.map(Self::DoomworldTopic);
        let page = record.doomwiki_page.clone().map(Self::DoomwikiPage);
        [idgames, topic, page].into_iter().flatten().collect()
    }

    /// Fetch the WAD from this source and put it in the library. A forum thread's
    /// title is whatever its first post calls it, so the curated title and author
    /// win there. The other sources have their own canonical ones.
    fn import(
        &self,
        conn: &rusqlite::Connection,
        service: &ImportService,
        record: &caco_core::db::CacowardRecord,
    ) -> Result<caco_sources::import_service::ImportResult, String> {
        match self {
            Self::Idgames(id) => {
                let entry = caco_sources::idgames::IdgamesClient::new()
                    .get(Some(*id), None)
                    .map_err(|e| e.to_string())?;
                Ok(service.import_idgames(conn, &entry, None, false))
            }
            Self::DoomworldTopic(topic) => {
                let url = caco_core::source_url::doomworld_topic_url(*topic);
                let thread = caco_sources::doomworld::DoomworldClient::new()
                    .get_thread(&url)
                    .map_err(|e| e.to_string())?;
                Ok(service.import_doomworld(
                    conn,
                    &thread,
                    None,
                    Some(&record.wad_title),
                    record.wad_author.as_deref(),
                    None,
                    false,
                ))
            }
            Self::DoomwikiPage(page) => {
                let title = page.replace('_', " ");
                let entry = caco_sources::doomwiki::DoomwikiClient::new()
                    .get_entry(&title)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("Doom Wiki page '{title}' not found"))?;
                Ok(service.import_doomwiki(conn, &entry, None, false))
            }
        }
    }
}

/// Fetch the entry from the best source it names, falling through to the next when
/// one fails, since which site is behind a challenge changes from day to day. If
/// they all fail, the first failure is the one to report: it is about the source
/// that would have been the most use.
fn import_from_any_source(
    conn: &rusqlite::Connection,
    record: &caco_core::db::CacowardRecord,
) -> Result<caco_sources::import_service::ImportResult, String> {
    let service = ImportService::new();
    let mut first_failure = None;
    for source in CacowardSource::of(record) {
        match source.import(conn, &service, record) {
            Ok(imported) => return Ok(imported),
            Err(failure) => {
                first_failure.get_or_insert(failure);
            }
        }
    }
    Err(first_failure.unwrap_or_else(|| {
        format!(
            "{} has no idgames id, Doomworld topic or Doom Wiki page: import it by hand",
            record.wad_title
        )
    }))
}

pub fn spawn_import_form(
    sender: BackgroundSender,
    db_path: PathBuf,
    kind: FormKind,
    values: Vec<(String, String)>,
) {
    thread::spawn(move || {
        let result: Result<_, String> = (|| {
            let conn = caco_core::db::open_connection(&db_path).map_err(|e| e.to_string())?;
            let service = ImportService::new();

            let get = |name: &str| -> String {
                values
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let get_opt = |name: &str| -> Option<String> {
                values
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.clone())
                    .filter(|v| !v.is_empty())
            };
            let tags_opt = caco_sources::import_service::normalize_tags(get_opt("tags").as_deref());

            let result = match kind {
                FormKind::Doomworld => {
                    let url = get("url");
                    let title_override = get_opt("title");
                    let author_override = get_opt("author");
                    let year_override = get_opt("year").and_then(|y| y.parse::<i32>().ok());

                    let client = caco_sources::doomworld::DoomworldClient::new();
                    let thread = client.get_thread(&url).map_err(|e| e.to_string())?;
                    service.import_doomworld(
                        &conn,
                        &thread,
                        tags_opt,
                        title_override.as_deref(),
                        author_override.as_deref(),
                        year_override,
                        false,
                    )
                }
                FormKind::Url => {
                    let title = get("title");
                    let url = get("url");
                    let author = get_opt("author");
                    let year = get_opt("year").and_then(|y| y.parse::<i32>().ok());
                    let notes = get_opt("notes");
                    service.import_url(
                        &conn,
                        &title,
                        &url,
                        author.as_deref(),
                        year,
                        notes.as_deref(),
                        tags_opt,
                        false,
                    )
                }
                FormKind::Local => {
                    let path = get("path");
                    let title = get("title");
                    let author = get_opt("author");
                    let year = get_opt("year").and_then(|y| y.parse::<i32>().ok());
                    service.import_local(
                        &conn,
                        &title,
                        &std::path::PathBuf::from(&path),
                        author.as_deref(),
                        year,
                        None,
                        tags_opt,
                        false,
                    )
                }
            };

            if let Some(err) = &result.error {
                return Err(err.clone());
            }
            Ok(result)
        })();
        sender.send(AppMessage::ImportComplete(result));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{NewCacoward, get_cacoward, init_db, open_memory, upsert_cacoward};

    fn record_with(entry: NewCacoward) -> caco_core::db::CacowardRecord {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        let id = upsert_cacoward(
            &conn,
            &NewCacoward {
                key: "k".to_string(),
                year: 2025,
                category: "winner".to_string(),
                page_order: 1,
                wad_title: "Some WAD".to_string(),
                ..entry
            },
        )
        .unwrap();
        get_cacoward(&conn, id).unwrap().unwrap()
    }

    #[test]
    fn an_entrys_sources_are_tried_best_first() {
        let record = record_with(NewCacoward {
            idgames_id: Some(1),
            doomworld_id: Some(2),
            doomwiki_page: Some("Some_WAD".to_string()),
            ..Default::default()
        });
        assert_eq!(
            CacowardSource::of(&record),
            [
                CacowardSource::Idgames(1),
                CacowardSource::DoomworldTopic(2),
                CacowardSource::DoomwikiPage("Some_WAD".to_string()),
            ]
        );
    }

    #[test]
    fn an_entry_only_offers_the_sources_it_names() {
        let record = record_with(NewCacoward {
            doomworld_id: Some(2),
            ..Default::default()
        });
        assert_eq!(
            CacowardSource::of(&record),
            [CacowardSource::DoomworldTopic(2)]
        );
    }

    #[test]
    fn an_entry_with_no_source_says_to_import_it_by_hand() {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        let record = record_with(NewCacoward::default());
        let failure = import_from_any_source(&conn, &record).err().unwrap();
        assert!(failure.contains("Some WAD"), "{failure}");
        assert!(failure.contains("import it by hand"), "{failure}");
    }
}
