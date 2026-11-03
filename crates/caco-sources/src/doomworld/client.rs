use reqwest::blocking::Client;

use super::models::ForumThread;
use super::parser::DoomworldParser;
use crate::error::{Result, SourceError};
use crate::http::build_client;

const USER_AGENT: &str = "Caco/1.0 (Doom WAD library manager; https://github.com/eshen/caco)";

/// Client for fetching Doomworld forum threads.
///
/// Fetches forum thread pages and parses metadata using the `DoomworldParser`.
pub struct DoomworldClient {
    client: Client,
    parser: DoomworldParser,
}

impl DoomworldClient {
    /// Create a new client with default settings.
    pub fn new() -> Self {
        Self {
            client: build_client(None, Some(USER_AGENT)),
            parser: DoomworldParser::new(),
        }
    }

    /// Fetch and parse a forum thread by URL.
    ///
    /// Validates URL, fetches HTML, and parses metadata.
    pub fn get_thread(&self, url: &str) -> Result<ForumThread> {
        // Validate URL — accept both new and old formats
        if !url.contains("doomworld.com/forum/topic/") && !url.contains("doomworld.com/vb/thread/")
        {
            return Err(SourceError::Api(format!(
                "Invalid Doomworld forum URL: {url}"
            )));
        }

        let response = self.client.get(url).send()?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(SourceError::Api(format!("Thread not found: {url}")));
        }
        response.error_for_status_ref()?;

        let html_content = response.text()?;
        let thread = self.parser.parse(&html_content, url);

        if thread.thread_id == 0 {
            return Err(SourceError::Api(format!(
                "Could not extract thread ID from URL: {url}"
            )));
        }

        Ok(thread)
    }
}

impl Default for DoomworldClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_url() {
        let client = DoomworldClient::new();
        let result = client.get_thread("https://example.com/not-doomworld");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid Doomworld forum URL"));
    }
}
