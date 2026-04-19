/// Parsed data from a Doomworld forum thread.
///
/// Constructed by the parser, not deserialized from JSON directly.
#[derive(Debug, Clone)]
pub struct ForumThread {
    /// Extracted from URL: /forum/topic/{id}-{slug}/
    pub thread_id: i64,
    /// Thread title.
    pub title: String,
    /// OP username.
    pub author: String,
    /// ISO date string.
    pub posted_date: String,
    /// HTML content of first post.
    pub first_post_html: String,
    /// Plain text of first post (stripped HTML).
    pub first_post_text: String,
    /// Full URL to the thread.
    pub thread_url: String,
    /// URLs to download files found in the post.
    pub download_links: Vec<String>,
    /// Compatibility level (e.g., 9 for Boom).
    pub complevel: Option<i32>,
    /// Required IWAD (e.g., "doom2", "plutonia").
    pub iwad: Option<String>,
    /// Required sourceport (e.g., "gzdoom", "dsda-doom").
    pub sourceport: Option<String>,
    /// Version string scraped from the title or post body (e.g., "v1.0", "RC3").
    pub version: Option<String>,
}

impl ForumThread {
    /// Return the best available name for display.
    pub fn display_name(&self) -> &str {
        &self.title
    }

    /// Check if any technical metadata was extracted.
    pub fn has_technical_info(&self) -> bool {
        !self.download_links.is_empty()
            || self.complevel.is_some()
            || self.iwad.is_some()
            || self.sourceport.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_thread() -> ForumThread {
        ForumThread {
            thread_id: 12345,
            title: "My Cool WAD".to_string(),
            author: "mapper".to_string(),
            posted_date: "2024-01-15".to_string(),
            first_post_html: String::new(),
            first_post_text: String::new(),
            thread_url: "https://www.doomworld.com/forum/topic/12345-my-cool-wad/".to_string(),
            download_links: Vec::new(),
            complevel: None,
            iwad: None,
            sourceport: None,
            version: None,
        }
    }

    #[test]
    fn test_display_name() {
        let t = make_thread();
        assert_eq!(t.display_name(), "My Cool WAD");
    }

    #[test]
    fn test_has_technical_info_empty() {
        let t = make_thread();
        assert!(!t.has_technical_info());
    }

    #[test]
    fn test_has_technical_info_complevel() {
        let mut t = make_thread();
        t.complevel = Some(9);
        assert!(t.has_technical_info());
    }

    #[test]
    fn test_has_technical_info_downloads() {
        let mut t = make_thread();
        t.download_links = vec!["https://example.com/test.zip".to_string()];
        assert!(t.has_technical_info());
    }
}
