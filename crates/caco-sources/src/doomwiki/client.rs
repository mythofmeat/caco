use reqwest::blocking::Client;

use super::models::{SearchResult, WikiEntry};
use super::parser::WikitextParser;
use crate::error::{Result, SourceError};
use crate::http::build_client;

const API_URL: &str = "https://doomwiki.org/w/api.php";
const USER_AGENT: &str = "Caco/1.0 (Doom WAD library manager; https://github.com/eshen/caco)";

/// Client for the Doom Wiki MediaWiki API.
pub struct DoomwikiClient {
    client: Client,
    parser: WikitextParser,
}

impl DoomwikiClient {
    /// Create a new client with default settings.
    pub fn new() -> Self {
        Self {
            client: build_client(None, Some(USER_AGENT)),
            parser: WikitextParser::new(),
        }
    }

    /// Create a client with a custom reqwest client (for testing).
    pub fn with_client(client: Client) -> Self {
        Self {
            client,
            parser: WikitextParser::new(),
        }
    }

    /// Make a request to the MediaWiki API.
    fn request(&self, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let mut query: Vec<(&str, &str)> = params.to_vec();
        query.push(("format", "json"));

        let response = self.client.get(API_URL).query(&query).send()?;
        response.error_for_status_ref()?;

        let data: serde_json::Value = response.json()?;

        if let Some(error) = data.get("error") {
            let msg = error
                .get("info")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(SourceError::Api(msg.to_string()));
        }

        Ok(data)
    }

    /// Search the wiki for pages matching the query.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let limit_str = limit.to_string();
        let data = self.request(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("srlimit", &limit_str),
            ("srprop", "snippet"),
        ])?;

        let mut results = Vec::new();
        if let Some(search_arr) = data
            .get("query")
            .and_then(|q| q.get("search"))
            .and_then(|s| s.as_array())
        {
            for item in search_arr {
                results.push(SearchResult {
                    page_id: item
                        .get("pageid")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    title: item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    snippet: item
                        .get("snippet")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                });
            }
        }

        Ok(results)
    }

    /// Get the raw wikitext content of a page by title.
    ///
    /// Returns `(page_id, wikitext)` or `None` if the page doesn't exist.
    pub fn get_page_content(&self, title: &str) -> Result<Option<(i64, String)>> {
        let data = self.request(&[
            ("action", "query"),
            ("titles", title),
            ("prop", "revisions"),
            ("rvprop", "content"),
        ])?;

        Ok(extract_page_content(&data))
    }

    /// Get the raw wikitext content of a page by ID.
    ///
    /// Returns `(title, wikitext)` or `None` if the page doesn't exist.
    pub fn get_page_content_by_id(&self, page_id: i64) -> Result<Option<(String, String)>> {
        let id_str = page_id.to_string();
        let data = self.request(&[
            ("action", "query"),
            ("pageids", &id_str),
            ("prop", "revisions"),
            ("rvprop", "content"),
        ])?;

        if let Some(pages) = data.get("query").and_then(|q| q.get("pages")).and_then(|p| p.as_object()) {
            let page_data = match pages.get(&id_str) {
                Some(p) => p,
                None => return Ok(None),
            };

            if page_data.get("missing").is_some() {
                return Ok(None);
            }

            let title = page_data
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(content) = extract_revision_content(page_data) {
                return Ok(Some((title, content)));
            }
        }

        Ok(None)
    }

    /// Get parsed WAD entry for a wiki page by title.
    pub fn get_entry(&self, title: &str) -> Result<Option<WikiEntry>> {
        let result = self.get_page_content(title)?;
        match result {
            Some((page_id, wikitext)) => {
                Ok(Some(self.parser.parse(&wikitext, title, page_id)))
            }
            None => Ok(None),
        }
    }

    /// Get parsed WAD entry for a wiki page by ID.
    pub fn get_entry_by_id(&self, page_id: i64) -> Result<Option<WikiEntry>> {
        let result = self.get_page_content_by_id(page_id)?;
        match result {
            Some((title, wikitext)) => {
                Ok(Some(self.parser.parse(&wikitext, &title, page_id)))
            }
            None => Ok(None),
        }
    }

    /// Fetch multiple page contents in a single API request.
    ///
    /// Uses MediaWiki pipe-separated titles API (max 50 per request).
    pub fn get_pages_batch(
        &self,
        titles: &[String],
    ) -> Result<std::collections::HashMap<String, (i64, String)>> {
        if titles.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let mut results = std::collections::HashMap::new();

        for chunk in titles.chunks(50) {
            let joined = chunk.join("|");
            let data = self.request(&[
                ("action", "query"),
                ("titles", &joined),
                ("prop", "revisions"),
                ("rvprop", "content"),
            ])?;

            if let Some(pages) = data
                .get("query")
                .and_then(|q| q.get("pages"))
                .and_then(|p| p.as_object())
            {
                for (page_id_str, page_data) in pages {
                    if page_id_str == "-1" || page_data.get("missing").is_some() {
                        continue;
                    }
                    let title = page_data
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(content) = extract_revision_content(page_data)
                        && let Ok(pid) = page_id_str.parse::<i64>() {
                            results.insert(title, (pid, content));
                        }
                }
            }
        }

        Ok(results)
    }

    /// Search for WAD pages and return parsed entries.
    ///
    /// Only returns pages that contain a `{{Wad}}` infobox template.
    /// Uses batch page fetch to minimize API requests.
    pub fn search_wads(&self, query: &str, limit: usize) -> Result<Vec<WikiEntry>> {
        let search_results = self.search(query, limit)?;
        if search_results.is_empty() {
            return Ok(Vec::new());
        }

        let titles: Vec<String> = search_results.iter().map(|r| r.title.clone()).collect();
        let pages = self.get_pages_batch(&titles)?;

        let mut entries = Vec::new();
        for result in &search_results {
            if let Some((page_id, wikitext)) = pages.get(&result.title)
                && self.parser.has_wad_template(wikitext) {
                    entries.push(self.parser.parse(wikitext, &result.title, *page_id));
                }
        }

        Ok(entries)
    }
}

impl Default for DoomwikiClient {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract (page_id, wikitext) from a MediaWiki query response.
fn extract_page_content(data: &serde_json::Value) -> Option<(i64, String)> {
    let pages = data.get("query")?.get("pages")?.as_object()?;
    for (page_id_str, page_data) in pages {
        if page_id_str == "-1" {
            return None;
        }
        if let Some(content) = extract_revision_content(page_data) {
            return Some((page_id_str.parse().ok()?, content));
        }
    }
    None
}

/// Extract wikitext from a page's revision data.
fn extract_revision_content(page_data: &serde_json::Value) -> Option<String> {
    page_data
        .get("revisions")?
        .as_array()?
        .first()?
        .get("*")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_page_content() {
        let data = serde_json::json!({
            "query": {
                "pages": {
                    "12345": {
                        "pageid": 12345,
                        "title": "Scythe",
                        "revisions": [
                            {"*": "{{Wad|name=Scythe}}"}
                        ]
                    }
                }
            }
        });

        let result = extract_page_content(&data).unwrap();
        assert_eq!(result.0, 12345);
        assert_eq!(result.1, "{{Wad|name=Scythe}}");
    }

    #[test]
    fn test_extract_page_content_missing() {
        let data = serde_json::json!({
            "query": {
                "pages": {
                    "-1": {
                        "missing": ""
                    }
                }
            }
        });
        assert!(extract_page_content(&data).is_none());
    }

    #[test]
    fn test_extract_revision_content() {
        let page = serde_json::json!({
            "revisions": [{"*": "wiki content here"}]
        });
        assert_eq!(
            extract_revision_content(&page).unwrap(),
            "wiki content here"
        );
    }

    #[test]
    fn test_extract_revision_content_no_revisions() {
        let page = serde_json::json!({"title": "Test"});
        assert!(extract_revision_content(&page).is_none());
    }
}
