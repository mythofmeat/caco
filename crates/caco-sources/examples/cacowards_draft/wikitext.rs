//! Parses a Doom Wiki `Cacowards_YYYY` page into draft dataset entries.
//!
//! Pure, so the tests can pin real lines from real pages. Tolerant, because the
//! pages were written by many hands over twenty years, and honest about it: whatever
//! it is unsure of goes in an entry's `notes`, which the draft prints as `# CHECK:`
//! comments for the human pass, rather than being guessed at or dropped.

use std::collections::HashMap;
use std::sync::LazyLock;

use caco_core::cacowards::slug;
use caco_core::db::{
    CATEGORY_HONORABLE_MENTION, CATEGORY_MORDETH, CATEGORY_RUNNER_UP, CATEGORY_WINNER,
};
use regex::Regex;

/// Everything a draft needs from one year's page.
#[derive(Debug, Default)]
pub struct DraftPage {
    /// Doomworld's own page for the year, from the External links section.
    pub page: Option<String>,
    pub entries: Vec<DraftEntry>,
    /// Sections left out because they award something the library cannot hold.
    pub skipped: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DraftEntry {
    pub category: String,
    pub order: i64,
    pub title: String,
    pub author: Option<String>,
    pub idgames: Option<IdgamesRef>,
    pub doomworld: Option<i64>,
    pub doomwiki: Option<String>,
    /// What the human pass should look at.
    pub notes: Vec<String>,
}

/// How the page points at the archive. A path has to be resolved to an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdgamesRef {
    Id(i64),
    File(String),
}

/// What a section's bullets are.
#[derive(Debug, PartialEq, Eq)]
enum Section {
    Awards(String),
    /// People and tools: real awards, but nothing the library could hold.
    NotEntries,
    ExternalLinks,
    /// Prose, or lists of something else ("See also").
    Other,
}

/// Which section a heading starts. The headings have varied a lot over the years,
/// hence the substring matches, and the order matters: "Multiplayer runner-up" is a
/// multiplayer award and "Mockaward Runners-Up" a mockaward. A heading seen only
/// once (Doom MOMA, What the Hell!?) is its own category, named after itself.
fn classify_heading(heading: &str) -> Section {
    let lower = heading.trim().to_lowercase();
    let is = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));

    if is(&["external links"]) {
        return Section::ExternalLinks;
    }
    if is(&["see also", "references", "doot eternal"]) {
        return Section::Other;
    }
    if is(&[
        "of the year",
        "espi",
        "newcomers",
        "codeaward",
        "consolation prize",
    ]) {
        return Section::NotEntries;
    }

    let category = if is(&["mordeth"]) {
        CATEGORY_MORDETH
    } else if is(&["honorable", "honourable", "also mentioned"]) {
        CATEGORY_HONORABLE_MENTION
    } else if is(&["multiplayer", "best dm level"]) {
        "multiplayer"
    } else if is(&["gameplay mod"]) {
        "gameplay-mod"
    } else if is(&["mockaward"]) {
        "mockaward"
    } else if is(&["honoring a newcomer"]) {
        "newcomer"
    } else if is(&["special feature"]) {
        "special-feature"
    } else if is(&["runner"]) {
        CATEGORY_RUNNER_UP
    } else if is(&["winner"]) {
        CATEGORY_WINNER
    } else {
        return Section::Awards(slug(heading));
    };
    Section::Awards(category.to_string())
}

/// The text of a `== heading ==` line, at any level below the page title.
fn parse_heading(line: &str) -> Option<&str> {
    let leading = line.bytes().take_while(|&b| b == b'=').count();
    let trailing = line.bytes().rev().take_while(|&b| b == b'=').count();
    if leading != trailing || leading < 2 || line.len() <= leading + trailing {
        return None;
    }
    let inner = line[leading..line.len() - trailing].trim();
    (!inner.is_empty()).then_some(inner)
}

/// `[[Page]]` or `[[Page|Display]]`. The display text can hold single brackets and
/// pipes: `[[Wormwood III - The Horror|Wormwood ]|[ - The Horror]]`.
static WIKILINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[(.*?)\]\]").unwrap());

/// `[https://example.com Label]`, where the label is optional.
static EXTERNAL_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(https?://[^\s\]]+)(?: ([^\]]*))?\]").unwrap());

static IG_TEMPLATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\{\{ig\|(?:id=(\d+)|file=([^}|]+))(?:\|[^}]*)?\}\}").unwrap()
});

static DWFORUMS_TEMPLATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\{\{dwforums\|(\d+)(?:\|[^}]*)?\}\}").unwrap());

/// A link to one post rather than a topic, which is no use as an identifier.
static DWFORUMSP_TEMPLATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\{\{dwforumsp\|(\d+)(?:\|[^}]*)?\}\}").unwrap());

/// Any other way a bullet says where the release lives.
static OTHER_REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{[^}]+\}\}|\[https?://[^\]]+\]").unwrap());

/// Split a wikilink's insides into its page and the text it displays.
fn split_link(inner: &str) -> (&str, &str) {
    match inner.split_once('|') {
        Some((page, display)) => (page.trim(), display.trim()),
        None => (inner.trim(), inner.trim()),
    }
}

/// Wiki markup as the text a reader would see, and the pages it linked to.
fn render_inline(text: &str) -> (String, Vec<String>) {
    let mut pages = Vec::new();
    let text = WIKILINK.replace_all(text, |caps: &regex::Captures| {
        let (page, display) = split_link(&caps[1]);
        pages.push(page.replace(' ', "_"));
        display.to_string()
    });
    let text = EXTERNAL_LINK.replace_all(&text, |caps: &regex::Captures| {
        caps.get(2).map_or("", |label| label.as_str()).to_string()
    });
    let text = text.replace("''", "");
    (text.split_whitespace().collect::<Vec<_>>().join(" "), pages)
}

/// Where a bullet's first top-level ` - ` is, skipping any inside a link: a lot of
/// titles have one (`[[Hi-Tech Hell 2 - Alien Tech]]`).
fn find_separator(text: &str) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let bytes = text.as_bytes();
    for (index, _) in text.char_indices() {
        let rest = &bytes[index..];
        if rest.starts_with(b"[[") || rest.starts_with(b"{{") {
            depth += 1;
        } else if (rest.starts_with(b"]]") || rest.starts_with(b"}}")) && depth > 0 {
            depth -= 1;
        } else if depth == 0 {
            for separator in [" - ", " – ", " — "] {
                if rest.starts_with(separator.as_bytes()) {
                    return Some((index, index + separator.len()));
                }
            }
        }
    }
    None
}

/// Where a run of bullet text gives way to the release's links, outside any wikilink.
fn find_references(text: &str) -> usize {
    let mut depth = 0usize;
    let bytes = text.as_bytes();
    for (index, _) in text.char_indices() {
        let rest = &bytes[index..];
        if rest.starts_with(b"[[") {
            depth += 1;
        } else if rest.starts_with(b"]]") && depth > 0 {
            depth -= 1;
        } else if depth == 0
            && (rest.starts_with(b"{{")
                || rest.starts_with(b"[http")
                || rest.starts_with(b"({{")
                || rest.starts_with(b"([http"))
        {
            return index;
        }
    }
    text.len()
}

/// Turn a bullet's text into an entry, or `None` if there is no title to be had.
fn parse_bullet(body: &str, category: &str, order: i64) -> Option<DraftEntry> {
    let mut notes = Vec::new();

    let (title_part, after_title) = match find_separator(body) {
        Some((start, end)) => (&body[..start], &body[end..]),
        None => body.split_at(find_references(body)),
    };

    // "[[Bastion of Chaos]] OST: "The Coming Hour"" is the title, not just the link.
    let title_end = find_references(title_part);
    let (title_text, after_title) = if title_end < title_part.len() {
        (&title_part[..title_end], &body[title_end..])
    } else {
        (title_part, after_title)
    };
    let (title, pages) = render_inline(title_text);
    if title.is_empty() {
        return None;
    }
    if pages.len() > 1 {
        notes.push(format!(
            "several links in the title ({}): split by hand if these are separate WADs",
            pages.join(", ")
        ));
    }
    if pages.is_empty() {
        notes.push("no link in the title: check this is one WAD".to_string());
    }

    let author = {
        let segment = match find_separator(after_title) {
            Some((start, _)) => &after_title[..start],
            None => after_title,
        };
        // A label in this position names someone: "and [https://… Ru5ty Sh4ck]".
        // One with no label is a stray reference.
        let cut = match segment.find("({{").or_else(|| segment.find("([http")) {
            Some(paren) => paren,
            None => segment.find("{{").unwrap_or(segment.len()),
        };
        let (text, _) = render_inline(&segment[..cut]);
        Some(text)
            .map(|text| text.trim_end_matches([',', '-', ' ']).to_string())
            .filter(|text| !text.is_empty())
    };

    let mut idgames = None;
    let ig_links: Vec<_> = IG_TEMPLATE.captures_iter(after_title).collect();
    if let Some(caps) = ig_links.first() {
        idgames = match (caps.get(1), caps.get(2)) {
            (Some(id), _) => id.as_str().parse().ok().map(IdgamesRef::Id),
            (_, Some(file)) => Some(IdgamesRef::File(file.as_str().trim().to_string())),
            _ => None,
        };
    }
    if ig_links.len() > 1 {
        notes.push(format!(
            "{} idgames links on this line, used the first",
            ig_links.len()
        ));
    }

    let topics: Vec<i64> = DWFORUMS_TEMPLATE
        .captures_iter(after_title)
        .filter_map(|caps| caps[1].parse().ok())
        .collect();
    if topics.len() > 1 {
        notes.push(format!(
            "{} Doomworld topics on this line, used the first",
            topics.len()
        ));
    }
    let doomworld = topics.first().copied();

    if let Some(caps) = DWFORUMSP_TEMPLATE.captures(after_title) {
        notes.push(format!(
            "links Doomworld post {}: find its topic by hand",
            &caps[1]
        ));
    }

    if idgames.is_none() && doomworld.is_none() {
        let others: Vec<&str> = OTHER_REFERENCE
            .find_iter(after_title)
            .map(|found| found.as_str())
            .filter(|found| !DWFORUMSP_TEMPLATE.is_match(found))
            .collect();
        if !others.is_empty() {
            notes.push(format!("only other links: {}", others.join(" ")));
        }
    }

    Some(DraftEntry {
        category: category.to_string(),
        order,
        title,
        author,
        idgames,
        doomworld,
        doomwiki: pages.into_iter().next(),
        notes,
    })
}

/// Parse a whole page.
pub fn parse_page(wikitext: &str) -> DraftPage {
    let mut draft = DraftPage::default();
    let mut section = Section::Other;
    let mut orders: HashMap<String, i64> = HashMap::new();

    for line in wikitext.lines() {
        let line = line.trim();

        if let Some(heading) = parse_heading(line) {
            section = classify_heading(heading);
            if section == Section::NotEntries {
                draft.skipped.push(heading.to_string());
            }
            continue;
        }

        let Some(body) = line.strip_prefix('*') else {
            continue;
        };
        let body = body.trim();

        match &section {
            Section::Awards(category) => {
                // A sub-bullet is commentary on the one above.
                if body.starts_with('*') {
                    continue;
                }
                let order = orders.get(category).copied().unwrap_or(0) + 1;
                if let Some(entry) = parse_bullet(body, category, order) {
                    orders.insert(category.clone(), order);
                    draft.entries.push(entry);
                }
            }
            Section::ExternalLinks if draft.page.is_none() => {
                if let Some(caps) = EXTERNAL_LINK.captures(body)
                    && caps[1].contains("doomworld.com")
                {
                    draft.page = Some(caps[1].to_string());
                }
            }
            _ => {}
        }
    }

    draft
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bullet(line: &str) -> DraftEntry {
        parse_bullet(line, "winner", 1).unwrap()
    }

    #[test]
    fn a_modern_bullet() {
        let entry =
            bullet("[[Piña Colada]] - [[Alex Decker (myolden)]] et al. - ({{ig|id=20917}})");
        assert_eq!(
            entry,
            DraftEntry {
                category: "winner".to_string(),
                order: 1,
                title: "Piña Colada".to_string(),
                author: Some("Alex Decker (myolden) et al.".to_string()),
                idgames: Some(IdgamesRef::Id(20917)),
                doomworld: None,
                doomwiki: Some("Piña_Colada".to_string()),
                notes: vec![],
            }
        );
    }

    #[test]
    fn an_old_bullet_with_no_second_separator() {
        let entry = bullet("[[Scythe 2]] - [[Erik Alm]] ({{ig|id=13600}})");
        assert_eq!(entry.title, "Scythe 2");
        assert_eq!(entry.author.as_deref(), Some("Erik Alm"));
        assert_eq!(entry.idgames, Some(IdgamesRef::Id(13600)));
    }

    #[test]
    fn a_doomworld_only_release_has_its_topic_and_no_idgames() {
        let entry =
            bullet("[[Lonesome Road]] - [[Thomas Seifert (tourniquet)]] - ({{dwforums|151858}})");
        assert_eq!(entry.doomworld, Some(151858));
        assert_eq!(entry.idgames, None);
        assert_eq!(entry.notes, Vec::<String>::new());
    }

    #[test]
    fn a_piped_link_shows_its_display_text_and_keeps_its_page() {
        let entry =
            bullet("[[Crusader (2023 WAD)|Crusader]] - [[Cheesewheel]] - ({{dwforums|139104}})");
        assert_eq!(entry.title, "Crusader");
        assert_eq!(entry.doomwiki.as_deref(), Some("Crusader_(2023_WAD)"));
    }

    #[test]
    fn text_after_the_link_is_part_of_the_title() {
        for (line, title) in [
            (
                "[[Back to Saturn X]]: Episode 1 - Various authors ({{ig|id=18748}})",
                "Back to Saturn X: Episode 1",
            ),
            (
                "[[Dark Universe]], Part 1 - [[Paul Dechene (Payload4367)]] - ({{ig|id=19060}})",
                "Dark Universe, Part 1",
            ),
            (
                "[[Foreverhood]] 1.0 - [[Brian Goodsell (Skadoomer)]]",
                "Foreverhood 1.0",
            ),
            (
                "[[Bellatrix: Tales of Orionis]] (Episode 1) - [[Franck Livolant (franckFRAG)]] - ({{dwforums|131150}})",
                "Bellatrix: Tales of Orionis (Episode 1)",
            ),
            (
                "[[Bastion of Chaos]] OST: \"The Coming Hour\" - [[John Weekley (PRIMEVAL)]]",
                "Bastion of Chaos OST: \"The Coming Hour\"",
            ),
        ] {
            assert_eq!(bullet(line).title, title, "{line}");
        }
    }

    #[test]
    fn a_separator_inside_a_link_is_not_a_separator() {
        let entry = bullet("[[Hi-Tech Hell 2 - Alien Tech]] - [[Simo Malinen]] ({{ig|id=13603}})");
        assert_eq!(entry.title, "Hi-Tech Hell 2 - Alien Tech");
        assert_eq!(entry.author.as_deref(), Some("Simo Malinen"));
    }

    #[test]
    fn brackets_and_pipes_in_the_display_text_survive() {
        let entry = bullet(
            "[[Wormwood III - The Horror|Wormwood ]|[ - The Horror]] - [[Zachary Stephens (Ribbiks)]] and [[Grain of Salt]] - ({{dwforums|125529}})",
        );
        assert_eq!(entry.title, "Wormwood ]|[ - The Horror");
        assert_eq!(entry.doomwiki.as_deref(), Some("Wormwood_III_-_The_Horror"));
        assert_eq!(
            entry.author.as_deref(),
            Some("Zachary Stephens (Ribbiks) and Grain of Salt")
        );

        let entry = bullet("[[Iivydotwad | ][vydotwad]] - ivymagnapinna - ({{ig|id=21773}})");
        assert_eq!(entry.title, "][vydotwad");
        assert_eq!(entry.doomwiki.as_deref(), Some("Iivydotwad"));
    }

    #[test]
    fn a_bullet_with_no_link_in_the_title_takes_its_text_and_says_so() {
        // The old parser took the first link as the title, which here is an author.
        let entry = bullet(
            "Mohrta - [[scumhead]] and Osiol - ([https://www.gog.com/en/game/mohrta GOG]) ([https://store.steampowered.com/app/2881610/Mohrta/ Steam])",
        );
        assert_eq!(entry.title, "Mohrta");
        assert_eq!(entry.author.as_deref(), Some("scumhead and Osiol"));
        assert_eq!(entry.doomwiki, None);
        assert!(
            entry
                .notes
                .iter()
                .any(|note| note.contains("no link in the title"))
        );
        assert!(entry.notes.iter().any(|note| note.contains("gog.com")));
    }

    #[test]
    fn a_joint_entry_is_flagged_for_splitting() {
        let entry = bullet(
            "[[Junkfood]] and [[Junkfood 2: Summertime Slotter]] - Various - ({{ig|id=20577}} and {{ig|id=20834}})",
        );
        assert_eq!(entry.title, "Junkfood and Junkfood 2: Summertime Slotter");
        assert_eq!(entry.idgames, Some(IdgamesRef::Id(20577)));
        assert_eq!(entry.doomwiki.as_deref(), Some("Junkfood"));
        assert!(
            entry
                .notes
                .iter()
                .any(|note| note.contains("several links"))
        );
        assert!(
            entry
                .notes
                .iter()
                .any(|note| note.contains("2 idgames links"))
        );
    }

    #[test]
    fn idgames_paths_with_and_without_a_label() {
        for (line, file) in [
            (
                "[[Sunder]] - x ({{ig|file=levels/doom2/Ports/megawads/sunder}})",
                "levels/doom2/Ports/megawads/sunder",
            ),
            (
                "[[Tarnsman's Projectile Hell]] - x - ({{ig|file=levels/doom2/Ports/megawads/tph.zip}})",
                "levels/doom2/Ports/megawads/tph.zip",
            ),
            (
                "[[Part I]] - x ({{ig|file=levels/doom2/Ports/m-o/mohu1|Part I}})",
                "levels/doom2/Ports/m-o/mohu1",
            ),
        ] {
            assert_eq!(
                bullet(line).idgames,
                Some(IdgamesRef::File(file.to_string())),
                "{line}"
            );
        }
    }

    #[test]
    fn a_template_in_any_case_is_read() {
        let entry = bullet("[[Waterlab GZD]] - x - ({{Dwforums|55741|Doomworld thread}})");
        assert_eq!(entry.doomworld, Some(55741));
    }

    #[test]
    fn a_post_link_is_flagged_and_is_not_a_topic() {
        let entry = bullet("[[Return to Void]] - [[Mike Watson (Cyb)]] - ({{dwforumsp|2913152}})");
        assert_eq!(entry.doomworld, None);
        assert!(entry.notes.iter().any(|note| note.contains("post 2913152")));
    }

    #[test]
    fn other_links_are_left_for_the_human() {
        let entry = bullet(
            "[[Dragon Sector Remake]] - x - ([https://www.moddb.com/downloads/dragon-sector-remake ModDB])",
        );
        assert_eq!(entry.idgames, None);
        assert!(entry.notes.iter().any(|note| note.contains("moddb.com")));
    }

    #[test]
    fn a_labelled_link_in_the_author_names_someone() {
        let entry = bullet(
            "[[Don't Be A Bitch 4]] - [[Mechanix Union]] and [https://www.ru5tysh4ck.net/wp/ Ru5ty Sh4ck] - ({{dwforums|146945}})",
        );
        assert_eq!(
            entry.author.as_deref(),
            Some("Mechanix Union and Ru5ty Sh4ck")
        );
    }

    #[test]
    fn an_unlabelled_link_after_the_author_is_not_part_of_it() {
        let entry = bullet(
            "[[Brutal Doom]] - [[Marcos Abenante (Sergeant Mark IV)]] [http://www.wadhost.fathax.com/files/brutaldoomv013.zip]",
        );
        assert_eq!(
            entry.author.as_deref(),
            Some("Marcos Abenante (Sergeant Mark IV)")
        );
    }

    #[test]
    fn a_bullet_with_no_author() {
        for line in [
            "[[Threshold of Pain]] - ({{ig|id=16196}})",
            "[[Legacy of Rust]]",
            "[[Knee-Deep in Knee-Deep in ZDoom]] vanilla tech",
        ] {
            assert_eq!(bullet(line).author, None, "{line}");
        }
    }

    #[test]
    fn italics_are_stripped_from_the_author() {
        let entry =
            bullet("[[The Mapwich 2: The Mapwichening]] - ''various'' - ({{dwforums|117485}})");
        assert_eq!(entry.author.as_deref(), Some("various"));
    }

    #[test]
    fn an_empty_bullet_is_no_entry() {
        assert_eq!(parse_bullet("", "winner", 1), None);
        assert_eq!(parse_bullet("({{ig|id=1}})", "winner", 1), None);
    }

    #[test]
    fn headings_at_every_level_are_read() {
        assert_eq!(parse_heading("== Top =="), Some("Top"));
        assert_eq!(parse_heading("===Winners==="), Some("Winners"));
        assert_eq!(parse_heading("==== Doom MOMA ===="), Some("Doom MOMA"));
        assert_eq!(parse_heading("== External links=="), Some("External links"));
        // Mismatched equals, the page title, and no heading at all.
        assert_eq!(parse_heading("=== Off =="), None);
        assert_eq!(parse_heading("= Title ="), None);
        assert_eq!(parse_heading("===="), None);
        assert_eq!(parse_heading("Just text"), None);
    }

    fn category_of(heading: &str) -> Section {
        classify_heading(heading)
    }

    fn awards(category: &str) -> Section {
        Section::Awards(category.to_string())
    }

    #[test]
    fn every_heading_the_pages_have_used_maps_to_something_sensible() {
        for (heading, category) in [
            ("Winners", "winner"),
            ("Runners-Up", "runner-up"),
            ("Runners-up", "runner-up"),
            ("Honorable Mention", "honorable-mention"),
            (
                "Honorable Mentions and Special Features",
                "honorable-mention",
            ),
            ("Also Mentioned", "honorable-mention"),
            ("Mordeth Award", "mordeth"),
            ("Mordeth award", "mordeth"),
            ("Best DM Level", "multiplayer"),
            ("Best Multiplayer Level", "multiplayer"),
            ("Best Multiplayer", "multiplayer"),
            ("Multiplayer Awards", "multiplayer"),
            ("Multiplayer award", "multiplayer"),
            ("Multiplayer runner-up", "multiplayer"),
            ("Best Gameplay Mod", "gameplay-mod"),
            ("Gameplay Mod Awards", "gameplay-mod"),
            ("Mockaward", "mockaward"),
            ("Mockaward Runners-Up", "mockaward"),
            ("Machaward", "machaward"),
            ("Worst Wad", "worst-wad"),
            ("Honoring a Newcomer", "newcomer"),
            ("Special features", "special-feature"),
            ("Special Features", "special-feature"),
            ("Doom MOMA", "doom-moma"),
            ("What the Hell!?", "what-the-hell"),
            ("Spaceship of Theseus", "spaceship-of-theseus"),
            ("Adrian's Pen", "adrians-pen"),
            ("Odyssey of Noises", "odyssey-of-noises"),
        ] {
            assert_eq!(category_of(heading), awards(category), "{heading}");
        }
    }

    #[test]
    fn people_and_tools_are_not_entries() {
        for heading in [
            "Mapper of the Year",
            "Creator of the Year",
            "Espi Award",
            "Espi award",
            "Most Promising Newcomers",
            "Most promising newcomers",
            "Codeaward",
            "Consolation Prize",
        ] {
            assert_eq!(category_of(heading), Section::NotEntries, "{heading}");
        }
    }

    const PAGE: &str = "{{Cacowards}}
The '''2023 Cacowards''' is the twentieth annual feature.

=== Winners ===
* [[Dreamblood]] - [[HeadshotTAS]] - ({{dwforums|136190}})
*[[Sepia]] - [[Petyan]] et al. - ({{dwforums|132145}})
** A sub-bullet about Sepia, which is no entry.

=== Multiplayer awards ===
* [[Culling Strike]] - [[The Hellforge]] - ({{dwforums|137962}})
==== Multiplayer runner-up ====
* [[Vignettes of Deathmatch]] - [[Shane Lopez (EmZee712)]] - ({{ig|id=20614}})

=== Special features ===
==== Doom MOMA ====
* [[Content]] - [[OverflowingMocha]] - ({{ig|id=21686}})

=== Most Promising Newcomers ===
* [[EduardoAndFriends]]

=== Creator of the Year ===
* [[Pieruskwurje]]

== See also ==
* [[Missed Cacowards 2]]

== External links ==
* [https://www.doomworld.com/cacowards/2023/ The 2023 Cacowards]
* {{dwforums|133079|Nominations thread on the Doomworld forums}}
";

    #[test]
    fn a_page_comes_apart_into_categories_in_page_order() {
        let draft = parse_page(PAGE);
        let rows: Vec<(&str, i64, &str)> = draft
            .entries
            .iter()
            .map(|entry| (entry.category.as_str(), entry.order, entry.title.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("winner", 1, "Dreamblood"),
                ("winner", 2, "Sepia"),
                ("multiplayer", 1, "Culling Strike"),
                // The sub-heading's bullets carry on its category's count.
                ("multiplayer", 2, "Vignettes of Deathmatch"),
                ("doom-moma", 1, "Content"),
            ]
        );
    }

    #[test]
    fn a_page_says_what_it_left_out_and_where_the_year_lives() {
        let draft = parse_page(PAGE);
        assert_eq!(
            draft.skipped,
            ["Most Promising Newcomers", "Creator of the Year"]
        );
        assert_eq!(
            draft.page.as_deref(),
            Some("https://www.doomworld.com/cacowards/2023/")
        );
    }
}
