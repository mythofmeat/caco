//! MAPINFO, ZMAPINFO and UMAPINFO, read for map flow.
//!
//! Reads what completion detection needs: each map's `next` and secret exit,
//! whether it ends the game, the `defaultmap` they inherit, and where the
//! episode menu starts.
//!
//! Read as ZDoom reads them (`FScanner` and `FMapInfoParser`): as a stream of
//! tokens, where a line break means nothing, except in the older format ZDoom
//! still accepts, which has no braces and a property to a line. Sunder's
//! MAPINFO is written that way, as are many from before 2009. A reader that
//! went by lines used to be here, and checked against real MAPINFO it lost a
//! block written on one line and the blocks after it, a property on the line
//! of a brace, the whole of the older format, a quoted episode or map name, and
//! everything after a banner such as `//*****`, which it took for the start of
//! a block comment.

use std::borrow::Cow;
use std::collections::HashMap;

/// How much included text [`parse_mapinfo`] reads before it stops following
/// includes: far more MAPINFO than any WAD has, and a bound on one whose files
/// include each other many times over.
const MAX_INCLUDED_BYTES: usize = 4 << 20;

/// The keywords a definition starts with at the top level. In the older format
/// a map's properties run until one of these. `cluster` is not among them, as
/// it is a map's property there: only the braced format has a `cluster` block.
const TOP_LEVEL: &[&str] = &[
    "include",
    "gamedefaults",
    "defaultmap",
    "adddefaultmap",
    "map",
    "clusterdef",
    "episode",
    "clearepisodes",
    "skill",
    "clearskills",
    "gameinfo",
    "intermission",
    "doomednums",
    "damagetype",
    "spawnnums",
    "conversationids",
    "automap",
    "automap_overlay",
];

/// What MAPINFO's `next` takes for the end of the game rather than a map, as
/// ZDoom's `CheckEndSequence` does, besides anything starting `EndGame`.
const END_SEQUENCES: &[&str] = &[
    "endpic",
    "endbunny",
    "endcast",
    "enddemon",
    "endchess",
    "endunderwater",
    "endbuystrife",
    "endtitle",
    "endsequence",
];

/// Properties of a map that end the game wherever it stands. Not ZDoom's, which
/// ends a game with `next`, but UMAPINFO's spelling is easy to carry over.
const END_PROPERTIES: &[&str] = &[
    "endgame",
    "endpic",
    "endcast",
    "endbunny",
    "endtitle",
    "endsequence",
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// What one map's definition says about where play goes from it.
#[derive(Debug, Clone, Default)]
pub struct MapinfoEntry {
    /// Next map after normal exit.
    pub next: Option<String>,
    /// Next map after secret exit.
    pub secretnext: Option<String>,
    /// Whether this map ends the game.
    pub has_endgame: bool,
}

/// What a MAPINFO or ZMAPINFO says about map flow.
#[derive(Debug, Default)]
pub struct Mapinfo {
    /// Each map's definition, by its name in upper case.
    pub maps: HashMap<String, MapinfoEntry>,
    /// The map each `episode` starts at, in upper case and in the order they
    /// are declared. MAPINFO's `episode clear` is here as `CLEAR`: callers keep
    /// the ones that are maps.
    pub episodes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/// Parse MAPINFO or ZMAPINFO, reading each `include` where it stands from what
/// `read` gives for its name.
///
/// Each file starts from the `gamedefaults` with no `defaultmap`, as in ZDoom,
/// and what an included file sets stays set after it.
pub fn parse_mapinfo(text: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Mapinfo {
    let mut parser = Parser {
        read,
        out: Mapinfo::default(),
        gamedefaults: MapinfoEntry::default(),
        defaults: MapinfoEntry::default(),
        reading: Vec::new(),
        included: 0,
    };
    parser.parse_file(text);
    parser.out
}

/// Parse UMAPINFO.
///
/// UMAPINFO has one format, braced, and none of MAPINFO's defaults or includes.
/// An episode is a property of the map it starts at, `episode = "M_EPI1",
/// "Name", "k"`, where MAPINFO declares one on its own. nyan-doom adds each to
/// the menu as it reads it (`umapinfo.cpp`).
pub fn parse_umapinfo(text: &str) -> Mapinfo {
    let mut sc = Scanner::new(text);
    let mut out = Mapinfo::default();
    while let Some(lexeme) = sc.next(true) {
        if !lexeme.token.is_word("map") {
            continue;
        }
        let Some(name) = sc.next(true).and_then(|l| l.token.text().map(map_name)) else {
            break;
        };
        let mut entry = MapinfoEntry::default();
        let mut starts_episode = false;
        if sc.peek(true).is_some_and(|l| l.token.is('{')) {
            sc.next(true);
            read_block(&mut sc, &mut |key, values| {
                if key.eq_ignore_ascii_case("episode") {
                    // `episode = clear` takes the menu's episodes away and adds none.
                    starts_episode |= values
                        .first()
                        .is_some_and(|value| !value.eq_ignore_ascii_case("clear"));
                } else {
                    apply_umapinfo(&mut entry, key, values);
                }
            });
        }
        if starts_episode {
            out.episodes.push(name.clone());
        }
        out.maps.insert(name, entry);
    }
    out
}

struct Parser<'r> {
    read: &'r mut dyn FnMut(&str) -> Option<String>,
    out: Mapinfo,
    gamedefaults: MapinfoEntry,
    defaults: MapinfoEntry,
    /// The includes being read, for the one that would go round in a circle.
    reading: Vec<String>,
    /// How much included text has been read.
    included: usize,
}

impl Parser<'_> {
    fn parse_file(&mut self, text: &str) {
        self.defaults = self.gamedefaults.clone();
        let mut sc = Scanner::new(text);
        while let Some(lexeme) = sc.next(false) {
            let Token::Word(keyword) = lexeme.token else {
                continue;
            };
            match keyword.to_ascii_lowercase().as_str() {
                "map" => {
                    let Some(name) = sc.next(false) else { break };
                    let Some(name) = name.token.text().map(map_name) else {
                        continue;
                    };
                    skip_title(&mut sc, lexeme.line);
                    let mut entry = self.defaults.clone();
                    read_definition(&mut sc, &mut entry);
                    self.out.maps.insert(name, entry);
                }
                "defaultmap" => {
                    self.defaults = self.gamedefaults.clone();
                    read_definition(&mut sc, &mut self.defaults);
                }
                "adddefaultmap" => read_definition(&mut sc, &mut self.defaults),
                "gamedefaults" => {
                    self.gamedefaults = MapinfoEntry::default();
                    read_definition(&mut sc, &mut self.gamedefaults);
                    self.defaults = self.gamedefaults.clone();
                }
                "episode" => {
                    let Some(name) = sc.next(false) else { break };
                    if let Some(name) = name.token.text() {
                        self.out.episodes.push(map_name(name));
                    }
                    // Which map a shareware game starts at instead.
                    if sc.peek(false).is_some_and(|l| l.token.is_word("teaser")) {
                        sc.next(false);
                        sc.next(false);
                    }
                    skip_definition(&mut sc);
                }
                "include" => {
                    if let Some(name) = sc
                        .next(false)
                        .and_then(|l| l.token.text().map(str::to_owned))
                    {
                        self.include(&name);
                    }
                }
                "clearepisodes" | "clearskills" => {}
                _ if is_top_level(&lexeme.token) || keyword.eq_ignore_ascii_case("cluster") => {
                    skip_definition(&mut sc);
                }
                _ => {}
            }
        }
    }

    fn include(&mut self, name: &str) {
        // ZDoom's names are not case-sensitive, and a zip made on Windows can
        // separate with backslashes.
        let key = name.replace('\\', "/").to_lowercase();
        if self.included > MAX_INCLUDED_BYTES || self.reading.contains(&key) {
            return;
        }
        let Some(text) = (self.read)(name) else {
            return;
        };
        self.included += text.len();
        self.reading.push(key);
        self.parse_file(&text);
        self.reading.pop();
    }
}

/// Skip a map's title: `lookup` and the name of a string, or the title itself.
/// ZDoom insists on a title, where this lets a definition go without one, so
/// long as what follows is not on the line `map` is on, or is a brace.
fn skip_title(sc: &mut Scanner, header_line: usize) {
    let Some(next) = sc.peek(false) else { return };
    let is_title = match &next.token {
        Token::Quoted(_) => true,
        Token::Word(_) => next.line == header_line,
        Token::Punct(_) => false,
    };
    if !is_title {
        return;
    }
    sc.next(false);
    if next.token.is_word("lookup") {
        if sc.peek(false).is_some_and(|l| l.token.is(',')) {
            sc.next(false);
        }
        sc.next(false);
    }
}

/// Read a definition's properties into `entry`: a braced block, or in the older
/// format, a property to a line until the next top-level keyword.
fn read_definition(sc: &mut Scanner, entry: &mut MapinfoEntry) {
    if sc.peek(false).is_some_and(|l| l.token.is('{')) {
        sc.next(false);
        read_block(sc, &mut |key, values| apply_mapinfo(entry, key, values));
    } else {
        read_old_format(sc, entry);
    }
}

/// Read a braced block's properties, whose `{` has been read, up to and over its
/// `}`: `key`, or `key = value`, with more values after commas. Each goes to
/// `apply` as it is read.
fn read_block(sc: &mut Scanner, apply: &mut dyn FnMut(&str, &[&str])) {
    while let Some(lexeme) = sc.next(true) {
        let key = match &lexeme.token {
            Token::Punct('}') => return,
            // A block of its own, as `next = endgame { ... }` has.
            Token::Punct('{') => {
                skip_block(sc);
                continue;
            }
            Token::Punct(_) => continue,
            token => token.text().unwrap_or_default(),
        };
        let mut values = Vec::new();
        if sc.peek(true).is_some_and(|l| l.token.is('=')) {
            sc.next(true);
            loop {
                match sc.peek(true) {
                    Some(value) if value.token.text().is_some() => {
                        sc.next(true);
                        values.push(value.token);
                    }
                    _ => break,
                }
                if !sc.peek(true).is_some_and(|l| l.token.is(',')) {
                    break;
                }
                sc.next(true);
            }
        }
        let values: Vec<&str> = values.iter().filter_map(Token::text).collect();
        apply(key, &values);
    }
}

/// The older format: a property to a line, its values after it with no `=`,
/// until the next top-level keyword.
fn read_old_format(sc: &mut Scanner, entry: &mut MapinfoEntry) {
    while let Some(first) = sc.peek(false) {
        if is_top_level(&first.token) || matches!(first.token, Token::Punct('{' | '}')) {
            return;
        }
        sc.next(false);
        let mut values = Vec::new();
        while let Some(value) = sc.peek(false).filter(|l| l.line == first.line) {
            sc.next(false);
            values.push(value.token);
        }
        let values: Vec<&str> = values.iter().filter_map(Token::text).collect();
        if let Some(key) = first.token.text() {
            apply_mapinfo(entry, key, &values);
        }
    }
}

/// Skip what follows a keyword whose definition says nothing about map flow:
/// up to and over its block, or in the older format, up to the next top-level
/// keyword.
fn skip_definition(sc: &mut Scanner) {
    while let Some(lexeme) = sc.peek(false) {
        if is_top_level(&lexeme.token) {
            return;
        }
        sc.next(false);
        if lexeme.token.is('{') {
            skip_block(sc);
            return;
        }
    }
}

/// Skip to the `}` closing a block whose `{` has been read, and over it.
fn skip_block(sc: &mut Scanner) {
    let mut depth = 1;
    while let Some(lexeme) = sc.next(true) {
        if lexeme.token.is('{') {
            depth += 1;
        } else if lexeme.token.is('}') {
            depth -= 1;
            if depth == 0 {
                return;
            }
        }
    }
}

fn apply_mapinfo(entry: &mut MapinfoEntry, key: &str, values: &[&str]) {
    let first = values.first().copied();
    match key.to_ascii_lowercase().as_str() {
        "next" => {
            if let Some(value) = first {
                // One field in ZDoom, so the last `next` wins, whichever it names.
                if is_end_sequence(value) {
                    entry.next = None;
                    entry.has_endgame = true;
                } else {
                    entry.next = Some(map_name(value));
                    entry.has_endgame = false;
                }
            }
        }
        // `secret` is ZDoom's alias, for Vavoom's spelling. A secret exit that
        // ends the game leads to no map.
        "secretnext" | "secret" => {
            if let Some(value) = first.filter(|value| !is_end_sequence(value)) {
                entry.secretnext = Some(map_name(value));
            }
        }
        key if END_PROPERTIES.contains(&key) => entry.has_endgame = true,
        _ => {}
    }
}

fn apply_umapinfo(entry: &mut MapinfoEntry, key: &str, values: &[&str]) {
    let Some(value) = values.first().copied() else {
        return;
    };
    let yes = value.eq_ignore_ascii_case("true");
    match key.to_ascii_lowercase().as_str() {
        "next" => entry.next = Some(map_name(value)),
        "nextsecret" => entry.secretnext = Some(map_name(value)),
        "endpic" => entry.has_endgame = true,
        // `endgame = false` is how a map says it does not end the game where
        // the original did. A cast or a bunny that is not shown says nothing.
        "endgame" => entry.has_endgame = yes,
        "endcast" | "endbunny" if yes => entry.has_endgame = true,
        _ => {}
    }
}

/// Whether `next` names the end of the game rather than a map. Only the names
/// ZDoom knows: a map can be called ENDING.
fn is_end_sequence(value: &str) -> bool {
    // `get` rather than slicing, which could split a character in two.
    value
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("endgame"))
        || END_SEQUENCES
            .iter()
            .any(|end| value.eq_ignore_ascii_case(end))
}

/// A map's name as its lump is named: in upper case, and a number as the map
/// it means, since ZDoom takes `next = 2` for MAP02.
fn map_name(name: &str) -> String {
    let name = name.trim();
    match name.parse::<u8>() {
        Ok(number) => format!("MAP{number:02}"),
        Err(_) => name.to_uppercase(),
    }
}

fn is_top_level(token: &Token) -> bool {
    matches!(token, Token::Word(word) if TOP_LEVEL.iter().any(|k| word.eq_ignore_ascii_case(k)))
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token<'a> {
    /// A name, a number or a keyword.
    Word(&'a str),
    /// What was between double quotes.
    Quoted(Cow<'a, str>),
    /// `{`, `}`, `=` or `,`, and `;` inside a block.
    Punct(char),
}

impl Token<'_> {
    /// A word's text, or a quoted string's.
    fn text(&self) -> Option<&str> {
        match self {
            Token::Word(word) => Some(word),
            Token::Quoted(text) => Some(text),
            Token::Punct(_) => None,
        }
    }

    fn is(&self, punct: char) -> bool {
        matches!(self, Token::Punct(p) if *p == punct)
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self, Token::Word(w) if w.eq_ignore_ascii_case(word))
    }
}

#[derive(Debug)]
struct Lexeme<'a> {
    token: Token<'a>,
    /// The line it starts on, from 1.
    line: usize,
}

/// MAPINFO's tokens, as ZDoom's `FScanner` gives them.
///
/// Outside a block, `;` starts a comment, as it does in Hexen's MAPINFO and in
/// ZDoom's scanner until it reaches a `{`. Inside one it is a token of its own,
/// as in ZDoom's C mode, so a property ending in one is still read.
#[derive(Clone)]
struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Scanner {
            text,
            pos: 0,
            line: 1,
        }
    }

    fn peek(&self, in_block: bool) -> Option<Lexeme<'a>> {
        self.clone().next(in_block)
    }

    fn next(&mut self, in_block: bool) -> Option<Lexeme<'a>> {
        let bytes = self.text.as_bytes();
        loop {
            match bytes.get(self.pos)? {
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                b if *b <= b' ' => self.pos += 1,
                b'/' if bytes.get(self.pos + 1) == Some(&b'/') => self.skip_line(),
                b';' if !in_block => self.skip_line(),
                b'/' if bytes.get(self.pos + 1) == Some(&b'*') => {
                    let end = self.text[self.pos + 2..]
                        .find("*/")
                        .map_or(bytes.len(), |at| self.pos + 2 + at + 2);
                    self.line += self.text[self.pos..end].matches('\n').count();
                    self.pos = end;
                }
                _ => break,
            }
        }

        let start = self.pos;
        let line = self.line;
        let token = match bytes[start] {
            b'"' => {
                let mut end = start + 1;
                while end < bytes.len() && bytes[end] != b'"' {
                    // ZDoom's one escape in a string.
                    end += if bytes[end] == b'\\' && bytes.get(end + 1) == Some(&b'"') {
                        2
                    } else {
                        1
                    };
                }
                let inner = &self.text[start + 1..end.min(bytes.len())];
                self.line += inner.matches('\n').count();
                self.pos = (end + 1).min(bytes.len());
                Token::Quoted(if inner.contains("\\\"") {
                    Cow::Owned(inner.replace("\\\"", "\""))
                } else {
                    Cow::Borrowed(inner)
                })
            }
            punct @ (b'{' | b'}' | b'=' | b',' | b';') => {
                self.pos += 1;
                Token::Punct(punct as char)
            }
            _ => {
                let mut end = start;
                while end < bytes.len() && !ends_word(bytes, end) {
                    end += 1;
                }
                self.pos = end;
                Token::Word(&self.text[start..end])
            }
        };
        Some(Lexeme { token, line })
    }

    fn skip_line(&mut self) {
        self.pos = self.text[self.pos..]
            .find('\n')
            .map_or(self.text.len(), |at| self.pos + at);
    }
}

/// Whether the byte at `at` ends a word: space, a quote, punctuation, or the
/// start of a comment. Only ASCII ends one, so a word is never cut inside a
/// character.
fn ends_word(bytes: &[u8], at: usize) -> bool {
    match bytes[at] {
        b if b <= b' ' => true,
        b'"' | b'{' | b'}' | b'=' | b',' | b';' => true,
        b'/' => matches!(bytes.get(at + 1), Some(b'/' | b'*')),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The maps `text` defines, with no includes to read.
    fn parse(text: &str) -> HashMap<String, MapinfoEntry> {
        parse_mapinfo(text, &mut |_| None).maps
    }

    #[test]
    fn test_parse_basic_map() {
        let text = r#"
map MAP01 "Test Map"
{
    next = "MAP02"
}
"#;
        let entries = parse(text);
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
        let entries = parse(text);
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
        let entries = parse(text);
        assert_eq!(entries["MAP05"].next.as_deref(), Some("MAP06"));
        assert_eq!(entries["MAP05"].secretnext.as_deref(), Some("MAP41"));
    }

    #[test]
    fn test_parse_endgame_variants() {
        for keyword in &[
            "endgame",
            "endpic",
            "endcast",
            "endbunny",
            "endtitle",
            "endsequence",
        ] {
            let text = format!("map MAP30 \"End\"\n{{\n    {} = true\n}}\n", keyword);
            let entries = parse(&text);
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
        let entries = parse(text);
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
        let entries = parse(text);
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
        let entries = parse(text);
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
        let entries = parse(text);
        assert_eq!(entries["MAP01"].next.as_deref(), Some("MAP02"));
        assert_eq!(entries["MAP01"].secretnext.as_deref(), Some("MAP31"));
    }

    #[test]
    fn test_parse_empty() {
        let entries = parse("");
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
        let entries = parse(text);
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
        let entries = parse(text);
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("MAP01"));
    }

    #[test]
    fn test_next_endpic_is_endgame() {
        // ZDoom's `next = EndPic` (often `next = endpic, LUMP`) ends the
        // episode — it must not be recorded as an edge to a bogus "ENDPIC"
        // map. Mirrors Eisberg's tier finales.
        let text = r#"
map MAP03 "Finale"
{
    next = Endpic, CREDIT
}
"#;
        let entries = parse(text);
        assert!(entries["MAP03"].has_endgame);
        assert_eq!(entries["MAP03"].next, None);
    }

    #[test]
    fn test_next_unicode_target_does_not_panic() {
        // `\w` is Unicode-aware, so a multi-byte map target must not panic the
        // first-three-bytes endgame check. It's simply not an endgame target.
        let text = "map MAP01 \"X\"\n{\n  next = \u{00e9}poque\n}\n";
        let entries = parse(text);
        assert!(!entries["MAP01"].has_endgame);
    }

    #[test]
    fn test_parse_episode_starts() {
        let text = r#"
episode MAP01
{
    name = "Tier 1"
}
episode MAP04
{
    name = "Tier 2"
}
clearepisodes
episode clear
map MAP01 "A"
{
    next = Map02
}
"#;
        let starts = parse_mapinfo(text, &mut |_| None).episodes;
        assert_eq!(starts, vec!["MAP01", "MAP04", "CLEAR"]);
        // `episode = "..."` (a per-map property) must not be captured.
        let prop = parse_mapinfo("map MAP01 {\n  episode = \"foo\"\n}", &mut |_| None);
        assert!(prop.episodes.is_empty());
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
        let entries = parse(text);
        assert_eq!(entries["MAP01"].next.as_deref(), Some("INTERMAP"));
        assert_eq!(entries["MAP02"].next.as_deref(), Some("HUBMAP"));
    }

    /// A reader over named files, which answers names in any case, as ZDoom's lookup does.
    fn files<'a>(named: &'a [(&'a str, &'a str)]) -> impl FnMut(&str) -> Option<String> + 'a {
        |wanted| {
            named
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
                .map(|(_, text)| text.to_string())
        }
    }

    #[test]
    fn test_includes_are_read_where_they_stand() {
        // The Warrens' ZMAPINFO, which has nothing of its own.
        let zmapinfo = "include \"mapinfo/Maps\"\ninclude \"mapinfo/Episodes\"\n";
        let maps = "Map BTW01 \"Burrow of The Were\"\n{\n\tNext = \"MAP01\"\n}\n";
        let episodes = "clearepisodes\n\nEpisode\tBTW01 \n{\n\tname = \"Burrow\"\n}\n";
        let named = [("mapinfo/maps", maps), ("MAPINFO/Episodes", episodes)];
        let mut read = files(&named);

        let mapinfo = parse_mapinfo(zmapinfo, &mut read);

        assert_eq!(mapinfo.maps["BTW01"].next.as_deref(), Some("MAP01"));
        assert_eq!(mapinfo.episodes, ["BTW01"]);
    }

    #[test]
    fn test_includes_that_are_commented_out_or_missing_are_left_out() {
        let text = "// include \"a\"\n/* include \"a\" */\ninclude \"gone\"\nmap MAP01 {\n}\n";
        let named = [("a", "map MAP02 {\n}\n")];
        let mut read = files(&named);

        let entries = parse_mapinfo(text, &mut read).maps;

        assert!(entries.contains_key("MAP01"));
        assert!(!entries.contains_key("MAP02"));
    }

    #[test]
    fn test_includes_that_go_round_in_a_circle_end() {
        let named = [
            ("a", "include \"B\"\nmap MAP01 {\n}\n"),
            ("b", "include \"a\"\ninclude \"b\"\nmap MAP02 {\n}\n"),
        ];
        let mut read = files(&named);

        let entries = parse_mapinfo("include \"a\"\n", &mut read).maps;

        assert!(entries.contains_key("MAP01"));
        assert!(entries.contains_key("MAP02"));
    }

    // -----------------------------------------------------------------------
    // Shapes real MAPINFO has, which reading by lines got wrong
    // -----------------------------------------------------------------------

    /// What a map's definition says, as (next, secret exit, ends the game).
    fn flow(
        entries: &HashMap<String, MapinfoEntry>,
        map: &str,
    ) -> (Option<String>, Option<String>, bool) {
        let entry = &entries[map];
        (
            entry.next.clone(),
            entry.secretnext.clone(),
            entry.has_endgame,
        )
    }

    fn to(map: &str) -> Option<String> {
        Some(map.to_string())
    }

    #[test]
    fn test_blocks_on_one_line_and_the_blocks_after_them() {
        // As a ZDoom mod on GitHub has them. Each block used to swallow the ones
        // after it, looking for a `}` on a line of its own.
        let text = "map hq1 \"Headquarters 1\" { next = \"hq2\" cluster = 1 NoAutosaveHint }\n\
                    map hq2 \"Headquarters 2\" { next = \"hq3\" secretnext = \"hq9\" }\n\
                    map hq3 \"Headquarters 3\" { next = EndGameC }\n";

        let entries = parse(text);

        assert_eq!(flow(&entries, "HQ1"), (to("HQ2"), None, false));
        assert_eq!(flow(&entries, "HQ2"), (to("HQ3"), to("HQ9"), false));
        assert_eq!(flow(&entries, "HQ3"), (None, None, true));
    }

    #[test]
    fn test_properties_on_the_lines_of_braces() {
        // A Fallout-like ZDoom mod's ZMAPINFO: a property after `{` was lost, and so
        // was the last `next`, the one on the line of `}`, so MAP07 went to MAP09.
        let text = r#"
map zmap01 "Vault 13 level 2&3"
{sky1 = HLSKY1, 0.0
next = "zmap02"
cluster = 1 }

map map07 "Out of the Frying Pan ...."
{sky1 = HLSKY2, 0.0
next = "map09"
//music = D_MAP07
cluster = 1
next=map08}

cluster 1 {hub
pic = TITLEPIC
}

map map08 "Into the Fire" {next = "map09"}
"#;
        let entries = parse(text);

        assert_eq!(flow(&entries, "ZMAP01"), (to("ZMAP02"), None, false));
        assert_eq!(flow(&entries, "MAP07"), (to("MAP08"), None, false));
        assert_eq!(flow(&entries, "MAP08"), (to("MAP09"), None, false));
    }

    #[test]
    fn test_the_older_format_has_no_braces() {
        // Triangulum's, from 2014: a property to a line, no `=`, until the next
        // definition. None of it was read, so MAP30 ended the game, and the WAD
        // was finished two maps early. `cluster` is a map's property here.
        let text = "; Triangulum\r\n\
                    map MAP30 \"Shielding aura\"\r\n\
                    sky1 STARFUCK\r\n\
                    cluster 1\r\n\
                    next MAP33\r\n\
                    secretnext MAP31 ; to the tower\r\n\
                    \r\n\
                    clusterdef 1\r\n\
                    exittext\r\n\
                    \"Not every map\r\n\
                    is the last.\"\r\n\
                    map MAP33 \"Ghorrock\"\r\n\
                    next MAP34\r\n\
                    map MAP34 \"Attack on Titan\"\r\n\
                    next EndGame3";

        let entries = parse(text);

        assert_eq!(flow(&entries, "MAP30"), (to("MAP33"), to("MAP31"), false));
        assert_eq!(flow(&entries, "MAP33"), (to("MAP34"), None, false));
        assert_eq!(flow(&entries, "MAP34"), (None, None, true));
    }

    #[test]
    fn test_a_banner_is_only_a_comment() {
        // A Hexen MAPINFO on GitHub: the `/*` in `//**` was taken for the start of a
        // block comment that never ended, and every map after it was lost.
        let text = "//**********************\n\
                    //**Classic Hexen Maps**\n\
                    //**********************\n\
                    map MAP01 \"WINNOWING HALL\"\n{\n\tnext = \"MAP02\"\n}\n";

        assert_eq!(flow(&parse(text), "MAP01"), (to("MAP02"), None, false));
    }

    #[test]
    fn test_a_banner_in_an_included_file_is_only_a_comment() {
        let named = [(
            "maps",
            "//*** Episode 1 ***\nmap MAP01 \"A\" { next = MAP02 }\n",
        )];
        let mut read = files(&named);

        let entries = parse_mapinfo("include \"maps\" //** the maps\n", &mut read).maps;

        assert_eq!(flow(&entries, "MAP01"), (to("MAP02"), None, false));
    }

    #[test]
    fn test_quoted_episode_and_map_names() {
        // WhoDunIt's second episode was never seen, so its maps could not be
        // required: the Eisberg bug again.
        let text = r#"
episode "WDI01"
{
    name = "Who Dun It?"
}
episode "WDIEXHUB"
{
    name = "Extras"
}
map "WDI01" "The Manor"
{
    next = "WDI02"
}
"#;
        let mapinfo = parse_mapinfo(text, &mut |_| None);

        assert_eq!(mapinfo.episodes, ["WDI01", "WDIEXHUB"]);
        assert_eq!(flow(&mapinfo.maps, "WDI01"), (to("WDI02"), None, false));
    }

    #[test]
    fn test_the_last_next_wins_whichever_it_names() {
        // WolfenDoom's credits map: the default `next` and then the end of the game.
        // And the other way round, a default ending a map that says where it goes.
        let text = r#"
defaultmap
{
    next = INTERMAP
}
map CREDITSM lookup "CREDITSM"
{
    next = EndTitle
}
defaultmap
{
    next = EndGameC
}
map MAP01 "A"
{
    next = MAP02
}
"#;
        let entries = parse(text);

        assert_eq!(flow(&entries, "CREDITSM"), (None, None, true));
        assert_eq!(flow(&entries, "MAP01"), (to("MAP02"), None, false));
    }

    #[test]
    fn test_only_the_ends_zdoom_knows_end_a_map() {
        // Freedoom: Badass Edition has maps called ENDLES01 and ENDLES02, which a
        // rule of "anything starting with End" took for the end of the game.
        let text = r#"
map ENDLES01 "Endless" { next = "ENDLES02" }
map ENDLES02 "Endless II" { next = EndGame2 }
map MAP01 "A" { next = endtitle }
map MAP02 "B" { next = EndSequence, "Inter_Chess" }
"#;
        let entries = parse(text);

        assert_eq!(flow(&entries, "ENDLES01"), (to("ENDLES02"), None, false));
        assert_eq!(flow(&entries, "ENDLES02"), (None, None, true));
        assert_eq!(flow(&entries, "MAP01"), (None, None, true));
        assert_eq!(flow(&entries, "MAP02"), (None, None, true));
    }

    #[test]
    fn test_a_number_names_the_map_it_means() {
        // Lithium's test maps: ZDoom takes `next = 1` for MAP01.
        let entries = parse("map LITHTEST \"Test\" { next = 1 }\nmap 2 \"Two\"\nnext 3\n");

        assert_eq!(flow(&entries, "LITHTEST"), (to("MAP01"), None, false));
        assert_eq!(flow(&entries, "MAP02"), (to("MAP03"), None, false));
    }

    #[test]
    fn test_an_ending_with_a_block_of_its_own() {
        // `next = endgame { ... }` defines the ending in place. Its block is not the
        // map's end, and what follows it is still the map's.
        let text = r#"
map MAP07 "Finale"
{
    next = endgame
    {
        pic = "CREDIT"
        music = "D_VICTOR"
    }
    secretnext = "MAP31"
}
map MAP31 "Secret" { next = MAP08 }
"#;
        let entries = parse(text);

        assert_eq!(flow(&entries, "MAP07"), (None, to("MAP31"), true));
        assert_eq!(flow(&entries, "MAP31"), (to("MAP08"), None, false));
    }

    #[test]
    fn test_a_semicolon_ends_a_property_in_a_block() {
        // Outside a block it starts a comment, as in Hexen's MAPINFO.
        let text = "; the first map\nmap MAP01 \"A\" ; and its title\n{ next = \"MAP02\"; secretnext = \"MAP31\"; }\n";

        assert_eq!(
            flow(&parse(text), "MAP01"),
            (to("MAP02"), to("MAP31"), false)
        );
    }

    #[test]
    fn test_an_included_file_starts_from_the_gamedefaults() {
        // As ZDoom reads one: a `defaultmap` does not reach into a file it
        // includes, and one set there is still set after it.
        let named = [("more", "map MAP02 \"B\" { }\ndefaultmap { next = HUB }\n")];
        let mut read = files(&named);
        let text = "defaultmap { next = INTERMAP }\ninclude \"more\"\nmap MAP03 \"C\" { }\n";

        let entries = parse_mapinfo(text, &mut read).maps;

        assert_eq!(entries["MAP02"].next, None);
        assert_eq!(entries["MAP03"].next.as_deref(), Some("HUB"));
    }

    // -----------------------------------------------------------------------
    // UMAPINFO
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_umapinfo_basic() {
        let text = r#"
MAP MAP01
{
    next = "MAP02"
    nextsecret = "MAP31"
}
"#;
        let entries = parse_umapinfo(text).maps;
        let map01 = &entries["MAP01"];
        assert_eq!(map01.next, Some("MAP02".to_string()));
        assert_eq!(map01.secretnext, Some("MAP31".to_string()));
    }

    #[test]
    fn test_parse_umapinfo_endgame_variants() {
        for kw in &["endgame", "endpic", "endcast", "endbunny"] {
            let text = format!("MAP MAP30\n{{\n    {} = true\n}}", kw);
            let entries = parse_umapinfo(&text).maps;
            assert!(entries["MAP30"].has_endgame, "failed for {kw}");
        }
    }

    #[test]
    fn test_umapinfo_endgame_false_does_not_end_the_game() {
        // lc-earthtb's UMAPINFO says so of its one map, and any `endgame =` used to
        // read as the end of the game, which would finish a WAD at that map.
        let text = "MAP MAP01\n{\n   levelname = \"Earthtone Base\"\n   endgame = false\n   endcast = false\n}\n";

        assert!(!parse_umapinfo(text).maps["MAP01"].has_endgame);
    }

    #[test]
    fn test_umapinfo_on_one_line_with_comments() {
        let text = "// Episode 1\nMAP MAP01 { levelname = \"A\" next = \"MAP02\" nextsecret = \"MAP31\" }\n\
                    /* the secret */ MAP MAP31 { next = \"MAP02\" } MAP MAP02 { endpic = \"CREDIT\" }\n";

        let entries = parse_umapinfo(text).maps;

        assert_eq!(flow(&entries, "MAP01"), (to("MAP02"), to("MAP31"), false));
        assert_eq!(flow(&entries, "MAP31"), (to("MAP02"), None, false));
        assert_eq!(flow(&entries, "MAP02"), (None, None, true));
    }

    #[test]
    fn test_umapinfo_episodes_start_at_the_map_that_declares_them() {
        let text = "MAP MAP00 { episode = \"M_EPI0\", \"Tutorial\", \"t\" next = \"MAP01\" }\n\
                    MAP MAP01 { episode = clear }\n\
                    MAP MAP06 { levelname = \"Six\" episode = \"M_EPI2\" }\n\
                    MAP MAP07 { next = \"MAP08\" }\n";

        let umapinfo = parse_umapinfo(text);

        assert_eq!(umapinfo.episodes, ["MAP00", "MAP06"]);
        assert_eq!(umapinfo.maps["MAP00"].next.as_deref(), Some("MAP01"));
    }
}
