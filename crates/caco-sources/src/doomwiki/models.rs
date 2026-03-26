/// A search result from the MediaWiki search API.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub page_id: i64,
    pub title: String,
    pub snippet: String,
}

/// Parsed WAD data from a Doom Wiki page.
///
/// Constructed by the wikitext parser, not deserialized from JSON directly.
#[derive(Debug, Clone)]
pub struct WikiEntry {
    pub page_id: i64,
    /// Wiki page title.
    pub title: String,
    /// Name from infobox (may differ from page title).
    pub name: String,
    pub author: String,
    pub year: Option<i32>,
    /// Required IWAD (e.g., "Doom II", "Ultimate Doom").
    pub iwad: String,
    /// Required source port (e.g., "Limit-removing", "GZDoom").
    pub port: String,
    /// Download URL (often idgames).
    pub link: String,
    /// First paragraph of wiki page.
    pub description: String,
    /// URL to the wiki page.
    pub wiki_url: String,
}

impl WikiEntry {
    /// Return the best available name for display.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.title
        } else {
            &self.name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_name_with_name() {
        let entry = WikiEntry {
            page_id: 1,
            title: "Page Title".to_string(),
            name: "Actual Name".to_string(),
            author: String::new(),
            year: None,
            iwad: String::new(),
            port: String::new(),
            link: String::new(),
            description: String::new(),
            wiki_url: String::new(),
        };
        assert_eq!(entry.display_name(), "Actual Name");
    }

    #[test]
    fn test_display_name_fallback() {
        let entry = WikiEntry {
            page_id: 1,
            title: "Page Title".to_string(),
            name: String::new(),
            author: String::new(),
            year: None,
            iwad: String::new(),
            port: String::new(),
            link: String::new(),
            description: String::new(),
            wiki_url: String::new(),
        };
        assert_eq!(entry.display_name(), "Page Title");
    }
}
