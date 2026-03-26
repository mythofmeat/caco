use serde::Deserialize;

use crate::http::{coerce_f64, coerce_i64, coerce_str};

/// A review for a file on the idgames archive.
#[derive(Debug, Clone, Deserialize)]
pub struct Review {
    #[serde(default, deserialize_with = "coerce_str")]
    pub text: String,
    pub vote: Option<i64>,
    #[serde(default, deserialize_with = "coerce_str")]
    pub username: String,
}

/// File metadata from the idgames archive.
#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    #[serde(default, deserialize_with = "coerce_i64")]
    pub id: i64,
    #[serde(default, deserialize_with = "coerce_str")]
    pub title: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub dir: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub filename: String,
    #[serde(default, deserialize_with = "coerce_i64")]
    pub size: i64,
    #[serde(default, deserialize_with = "coerce_i64")]
    pub age: i64,
    #[serde(default, deserialize_with = "coerce_str")]
    pub date: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub author: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub email: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub description: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub credits: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub base: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub buildtime: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub editors: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub bugs: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub textfile: String,
    #[serde(default, deserialize_with = "coerce_f64")]
    pub rating: f64,
    #[serde(default, deserialize_with = "coerce_i64")]
    pub votes: i64,
    #[serde(default, deserialize_with = "coerce_str")]
    pub url: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub idgamesurl: String,
    /// Reviews are populated by the client after normalization
    /// (the API returns a nested dict/list structure).
    #[serde(default, skip_deserializing)]
    pub reviews: Vec<Review>,
}

/// Directory info from the idgames archive.
#[derive(Debug, Clone, Deserialize)]
pub struct Directory {
    #[serde(default, deserialize_with = "coerce_i64")]
    pub id: i64,
    #[serde(default, deserialize_with = "coerce_str")]
    pub name: String,
}

/// A vote entry.
#[derive(Debug, Clone, Deserialize)]
pub struct Vote {
    #[serde(default, deserialize_with = "coerce_i64")]
    pub id: i64,
    #[serde(default, deserialize_with = "coerce_i64")]
    pub file: i64,
    #[serde(default, deserialize_with = "coerce_str")]
    pub title: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub author: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub description: String,
    #[serde(default, deserialize_with = "coerce_f64")]
    pub rating: f64,
    #[serde(default, deserialize_with = "coerce_str")]
    pub reviewtext: String,
}

/// API information from the about endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiInfo {
    #[serde(default, deserialize_with = "coerce_str")]
    pub version: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub credits: String,
    #[serde(default, deserialize_with = "coerce_str")]
    pub copyright: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_entry_null_fields() {
        let json = r#"{
            "id": 12345,
            "title": null,
            "dir": "levels/doom2/",
            "filename": "test.wad",
            "size": null,
            "age": 0,
            "date": "2023-01-01",
            "author": null,
            "email": "",
            "description": "A test WAD",
            "credits": null,
            "base": null,
            "buildtime": null,
            "editors": null,
            "bugs": null,
            "textfile": null,
            "rating": null,
            "votes": null,
            "url": "https://example.com",
            "idgamesurl": null
        }"#;
        let entry: FileEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id, 12345);
        assert_eq!(entry.title, "");
        assert_eq!(entry.author, "");
        assert_eq!(entry.size, 0);
        assert_eq!(entry.rating, 0.0);
        assert_eq!(entry.votes, 0);
        assert!(entry.reviews.is_empty());
    }

    #[test]
    fn test_file_entry_minimal() {
        let json = r#"{"id": 1, "filename": "a.wad"}"#;
        let entry: FileEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id, 1);
        assert_eq!(entry.filename, "a.wad");
        assert_eq!(entry.title, "");
    }

    #[test]
    fn test_review_deserialization() {
        let json = r#"{"text": "Great!", "vote": 5, "username": "user1"}"#;
        let review: Review = serde_json::from_str(json).unwrap();
        assert_eq!(review.text, "Great!");
        assert_eq!(review.vote, Some(5));
        assert_eq!(review.username, "user1");
    }

    #[test]
    fn test_review_null_fields() {
        let json = r#"{"text": null, "vote": null, "username": null}"#;
        let review: Review = serde_json::from_str(json).unwrap();
        assert_eq!(review.text, "");
        assert!(review.vote.is_none());
        assert_eq!(review.username, "");
    }

    #[test]
    fn test_directory() {
        let json = r#"{"id": 1, "name": "levels/doom2/"}"#;
        let dir: Directory = serde_json::from_str(json).unwrap();
        assert_eq!(dir.id, 1);
        assert_eq!(dir.name, "levels/doom2/");
    }

    #[test]
    fn test_vote() {
        let json = r#"{"id": 1, "file": 100, "title": "Test", "author": "Me", "description": "Desc", "rating": 4.5, "reviewtext": "Nice"}"#;
        let vote: Vote = serde_json::from_str(json).unwrap();
        assert_eq!(vote.id, 1);
        assert_eq!(vote.rating, 4.5);
    }

    #[test]
    fn test_api_info() {
        let json = r#"{"version": "3", "credits": "dn", "copyright": "c"}"#;
        let info: ApiInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.version, "3");
    }
}
