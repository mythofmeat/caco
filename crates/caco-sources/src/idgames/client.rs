use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;

use super::models::{ApiInfo, Directory, FileEntry, Review, Vote};
use crate::error::{Result, SourceError};
use crate::http::build_client;

/// Download mirrors for the idgames archive.
pub const MIRRORS: &[&str] = &[
    "https://youfailit.net/pub/idgames/",
    "https://www.quaddicted.com/files/idgames/",
    "https://ftpmirror1.infania.net/pub/idgames/",
    "https://mirror.braindrainlan.nu/pub/idgames/",
    "https://files.xvertigox.com/idgames/",
];

const BASE_URL: &str = "https://www.doomworld.com/idgames/api/api.php";

/// Client for the idgames archive API.
pub struct IdgamesClient {
    client: Client,
}

impl IdgamesClient {
    /// Create a new client with default settings.
    pub fn new() -> Self {
        Self {
            client: build_client(None, None),
        }
    }

    /// Create a client with a custom reqwest client (for testing).
    pub fn with_client(client: Client) -> Self {
        Self { client }
    }

    /// Make a request to the idgames API and return the `content` object.
    fn request(&self, action: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
        let mut query: Vec<(&str, &str)> = params.to_vec();
        query.push(("action", action));
        query.push(("out", "json"));

        let response = self.client.get(BASE_URL).query(&query).send()?;

        // Detect Cloudflare WAF challenge
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            let cf_mitigated = response
                .headers()
                .get("cf-mitigated")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if cf_mitigated == "challenge" {
                return Err(SourceError::WafBlocked {
                    api_name: "idgames".to_string(),
                    message: "idgames API blocked by Cloudflare challenge. This is usually temporary — try again later.".to_string(),
                });
            }
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

        Ok(data.get("content").cloned().unwrap_or(serde_json::Value::Null))
    }

    /// Check if the API server is responding.
    pub fn ping(&self) -> Result<String> {
        let content = self.request("ping", &[])?;
        Ok(content
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// Check if the database is responding.
    pub fn dbping(&self) -> Result<String> {
        let content = self.request("dbping", &[])?;
        Ok(content
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// Get API information.
    pub fn about(&self) -> Result<ApiInfo> {
        let content = self.request("about", &[])?;
        let info: ApiInfo = serde_json::from_value(content)?;
        Ok(info)
    }

    /// Get file details by ID or filename.
    pub fn get(&self, id: Option<i64>, file: Option<&str>) -> Result<FileEntry> {
        if id.is_none() && file.is_none() {
            return Err(SourceError::Api("Must provide either id or file".to_string()));
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
        Ok(entry)
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

    /// Get the latest files.
    pub fn latest_files(
        &self,
        limit: Option<i64>,
        startid: Option<i64>,
    ) -> Result<Vec<FileEntry>> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        if let Some(s) = startid {
            params.push(("startid", s.to_string()));
        }
        let param_refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();

        let content = self.request("latestfiles", &param_refs)?;
        parse_file_list(&content)
    }

    /// Get the latest votes.
    pub fn latest_votes(&self, limit: Option<i64>) -> Result<Vec<Vote>> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        let param_refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();

        let content = self.request("latestvotes", &param_refs)?;
        if content.is_null() {
            return Ok(Vec::new());
        }
        let votes_val = content.get("vote").cloned().unwrap_or(serde_json::Value::Null);
        Ok(normalize_list::<Vote>(&votes_val))
    }

    /// Get subdirectories.
    pub fn get_dirs(&self, id: Option<i64>, name: Option<&str>) -> Result<Vec<Directory>> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(i) = id {
            params.push(("id", i.to_string()));
        }
        if let Some(n) = name {
            params.push(("name", n.to_string()));
        }
        let param_refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();

        let content = self.request("getdirs", &param_refs)?;
        if content.is_null() {
            return Ok(Vec::new());
        }
        let dirs_val = content.get("dir").cloned().unwrap_or(serde_json::Value::Null);
        Ok(normalize_list::<Directory>(&dirs_val))
    }

    /// Get the download URL for a file entry.
    pub fn get_download_url(&self, entry: &FileEntry, mirror: usize) -> String {
        let path = format!(
            "{}/{}",
            entry.dir.trim_matches('/'),
            entry.filename
        )
        .replace("//", "/");
        format!("{}{}", MIRRORS[mirror % MIRRORS.len()], path)
    }

    /// Download a file with optional progress callback.
    ///
    /// Uses atomic download: writes to a `.partial` file first, then renames
    /// on success. Cleans up on failure.
    pub fn download(
        &self,
        entry: &FileEntry,
        dest: Option<&Path>,
        mirror: usize,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<PathBuf> {
        let url = self.get_download_url(entry, mirror);
        let dest_path = match dest {
            Some(p) => p.to_path_buf(),
            None => PathBuf::from(&entry.filename),
        };
        let partial = dest_path.with_extension(
            format!(
                "{}.partial",
                dest_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
            ),
        );

        let result = (|| -> Result<PathBuf> {
            let response = self.client.get(&url).send()?;
            response.error_for_status_ref()?;

            let total = response
                .content_length()
                .unwrap_or(0);

            if let Some(parent) = partial.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut file = fs::File::create(&partial)?;
            let bytes = response.bytes()?;
            file.write_all(&bytes)?;
            let downloaded = bytes.len() as u64;

            if let Some(cb) = progress {
                cb(downloaded, total);
            }

            drop(file);
            fs::rename(&partial, &dest_path)?;
            Ok(dest_path.clone())
        })();

        if result.is_err() && partial.exists() {
            let _ = fs::remove_file(&partial);
        }

        result
    }

    /// Download directly from a mirror, bypassing the API.
    ///
    /// Used as a fallback when the idgames API is blocked (e.g. Cloudflare).
    /// Constructs the mirror URL from a WAD's stored `source_url` and `filename`.
    ///
    /// `source_url` format: `https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/sunlust`
    /// The last segment is the WAD slug; the parent is the actual mirror directory.
    pub fn download_direct(
        &self,
        source_url: &str,
        filename: &str,
        dest_dir: &Path,
        mirror: usize,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<PathBuf> {
        if filename.is_empty() || !source_url.contains("/idgames/") {
            return Err(SourceError::Api(
                "Cannot construct direct download URL: missing idgames path or filename".to_string(),
            ));
        }

        // Extract the directory path from source_url
        let idgames_path = source_url
            .split("/idgames/")
            .nth(1)
            .unwrap_or("")
            .trim_end_matches('/');
        let dir_path = match idgames_path.rsplit_once('/') {
            Some((dir, _slug)) => format!("{dir}/"),
            None => String::new(),
        };
        let download_path = format!("{dir_path}{filename}");

        let mirror_base = MIRRORS[mirror % MIRRORS.len()];
        let url = format!("{mirror_base}{download_path}");

        let dest_path = dest_dir.join(filename);
        let partial = dest_path.with_extension(format!(
            "{}.partial",
            dest_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
        ));

        let result = (|| -> Result<PathBuf> {
            let response = self.client.get(&url).send()?;
            response.error_for_status_ref()?;

            let total = response.content_length().unwrap_or(0);

            if let Some(parent) = partial.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut file = fs::File::create(&partial)?;
            let bytes = response.bytes()?;
            file.write_all(&bytes)?;
            let downloaded = bytes.len() as u64;

            if let Some(cb) = progress {
                cb(downloaded, total);
            }

            drop(file);
            fs::rename(&partial, &dest_path)?;
            Ok(dest_path.clone())
        })();

        if result.is_err() && partial.exists() {
            let _ = fs::remove_file(&partial);
        }

        result
    }
}

impl Default for IdgamesClient {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Normalization helpers
// ---------------------------------------------------------------------------

/// Normalize a JSON value that may be a single object or an array into a `Vec<T>`.
fn normalize_list<T: serde::de::DeserializeOwned>(val: &serde_json::Value) -> Vec<T> {
    if val.is_null() {
        return Vec::new();
    }
    if val.is_array() {
        serde_json::from_value(val.clone()).unwrap_or_default()
    } else if val.is_object() {
        match serde_json::from_value::<T>(val.clone()) {
            Ok(item) => vec![item],
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
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
    let files_val = content.get("file").cloned().unwrap_or(serde_json::Value::Null);
    Ok(normalize_list::<FileEntry>(&files_val))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_list_array() {
        let val: serde_json::Value = serde_json::json!([
            {"id": 1, "name": "a"},
            {"id": 2, "name": "b"},
        ]);
        let dirs: Vec<Directory> = normalize_list(&val);
        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0].name, "a");
    }

    #[test]
    fn test_normalize_list_single_object() {
        let val: serde_json::Value = serde_json::json!({"id": 1, "name": "a"});
        let dirs: Vec<Directory> = normalize_list(&val);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "a");
    }

    #[test]
    fn test_normalize_list_null() {
        let val = serde_json::Value::Null;
        let dirs: Vec<Directory> = normalize_list(&val);
        assert!(dirs.is_empty());
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
    fn test_get_download_url() {
        let client = IdgamesClient::new();
        let entry = FileEntry {
            id: 1,
            title: String::new(),
            dir: "levels/doom2/Ports/megawads/".to_string(),
            filename: "sunlust.zip".to_string(),
            size: 0,
            age: 0,
            date: String::new(),
            author: String::new(),
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
        let url = client.get_download_url(&entry, 0);
        assert_eq!(
            url,
            "https://youfailit.net/pub/idgames/levels/doom2/Ports/megawads/sunlust.zip"
        );
    }

    #[test]
    fn test_get_download_url_mirror_wrap() {
        let client = IdgamesClient::new();
        let entry = FileEntry {
            id: 1,
            title: String::new(),
            dir: "levels/doom2/".to_string(),
            filename: "test.zip".to_string(),
            size: 0,
            age: 0,
            date: String::new(),
            author: String::new(),
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

        // Mirror index wraps around
        let url0 = client.get_download_url(&entry, 0);
        let url5 = client.get_download_url(&entry, 5);
        assert_eq!(url0, url5);
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
}
