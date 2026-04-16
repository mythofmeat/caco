use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use super::models::ForumThread;

// =============================================================================
// Complevel Detection
// =============================================================================

/// How to extract a complevel value from a regex match.
enum ComplevelExtractor {
    /// Use the capture group at the given index, parse as i32.
    CaptureGroup(usize),
    /// Return a constant value.
    Constant(i32),
}

/// (compiled regex, extractor)
static COMPLEVEL_PATTERNS: LazyLock<Vec<(Regex, ComplevelExtractor)>> = LazyLock::new(|| {
    vec![
        // Explicit complevel mentions
        (
            Regex::new(r"(?i)\bcomplevel\s*[-:]?\s*(\d{1,2})\b").unwrap(),
            ComplevelExtractor::CaptureGroup(1),
        ),
        (
            Regex::new(r"(?i)\bcl\s*[-:]?\s*(\d{1,2})\b").unwrap(),
            ComplevelExtractor::CaptureGroup(1),
        ),
        (
            Regex::new(r"(?i)\b-complevel\s+(\d{1,2})\b").unwrap(),
            ComplevelExtractor::CaptureGroup(1),
        ),
        // Named compatibility levels
        (
            Regex::new(r"(?i)\bvanilla\s*(?:doom|compatible|compat)?\b").unwrap(),
            ComplevelExtractor::Constant(2),
        ),
        (
            Regex::new(r"(?i)\bdoom\s*2?\s*vanilla\b").unwrap(),
            ComplevelExtractor::Constant(2),
        ),
        (
            Regex::new(r"(?i)\bchocolate\s*doom\b").unwrap(),
            ComplevelExtractor::Constant(2),
        ),
        (
            Regex::new(r"(?i)\blimit[- ]?removing\b").unwrap(),
            ComplevelExtractor::Constant(2),
        ),
        (
            Regex::new(r"(?i)\bboom\s*(?:compatible|compat)?\b").unwrap(),
            ComplevelExtractor::Constant(9),
        ),
        (
            Regex::new(r"(?i)\bmbf\s*(?:compatible|compat)?\b").unwrap(),
            ComplevelExtractor::Constant(11),
        ),
        (
            Regex::new(r"(?i)\bmbf21\b").unwrap(),
            ComplevelExtractor::Constant(21),
        ),
        (
            Regex::new(r"(?i)\bdsda[- ]?doom\b").unwrap(),
            ComplevelExtractor::Constant(21),
        ),
    ]
});

/// Extract complevel from post text.
fn extract_complevel(text: &str) -> Option<i32> {
    for (re, extractor) in COMPLEVEL_PATTERNS.iter() {
        if let Some(m) = re.find(text) {
            return match extractor {
                ComplevelExtractor::CaptureGroup(idx) => {
                    let caps = re.captures(&text[m.start()..])?;
                    caps.get(*idx)?.as_str().parse().ok()
                }
                ComplevelExtractor::Constant(val) => Some(*val),
            };
        }
    }
    None
}

// =============================================================================
// IWAD Detection
// =============================================================================

/// (compiled regex, normalized IWAD name)
static IWAD_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        // Doom II variants (check first)
        (Regex::new(r"(?i)\bdoom\s*(?:ii|2)\b").unwrap(), "doom2"),
        (
            Regex::new(r"(?i)\bfor\s+doom\s*(?:ii|2)\b").unwrap(),
            "doom2",
        ),
        (
            Regex::new(r"(?i)\brequires?\s+doom\s*(?:ii|2)\b").unwrap(),
            "doom2",
        ),
        (Regex::new(r"(?i)\bdoom2\.wad\b").unwrap(), "doom2"),
        // Final Doom
        (Regex::new(r"(?i)\btnt\.wad\b").unwrap(), "tnt"),
        (Regex::new(r"(?i)\btnt[:\s]+evilution\b").unwrap(), "tnt"),
        (Regex::new(r"(?i)\bevilution\b").unwrap(), "tnt"),
        (
            Regex::new(r"(?i)\bpluton(?:ia)?\.wad\b").unwrap(),
            "plutonia",
        ),
        (
            Regex::new(r"(?i)\bpluton(?:ia)?\s*(?:experiment)?\b").unwrap(),
            "plutonia",
        ),
        (Regex::new(r"(?i)\bfinal\s*doom\b").unwrap(), "finaldoom"),
        // Ultimate Doom / Doom 1
        (Regex::new(r"(?i)\bultimate\s*doom\b").unwrap(), "doom"),
        (Regex::new(r"(?i)\bdoom\.wad\b").unwrap(), "doom"),
        (Regex::new(r"(?i)\bdoom\s*1\b").unwrap(), "doom"),
        (Regex::new(r"(?i)\bfor\s+doom\s*1?\b").unwrap(), "doom"),
        // Note: "requires doom" without "II"/"2" — Doom II patterns above match first
        (Regex::new(r"(?i)\brequires?\s+doom\b").unwrap(), "doom"),
        // Heretic
        (Regex::new(r"(?i)\bheretic\b").unwrap(), "heretic"),
        // Hexen
        (Regex::new(r"(?i)\bhexen\b").unwrap(), "hexen"),
        // Strife
        (Regex::new(r"(?i)\bstrife\b").unwrap(), "strife"),
        // Chex Quest
        (Regex::new(r"(?i)\bchex\s*quest\b").unwrap(), "chex"),
        // FreeDoom
        (Regex::new(r"(?i)\bfreedoom\b").unwrap(), "freedoom"),
    ]
});

/// Extract IWAD requirement from post text.
fn extract_iwad(text: &str) -> Option<&'static str> {
    for (re, iwad) in IWAD_PATTERNS.iter() {
        if re.is_match(text) {
            return Some(iwad);
        }
    }
    None
}

// =============================================================================
// Sourceport Detection
// =============================================================================

/// (compiled regex, normalized sourceport name)
static SOURCEPORT_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        // GZDoom family
        (Regex::new(r"(?i)\bgzdoom\b").unwrap(), "gzdoom"),
        (Regex::new(r"(?i)\blzdoom\b").unwrap(), "lzdoom"),
        (Regex::new(r"(?i)\bvkdoom\b").unwrap(), "vkdoom"),
        (Regex::new(r"(?i)\bqzdoom\b").unwrap(), "qzdoom"),
        (Regex::new(r"(?i)\bzdoom\b").unwrap(), "zdoom"),
        // DSDA-Doom / PrBoom family
        (Regex::new(r"(?i)\bdsda[- ]?doom\b").unwrap(), "dsda-doom"),
        (Regex::new(r"(?i)\bprboom\+?\b").unwrap(), "prboom+"),
        (Regex::new(r"(?i)\bglboom\+?\b").unwrap(), "glboom+"),
        (Regex::new(r"(?i)\bumapinfo\b").unwrap(), "dsda-doom"),
        // Eternity
        (
            Regex::new(r"(?i)\beternity\s*(?:engine)?\b").unwrap(),
            "eternity",
        ),
        // Crispy Doom
        (Regex::new(r"(?i)\bcrispy\s*doom\b").unwrap(), "crispy-doom"),
        // Chocolate Doom
        (
            Regex::new(r"(?i)\bchocolate\s*doom\b").unwrap(),
            "chocolate-doom",
        ),
        // Woof!
        (Regex::new(r"(?i)\bwoof!?\b").unwrap(), "woof"),
        // Nugget Doom
        (Regex::new(r"(?i)\bnugget\s*doom\b").unwrap(), "nugget-doom"),
        // EDGE
        (Regex::new(r"(?i)\bedge(?:-classic)?\b").unwrap(), "edge"),
        // Doomsday
        (Regex::new(r"(?i)\bdoomsday\b").unwrap(), "doomsday"),
        // Zandronum
        (Regex::new(r"(?i)\bzandronum\b").unwrap(), "zandronum"),
        // Odamex
        (Regex::new(r"(?i)\bodamex\b").unwrap(), "odamex"),
        // 3DGE
        (Regex::new(r"(?i)\b3dge\b").unwrap(), "3dge"),
        // Limit-removing (generic)
        (
            Regex::new(r"(?i)\blimit[- ]?removing\b").unwrap(),
            "limit-removing",
        ),
    ]
});

/// Extract sourceport requirement from post text.
fn extract_sourceport(text: &str) -> Option<&'static str> {
    for (re, port) in SOURCEPORT_PATTERNS.iter() {
        if re.is_match(text) {
            return Some(port);
        }
    }
    None
}

// =============================================================================
// Download Link Extraction
// =============================================================================

static DOWNLOAD_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // Direct file downloads
        Regex::new(r#"(?i)https?://[^\s<>"'\)\]]+\.(?:zip|wad|pk3|pk7|7z|rar|tar\.gz)"#).unwrap(),
        // Dropbox
        Regex::new(r#"(?i)https?://(?:www\.)?dropbox\.com/[^\s<>"'\)\]]+"#).unwrap(),
        Regex::new(r#"(?i)https?://dl\.dropbox(?:usercontent)?\.com/[^\s<>"'\)\]]+"#).unwrap(),
        // Google Drive
        Regex::new(r#"(?i)https?://drive\.google\.com/[^\s<>"'\)\]]+"#).unwrap(),
        // Mediafire
        Regex::new(r#"(?i)https?://(?:www\.)?mediafire\.com/[^\s<>"'\)\]]+"#).unwrap(),
        // Mega
        Regex::new(r#"(?i)https?://mega\.(?:nz|co\.nz)/[^\s<>"'\)\]]+"#).unwrap(),
        // GitHub releases
        Regex::new(r#"(?i)https?://github\.com/[^\s<>"'\)\]]+/releases/[^\s<>"'\)\]]+"#).unwrap(),
        Regex::new(r#"(?i)https?://github\.com/[^\s<>"'\)\]]+\.(?:zip|wad|pk3|pk7)"#).unwrap(),
        // itch.io
        Regex::new(r#"(?i)https?://[^\s<>"'\)\]]+\.itch\.io/[^\s<>"'\)\]]+"#).unwrap(),
        // ModDB
        Regex::new(r#"(?i)https?://(?:www\.)?moddb\.com/[^\s<>"'\)\]]+/downloads/[^\s<>"'\)\]]+"#)
            .unwrap(),
        // Doomworld idgames
        Regex::new(r#"(?i)https?://(?:www\.)?doomworld\.com/idgames/[^\s<>"'\)\]]+"#).unwrap(),
        // idgames mirror
        Regex::new(r#"(?i)https?://[^\s<>"'\)\]]*idgames[^\s<>"'\)\]]*\.(?:zip|wad)"#).unwrap(),
        // GameBanana
        Regex::new(r#"(?i)https?://(?:www\.)?gamebanana\.com/[^\s<>"'\)\]]+"#).unwrap(),
        // OneDrive
        Regex::new(r#"(?i)https?://(?:1drv\.ms|onedrive\.live\.com)/[^\s<>"'\)\]]+"#).unwrap(),
        // Catbox
        Regex::new(r#"(?i)https?://files\.catbox\.moe/[^\s<>"'\)\]]+"#).unwrap(),
        // Litterbox
        Regex::new(r#"(?i)https?://litter\.catbox\.moe/[^\s<>"'\)\]]+"#).unwrap(),
    ]
});

static HREF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)href=["']([^"']+)["']"#).unwrap());

/// Extract potential download URLs from post text.
fn extract_download_links(text: &str) -> Vec<String> {
    let mut all_urls: Vec<String> = Vec::new();

    // Check hrefs first (more reliable than text matching)
    for caps in HREF_PATTERN.captures_iter(text) {
        let href = &caps[1];
        for pattern in DOWNLOAD_PATTERNS.iter() {
            if pattern.is_match(href) {
                all_urls.push(href.to_string());
                break;
            }
        }
    }

    // Then check plain text
    for pattern in DOWNLOAD_PATTERNS.iter() {
        for m in pattern.find_iter(text) {
            all_urls.push(m.as_str().to_string());
        }
    }

    // Deduplicate while preserving order
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for url in all_urls {
        let cleaned = url
            .trim_end_matches(['.', ',', ';', ':', '!', '?'])
            .to_string();
        let key = cleaned.to_lowercase();
        if seen.insert(key) {
            unique.push(cleaned);
        }
    }

    unique
}

// =============================================================================
// HTML Parsing Helpers
// =============================================================================

static JSON_LD_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script[^>]*type=["']application/ld\+json["'][^>]*>(.*?)</script>"#).unwrap()
});

static HTML_TITLE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());

static FIRST_POST_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<div[^>]*data-role=["']commentContent["'][^>]*>(.*?)</div>\s*(?:<div[^>]*class=["'][^"']*ipsSigned|</article)"#,
    )
    .unwrap()
});

static FIRST_POST_FALLBACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<article[^>]*>.*?<div[^>]*class=["'][^"']*ipsType_richText[^"']*["'][^>]*>(.*?)</div>"#,
    )
    .unwrap()
});

static THREAD_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(?:forum/topic|vb/thread)/(\d+)").unwrap());

// HTML-to-text patterns
static BR_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?\s*>").unwrap());
static P_CLOSE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</p>").unwrap());
static DIV_CLOSE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</div>").unwrap());
static LI_CLOSE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</li>").unwrap());
static ALL_TAGS_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static MULTI_NEWLINE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").unwrap());
static MULTI_SPACE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" +").unwrap());

// =============================================================================
// Main Parser
// =============================================================================

/// Parser for extracting metadata from Doomworld forum thread pages.
///
/// Uses a multi-strategy approach:
/// 1. JSON-LD structured data (preferred, most reliable)
/// 2. HTML meta tags and content (fallback)
/// 3. Regex-based extraction for technical requirements
pub struct DoomworldParser;

impl DoomworldParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse a Doomworld forum thread page.
    pub fn parse(&self, html_content: &str, url: &str) -> ForumThread {
        let thread_id = self.extract_thread_id(url);
        let mut title = String::new();
        let mut author = String::new();
        let mut posted_date = String::new();

        // Try JSON-LD first (most reliable)
        if let Some(json_ld) = self.extract_json_ld(html_content) {
            title = json_ld
                .get("headline")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let author_data = json_ld.get("author");
            if let Some(obj) = author_data.and_then(|a| a.as_object()) {
                author = obj
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            } else if let Some(s) = author_data.and_then(|a| a.as_str()) {
                author = s.to_string();
            }

            posted_date = json_ld
                .get("dateCreated")
                .or_else(|| json_ld.get("datePublished"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }

        // Fallback to HTML title
        if title.is_empty() {
            title = self.extract_html_title(html_content);
        }

        // Extract first post content
        let first_post_html = self.extract_first_post(html_content);
        let first_post_text = html_to_text(&first_post_html);

        // Extract technical metadata from first post
        let combined_text = format!("{first_post_html} {first_post_text}");
        let download_links = extract_download_links(&combined_text);
        let complevel = extract_complevel(&combined_text);
        let iwad = extract_iwad(&combined_text).map(|s| s.to_string());
        let sourceport = extract_sourceport(&combined_text).map(|s| s.to_string());

        ForumThread {
            thread_id,
            title,
            author,
            posted_date,
            first_post_html,
            first_post_text,
            thread_url: url.to_string(),
            download_links,
            complevel,
            iwad,
            sourceport,
        }
    }

    /// Extract thread ID from Doomworld forum URL.
    fn extract_thread_id(&self, url: &str) -> i64 {
        THREAD_ID_PATTERN
            .captures(url)
            .and_then(|caps| caps.get(1)?.as_str().parse().ok())
            .unwrap_or(0)
    }

    /// Extract JSON-LD structured data from HTML.
    fn extract_json_ld(&self, html_content: &str) -> Option<serde_json::Value> {
        for caps in JSON_LD_PATTERN.captures_iter(html_content) {
            let json_str = caps.get(1)?.as_str().trim();
            let data: serde_json::Value = match serde_json::from_str(json_str) {
                Ok(d) => d,
                Err(_) => continue,
            };

            // Handle @graph format
            if let Some(graph) = data.get("@graph").and_then(|g| g.as_array()) {
                for item in graph {
                    if item.get("@type").and_then(|t| t.as_str()) == Some("DiscussionForumPosting")
                    {
                        return Some(item.clone());
                    }
                }
            }
            // Direct DiscussionForumPosting
            if data.get("@type").and_then(|t| t.as_str()) == Some("DiscussionForumPosting") {
                return Some(data);
            }
            // Array of items
            if let Some(arr) = data.as_array() {
                for item in arr {
                    if item.get("@type").and_then(|t| t.as_str()) == Some("DiscussionForumPosting")
                    {
                        return Some(item.clone());
                    }
                }
            }
        }
        None
    }

    /// Extract title from HTML `<title>` tag, cleaning up suffix.
    fn extract_html_title(&self, html_content: &str) -> String {
        let caps = match HTML_TITLE_PATTERN.captures(html_content) {
            Some(c) => c,
            None => return String::new(),
        };
        let mut title = caps
            .get(1)
            .map(|m| m.as_str().trim())
            .unwrap_or("")
            .to_string();

        // Remove common suffixes
        for suffix in [" - Doomworld", " - WADs & Mods", " - Everything Else"] {
            if let Some(stripped) = title.strip_suffix(suffix) {
                title = stripped.to_string();
            }
        }

        html_escape::decode_html_entities(&title).trim().to_string()
    }

    /// Extract HTML content of the first post.
    fn extract_first_post(&self, html_content: &str) -> String {
        // Try data-role attribute first (Invision Community 4.x)
        if let Some(caps) = FIRST_POST_PATTERN.captures(html_content) {
            return caps
                .get(1)
                .map(|m| m.as_str().trim())
                .unwrap_or("")
                .to_string();
        }

        // Fallback: ipsType_richText inside first article
        if let Some(caps) = FIRST_POST_FALLBACK.captures(html_content) {
            return caps
                .get(1)
                .map(|m| m.as_str().trim())
                .unwrap_or("")
                .to_string();
        }

        String::new()
    }
}

impl Default for DoomworldParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert HTML to plain text, preserving paragraph breaks.
fn html_to_text(html_content: &str) -> String {
    if html_content.is_empty() {
        return String::new();
    }

    let mut text = html_content.to_string();

    // Replace block elements with newlines
    text = BR_PATTERN.replace_all(&text, "\n").to_string();
    text = P_CLOSE_PATTERN.replace_all(&text, "\n\n").to_string();
    text = DIV_CLOSE_PATTERN.replace_all(&text, "\n").to_string();
    text = LI_CLOSE_PATTERN.replace_all(&text, "\n").to_string();

    // Remove all other tags
    text = ALL_TAGS_PATTERN.replace_all(&text, "").to_string();

    // Decode HTML entities
    text = html_escape::decode_html_entities(&text).to_string();

    // Normalize whitespace
    text = MULTI_NEWLINE_PATTERN.replace_all(&text, "\n\n").to_string();
    text = MULTI_SPACE_PATTERN.replace_all(&text, " ").to_string();

    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Complevel detection
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_complevel_explicit() {
        assert_eq!(extract_complevel("complevel 9"), Some(9));
        assert_eq!(extract_complevel("cl21"), Some(21));
        assert_eq!(extract_complevel("-complevel 11"), Some(11));
        assert_eq!(extract_complevel("complevel: 2"), Some(2));
    }

    #[test]
    fn test_extract_complevel_named() {
        assert_eq!(extract_complevel("vanilla compatible"), Some(2));
        assert_eq!(extract_complevel("boom compatible"), Some(9));
        assert_eq!(extract_complevel("MBF21"), Some(21));
        assert_eq!(extract_complevel("limit-removing"), Some(2));
    }

    #[test]
    fn test_extract_complevel_none() {
        assert_eq!(extract_complevel("just a regular doom map"), None);
    }

    // -------------------------------------------------------------------------
    // IWAD detection
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_iwad_doom2() {
        assert_eq!(extract_iwad("requires Doom II"), Some("doom2"));
        assert_eq!(extract_iwad("doom2.wad"), Some("doom2"));
        assert_eq!(extract_iwad("for Doom 2"), Some("doom2"));
    }

    #[test]
    fn test_extract_iwad_tnt() {
        assert_eq!(extract_iwad("tnt.wad"), Some("tnt"));
        assert_eq!(extract_iwad("TNT: Evilution"), Some("tnt"));
    }

    #[test]
    fn test_extract_iwad_plutonia() {
        assert_eq!(extract_iwad("plutonia.wad"), Some("plutonia"));
    }

    #[test]
    fn test_extract_iwad_doom1() {
        assert_eq!(extract_iwad("Ultimate Doom"), Some("doom"));
        assert_eq!(extract_iwad("doom.wad"), Some("doom"));
    }

    #[test]
    fn test_extract_iwad_heretic() {
        assert_eq!(extract_iwad("Heretic"), Some("heretic"));
    }

    #[test]
    fn test_extract_iwad_none() {
        assert_eq!(extract_iwad("no iwad info here"), None);
    }

    // -------------------------------------------------------------------------
    // Sourceport detection
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_sourceport() {
        assert_eq!(extract_sourceport("GZDoom required"), Some("gzdoom"));
        assert_eq!(extract_sourceport("tested in dsda-doom"), Some("dsda-doom"));
        assert_eq!(extract_sourceport("Eternity Engine"), Some("eternity"));
        assert_eq!(extract_sourceport("crispy doom"), Some("crispy-doom"));
    }

    #[test]
    fn test_extract_sourceport_none() {
        assert_eq!(extract_sourceport("no port mentioned"), None);
    }

    // -------------------------------------------------------------------------
    // Download links
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_download_links_direct() {
        let text = "Download here: https://example.com/test.zip";
        let links = extract_download_links(text);
        assert_eq!(links, vec!["https://example.com/test.zip"]);
    }

    #[test]
    fn test_extract_download_links_href() {
        let text = r#"<a href="https://example.com/test.wad">Download</a>"#;
        let links = extract_download_links(text);
        assert!(!links.is_empty());
        assert!(links[0].contains("test.wad"));
    }

    #[test]
    fn test_extract_download_links_dedup() {
        let text = r#"https://example.com/test.zip https://example.com/test.zip"#;
        let links = extract_download_links(text);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn test_extract_download_links_hosting_services() {
        let text = "https://www.dropbox.com/s/abc/test.zip \
                     https://mega.nz/file/abc \
                     https://drive.google.com/file/d/abc";
        let links = extract_download_links(text);
        assert_eq!(links.len(), 3);
    }

    #[test]
    fn test_extract_download_links_trailing_punct() {
        let text = "Get it at https://example.com/test.zip.";
        let links = extract_download_links(text);
        assert_eq!(links[0], "https://example.com/test.zip");
    }

    // -------------------------------------------------------------------------
    // HTML-to-text
    // -------------------------------------------------------------------------

    #[test]
    fn test_html_to_text_basic() {
        let html = "<p>Hello</p><p>World</p>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_html_to_text_br() {
        let html = "Line 1<br>Line 2<br/>Line 3";
        let text = html_to_text(html);
        assert!(text.contains("Line 1\nLine 2\nLine 3"));
    }

    #[test]
    fn test_html_to_text_entities() {
        let html = "&amp; &lt; &gt; &quot;";
        let text = html_to_text(html);
        assert!(text.contains("& < > \""));
    }

    #[test]
    fn test_html_to_text_empty() {
        assert_eq!(html_to_text(""), "");
    }

    // -------------------------------------------------------------------------
    // Thread ID extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_thread_id() {
        let parser = DoomworldParser::new();
        assert_eq!(
            parser.extract_thread_id("https://www.doomworld.com/forum/topic/134292-myhousewad/"),
            134292
        );
        assert_eq!(
            parser.extract_thread_id(
                "https://www.doomworld.com/forum/topic/134292-myhousewad/?page=5"
            ),
            134292
        );
        assert_eq!(
            parser.extract_thread_id("https://www.doomworld.com/vb/thread/153124"),
            153124
        );
        assert_eq!(parser.extract_thread_id("https://invalid-url.com"), 0);
    }

    // -------------------------------------------------------------------------
    // JSON-LD extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_json_ld() {
        let parser = DoomworldParser::new();
        let html = r#"
            <script type="application/ld+json">
            {
                "@type": "DiscussionForumPosting",
                "headline": "My Cool WAD",
                "author": {"name": "mapper"},
                "dateCreated": "2024-01-15"
            }
            </script>
        "#;
        let result = parser.extract_json_ld(html).unwrap();
        assert_eq!(result["headline"], "My Cool WAD");
        assert_eq!(result["author"]["name"], "mapper");
    }

    #[test]
    fn test_extract_json_ld_graph() {
        let parser = DoomworldParser::new();
        let html = r#"
            <script type="application/ld+json">
            {
                "@graph": [
                    {"@type": "WebPage"},
                    {"@type": "DiscussionForumPosting", "headline": "Found It"}
                ]
            }
            </script>
        "#;
        let result = parser.extract_json_ld(html).unwrap();
        assert_eq!(result["headline"], "Found It");
    }

    #[test]
    fn test_extract_json_ld_none() {
        let parser = DoomworldParser::new();
        assert!(
            parser
                .extract_json_ld("<html><body>no json</body></html>")
                .is_none()
        );
    }

    // -------------------------------------------------------------------------
    // HTML title extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_html_title() {
        let parser = DoomworldParser::new();
        assert_eq!(
            parser.extract_html_title("<title>Cool WAD - WADs &amp; Mods - Doomworld</title>"),
            "Cool WAD - WADs & Mods"
        );
        assert_eq!(
            parser.extract_html_title("<title>Thread Title - Doomworld</title>"),
            "Thread Title"
        );
    }

    #[test]
    fn test_extract_html_title_missing() {
        let parser = DoomworldParser::new();
        assert_eq!(
            parser.extract_html_title("<html><body>no title</body></html>"),
            ""
        );
    }

    // -------------------------------------------------------------------------
    // First post extraction
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_first_post() {
        let parser = DoomworldParser::new();
        let html = r#"
            <div data-role="commentContent">
                <p>First post content here</p>
            </div>
            <div class="ipsSigned">
        "#;
        let post = parser.extract_first_post(html);
        assert!(post.contains("First post content here"));
    }

    #[test]
    fn test_extract_first_post_fallback() {
        let parser = DoomworldParser::new();
        let html = r#"
            <article id="comment-123">
                <div class="ipsType_richText">Fallback content</div>
            </article>
        "#;
        let post = parser.extract_first_post(html);
        assert!(post.contains("Fallback content"));
    }

    // -------------------------------------------------------------------------
    // Full parse integration
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_full() {
        let parser = DoomworldParser::new();
        let html = r#"
            <html>
            <head>
            <title>Test WAD v2 - WADs &amp; Mods - Doomworld</title>
            <script type="application/ld+json">
            {
                "@type": "DiscussionForumPosting",
                "headline": "Test WAD v2",
                "author": {"name": "TestMapper"},
                "dateCreated": "2024-06-15"
            }
            </script>
            </head>
            <body>
            <div data-role="commentContent">
                <p>A Boom compatible map for Doom II.</p>
                <p>Download: <a href="https://example.com/testwad.zip">here</a></p>
            </div>
            <div class="ipsSigned"></div>
            </body>
            </html>
        "#;

        let thread = parser.parse(
            html,
            "https://www.doomworld.com/forum/topic/99999-test-wad-v2/",
        );
        assert_eq!(thread.thread_id, 99999);
        assert_eq!(thread.title, "Test WAD v2");
        assert_eq!(thread.author, "TestMapper");
        assert_eq!(thread.posted_date, "2024-06-15");
        assert_eq!(thread.complevel, Some(9)); // "Boom compatible"
        assert_eq!(thread.iwad.as_deref(), Some("doom2")); // "Doom II"
        assert!(!thread.download_links.is_empty());
        assert!(thread.has_technical_info());
    }
}
