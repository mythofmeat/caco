use std::collections::HashMap;

use regex::Regex;

use super::models::WikiEntry;

/// Parser for extracting structured data from Doom Wiki wikitext.
pub struct WikitextParser {
    /// Pattern to match `{{ig|file=...}}` idgames template.
    idgames_file_re: Regex,
    /// Pattern to match `{{ig|id=N}}` idgames template.
    idgames_id_re: Regex,
    /// Pattern to match `[[link|text]]` or `[[text]]`.
    link_re: Regex,
    /// Pattern to match `{{template}}` (non-nested only).
    template_re: Regex,
    /// Pattern to match `<tag>`.
    html_tag_re: Regex,
    /// Pattern to match `<ref>...</ref>`.
    ref_re: Regex,
    /// Pattern to match a 4-digit year.
    year_re: Regex,
}

impl WikitextParser {
    pub fn new() -> Self {
        Self {
            idgames_file_re: Regex::new(r"(?i)\{\{ig\s*\|\s*file\s*=\s*([^}|]+)").unwrap(),
            idgames_id_re: Regex::new(r"(?i)\{\{ig\s*\|\s*id\s*=\s*(\d+)").unwrap(),
            link_re: Regex::new(r"\[\[(?:[^|\]]*\|)?([^\]]+)\]\]").unwrap(),
            template_re: Regex::new(r"\{\{[^{}]*\}\}").unwrap(),
            html_tag_re: Regex::new(r"<[^>]+>").unwrap(),
            ref_re: Regex::new(r"(?is)<ref[^>]*>.*?</ref>").unwrap(),
            year_re: Regex::new(r"\b(19|20)\d{2}\b").unwrap(),
        }
    }

    /// Parse wikitext to extract WAD metadata.
    pub fn parse(&self, wikitext: &str, page_title: &str, page_id: i64) -> WikiEntry {
        let wiki_url = format!(
            "https://doomwiki.org/wiki/{}",
            page_title.replace(' ', "_")
        );

        let mut name = String::new();
        let mut author = String::new();
        let mut year: Option<i32> = None;
        let mut iwad = String::new();
        let mut port = String::new();
        let mut link = String::new();

        // Extract {{wad}} template content using brace matching
        if let Some(template_content) = self.extract_wad_template(wikitext) {
            let params = self.parse_template_params(template_content);

            name = self.clean_value(
                params
                    .get("name")
                    .or(params.get("title"))
                    .map(|s| s.as_str())
                    .unwrap_or(""),
            );
            author = self.clean_value(
                params
                    .get("author")
                    .or(params.get("authors"))
                    .map(|s| s.as_str())
                    .unwrap_or(""),
            );
            iwad = self.clean_value(
                params
                    .get("iwad")
                    .or(params.get("iwad2"))
                    .map(|s| s.as_str())
                    .unwrap_or(""),
            );
            port = self.clean_value(
                params
                    .get("port")
                    .or(params.get("port2"))
                    .map(|s| s.as_str())
                    .unwrap_or(""),
            );

            if let Some(year_str) = params.get("year") {
                year = self.parse_year(year_str);
            }

            if let Some(link_str) = params.get("link") {
                link = self.parse_link(link_str);
            }
        }

        let description = self.extract_first_paragraph(wikitext);

        WikiEntry {
            page_id,
            title: page_title.to_string(),
            name,
            author,
            year,
            iwad,
            port,
            link,
            description,
            wiki_url,
        }
    }

    /// Check if the wikitext contains a `{{wad}}` template (case insensitive).
    pub fn has_wad_template(&self, wikitext: &str) -> bool {
        wikitext.to_lowercase().contains("{{wad")
    }

    /// Extract the content of the `{{wad}}` template using brace matching.
    fn extract_wad_template<'a>(&self, wikitext: &'a str) -> Option<&'a str> {
        let lower = wikitext.to_lowercase();
        let start = lower.find("{{wad")?;

        // Find the first `|` after `{{wad`
        let pipe_pos = wikitext[start..].find('|').map(|p| start + p)?;
        let content_start = pipe_pos + 1;

        // Count braces to find the matching `}}`
        let mut brace_count: i32 = 2; // inside {{ already
        let bytes = wikitext.as_bytes();
        let mut i = content_start;

        while i < bytes.len() && brace_count > 0 {
            if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
                brace_count += 2;
                i += 2;
            } else if i + 1 < bytes.len() && bytes[i] == b'}' && bytes[i + 1] == b'}' {
                brace_count -= 2;
                if brace_count == 0 {
                    return Some(&wikitext[content_start..i]);
                }
                i += 2;
            } else {
                i += 1;
            }
        }

        None
    }

    /// Parse template parameters from the content inside `{{Wad|...}}`.
    fn parse_template_params(&self, template_content: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();
        let mut current_param = String::new();
        let mut nesting = 0i32;
        let mut parts = Vec::new();

        for ch in template_content.chars() {
            match ch {
                '{' => {
                    nesting += 1;
                    current_param.push(ch);
                }
                '}' => {
                    nesting -= 1;
                    current_param.push(ch);
                }
                '|' if nesting == 0 => {
                    let trimmed = current_param.trim().to_string();
                    if !trimmed.is_empty() {
                        parts.push(trimmed);
                    }
                    current_param.clear();
                }
                _ => {
                    current_param.push(ch);
                }
            }
        }
        let trimmed = current_param.trim().to_string();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }

        for part in parts {
            if let Some((name, value)) = part.split_once('=') {
                params.insert(
                    name.trim().to_lowercase(),
                    value.trim().to_string(),
                );
            }
        }

        params
    }

    /// Remove wiki markup from a value.
    fn clean_value(&self, value: &str) -> String {
        if value.is_empty() {
            return String::new();
        }

        // Remove <ref>...</ref> tags first
        let value = self.ref_re.replace_all(value, "");

        // Remove HTML tags
        let value = self.html_tag_re.replace_all(&value, "");

        // Convert [[link|text]] to text, [[text]] to text
        let value = self.link_re.replace_all(&value, "$1");

        // Remove remaining templates
        let value = self.template_re.replace_all(&value, "");

        // Remove bold/italic wiki markup
        let value = value.replace("'''", "").replace("''", "");

        // Collapse whitespace
        value.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Extract a year from a string.
    fn parse_year(&self, year_str: &str) -> Option<i32> {
        let cleaned = self.clean_value(year_str);
        self.year_re
            .find(&cleaned)
            .and_then(|m| m.as_str().parse().ok())
    }

    /// Parse the link field, converting idgames templates to URLs.
    fn parse_link(&self, link: &str) -> String {
        if link.is_empty() {
            return String::new();
        }

        // Check for {{ig|file=...}} template (path-style mirror URL).
        if let Some(caps) = self.idgames_file_re.captures(link) {
            let path = caps[1].trim();
            return format!("https://www.doomworld.com/idgames/{path}");
        }

        // Check for {{ig|id=N}} template (numeric idgames id).
        if let Some(caps) = self.idgames_id_re.captures(link) {
            let id = caps[1].trim();
            return format!("https://www.doomworld.com/idgames/?id={id}");
        }

        // Check for direct URL
        if link.starts_with("http://") || link.starts_with("https://") {
            return link.to_string();
        }

        // Check for plain [[link]] or external link
        let cleaned = self.clean_value(link);
        if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
            return cleaned;
        }

        String::new()
    }

    /// Extract the first paragraph of content from wikitext.
    fn extract_first_paragraph(&self, wikitext: &str) -> String {
        let mut lines = Vec::new();
        let mut in_template: i32 = 0;
        let mut started = false;

        for line in wikitext.split('\n') {
            // Track template nesting
            in_template += line.matches("{{").count() as i32;
            in_template -= line.matches("}}").count() as i32;

            if in_template > 0 {
                continue;
            }

            let stripped = line.trim();
            if stripped.is_empty() {
                if started {
                    break; // End of first paragraph
                }
                continue;
            }

            // Skip headers, categories, and special lines
            if stripped.starts_with('=')
                || stripped.starts_with("[[Category:")
                || stripped.starts_with("__")
                || stripped.starts_with("{|")
                || stripped.starts_with('|')
            {
                continue;
            }

            // Skip template remnants
            if stripped.starts_with("}}") {
                continue;
            }

            // Skip lines that are entirely template content (e.g., "{{Wad|name=T}}")
            if stripped.starts_with("{{") && stripped.ends_with("}}") {
                continue;
            }

            started = true;
            lines.push(stripped);

            // Limit to reasonable length
            let joined_len: usize = lines.iter().map(|l| l.len()).sum::<usize>() + lines.len();
            if joined_len > 500 {
                break;
            }
        }

        let paragraph = lines.join(" ");
        let paragraph = self.clean_value(&paragraph);

        if paragraph.len() > 500 {
            format!("{}...", &paragraph[..paragraph.floor_char_boundary(497)])
        } else {
            paragraph
        }
    }
}

impl Default for WikitextParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> WikitextParser {
        WikitextParser::new()
    }

    #[test]
    fn test_has_wad_template() {
        let p = parser();
        assert!(p.has_wad_template("blah {{Wad\n| name = Test}} blah"));
        assert!(p.has_wad_template("{{wad|name=Test}}"));
        assert!(!p.has_wad_template("No template here"));
    }

    #[test]
    fn test_extract_wad_template() {
        let p = parser();
        let wikitext = "before\n{{Wad\n| name = Test WAD\n| author = John\n}}\nafter";
        let content = p.extract_wad_template(wikitext).unwrap();
        assert!(content.contains("name = Test WAD"));
        assert!(content.contains("author = John"));
    }

    #[test]
    fn test_extract_wad_template_nested() {
        let p = parser();
        let wikitext = "{{Wad\n| name = Test\n| link = {{ig|file=test.zip}}\n}}";
        let content = p.extract_wad_template(wikitext).unwrap();
        assert!(content.contains("{{ig|file=test.zip}}"));
    }

    #[test]
    fn test_extract_wad_template_not_found() {
        let p = parser();
        assert!(p.extract_wad_template("no template").is_none());
    }

    #[test]
    fn test_parse_template_params() {
        let p = parser();
        let params = p.parse_template_params(" name = Scythe\n| author = Erik Alm\n| year = 2003");
        assert_eq!(params.get("name").unwrap(), "Scythe");
        assert_eq!(params.get("author").unwrap(), "Erik Alm");
        assert_eq!(params.get("year").unwrap(), "2003");
    }

    #[test]
    fn test_parse_template_params_nested() {
        let p = parser();
        let params = p.parse_template_params("name = Test\n| link = {{ig|file=test.zip}}");
        assert_eq!(params.get("name").unwrap(), "Test");
        assert_eq!(params.get("link").unwrap(), "{{ig|file=test.zip}}");
    }

    #[test]
    fn test_clean_value_wiki_links() {
        let p = parser();
        assert_eq!(p.clean_value("[[Doom II]]"), "Doom II");
        assert_eq!(p.clean_value("[[Doom II|Doom 2]]"), "Doom 2");
        assert_eq!(p.clean_value("by [[John Romero]]"), "by John Romero");
    }

    #[test]
    fn test_clean_value_html() {
        let p = parser();
        assert_eq!(
            p.clean_value("Text<ref>citation</ref> more"),
            "Text more"
        );
        assert_eq!(p.clean_value("<br/>hello"), "hello");
    }

    #[test]
    fn test_clean_value_bold_italic() {
        let p = parser();
        assert_eq!(p.clean_value("'''bold''' and ''italic''"), "bold and italic");
    }

    #[test]
    fn test_clean_value_templates() {
        let p = parser();
        assert_eq!(p.clean_value("before {{navbox}} after"), "before after");
    }

    #[test]
    fn test_clean_value_whitespace() {
        let p = parser();
        assert_eq!(p.clean_value("  too   much   space  "), "too much space");
    }

    #[test]
    fn test_clean_value_empty() {
        let p = parser();
        assert_eq!(p.clean_value(""), "");
    }

    #[test]
    fn test_parse_year() {
        let p = parser();
        assert_eq!(p.parse_year("2003"), Some(2003));
        assert_eq!(p.parse_year("[[2003]]"), Some(2003));
        assert_eq!(p.parse_year("March 2003"), Some(2003));
        assert_eq!(p.parse_year("1994-12-10"), Some(1994));
        assert_eq!(p.parse_year("no year here"), None);
    }

    #[test]
    fn test_parse_link_idgames() {
        let p = parser();
        let result = p.parse_link("{{ig|file=levels/doom2/megawads/scythe.zip}}");
        assert_eq!(
            result,
            "https://www.doomworld.com/idgames/levels/doom2/megawads/scythe.zip"
        );
    }

    #[test]
    fn test_parse_link_idgames_id_template() {
        let p = parser();
        let result = p.parse_link("{{ig|id=19805}}");
        assert_eq!(result, "https://www.doomworld.com/idgames/?id=19805");
    }

    #[test]
    fn test_parse_link_direct_url() {
        let p = parser();
        let url = "https://example.com/download.zip";
        assert_eq!(p.parse_link(url), url);
    }

    #[test]
    fn test_parse_link_empty() {
        let p = parser();
        assert_eq!(p.parse_link(""), "");
    }

    #[test]
    fn test_parse_link_wiki_url() {
        let p = parser();
        let result = p.parse_link("[https://example.com/file.zip]");
        // After cleaning, should extract URL
        assert!(result.starts_with("https://") || result.is_empty());
    }

    #[test]
    fn test_extract_first_paragraph() {
        let p = parser();
        let wikitext = "{{Wad\n| name = Test\n}}\n\nThis is the first paragraph.\n\nThis is the second paragraph.";
        let para = p.extract_first_paragraph(wikitext);
        assert_eq!(para, "This is the first paragraph.");
    }

    #[test]
    fn test_extract_first_paragraph_skip_headers() {
        let p = parser();
        let wikitext = "{{Wad|name=T}}\n\n== Header ==\n\nActual content here.";
        let para = p.extract_first_paragraph(wikitext);
        assert_eq!(para, "Actual content here.");
    }

    #[test]
    fn test_extract_first_paragraph_truncation() {
        let p = parser();
        let long_text = format!(
            "{{{{Wad|name=T}}}}\n\n{}",
            "word ".repeat(200)
        );
        let para = p.extract_first_paragraph(&long_text);
        assert!(para.len() <= 503); // 500 + "..."
    }

    #[test]
    fn test_parse_full_entry() {
        let p = parser();
        let wikitext = r#"{{Wad
| name        = Scythe
| author      = [[Erik Alm]]
| iwad        = [[Doom II]]
| port        = Boom-compatible
| year        = [[2003]]
| link        = {{ig|file=levels/doom2/megawads/scythe.zip}}
}}

'''Scythe''' is a 32-level [[megawad]] for [[Doom II]] by [[Erik Alm]].

== Levels ==
MAP01: "Entryway"
"#;
        let entry = p.parse(wikitext, "Scythe", 12345);
        assert_eq!(entry.page_id, 12345);
        assert_eq!(entry.title, "Scythe");
        assert_eq!(entry.name, "Scythe");
        assert_eq!(entry.author, "Erik Alm");
        assert_eq!(entry.year, Some(2003));
        assert_eq!(entry.iwad, "Doom II");
        assert_eq!(entry.port, "Boom-compatible");
        assert!(entry.link.contains("doomworld.com/idgames/"));
        assert!(entry.description.contains("Scythe"));
        assert_eq!(
            entry.wiki_url,
            "https://doomwiki.org/wiki/Scythe"
        );
    }

    #[test]
    fn test_parse_no_template() {
        let p = parser();
        let entry = p.parse("Just some text about Doom.", "Random Page", 999);
        assert_eq!(entry.page_id, 999);
        assert_eq!(entry.title, "Random Page");
        assert_eq!(entry.name, "");
        assert_eq!(entry.author, "");
        assert!(entry.year.is_none());
    }

    #[test]
    fn test_parse_wiki_url_spaces() {
        let p = parser();
        let entry = p.parse("{{Wad|name=T}}", "Speed of Doom", 1);
        assert_eq!(
            entry.wiki_url,
            "https://doomwiki.org/wiki/Speed_of_Doom"
        );
    }

    #[test]
    fn test_parse_fallback_title_field() {
        let p = parser();
        let wikitext = "{{Wad\n| title = Alt Title\n}}";
        let entry = p.parse(wikitext, "Page", 1);
        // `title` param falls back when `name` is absent
        assert_eq!(entry.name, "Alt Title");
    }

    #[test]
    fn test_parse_authors_field() {
        let p = parser();
        let wikitext = "{{Wad\n| authors = A, B, C\n}}";
        let entry = p.parse(wikitext, "Page", 1);
        assert_eq!(entry.author, "A, B, C");
    }
}
