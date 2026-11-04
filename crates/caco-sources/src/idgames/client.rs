use std::path::Path;

use reqwest::blocking::{Client, Response};

use super::models::{FileEntry, Review};
use crate::error::{Result, SourceError};
use crate::http::{build_client, waf_challenge};

/// Download mirrors for the idgames archive.
pub const MIRRORS: &[&str] = &[
    "https://youfailit.net/pub/idgames/",
    "https://www.quaddicted.com/files/idgames/",
    "https://ftpmirror1.infania.net/pub/idgames/",
    "https://mirror.braindrainlan.nu/pub/idgames/",
    "https://files.xvertigox.com/idgames/",
];

const API_URL: &str = "https://www.doomworld.com/idgames/api/api.php";

/// Client for the idgames archive API.
pub struct IdgamesClient {
    client: Client,
    api_url: String,
}

impl IdgamesClient {
    /// A client for the real API, unless `CACO_IDGAMES_API` says it is somewhere else. Which
    /// is how a run of the real app with nothing to answer it, in the screenshot harness,
    /// presses buttons that end up asking it.
    pub fn new() -> Self {
        match std::env::var("CACO_IDGAMES_API") {
            Ok(api_url) => Self::with_api_url(api_url),
            Err(_) => Self::with_api_url(API_URL),
        }
    }

    /// A client for an API somewhere else.
    pub fn with_api_url(api_url: impl Into<String>) -> Self {
        Self {
            client: build_client(),
            api_url: api_url.into(),
        }
    }

    /// Make a request to the idgames API and return the `content` object.
    fn request(&self, action: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let mut query: Vec<(&str, &str)> = params.to_vec();
        query.push(("action", action));
        query.push(("out", "json"));

        let response = self.client.get(&self.api_url).query(&query).send()?;
        read_api_response(response)
    }

    /// Look up an entry by archive path, with a filename-search fallback.
    ///
    /// The idgames API's `get(file=...)` endpoint has been observed to return
    /// `Bad File: No file associated with that path` transiently even for
    /// entries that plainly exist (the backend's file-index is not always
    /// in sync with the DB, and different mirrors may disagree). A
    /// `search(type=filename)` lookup of the same basename reliably returns
    /// the entry. This method tries `get` first and, on any non-WAF API
    /// error, falls back to a filename search filtered to the same archive
    /// directory so we don't pick up unrelated same-named files.
    ///
    /// A WAF block short-circuits: the search would only hit the same wall.
    pub fn get_by_path(&self, file_path: &str) -> Result<FileEntry> {
        let get_err = match self.get(None, Some(file_path)) {
            Ok(entry) => return Ok(entry),
            Err(e @ SourceError::WafBlocked { .. }) => return Err(e),
            Err(e) => e,
        };

        // Fallback: search by the filename stem (without extension), then
        // match by the dir+filename of the original path.
        let basename = Path::new(file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if basename.is_empty() {
            return Err(get_err);
        }

        let Ok(results) = self.search(basename, Some("filename"), None, None) else {
            return Err(get_err);
        };

        let want = file_path.to_lowercase();
        for entry in results {
            let got = format!(
                "{}/{}",
                entry.dir.trim_matches('/').to_lowercase(),
                entry.filename.to_lowercase(),
            );
            if got == want {
                return Ok(entry);
            }
        }

        Err(get_err)
    }

    /// Get file details by ID or filename.
    pub fn get(&self, id: Option<i64>, file: Option<&str>) -> Result<FileEntry> {
        if id.is_none() && file.is_none() {
            return Err(SourceError::Api(
                "Must provide either id or file".to_string(),
            ));
        }

        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(id) = id {
            params.push(("id", id.to_string()));
        }
        if let Some(f) = file {
            params.push(("file", f.to_string()));
        }
        let param_refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();

        let content = self.request("get", &param_refs)?;

        // Parse reviews from nested structure before deserializing FileEntry.
        // The API returns: "reviews": {"review": {...}} for single,
        //                  "reviews": {"review": [{...}, ...]} for multiple.
        let reviews = parse_reviews(&content);

        let mut entry: FileEntry = serde_json::from_value(content)?;
        entry.reviews = reviews;
        entry.clean_text();
        Ok(entry)
    }

    /// Every file in a directory of the archive, in one call. No more than what a search
    /// would say about each: the text file itself and the reviews only come from
    /// [`get`](Self::get).
    pub fn list_directory(&self, dir: &str) -> Result<Vec<FileEntry>> {
        let content = self.request("getfiles", &[("name", dir)])?;
        parse_file_list(&content)
    }

    /// Search for files.
    pub fn search(
        &self,
        query: &str,
        search_type: Option<&str>,
        sort: Option<&str>,
        sort_dir: Option<&str>,
    ) -> Result<Vec<FileEntry>> {
        let mut params: Vec<(&str, &str)> = vec![("query", query)];
        if let Some(t) = search_type {
            params.push(("type", t));
        }
        if let Some(s) = sort {
            params.push(("sort", s));
        }
        if let Some(d) = sort_dir {
            params.push(("dir", d));
        }

        let content = self.request("search", &params)?;
        parse_file_list(&content)
    }
}

impl Default for IdgamesClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Turn an idgames API response into its `content`, or the error it amounts to.
fn read_api_response(response: Response) -> Result<serde_json::Value> {
    if let Some(kind) = waf_challenge(&response) {
        return Err(SourceError::WafBlocked {
            site: "idgames",
            kind,
        });
    }

    response.error_for_status_ref()?;

    let data: serde_json::Value = response.json()?;

    if let Some(error) = data.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");
        return Err(SourceError::Api(msg.to_string()));
    }

    Ok(data
        .get("content")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

// ---------------------------------------------------------------------------
// Normalization helpers
// ---------------------------------------------------------------------------

/// Normalize a JSON value that may be a single object or an array into a `Vec<T>`.
///
/// Each entry on its own, so that one the API has spelled some new way costs that entry
/// and not all of them. It was one call for the whole array, which threw away every file
/// in a directory, or every result of a search, for one number where a string should be.
fn normalize_list<T: serde::de::DeserializeOwned>(val: &serde_json::Value) -> Vec<T> {
    let parse = |item: &serde_json::Value| serde_json::from_value::<T>(item.clone()).ok();
    match val {
        serde_json::Value::Array(items) => items.iter().filter_map(parse).collect(),
        serde_json::Value::Object(_) => parse(val).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Parse the nested `reviews` field from a file entry's raw JSON.
fn parse_reviews(content: &serde_json::Value) -> Vec<Review> {
    let reviews_obj = match content.get("reviews") {
        Some(v) if !v.is_null() => v,
        _ => return Vec::new(),
    };

    let review_val = match reviews_obj.get("review") {
        Some(v) if !v.is_null() => v,
        _ => return Vec::new(),
    };

    normalize_list::<Review>(review_val)
}

/// Parse a `file` field that may be a single object or an array into a list of `FileEntry`.
fn parse_file_list(content: &serde_json::Value) -> Result<Vec<FileEntry>> {
    if content.is_null() {
        return Ok(Vec::new());
    }
    let files_val = content
        .get("file")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let mut entries = normalize_list::<FileEntry>(&files_val);
    entries.iter_mut().for_each(FileEntry::clean_text);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{WafKind, canned_response};

    #[test]
    fn test_read_api_response_reports_a_cloudflare_challenge_as_blocked() {
        let err = read_api_response(canned_response(
            403,
            &[("server", "cloudflare"), ("cf-mitigated", "challenge")],
            "<html>Just a moment...</html>",
        ))
        .unwrap_err();

        assert!(matches!(
            err,
            SourceError::WafBlocked {
                site: "idgames",
                kind: WafKind::Cloudflare,
            }
        ));
    }

    /// This client used to check for Cloudflare only.
    #[test]
    fn test_read_api_response_reports_an_aws_challenge_as_blocked() {
        let err = read_api_response(canned_response(
            202,
            &[("x-amzn-waf-action", "challenge")],
            "<html>challenge script</html>",
        ))
        .unwrap_err();

        assert!(matches!(
            err,
            SourceError::WafBlocked {
                kind: WafKind::AwsWaf,
                ..
            }
        ));
    }

    /// This API is behind a WAF and answers normally, which must stay an answer.
    #[test]
    fn test_read_api_response_returns_the_content() {
        let content = read_api_response(canned_response(
            200,
            &[("server", "cloudflare")],
            r#"{"content": {"status": "true"}}"#,
        ))
        .unwrap();

        assert_eq!(content["status"], "true");
    }

    #[test]
    fn test_read_api_response_keeps_the_status_of_a_plain_403() {
        let err = read_api_response(canned_response(403, &[], "Forbidden")).unwrap_err();

        let SourceError::Http(http) = err else {
            panic!("expected an HTTP error, got {err:?}");
        };
        assert_eq!(http.status(), Some(reqwest::StatusCode::FORBIDDEN));
    }

    #[test]
    fn test_read_api_response_reports_an_api_error() {
        let err = read_api_response(canned_response(
            200,
            &[],
            r#"{"error": {"message": "Bad File"}}"#,
        ))
        .unwrap_err();

        assert!(matches!(err, SourceError::Api(msg) if msg == "Bad File"));
    }

    #[test]
    fn test_normalize_list_array() {
        let val: serde_json::Value = serde_json::json!([
            {"text": "a", "vote": 1, "username": "x"},
            {"text": "b", "vote": 2, "username": "y"},
        ]);
        let reviews: Vec<Review> = normalize_list(&val);
        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0].text, "a");
    }

    #[test]
    fn test_normalize_list_single_object() {
        let val: serde_json::Value = serde_json::json!({"text": "a", "vote": 1, "username": "x"});
        let reviews: Vec<Review> = normalize_list(&val);
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].text, "a");
    }

    /// What used to empty a whole directory: 263 files, for one title of `2014`.
    #[test]
    fn test_one_entry_that_cannot_be_read_costs_only_that_entry() {
        let val = serde_json::json!([
            {"text": "a", "vote": 1, "username": "x"},
            {"text": "b", "vote": "not a number", "username": "y"},
            {"text": "c", "vote": 3, "username": "z"},
        ]);
        let reviews: Vec<Review> = normalize_list(&val);
        let texts: Vec<&str> = reviews.iter().map(|review| review.text.as_str()).collect();
        assert_eq!(texts, ["a", "c"]);
    }

    #[test]
    fn test_a_title_that_looks_like_a_number_is_still_a_title() {
        let content = serde_json::json!({
            "file": [
                {"id": 1, "filename": "vtm_2014.zip", "title": 2014},
                {"id": 2, "filename": "hexndm.zip", "author": 8130423},
                {"id": 3, "filename": "plain.zip", "title": "Plain"},
            ]
        });
        let files = parse_file_list(&content).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].title, "2014");
        assert_eq!(files[1].author, "8130423");
    }

    #[test]
    fn test_normalize_list_null() {
        let val = serde_json::Value::Null;
        let reviews: Vec<Review> = normalize_list(&val);
        assert!(reviews.is_empty());
    }

    #[test]
    fn test_parse_reviews_single() {
        let content = serde_json::json!({
            "id": 1,
            "reviews": {
                "review": {"text": "Good", "vote": 5, "username": "user1"}
            }
        });
        let reviews = parse_reviews(&content);
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].text, "Good");
    }

    #[test]
    fn test_parse_reviews_multiple() {
        let content = serde_json::json!({
            "id": 1,
            "reviews": {
                "review": [
                    {"text": "Good", "vote": 5, "username": "user1"},
                    {"text": "Bad", "vote": 1, "username": "user2"}
                ]
            }
        });
        let reviews = parse_reviews(&content);
        assert_eq!(reviews.len(), 2);
    }

    #[test]
    fn test_parse_reviews_none() {
        let content = serde_json::json!({"id": 1});
        assert!(parse_reviews(&content).is_empty());
    }

    #[test]
    fn test_parse_reviews_null() {
        let content = serde_json::json!({"id": 1, "reviews": null});
        assert!(parse_reviews(&content).is_empty());
    }

    #[test]
    fn test_parse_file_list_single() {
        let content = serde_json::json!({
            "file": {"id": 42, "filename": "test.wad"}
        });
        let files = parse_file_list(&content).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, 42);
    }

    #[test]
    fn test_parse_file_list_multiple() {
        let content = serde_json::json!({
            "file": [
                {"id": 1, "filename": "a.wad"},
                {"id": 2, "filename": "b.wad"}
            ]
        });
        let files = parse_file_list(&content).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_parse_file_list_empty() {
        let content = serde_json::Value::Null;
        let files = parse_file_list(&content).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_full_api_response_parse() {
        // Simulate a complete API response for the `get` action
        let raw = serde_json::json!({
            "content": {
                "id": 19312,
                "title": "Sunlust",
                "dir": "levels/doom2/Ports/megawads/",
                "filename": "sunlust.zip",
                "size": 14237696,
                "age": 1441065600,
                "date": "2015-09-01",
                "author": "Ribbiks & Dannebubinga",
                "email": null,
                "description": "A set of 32 boom-compatible maps.",
                "credits": null,
                "base": null,
                "buildtime": null,
                "editors": null,
                "bugs": null,
                "textfile": null,
                "rating": 4.7368,
                "votes": 19,
                "url": "https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/sunlust",
                "idgamesurl": "idgames://levels/doom2/Ports/megawads/sunlust.zip",
                "reviews": {
                    "review": {"text": "Amazing!", "vote": 5, "username": "fan"}
                }
            }
        });

        let content = raw.get("content").unwrap();
        let reviews = parse_reviews(content);
        let mut entry: FileEntry = serde_json::from_value(content.clone()).unwrap();
        entry.reviews = reviews;

        assert_eq!(entry.id, 19312);
        assert_eq!(entry.title, "Sunlust");
        assert_eq!(entry.author, "Ribbiks & Dannebubinga");
        assert_eq!(entry.email, ""); // null coerced
        assert_eq!(entry.credits, ""); // null coerced
        assert_eq!(entry.rating, 4.7368);
        assert_eq!(entry.votes, 19);
        assert_eq!(entry.reviews.len(), 1);
        assert_eq!(entry.reviews[0].text, "Amazing!");
    }

    /// What the site renders, which is no use anywhere else.
    #[test]
    fn test_the_markup_in_what_the_api_says_is_taken_out() {
        let content = serde_json::json!({
            "file": {
                "id": 1,
                "title": "Tom &amp; Jerry",
                "description": "First part.<br><br>\n\nSecond part.<BR />Third.",
            }
        });
        let files = parse_file_list(&content).unwrap();
        assert_eq!(files[0].title, "Tom & Jerry");
        assert_eq!(files[0].description, "First part.\n\nSecond part.\nThird.");
    }
}
