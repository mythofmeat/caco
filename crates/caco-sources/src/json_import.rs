//! Parse saved API JSON responses for offline import.
//!
//! When idgames or Doom Wiki APIs are blocked by WAF challenges,
//! users can visit the API URL in their browser, save the JSON response,
//! and import from the saved file.

use std::path::Path;

use crate::doomwiki::models::WikiEntry;
use crate::doomwiki::parser::WikitextParser;
use crate::error::{Result, SourceError};
use crate::idgames::models::FileEntry;

/// Detected JSON source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonSource {
    Idgames,
    Doomwiki,
}

/// Detect whether a JSON file is an idgames or Doom Wiki API response.
///
/// Returns `None` if unrecognized.
pub fn detect_json_source(path: &Path) -> Option<JsonSource> {
    let text = std::fs::read_to_string(path).ok()?;
    let data: serde_json::Value = serde_json::from_str(&text).ok()?;
    detect_json_source_from_value(&data)
}

/// Detect JSON source from an already-parsed value.
fn detect_json_source_from_value(data: &serde_json::Value) -> Option<JsonSource> {
    // idgames: {"content": {"file": ...}} or {"content": {"status": ...}}
    if let Some(content) = data.get("content").and_then(|c| c.as_object())
        && (content.contains_key("file") || content.contains_key("status"))
    {
        return Some(JsonSource::Idgames);
    }

    // Doom Wiki: {"query": {"pages": {...}}}
    if let Some(query) = data.get("query").and_then(|q| q.as_object())
        && query.contains_key("pages")
    {
        return Some(JsonSource::Doomwiki);
    }

    None
}

/// Parse a saved idgames API JSON response into `FileEntry` objects.
///
/// Handles both single-file (`action=get`) and search (`action=search`) responses.
pub fn parse_idgames_json(path: &Path) -> Result<Vec<FileEntry>> {
    let text = std::fs::read_to_string(path)
        .map_err(SourceError::Io)?;
    let data: serde_json::Value = serde_json::from_str(&text)?;
    parse_idgames_json_value(&data)
}

/// Parse idgames entries from an already-parsed JSON value.
fn parse_idgames_json_value(data: &serde_json::Value) -> Result<Vec<FileEntry>> {
    let content = match data.get("content") {
        Some(c) if !c.is_null() => c,
        _ => return Ok(Vec::new()),
    };

    // Single file (action=get): content is the file entry directly
    // Search results (action=search): content has a "file" field
    let files = match content.get("file") {
        Some(val) if val.is_array() => val.as_array().expect("checked is_array").clone(),
        Some(val) if val.is_object() => vec![val.clone()],
        Some(_) => return Ok(Vec::new()),
        None if content.get("id").is_some() => {
            // Single file response (action=get): content IS the file entry
            vec![content.clone()]
        }
        None => return Ok(Vec::new()),
    };

    let mut entries = Vec::new();
    for file_val in &files {
        match serde_json::from_value::<FileEntry>(file_val.clone()) {
            Ok(mut entry) => {
                // Parse reviews if present (same nested structure as API)
                entry.reviews = parse_reviews_from_value(file_val);
                entries.push(entry);
            }
            Err(_) => continue,
        }
    }

    Ok(entries)
}

/// Parse reviews from a file entry's raw JSON value.
fn parse_reviews_from_value(file_val: &serde_json::Value) -> Vec<crate::idgames::models::Review> {
    let reviews_obj = match file_val.get("reviews") {
        Some(v) if !v.is_null() => v,
        _ => return Vec::new(),
    };

    let review_val = match reviews_obj.get("review") {
        Some(v) if !v.is_null() => v,
        _ => return Vec::new(),
    };

    if review_val.is_array() {
        serde_json::from_value(review_val.clone()).unwrap_or_default()
    } else if review_val.is_object() {
        match serde_json::from_value::<crate::idgames::models::Review>(review_val.clone()) {
            Ok(r) => vec![r],
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    }
}

/// Parse a saved Doom Wiki API JSON response into `WikiEntry` objects.
///
/// Only returns pages containing a `{{Wad}}` infobox template.
pub fn parse_doomwiki_json(path: &Path) -> Result<Vec<WikiEntry>> {
    let text = std::fs::read_to_string(path)
        .map_err(SourceError::Io)?;
    let data: serde_json::Value = serde_json::from_str(&text)?;
    parse_doomwiki_json_value(&data)
}

/// Parse Doom Wiki entries from an already-parsed JSON value.
fn parse_doomwiki_json_value(data: &serde_json::Value) -> Result<Vec<WikiEntry>> {
    let pages = match data
        .get("query")
        .and_then(|q| q.get("pages"))
        .and_then(|p| p.as_object())
    {
        Some(p) => p,
        None => return Ok(Vec::new()),
    };

    let parser = WikitextParser::new();
    let mut entries = Vec::new();

    for (page_id_str, page_data) in pages {
        if page_id_str == "-1" || page_data.get("missing").is_some() {
            continue;
        }

        let revisions = match page_data.get("revisions").and_then(|r| r.as_array()) {
            Some(r) => r,
            None => continue,
        };

        let wikitext = match revisions.first().and_then(|r| r.get("*")).and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };

        let title = page_data
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        if !parser.has_wad_template(wikitext) {
            continue;
        }

        let page_id: i64 = page_id_str.parse().unwrap_or(0);
        entries.push(parser.parse(wikitext, title, page_id));
    }

    Ok(entries)
}

/// Build the idgames API URL the user should visit in their browser.
pub fn idgames_api_url(query_or_id: &str) -> String {
    let base = "https://www.doomworld.com/idgames/api/api.php";
    if let Ok(id) = query_or_id.parse::<i64>() {
        format!("{base}?action=get&id={id}&out=json")
    } else {
        let encoded = urlencoding::encode(query_or_id);
        format!("{base}?action=search&query={encoded}&type=title&out=json")
    }
}

/// Build the Doom Wiki API URL the user should visit in their browser.
pub fn doomwiki_api_url(query_or_title: &str) -> String {
    let base = "https://doomwiki.org/w/api.php";
    let encoded = urlencoding::encode(query_or_title);
    format!("{base}?action=query&titles={encoded}&prop=revisions&rvprop=content&format=json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_json(val: &serde_json::Value) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(serde_json::to_string(val).unwrap().as_bytes())
            .unwrap();
        f.flush().unwrap();
        f
    }

    // -----------------------------------------------------------------------
    // detect_json_source
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_idgames_search() {
        let data = serde_json::json!({
            "content": {
                "file": [{"id": 1, "filename": "test.wad"}]
            }
        });
        assert_eq!(
            detect_json_source_from_value(&data),
            Some(JsonSource::Idgames)
        );
    }

    #[test]
    fn test_detect_idgames_get() {
        let data = serde_json::json!({
            "content": {
                "id": 1,
                "filename": "test.wad",
                "status": "ok"
            }
        });
        assert_eq!(
            detect_json_source_from_value(&data),
            Some(JsonSource::Idgames)
        );
    }

    #[test]
    fn test_detect_doomwiki() {
        let data = serde_json::json!({
            "query": {
                "pages": {
                    "12345": {
                        "title": "Scythe",
                        "revisions": [{"*": "{{Wad}}"}]
                    }
                }
            }
        });
        assert_eq!(
            detect_json_source_from_value(&data),
            Some(JsonSource::Doomwiki)
        );
    }

    #[test]
    fn test_detect_unknown() {
        let data = serde_json::json!({"foo": "bar"});
        assert_eq!(detect_json_source_from_value(&data), None);
    }

    #[test]
    fn test_detect_from_file() {
        let data = serde_json::json!({
            "content": {
                "file": {"id": 42, "filename": "test.wad"}
            }
        });
        let f = write_json(&data);
        assert_eq!(detect_json_source(f.path()), Some(JsonSource::Idgames));
    }

    // -----------------------------------------------------------------------
    // parse_idgames_json
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_idgames_search_results() {
        let data = serde_json::json!({
            "content": {
                "file": [
                    {
                        "id": 19312,
                        "title": "Sunlust",
                        "dir": "levels/doom2/Ports/megawads/",
                        "filename": "sunlust.zip",
                        "size": 14237696,
                        "age": 0,
                        "date": "2015-09-01",
                        "author": "Ribbiks",
                        "description": "32 maps"
                    },
                    {
                        "id": 100,
                        "title": "Test WAD",
                        "dir": "levels/doom2/",
                        "filename": "test.zip",
                        "author": "Author"
                    }
                ]
            }
        });
        let f = write_json(&data);
        let entries = parse_idgames_json(f.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, 19312);
        assert_eq!(entries[0].title, "Sunlust");
        assert_eq!(entries[0].author, "Ribbiks");
        assert_eq!(entries[1].id, 100);
    }

    #[test]
    fn test_parse_idgames_single_file() {
        let data = serde_json::json!({
            "content": {
                "file": {
                    "id": 42,
                    "title": "Single",
                    "dir": "levels/",
                    "filename": "single.zip",
                    "author": "Me"
                }
            }
        });
        let f = write_json(&data);
        let entries = parse_idgames_json(f.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 42);
        assert_eq!(entries[0].title, "Single");
    }

    #[test]
    fn test_parse_idgames_get_response() {
        // action=get returns content as the file entry directly (no "file" wrapper)
        let data = serde_json::json!({
            "content": {
                "id": 19312,
                "title": "Sunlust",
                "dir": "levels/doom2/Ports/megawads/",
                "filename": "sunlust.zip",
                "size": 14237696,
                "age": 0,
                "date": "2015-09-01",
                "author": "Ribbiks",
                "description": "32 maps",
                "reviews": {
                    "review": {"text": "Amazing!", "vote": 5, "username": "fan"}
                }
            }
        });
        let f = write_json(&data);
        let entries = parse_idgames_json(f.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 19312);
        assert_eq!(entries[0].title, "Sunlust");
        assert_eq!(entries[0].reviews.len(), 1);
        assert_eq!(entries[0].reviews[0].text, "Amazing!");
    }

    #[test]
    fn test_parse_idgames_empty() {
        let data = serde_json::json!({"content": {}});
        let f = write_json(&data);
        let entries = parse_idgames_json(f.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_idgames_null_content() {
        let data = serde_json::json!({"content": null});
        let f = write_json(&data);
        let entries = parse_idgames_json(f.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_idgames_with_reviews() {
        let data = serde_json::json!({
            "content": {
                "file": {
                    "id": 1,
                    "title": "Test",
                    "filename": "test.zip",
                    "reviews": {
                        "review": [
                            {"text": "Good", "vote": 5, "username": "user1"},
                            {"text": "Bad", "vote": 1, "username": "user2"}
                        ]
                    }
                }
            }
        });
        let f = write_json(&data);
        let entries = parse_idgames_json(f.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].reviews.len(), 2);
    }

    // -----------------------------------------------------------------------
    // parse_doomwiki_json
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_doomwiki_with_wad_template() {
        let data = serde_json::json!({
            "query": {
                "pages": {
                    "12345": {
                        "pageid": 12345,
                        "title": "Scythe",
                        "revisions": [{
                            "*": "{{Wad\n| name = Scythe\n| author = Erik Alm\n| year = 2003\n| iwad = Doom II\n| port = Boom-compatible\n}}\nScythe is a popular megawad."
                        }]
                    }
                }
            }
        });
        let f = write_json(&data);
        let entries = parse_doomwiki_json(f.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Scythe");
        assert_eq!(entries[0].author, "Erik Alm");
        assert_eq!(entries[0].page_id, 12345);
    }

    #[test]
    fn test_parse_doomwiki_no_wad_template() {
        let data = serde_json::json!({
            "query": {
                "pages": {
                    "99": {
                        "pageid": 99,
                        "title": "Some Article",
                        "revisions": [{"*": "Just a regular article without WAD template."}]
                    }
                }
            }
        });
        let f = write_json(&data);
        let entries = parse_doomwiki_json(f.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_doomwiki_missing_page() {
        let data = serde_json::json!({
            "query": {
                "pages": {
                    "-1": {
                        "missing": ""
                    }
                }
            }
        });
        let f = write_json(&data);
        let entries = parse_doomwiki_json(f.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_doomwiki_multiple_pages() {
        let data = serde_json::json!({
            "query": {
                "pages": {
                    "100": {
                        "pageid": 100,
                        "title": "WAD1",
                        "revisions": [{"*": "{{Wad\n| name = WAD1\n}}"}]
                    },
                    "200": {
                        "pageid": 200,
                        "title": "Not a WAD",
                        "revisions": [{"*": "Regular article"}]
                    },
                    "300": {
                        "pageid": 300,
                        "title": "WAD2",
                        "revisions": [{"*": "{{Wad\n| name = WAD2\n}}"}]
                    }
                }
            }
        });
        let f = write_json(&data);
        let entries = parse_doomwiki_json(f.path()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_doomwiki_empty() {
        let data = serde_json::json!({"query": {"pages": {}}});
        let f = write_json(&data);
        let entries = parse_doomwiki_json(f.path()).unwrap();
        assert!(entries.is_empty());
    }

    // -----------------------------------------------------------------------
    // URL builders
    // -----------------------------------------------------------------------

    #[test]
    fn test_idgames_api_url_id() {
        let url = idgames_api_url("19312");
        assert_eq!(
            url,
            "https://www.doomworld.com/idgames/api/api.php?action=get&id=19312&out=json"
        );
    }

    #[test]
    fn test_idgames_api_url_query() {
        let url = idgames_api_url("sunlust");
        assert_eq!(
            url,
            "https://www.doomworld.com/idgames/api/api.php?action=search&query=sunlust&type=title&out=json"
        );
    }

    #[test]
    fn test_idgames_api_url_query_with_spaces() {
        let url = idgames_api_url("ancient aliens");
        assert!(url.contains("ancient%20aliens"));
    }

    #[test]
    fn test_doomwiki_api_url() {
        let url = doomwiki_api_url("Scythe");
        assert_eq!(
            url,
            "https://doomwiki.org/w/api.php?action=query&titles=Scythe&prop=revisions&rvprop=content&format=json"
        );
    }

    #[test]
    fn test_doomwiki_api_url_with_spaces() {
        let url = doomwiki_api_url("Ancient Aliens");
        assert!(url.contains("Ancient%20Aliens"));
    }
}
