use reqwest::blocking::{Client, Response};

use super::models::ForumThread;
use super::parser::DoomworldParser;
use crate::error::{Result, SourceError};
use crate::http::{build_client, waf_challenge};

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
            client: build_client(),
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
        let html_content = read_thread_page(response, url)?;
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

/// Turn a forum response into the thread's HTML, or the error it amounts to.
fn read_thread_page(response: Response, url: &str) -> Result<String> {
    if let Some(kind) = waf_challenge(&response) {
        return Err(SourceError::WafBlocked {
            site: "Doomworld",
            kind,
        });
    }

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(SourceError::Api(format!("Thread not found: {url}")));
    }
    response.error_for_status_ref()?;

    Ok(response.text()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{WafKind, canned_response};

    const URL: &str = "https://www.doomworld.com/forum/topic/134292-myhousewad/";

    /// The bug this client had: it looked for no challenge at all.
    #[test]
    fn test_read_thread_page_reports_a_cloudflare_challenge_as_blocked() {
        let err = read_thread_page(
            canned_response(
                403,
                &[("server", "cloudflare"), ("cf-mitigated", "challenge")],
                "<html>Just a moment...</html>",
            ),
            URL,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            SourceError::WafBlocked {
                site: "Doomworld",
                kind: WafKind::Cloudflare,
            }
        ));
    }

    /// The worst way to miss one: a 202 passes `error_for_status`, the parser
    /// accepts any HTML, and the thread id comes from the URL, so the interstitial
    /// would import as a library row carrying its title, or none.
    #[test]
    fn test_read_thread_page_does_not_parse_an_aws_challenge() {
        let err = read_thread_page(
            canned_response(
                202,
                &[("x-amzn-waf-action", "challenge")],
                "<html>challenge script</html>",
            ),
            URL,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            SourceError::WafBlocked {
                kind: WafKind::AwsWaf,
                ..
            }
        ));
    }

    #[test]
    fn test_read_thread_page_reports_a_missing_thread() {
        let err = read_thread_page(canned_response(404, &[], "Not Found"), URL).unwrap_err();

        assert!(matches!(err, SourceError::Api(msg) if msg.contains("Thread not found")));
    }

    #[test]
    fn test_read_thread_page_keeps_the_status_of_a_plain_403() {
        let err = read_thread_page(canned_response(403, &[], "Forbidden"), URL).unwrap_err();

        let SourceError::Http(http) = err else {
            panic!("expected an HTTP error, got {err:?}");
        };
        assert_eq!(http.status(), Some(reqwest::StatusCode::FORBIDDEN));
    }

    #[test]
    fn test_read_thread_page_returns_the_html() {
        let html = read_thread_page(
            canned_response(200, &[("server", "cloudflare")], "<html>a thread</html>"),
            URL,
        )
        .unwrap();

        assert_eq!(html, "<html>a thread</html>");
    }

    #[test]
    fn test_invalid_url() {
        let client = DoomworldClient::new();
        let result = client.get_thread("https://example.com/not-doomworld");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid Doomworld forum URL"));
    }
}
