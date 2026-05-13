//! Scraper for the Doom Wiki's `Cacowards_YYYY` pages.
//!
//! Reuses the existing [`DoomwikiClient`] to fetch raw wikitext, then walks
//! the section headings (`=== Winners ===`, `=== Runners-up ===`, …) and
//! parses each bullet line into a [`caco_core::db::NewCacoward`] entry.
//!
//! The scraper is deliberately tolerant: malformed bullets are skipped rather
//! than failing the whole scrape. Edge cases (combined entries, mordeth
//! awards without URLs) are best-effort — manual override via the DB is the
//! escape hatch.

use std::sync::LazyLock;

use caco_core::db::{
    CATEGORY_HONORABLE_MENTION, CATEGORY_MORDETH, CATEGORY_RUNNER_UP, CATEGORY_WINNER, NewCacoward,
};
use regex::Regex;

use super::DoomwikiClient;
use crate::error::Result;

// =============================================================================
// Section heading → category mapping
// =============================================================================

/// Map a lowercased section heading to the core category it represents.
///
/// Older Cacowards pages occasionally vary the wording ("Honorable mentions"
/// vs. "Honorable Mentions and Special Features", etc.), so we use a substring
/// match rather than exact equality.
fn category_for_heading(heading: &str) -> Option<&'static str> {
    let lower = heading.trim().to_lowercase();
    if lower.contains("mordeth") {
        return Some(CATEGORY_MORDETH);
    }
    if lower.contains("honorable") || lower.contains("honourable") {
        return Some(CATEGORY_HONORABLE_MENTION);
    }
    if lower.contains("runner") {
        return Some(CATEGORY_RUNNER_UP);
    }
    if lower.contains("winner") {
        return Some(CATEGORY_WINNER);
    }
    None
}

// =============================================================================
// Regex patterns (compiled once)
// =============================================================================

/// First `[[link]]` in a bullet, with optional `Page|Display` pipe form.
/// Captures: (1) page name, (2) display text — display may equal page.
static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]\|]+)(?:\|([^\]]+))?\]\]").unwrap());

/// idgames template — `{{ig|id=NNNNN}}` or `{{ig|file=path}}`.
static IG_TEMPLATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{ig\|(?:id=(\d+)|file=([^}|]+))\}\}").unwrap());

/// Parse a MediaWiki heading line into its inner text, accepting only level-2
/// (`== … ==`) or level-3 (`=== … ===`) headings. Level-4+ headings are
/// rejected so that sub-sub-sections like `==== Multiplayer runner-up ====`
/// don't get conflated with the main `=== Runners-up ===` section.
///
/// We can't use a single regex with a backreference because Rust's `regex`
/// crate doesn't support backreferences — so we count equals signs manually.
fn parse_heading_level(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let leading = trimmed.bytes().take_while(|&b| b == b'=').count();
    let trailing = trimmed.bytes().rev().take_while(|&b| b == b'=').count();
    if leading != trailing || !(2..=3).contains(&leading) {
        return None;
    }
    let inner = &trimmed[leading..trimmed.len() - trailing];
    let inner = inner.trim();
    if inner.is_empty() { None } else { Some(inner) }
}

// =============================================================================
// Public entry point
// =============================================================================

/// Fetch and parse the `Cacowards_YYYY` Doom Wiki page for `year`.
///
/// Returns one [`NewCacoward`] per parsed bullet across the core four
/// categories. Returns an empty `Vec` (Ok) if the page exists but no
/// categories were recognised; returns an `Err` only on transport failure.
///
/// Missing pages map to `Err(SourceError::Api(...))` via the wiki API's own
/// error message — the caller can decide whether to treat that as fatal.
pub fn fetch_cacowards(client: &DoomwikiClient, year: i64) -> Result<Vec<NewCacoward>> {
    let title = format!("Cacowards {year}");
    let Some((_, wikitext)) = client.get_page_content(&title)? else {
        return Ok(Vec::new());
    };
    Ok(parse_cacowards_page(&wikitext, year))
}

// =============================================================================
// Pure parser (no I/O — fully unit-testable)
// =============================================================================

/// Parse the body of a `Cacowards_YYYY` page into entries. Pure function over
/// the wikitext — kept separate from the HTTP fetch so tests can pin known
/// inputs without going over the network.
pub fn parse_cacowards_page(wikitext: &str, year: i64) -> Vec<NewCacoward> {
    let mut entries = Vec::new();
    let mut current_category: Option<&'static str> = None;
    let mut rank_in_section: i64 = 0;

    for line in wikitext.lines() {
        let trimmed = line.trim();

        // Section heading? (level 2 or 3 only — level-4 sub-headings under
        // sections like `=== Multiplayer awards ===` would otherwise pollute
        // adjacent categories.)
        if let Some(heading) = parse_heading_level(trimmed) {
            current_category = category_for_heading(heading);
            rank_in_section = 0;
            continue;
        }

        // Bullet line in a recognised section?
        if let Some(category) = current_category
            && let Some(bullet_body) = trimmed.strip_prefix('*')
        {
            let bullet_body = bullet_body.trim_start();
            // Sub-bullets (e.g. "** ...") are commentary; skip them.
            if bullet_body.starts_with('*') {
                continue;
            }
            if let Some(entry) = parse_bullet(bullet_body, category, year) {
                rank_in_section += 1;
                let mut entry = entry;
                entry.rank = Some(rank_in_section);
                entries.push(entry);
            }
        }
    }

    entries
}

/// Parse a single bullet line into a [`NewCacoward`]. Returns `None` if the
/// line has no extractable WAD title (e.g. heading-only lines slipped through).
fn parse_bullet(body: &str, category: &'static str, year: i64) -> Option<NewCacoward> {
    // First wikilink is the WAD title.
    let first = WIKILINK_RE.captures(body)?;
    let page = first.get(1)?.as_str().trim();
    let display = first
        .get(2)
        .map(|m| m.as_str().trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(page);

    // Author: text between the first " - " (or " — ") after the title link and
    // the next " - " or end. Falls back to None if no separator is found.
    let after_title_idx = first.get(0)?.end();
    let after_title = &body[after_title_idx..];
    let author = extract_author(after_title);

    // idgames URL from the first {{ig|...}} template anywhere in the bullet.
    let idgames_url = extract_idgames_url(body);

    // Doom Wiki URL from the page name (before the pipe). Pages are linked by
    // their canonical title — spaces become underscores in the URL.
    let doomwiki_url = Some(format!(
        "https://doomwiki.org/wiki/{}",
        page.replace(' ', "_")
    ));

    Some(NewCacoward {
        year,
        category: category.to_string(),
        rank: None, // set by the caller based on bullet order
        wad_title: display.to_string(),
        wad_author: author,
        idgames_url,
        doomwiki_url,
        blurb: None,
    })
}

/// Pull the author segment out of "- Author - (...)" style suffixes. Strips
/// `[[...]]` wrappers and the trailing ` et al.` flag so the stored value is
/// presentable; the multi-author signal is implicit but not preserved.
fn extract_author(after_title: &str) -> Option<String> {
    // Skip the leading separator (" - " or " — ").
    let rest = after_title
        .trim_start()
        .trim_start_matches(['-', '–', '—'])
        .trim_start();

    // Find the URL-parens boundary: ` ({{` (template) or ` ([http` (external
    // link). We must NOT stop at a plain ` (` — wikilinks like
    // `[[Michael Fraize (Marcaek)]]` legitimately contain parens.
    // Require the paren to be preceded by whitespace so we don't trip on
    // parens that sit inside the author chunk itself.
    let url_paren = ["({{", "([http", "([h"]
        .iter()
        .filter_map(|needle| rest.find(needle))
        .min()
        .map(|p| {
            if p > 0 && rest.as_bytes()[p - 1] == b' ' {
                p - 1
            } else {
                p
            }
        });

    let end = [rest.find(" - "), rest.find(" — "), url_paren]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(rest.len());
    let raw = rest[..end].trim();
    if raw.is_empty() {
        return None;
    }

    // Unwrap wikilinks: `[[A]], [[B]] and [[C]]` -> `A, B and C`.
    let stripped = WIKILINK_RE.replace_all(raw, |caps: &regex::Captures| {
        caps.get(2)
            .or_else(|| caps.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    });

    // Strip a trailing ` et al.` / ` et al` flag. The "various authors"
    // signal is implicit in the rest of the entry; the stored value just
    // names the primary author.
    let cleaned = stripped
        .trim()
        .trim_end_matches('.')
        .trim_end()
        .trim_end_matches("et al")
        .trim_end();

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

/// Find the first `{{ig|id=N}}` or `{{ig|file=path}}` template and return a
/// canonical doomworld.com URL for it. Path-style entries get the `.zip`
/// fallback treatment for parity with `extract_idgames_file_path_from_url`.
fn extract_idgames_url(text: &str) -> Option<String> {
    let caps = IG_TEMPLATE_RE.captures(text)?;
    if let Some(id) = caps.get(1) {
        return Some(format!(
            "https://www.doomworld.com/idgames/?id={}",
            id.as_str()
        ));
    }
    if let Some(file) = caps.get(2) {
        let path = file.as_str().trim();
        let last = path.rsplit('/').next().unwrap_or("");
        let path = if last.contains('.') {
            path.to_string()
        } else {
            format!("{path}.zip")
        };
        return Some(format!("https://www.doomworld.com/idgames/{path}"));
    }
    None
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_2023: &str = r#"
{{Cacowards}}
The '''2023 Cacowards''' blah blah.

=== Winners ===
* [[Dreamblood]] - [[HeadshotTAS]] - ({{dwforums|136190}})
* [[Piña Colada]] - [[Alex Decker (myolden)]] et al. - ({{ig|id=20917}})
* [[Crusader (2023 WAD)|Crusader]] - [[Cheesewheel]] - ({{dwforums|139104}})

=== Runners-up ===
* [[1x1]] - Various - ({{ig|id=20879}})
* [[BOOMER: Beyond Vanilla]] - [[Fernito]] - ({{ig|id=20662}})

=== Honorable Mentions and Special Features ===
* [[Flesharmonic]] - Various - ({{ig|id=21006}})
* [[Junkfood]] and [[Junkfood 2: Summertime Slotter]] - Various - ({{ig|id=20577}} and {{ig|id=20834}})

=== Multiplayer awards ===
* [[Culling Strike]] - [[The Hellforge]] - ({{dwforums|137962}})

=== Mordeth award ===
* [[TNT 2: Devilution|TNT2 MAP26]] - [[Michael Fraize (Marcaek)]]
"#;

    #[test]
    fn parses_core_four_categories_only() {
        let entries = parse_cacowards_page(SAMPLE_2023, 2023);
        let categories: Vec<&str> = entries.iter().map(|e| e.category.as_str()).collect();

        // Three winners + two runners-up + two honorable + one mordeth = 8.
        // Multiplayer awards are not part of the core four and should be
        // ignored by the headline parser.
        assert_eq!(entries.len(), 8);
        assert_eq!(
            categories,
            vec![
                "winner",
                "winner",
                "winner",
                "runner-up",
                "runner-up",
                "honorable-mention",
                "honorable-mention",
                "mordeth",
            ]
        );
    }

    #[test]
    fn ranks_are_assigned_within_each_section() {
        let entries = parse_cacowards_page(SAMPLE_2023, 2023);
        let winners: Vec<i64> = entries
            .iter()
            .filter(|e| e.category == "winner")
            .filter_map(|e| e.rank)
            .collect();
        assert_eq!(winners, vec![1, 2, 3]);

        let mordeth: Vec<i64> = entries
            .iter()
            .filter(|e| e.category == "mordeth")
            .filter_map(|e| e.rank)
            .collect();
        assert_eq!(mordeth, vec![1]);
    }

    #[test]
    fn extracts_title_author_and_idgames_url() {
        let entries = parse_cacowards_page(SAMPLE_2023, 2023);
        let pina = entries
            .iter()
            .find(|e| e.wad_title == "Piña Colada")
            .expect("Piña Colada parsed");
        assert_eq!(pina.wad_author.as_deref(), Some("Alex Decker (myolden)"));
        assert_eq!(
            pina.idgames_url.as_deref(),
            Some("https://www.doomworld.com/idgames/?id=20917")
        );
        assert_eq!(
            pina.doomwiki_url.as_deref(),
            Some("https://doomwiki.org/wiki/Piña_Colada")
        );
    }

    #[test]
    fn pipe_form_wikilink_uses_display_text_as_title() {
        let entries = parse_cacowards_page(SAMPLE_2023, 2023);
        // [[Crusader (2023 WAD)|Crusader]] should appear as title "Crusader"
        let crusader = entries
            .iter()
            .find(|e| e.wad_title == "Crusader")
            .expect("Crusader parsed");
        // ...but the wiki URL should use the canonical page name.
        assert_eq!(
            crusader.doomwiki_url.as_deref(),
            Some("https://doomwiki.org/wiki/Crusader_(2023_WAD)")
        );
    }

    #[test]
    fn mordeth_without_idgames_url_still_parses() {
        let entries = parse_cacowards_page(SAMPLE_2023, 2023);
        let mordeth = entries
            .iter()
            .find(|e| e.category == "mordeth")
            .expect("mordeth parsed");
        assert_eq!(mordeth.wad_title, "TNT2 MAP26");
        assert!(mordeth.idgames_url.is_none());
        assert_eq!(
            mordeth.wad_author.as_deref(),
            Some("Michael Fraize (Marcaek)")
        );
    }

    #[test]
    fn dwforums_only_entries_have_no_idgames_url() {
        let entries = parse_cacowards_page(SAMPLE_2023, 2023);
        let dreamblood = entries
            .iter()
            .find(|e| e.wad_title == "Dreamblood")
            .expect("Dreamblood parsed");
        assert!(dreamblood.idgames_url.is_none());
    }

    #[test]
    fn various_author_passes_through() {
        let entries = parse_cacowards_page(SAMPLE_2023, 2023);
        let onexone = entries.iter().find(|e| e.wad_title == "1x1").unwrap();
        assert_eq!(onexone.wad_author.as_deref(), Some("Various"));
    }

    #[test]
    fn idgames_file_form_builds_path_url() {
        let url = extract_idgames_url("({{ig|file=levels/doom2/megawads/scythe.zip}})");
        assert_eq!(
            url.as_deref(),
            Some("https://www.doomworld.com/idgames/levels/doom2/megawads/scythe.zip")
        );
    }

    #[test]
    fn idgames_file_form_appends_zip_for_slug() {
        let url = extract_idgames_url("({{ig|file=levels/doom2/Ports/megawads/sunlust}})");
        assert_eq!(
            url.as_deref(),
            Some("https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/sunlust.zip")
        );
    }

    #[test]
    fn parse_heading_level_rejects_level_4() {
        assert_eq!(parse_heading_level("== Top ==").unwrap(), "Top");
        assert_eq!(parse_heading_level("=== Sub ===").unwrap(), "Sub");
        assert_eq!(parse_heading_level("==== Subsub ===="), None);
        // Mismatched equals (= 3 left, 2 right) — reject.
        assert_eq!(parse_heading_level("=== Off =="), None);
        // Single-level heading is page title — skip.
        assert_eq!(parse_heading_level("= Title ="), None);
    }

    #[test]
    fn multiplayer_runner_up_subheading_does_not_leak() {
        let wikitext = "=== Honorable Mentions ===
* [[Real HM]] - Author - ({{ig|id=1}})

=== Multiplayer awards ===
* [[Skip me]] - Author - ({{ig|id=2}})
==== Multiplayer runner-up ====
* [[Also skip]] - Author - ({{ig|id=3}})

=== Mordeth award ===
* [[Last]] - Author
";
        let entries = parse_cacowards_page(wikitext, 2023);
        let titles: Vec<&str> = entries.iter().map(|e| e.wad_title.as_str()).collect();
        assert_eq!(titles, vec!["Real HM", "Last"]);
    }

    #[test]
    fn category_for_heading_handles_known_variants() {
        assert_eq!(category_for_heading("Winners"), Some("winner"));
        assert_eq!(category_for_heading("Runners-up"), Some("runner-up"));
        assert_eq!(
            category_for_heading("Honorable Mentions and Special Features"),
            Some("honorable-mention")
        );
        assert_eq!(
            category_for_heading("Honourable mentions"),
            Some("honorable-mention")
        );
        assert_eq!(category_for_heading("Mordeth award"), Some("mordeth"));
        assert_eq!(category_for_heading("Mordeth Award"), Some("mordeth"));
        assert_eq!(category_for_heading("Codeaward"), None);
        assert_eq!(category_for_heading("Multiplayer awards"), None);
    }

    #[test]
    fn entries_outside_known_sections_are_ignored() {
        // Multiplayer / Gameplay mod / Codeaward sections shouldn't yield rows.
        let entries = parse_cacowards_page(SAMPLE_2023, 2023);
        assert!(!entries.iter().any(|e| e.wad_title == "Culling Strike"));
    }
}
