//! The curated Cacowards dataset: what Doomworld has awarded each year since 2004.
//!
//! One TOML file per year, compiled in and merged with any `*.toml` in
//! [`crate::config::cacoward_data_dir`]: the arrangement sourceport recipes use, so
//! a correction or a private addition does not need a release. This replaced
//! scraping the Doom Wiki on every user's machine (#53), which a Cloudflare
//! challenge kept breaking, and which only ever produced what the page happened to
//! say that day. The data is small, append-only and changes once a year, so it
//! does not need fetching at all.
//!
//! Facts only: titles, authors, categories and identifiers. The write-ups are
//! Doomworld's own text, so `page` links to them. An entry is something the library
//! can hold, so the awards that go to people (Creator of the Year, the Espi) and to
//! tools (the Codeaward) are not entries.
//!
//! `caco-sources`' `cacoward_sync` writes this into the `cacowards` table. The annual
//! update is one new file here, drafted by `examples/cacowards_draft` in
//! `caco-sources`, plus its line in [`BUILTIN`], which a test enforces.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

use crate::db::cacowards::NewCacoward;
use crate::error::Error;

/// The years compiled into the binary, with the name each is reported under.
const BUILTIN: &[(&str, &str)] = &[
    (include_str!("data/2004.toml"), "2004.toml"),
    (include_str!("data/2005.toml"), "2005.toml"),
    (include_str!("data/2006.toml"), "2006.toml"),
    (include_str!("data/2007.toml"), "2007.toml"),
    (include_str!("data/2008.toml"), "2008.toml"),
    (include_str!("data/2009.toml"), "2009.toml"),
    (include_str!("data/2010.toml"), "2010.toml"),
    (include_str!("data/2011.toml"), "2011.toml"),
    (include_str!("data/2012.toml"), "2012.toml"),
    (include_str!("data/2013.toml"), "2013.toml"),
    (include_str!("data/2014.toml"), "2014.toml"),
    (include_str!("data/2015.toml"), "2015.toml"),
    (include_str!("data/2016.toml"), "2016.toml"),
    (include_str!("data/2017.toml"), "2017.toml"),
    (include_str!("data/2018.toml"), "2018.toml"),
    (include_str!("data/2019.toml"), "2019.toml"),
    (include_str!("data/2020.toml"), "2020.toml"),
    (include_str!("data/2021.toml"), "2021.toml"),
    (include_str!("data/2022.toml"), "2022.toml"),
    (include_str!("data/2023.toml"), "2023.toml"),
    (include_str!("data/2024.toml"), "2024.toml"),
    (include_str!("data/2025.toml"), "2025.toml"),
];

/// One year's file. Everything else about an entry is on [`NewCacoward`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct YearFile {
    year: i64,
    /// Doomworld's page for the year, where the write-ups are.
    #[serde(default)]
    page: Option<String>,
    #[serde(default)]
    entry: Vec<NewCacoward>,
}

/// Every Cacoward caco knows about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dataset {
    /// Sorted by key, which is what makes one a merge of several files.
    pub entries: Vec<NewCacoward>,
    /// Doomworld's page for each year that has one.
    pub pages: BTreeMap<i64, String>,
}

/// Parse one year's file. `name` says which file in any error.
fn parse_year_file(name: &str, text: &str) -> crate::Result<YearFile> {
    let in_file = |message: &str| Error::Config(format!("{name}: {message}"));

    let mut file: YearFile = toml::from_str(text).map_err(|e| in_file(&e.to_string()))?;
    for entry in &mut file.entry {
        for (field, value) in [
            ("key", &entry.key),
            ("category", &entry.category),
            ("title", &entry.wad_title),
        ] {
            if value.trim().is_empty() {
                return Err(in_file(&format!(
                    "an entry has an empty `{field}` (key {:?})",
                    entry.key
                )));
            }
        }
        entry.year = file.year;
        entry.sha256 = entry.sha256.take().map(|hash| hash.to_ascii_lowercase());
    }
    Ok(file)
}

/// Accumulates files, later ones winning: a file can correct an entry by repeating
/// its key, and adds to the dataset with any other.
#[derive(Default)]
struct Merge {
    entries: BTreeMap<String, NewCacoward>,
    pages: BTreeMap<i64, String>,
}

impl Merge {
    fn add(&mut self, file: YearFile) {
        if let Some(page) = file.page {
            self.pages.insert(file.year, page);
        }
        for entry in file.entry {
            self.entries.insert(entry.key.clone(), entry);
        }
    }

    fn finish(self) -> Dataset {
        Dataset {
            entries: self.entries.into_values().collect(),
            pages: self.pages,
        }
    }
}

fn builtin_merge() -> Merge {
    let mut merge = Merge::default();
    for (text, name) in BUILTIN {
        merge.add(parse_year_file(name, text).expect("a built-in Cacowards file is malformed"));
    }
    merge
}

/// The dataset compiled into this binary.
///
/// Panics only if a built-in file is malformed, which is a build-time mistake the
/// tests at the bottom of this file catch.
pub fn builtin() -> Dataset {
    builtin_merge().finish()
}

/// The built-in dataset with every `*.toml` in `user_dir` merged over it by key.
///
/// Files are read in sorted order so two that repeat a key resolve predictably. A
/// malformed file is an error rather than a skip: quietly ignoring it would look
/// exactly like the correction not being picked up. A missing directory just means
/// nothing has been added.
pub fn load(user_dir: &Path) -> crate::Result<Dataset> {
    let mut merge = builtin_merge();

    let mut files: Vec<_> = match std::fs::read_dir(user_dir) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();

    for path in files {
        let text = std::fs::read_to_string(&path)?;
        merge.add(parse_year_file(&path.display().to_string(), &text)?);
    }
    Ok(merge.finish())
}

/// The key an entry gets when it is first curated: `2024-runner-up-tnt-overcharged`.
///
/// A key is stable forever once it is in a file, whatever later happens to the
/// title, so nothing recomputes this to find a row. It has two callers, both of
/// them at the moment a key comes into being: the draft tool, and the migration that
/// keyed rows scraped before there was a dataset. That those two agree is how a row
/// the user pinned before the migration finds its curated entry.
pub fn entry_key(year: i64, category: &str, title: &str) -> String {
    format!("{year}-{category}-{}", slug(title))
}

/// Text as lowercase ASCII words joined by hyphens: a title in a key, and a one-off
/// award's name as its category.
///
/// Accents go (`Piña` is `pina`) and apostrophes close up (`Don't` is `dont`), and
/// any other run of punctuation separates words, so `Break//Point//` is `break-point`
/// where a plain delete would make it `breakpoint`.
pub fn slug(title: &str) -> String {
    let mut words = String::new();
    for c in title.to_lowercase().nfd() {
        if unicode_normalization::char::is_combining_mark(c) || matches!(c, '\'' | '’' | '`') {
            continue;
        }
        match c {
            'æ' => words.push_str("ae"),
            'œ' => words.push_str("oe"),
            'ø' => words.push('o'),
            'ß' => words.push_str("ss"),
            'ł' => words.push('l'),
            'đ' => words.push('d'),
            c if c.is_ascii_alphanumeric() => words.push(c),
            _ => words.push(' '),
        }
    }
    let slug = words.split_whitespace().collect::<Vec<_>>().join("-");
    if slug.is_empty() {
        // A title of nothing but symbols still needs a key.
        "entry".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::write(dir.join(name), text).unwrap();
    }

    const MINIMAL: &str = r#"
year = 3000
page = "https://example.invalid/3000/"

[[entry]]
key      = "3000-winner-some-wad"
category = "winner"
order    = 1
title    = "Some WAD"
"#;

    #[test]
    fn entries_take_their_year_from_the_file() {
        let file = parse_year_file("t.toml", MINIMAL).unwrap();
        assert_eq!(file.entry[0].year, 3000);
        assert_eq!(file.page.as_deref(), Some("https://example.invalid/3000/"));
    }

    #[test]
    fn every_field_an_entry_can_have_is_read() {
        let file = parse_year_file(
            "t.toml",
            r#"
year = 2024

[[entry]]
key       = "2024-runner-up-tnt-overcharged"
category  = "runner-up"
order     = 12
title     = "TNT: Overcharged"
author    = "Veemon"
idgames   = 19951
doomworld = 151858
doomwiki  = "TNT:_Overcharged"
sha256    = "ABCDEF"
supported = false
"#,
        )
        .unwrap();
        assert_eq!(
            file.entry[0],
            NewCacoward {
                key: "2024-runner-up-tnt-overcharged".to_string(),
                year: 2024,
                category: "runner-up".to_string(),
                page_order: 12,
                wad_title: "TNT: Overcharged".to_string(),
                wad_author: Some("Veemon".to_string()),
                idgames_id: Some(19951),
                doomworld_id: Some(151858),
                doomwiki_page: Some("TNT:_Overcharged".to_string()),
                // What the hash columns hold, so it is what a match compares.
                sha256: Some("abcdef".to_string()),
                supported: false,
            }
        );
    }

    #[test]
    fn an_entry_is_supported_unless_it_says_otherwise() {
        let file = parse_year_file("t.toml", MINIMAL).unwrap();
        assert!(file.entry[0].supported);
    }

    #[test]
    fn a_misspelled_field_is_an_error_not_a_silent_skip() {
        let err = parse_year_file(
            "typo.toml",
            r#"
year = 2024
[[entry]]
key      = "k"
category = "winner"
order    = 1
title    = "T"
idgame   = 1
"#,
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("typo.toml"), "got: {message}");
        assert!(message.contains("idgame"), "got: {message}");
    }

    #[test]
    fn an_entry_with_an_empty_title_is_an_error() {
        let err = parse_year_file(
            "blank.toml",
            r#"
year = 2024
[[entry]]
key      = "k"
category = "winner"
order    = 1
title    = "  "
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("`title`"), "got: {err}");
    }

    #[test]
    fn a_file_without_a_year_is_an_error() {
        assert!(parse_year_file("t.toml", "page = \"x\"").is_err());
    }

    #[test]
    fn a_missing_user_directory_yields_the_builtins() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join("nope")).unwrap(), builtin());
    }

    #[test]
    fn a_user_file_adds_entries_and_years() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "extra.toml", MINIMAL);

        let dataset = load(dir.path()).unwrap();

        assert_eq!(dataset.entries.len(), builtin().entries.len() + 1);
        assert!(
            dataset
                .entries
                .iter()
                .any(|entry| entry.key == "3000-winner-some-wad")
        );
        assert_eq!(
            dataset.pages.get(&3000).map(String::as_str),
            Some("https://example.invalid/3000/")
        );
    }

    #[test]
    fn a_user_file_corrects_an_entry_by_repeating_its_key() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "first.toml", MINIMAL);
        write(
            dir.path(),
            "second.toml",
            r#"
year = 3000
[[entry]]
key      = "3000-winner-some-wad"
category = "winner"
order    = 1
title    = "Corrected"
"#,
        );

        let dataset = load(dir.path()).unwrap();

        let corrected: Vec<_> = dataset
            .entries
            .iter()
            .filter(|entry| entry.key == "3000-winner-some-wad")
            .collect();
        assert_eq!(corrected.len(), 1);
        assert_eq!(corrected[0].wad_title, "Corrected");
        // A correction that does not mention the page leaves it alone.
        assert_eq!(
            dataset.pages.get(&3000).map(String::as_str),
            Some("https://example.invalid/3000/")
        );
    }

    #[test]
    fn a_malformed_user_file_is_an_error_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "bad.toml", "this is not toml =");
        let err = load(dir.path()).unwrap_err();
        assert!(err.to_string().contains("bad.toml"), "got: {err}");
    }

    #[test]
    fn other_files_in_the_directory_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "notes.txt", "this is not toml =");
        assert_eq!(load(dir.path()).unwrap(), builtin());
    }

    #[test]
    fn a_key_is_the_year_the_category_and_a_slug_of_the_title() {
        for (year, category, title, key) in [
            (
                2024,
                "runner-up",
                "TNT: Overcharged",
                "2024-runner-up-tnt-overcharged",
            ),
            (2025, "winner", "Lonesome Road", "2025-winner-lonesome-road"),
            (2023, "winner", "Piña Colada", "2023-winner-pina-colada"),
            (2023, "winner", "worm\\V/wood", "2023-winner-worm-v-wood"),
            (
                2024,
                "winner",
                "Oops... All-Ghosts!",
                "2024-winner-oops-all-ghosts",
            ),
            (
                2025,
                "doom-moma",
                "¡Conchazumanga!",
                "2025-doom-moma-conchazumanga",
            ),
            (
                2025,
                "winner",
                "Drake O'Brien Ovational Megawad",
                "2025-winner-drake-obrien-ovational-megawad",
            ),
            (
                2023,
                "honorable-mention",
                "Break//Point//",
                "2023-honorable-mention-break-point",
            ),
            (2015, "winner", "Erkattäññe", "2015-winner-erkattanne"),
            (
                2010,
                "worst-wad",
                "Æons of Death",
                "2010-worst-wad-aeons-of-death",
            ),
        ] {
            assert_eq!(entry_key(year, category, title), key);
        }
    }

    #[test]
    fn a_title_of_only_symbols_still_gets_a_key() {
        assert_eq!(entry_key(2020, "winner", "!!!"), "2020-winner-entry");
    }

    // -- the data itself --

    fn is_slug(s: &str) -> bool {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }

    fn has_markup(s: &str) -> bool {
        ["[[", "]]", "{{", "}}", "''", "[http"]
            .iter()
            .any(|markup| s.contains(markup))
    }

    #[test]
    fn every_data_file_is_compiled_in() {
        // A year whose file is on disk but missing from BUILTIN would ship nothing
        // and look like it worked.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cacowards/data");
        let on_disk: HashSet<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|file| file.file_name().to_string_lossy().to_string())
            .collect();
        let compiled_in: HashSet<String> =
            BUILTIN.iter().map(|(_, name)| name.to_string()).collect();
        assert_eq!(on_disk, compiled_in);
    }

    #[test]
    fn each_file_is_named_after_its_year() {
        for (text, name) in BUILTIN {
            let file = parse_year_file(name, text).unwrap();
            assert_eq!(*name, format!("{}.toml", file.year));
        }
    }

    #[test]
    fn the_years_run_unbroken_from_the_first_cacowards() {
        let years: Vec<i64> = BUILTIN
            .iter()
            .map(|(text, name)| parse_year_file(name, text).unwrap().year)
            .collect();
        assert_eq!(years.first(), Some(&2004));
        for pair in years.windows(2) {
            assert_eq!(pair[0] + 1, pair[1]);
        }
    }

    #[test]
    fn no_key_is_used_twice_in_the_data() {
        // Merging by key would hide the second entry rather than complain, so this
        // has to look at the files, not the merge.
        let mut seen = HashSet::new();
        for (text, name) in BUILTIN {
            for entry in parse_year_file(name, text).unwrap().entry {
                assert!(
                    seen.insert(entry.key.clone()),
                    "{name}: {} twice",
                    entry.key
                );
            }
        }
    }

    #[test]
    fn each_key_starts_with_its_entrys_year_and_category() {
        for entry in builtin().entries {
            let prefix = format!("{}-{}-", entry.year, entry.category);
            assert!(
                entry.key.starts_with(&prefix),
                "{} is not {prefix}…",
                entry.key
            );
            assert!(is_slug(&entry.key), "{} is not a slug", entry.key);
            assert!(is_slug(&entry.category), "{} is not a slug", entry.category);
        }
    }

    #[test]
    fn page_order_counts_up_from_one_within_a_category() {
        let mut orders: HashMap<(i64, String), Vec<i64>> = HashMap::new();
        for entry in builtin().entries {
            orders
                .entry((entry.year, entry.category))
                .or_default()
                .push(entry.page_order);
        }
        for ((year, category), mut order) in orders {
            order.sort_unstable();
            let expected: Vec<i64> = (1..=order.len() as i64).collect();
            assert_eq!(order, expected, "{year} {category}");
        }
    }

    #[test]
    fn identifiers_are_shaped_like_identifiers() {
        for entry in builtin().entries {
            let key = &entry.key;
            assert!(entry.idgames_id.is_none_or(|id| id > 0), "{key}");
            assert!(entry.doomworld_id.is_none_or(|id| id > 0), "{key}");
            if let Some(page) = &entry.doomwiki_page {
                assert!(!page.is_empty() && !page.contains(' '), "{key}: {page:?}");
            }
            if let Some(hash) = &entry.sha256 {
                assert!(
                    hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()),
                    "{key}: {hash:?}"
                );
            }
        }
    }

    #[test]
    fn no_draft_notes_are_left_in_the_data() {
        // The draft tool's way of asking for a human: this is what makes it answered.
        for (text, name) in BUILTIN {
            assert!(!text.contains("# CHECK"), "{name} still has a `# CHECK`");
        }
    }

    #[test]
    fn no_wiki_markup_survived_curation() {
        for entry in builtin().entries {
            assert!(!has_markup(&entry.wad_title), "{}", entry.key);
            assert_eq!(entry.wad_title.trim(), entry.wad_title, "{}", entry.key);
            if let Some(author) = &entry.wad_author {
                assert!(!has_markup(author), "{}: {author}", entry.key);
                assert!(!author.trim().is_empty(), "{}", entry.key);
            }
        }
    }
}
