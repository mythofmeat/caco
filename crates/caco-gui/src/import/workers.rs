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
        let results = match source {
            SearchSource::Idgames => search_idgames(&query),
            SearchSource::Doomwiki => search_doomwiki(&query),
        };
        sender.send(AppMessage::SearchComplete(source, results));
    });
}

fn search_idgames(query: &str) -> Vec<SearchResultEntry> {
    let client = caco_sources::idgames::IdgamesClient::new();
    let entries = match client.search(query, None, None, None) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    entries
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
        .collect()
}

fn search_doomwiki(query: &str) -> Vec<SearchResultEntry> {
    let client = caco_sources::doomwiki::DoomwikiClient::new();
    let entries = match client.search_wads(query, 50) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    entries
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
        .collect()
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

/// Import the WAD referenced by a Cacoward entry (DB pk). Mirrors the CLI
/// `caco import --cacoward` flow: prefer the entry's idgames URL, fall back
/// to the doomwiki page, then link the new wad row to the cacoward entry
/// so the magazine view immediately reflects the new state.
pub fn spawn_import_cacoward(sender: BackgroundSender, db_path: PathBuf, cacoward_pk: i64) {
    thread::spawn(move || {
        let result: Result<_, String> = (|| {
            let conn = caco_core::db::open_connection(&db_path).map_err(|e| e.to_string())?;
            let record = caco_core::db::cacowards::get_cacoward(&conn, cacoward_pk)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no cacoward entry with pk {cacoward_pk}"))?;

            let service = ImportService::new();

            // Pass 1: idgames URL → numeric id → import_idgames.
            let import_result = if let Some(ref url) = record.idgames_url
                && let Some(id) = caco_sources::idgames::extract_idgames_id_from_url(url)
            {
                let client = caco_sources::idgames::IdgamesClient::new();
                let entry = client.get(Some(id), None).map_err(|e| e.to_string())?;
                service.import_idgames(&conn, &entry, None, false)
            } else if let Some(ref url) = record.doomwiki_url
                && let Some(title) = caco_sources::doomwiki::extract_doomwiki_title_from_url(url)
            {
                // Pass 2: Doom Wiki fallback.
                let client = caco_sources::doomwiki::DoomwikiClient::new();
                let entry = client
                    .get_entry(&title)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("Doom Wiki page '{title}' not found"))?;
                service.import_doomwiki(&conn, &entry, None, false)
            } else {
                return Err(format!(
                    "{} has neither an idgames link nor a Doom Wiki URL — import manually",
                    record.wad_title,
                ));
            };

            if let Some(err) = &import_result.error {
                return Err(err.clone());
            }

            // Link the cacoward entry to the resulting wad row (new or
            // pre-existing duplicate). Auto-link, never sets manual_override.
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
