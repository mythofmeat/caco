//! ZDoom MAPINFO/ZMAPINFO parser for map flow extraction.
//!
//! Parses the subset of MAPINFO needed for completion detection:
//! map definitions with `next`, `secretnext`, and endgame indicators.
//! Supports `defaultmap` inheritance.

use std::collections::HashMap;

use regex::Regex;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

static MAP_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*map\s+(\S+)").unwrap());

static DEFAULTMAP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(default|adddefault)map\b").unwrap());

static NEXT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)^\s*next\s*=\s*"?(\w+)"?"#).unwrap());

static SECRETNEXT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)^\s*secretnext\s*=\s*"?(\w+)"?"#).unwrap());

static ENDGAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(endgame|endpic|endcast|endbunny|endtitle|endsequence)\s*=").unwrap()
});

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Parsed data from a single MAPINFO map block.
#[derive(Debug, Clone, Default)]
pub struct MapinfoEntry {
    /// Next map after normal exit.
    pub next: Option<String>,
    /// Next map after secret exit.
    pub secretnext: Option<String>,
    /// Whether this map ends the game.
    pub has_endgame: bool,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse ZDoom MAPINFO/ZMAPINFO text into map flow entries.
///
/// Handles `defaultmap` inheritance: properties set in a `defaultmap` block
/// apply to all subsequent `map` blocks unless overridden. Multiple MAPINFO
/// lumps should be concatenated before calling.
///
/// Returns a HashMap keyed by uppercase map lump name.
pub fn parse_mapinfo(text: &str) -> HashMap<String, MapinfoEntry> {
    let mut entries = HashMap::new();
    let mut defaults = MapinfoEntry::default();

    // Strip C-style block comments
    let text = strip_block_comments(text);

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = strip_line_comment(lines[i]);

        if DEFAULTMAP_RE.is_match(line) {
            // Parse defaultmap block
            let (entry, next_i) = parse_block(&lines, i);
            defaults = entry;
            i = next_i;
        } else if let Some(caps) = MAP_HEADER_RE.captures(line) {
            let map_name = caps[1].to_uppercase();
            let (mut entry, next_i) = parse_block(&lines, i);
            // Apply defaults for unset fields
            if entry.next.is_none() {
                entry.next = defaults.next.clone();
            }
            if entry.secretnext.is_none() {
                entry.secretnext = defaults.secretnext.clone();
            }
            if !entry.has_endgame {
                entry.has_endgame = defaults.has_endgame;
            }
            entries.insert(map_name, entry);
            i = next_i;
        } else {
            i += 1;
        }
    }

    entries
}

/// Parse a block `{ ... }` starting from the header line.
/// Returns the parsed entry and the line index after the closing `}`.
fn parse_block(lines: &[&str], start: usize) -> (MapinfoEntry, usize) {
    let mut entry = MapinfoEntry::default();
    let mut i = start;

    // Find opening brace (may be on header line or next line)
    while i < lines.len() {
        if lines[i].contains('{') {
            break;
        }
        i += 1;
    }
    i += 1; // skip the line with `{`

    // Parse until closing brace
    let mut depth = 1;
    while i < lines.len() && depth > 0 {
        let line = strip_line_comment(lines[i]);

        if line.contains('{') {
            depth += 1;
        }
        if line.contains('}') {
            depth -= 1;
            if depth == 0 {
                i += 1;
                break;
            }
        }

        if let Some(caps) = NEXT_RE.captures(line) {
            entry.next = Some(caps[1].to_uppercase());
        } else if let Some(caps) = SECRETNEXT_RE.captures(line) {
            entry.secretnext = Some(caps[1].to_uppercase());
        } else if ENDGAME_RE.is_match(line) {
            entry.has_endgame = true;
        }

        i += 1;
    }

    (entry, i)
}

/// Strip `//` line comments.
fn strip_line_comment(line: &str) -> &str {
    if let Some(idx) = line.find("//") {
        &line[..idx]
    } else {
        line
    }
}

/// Strip `/* ... */` block comments.
fn strip_block_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_comment = false;

    while let Some(c) = chars.next() {
        if in_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next(); // consume '/'
                in_comment = false;
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next(); // consume '*'
            in_comment = true;
        } else {
            result.push(c);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_map() {
        let text = r#"
map MAP01 "Test Map"
{
    next = "MAP02"
}
"#;
        let entries = parse_mapinfo(text);
        assert_eq!(entries.len(), 1);
        let e = &entries["MAP01"];
        assert_eq!(e.next.as_deref(), Some("MAP02"));
        assert_eq!(e.secretnext, None);
        assert!(!e.has_endgame);
    }

    #[test]
    fn test_parse_defaultmap_inheritance() {
        let text = r#"
defaultmap
{
    next = INTERMAP
}

map MAP01 "First"
{
    music = D_E1M1
}

map MAP02 "Second"
{
    next = "MAP03"
}
"#;
        let entries = parse_mapinfo(text);
        // MAP01 inherits default next
        assert_eq!(entries["MAP01"].next.as_deref(), Some("INTERMAP"));
        // MAP02 overrides
        assert_eq!(entries["MAP02"].next.as_deref(), Some("MAP03"));
    }

    #[test]
    fn test_parse_secretnext() {
        let text = r#"
map MAP05 "Test"
{
    next = "MAP06"
    secretnext = "MAP41"
}
"#;
        let entries = parse_mapinfo(text);
        assert_eq!(entries["MAP05"].next.as_deref(), Some("MAP06"));
        assert_eq!(entries["MAP05"].secretnext.as_deref(), Some("MAP41"));
    }

    #[test]
    fn test_parse_endgame_variants() {
        for keyword in &["endgame", "endpic", "endcast", "endbunny", "endtitle", "endsequence"] {
            let text = format!(
                "map MAP30 \"End\"\n{{\n    {} = true\n}}\n",
                keyword
            );
            let entries = parse_mapinfo(&text);
            assert!(entries["MAP30"].has_endgame, "failed for {keyword}");
        }
    }

    #[test]
    fn test_parse_unquoted_values() {
        let text = r#"
map MAP01 "Test"
{
    next = MAP02
    secretnext = MAP31
}
"#;
        let entries = parse_mapinfo(text);
        assert_eq!(entries["MAP01"].next.as_deref(), Some("MAP02"));
        assert_eq!(entries["MAP01"].secretnext.as_deref(), Some("MAP31"));
    }

    #[test]
    fn test_parse_lookup_syntax() {
        let text = r#"
map MAP144 lookup MAP144NAME
{
    next = INTERMAP
}
"#;
        let entries = parse_mapinfo(text);
        assert_eq!(entries["MAP144"].next.as_deref(), Some("INTERMAP"));
    }

    #[test]
    fn test_parse_case_insensitive() {
        let text = r#"
Map map01 "test"
{
    Next = "map02"
    SecretNext = "MAP31"
}
"#;
        let entries = parse_mapinfo(text);
        // Keys are uppercased
        assert_eq!(entries["MAP01"].next.as_deref(), Some("MAP02"));
        assert_eq!(entries["MAP01"].secretnext.as_deref(), Some("MAP31"));
    }

    #[test]
    fn test_parse_comments() {
        let text = r#"
// This is a comment
map MAP01 "Test" // inline comment
{
    next = "MAP02" // next map
    /* block comment */
    secretnext = "MAP31"
}
"#;
        let entries = parse_mapinfo(text);
        assert_eq!(entries["MAP01"].next.as_deref(), Some("MAP02"));
        assert_eq!(entries["MAP01"].secretnext.as_deref(), Some("MAP31"));
    }

    #[test]
    fn test_parse_empty() {
        let entries = parse_mapinfo("");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_hub_style() {
        let text = r#"
defaultmap {
    next = INTERMAP
}

map HUBMAP "Castle"
{
    levelnum = 999
}

map INTERMAP "Level End"
{
    next = HUBMAP
}

map MAP01 lookup MAP01NAME
{
    music = M_MAP01
}

map MAP02 lookup MAP02NAME
{
    music = M_MAP02
}
"#;
        let entries = parse_mapinfo(text);
        assert_eq!(entries.len(), 4);
        // HUBMAP inherits default next
        assert_eq!(entries["HUBMAP"].next.as_deref(), Some("INTERMAP"));
        // INTERMAP overrides
        assert_eq!(entries["INTERMAP"].next.as_deref(), Some("HUBMAP"));
        // MAP01/02 inherit default
        assert_eq!(entries["MAP01"].next.as_deref(), Some("INTERMAP"));
        assert_eq!(entries["MAP02"].next.as_deref(), Some("INTERMAP"));
    }

    #[test]
    fn test_parse_non_map_blocks_ignored() {
        let text = r#"
gameinfo {
    playerclasses = "DoomPlayer"
}

skill easy {
    SpawnFilter = Easy
}

map MAP01 "Test"
{
    next = "MAP02"
}
"#;
        let entries = parse_mapinfo(text);
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("MAP01"));
    }

    #[test]
    fn test_parse_multiple_defaultmaps() {
        let text = r#"
defaultmap
{
    next = INTERMAP
}

map MAP01 "First"
{
}

defaultmap
{
    next = HUBMAP
}

map MAP02 "Second"
{
}
"#;
        let entries = parse_mapinfo(text);
        assert_eq!(entries["MAP01"].next.as_deref(), Some("INTERMAP"));
        assert_eq!(entries["MAP02"].next.as_deref(), Some("HUBMAP"));
    }
}
