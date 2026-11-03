//! Draft a year's Cacowards data file from the Doom Wiki's page for that year.
//!
//! ```sh
//! cargo run -p caco-sources --example cacowards_draft -- 2026 2026.wikitext \
//!     > crates/caco-core/src/cacowards/data/2026.toml
//! ```
//!
//! This is the scraper that used to run on every user's machine, and it runs once a
//! year now, for a maintainer. It reads wikitext saved by hand, because the wiki sits
//! behind a Cloudflare challenge a browser passes and a client does not: open
//! `https://doomwiki.org/w/index.php?title=Cacowards_2026&action=raw` and save it.
//! Add `--offline` to skip the idgames API, which is what fills in the ids of entries
//! the page links by path, checks the rest against what the archive says they are,
//! and looks for the ones a page only knows from a forum thread: an upload to the
//! archive can come well after the award, and an idgames id is what lets caco fetch
//! a WAD unattended.
//!
//! The output is a draft. Everything the tool is unsure of is a `# CHECK:` comment,
//! and a test refuses to ship a file that still has one. The human pass is: settle
//! each of those, drop anything the library could not hold, set `supported = false`
//! on what no sourceport caco launches can run, then add the year to `BUILTIN` in
//! `caco_core::cacowards`.

mod wikitext;

use std::fmt::Write as _;
use std::process::ExitCode;
use std::time::Duration;

use caco_core::cacowards::entry_key;
use caco_sources::idgames::{FileEntry, IdgamesClient};
use caco_sources::import_service::titles_match;

use wikitext::{DraftEntry, DraftPage, IdgamesRef, parse_page};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let offline = args.iter().any(|arg| arg == "--offline");
    let positional: Vec<&String> = args.iter().filter(|arg| !arg.starts_with("--")).collect();

    let [year, path] = positional.as_slice() else {
        eprintln!("usage: cacowards_draft <year> <wikitext-file> [--offline]");
        return ExitCode::from(2);
    };
    let Ok(year) = year.parse::<i64>() else {
        eprintln!("not a year: {year}");
        return ExitCode::from(2);
    };
    let wikitext = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut draft = parse_page(&wikitext);
    if draft.entries.is_empty() {
        eprintln!("{path}: no entries found. Is this the page's wikitext?");
        return ExitCode::FAILURE;
    }

    if !offline {
        let client = IdgamesClient::new();
        let lookup = |reference: &IdgamesRef| -> Result<FileEntry, String> {
            // One request per entry, once a year, but there is no call to be rude.
            std::thread::sleep(Duration::from_millis(200));
            match reference {
                IdgamesRef::Id(id) => client.get(Some(*id), None),
                IdgamesRef::File(path) => client.get_by_path(&archive_path(path)),
            }
            .map_err(|e| e.to_string())
        };
        check_against_archive(&mut draft, lookup);

        let search = |title: &str| -> Result<Vec<FileEntry>, String> {
            std::thread::sleep(Duration::from_millis(200));
            client
                .search(title, Some("title"), None, None)
                .map_err(|e| e.to_string())
        };
        suggest_from_archive(&mut draft, search);
    }

    print!("{}", render(year, &draft));
    ExitCode::SUCCESS
}

/// The API wants a file name, and the page's template takes a slug without one.
fn archive_path(path: &str) -> String {
    let last = path.rsplit('/').next().unwrap_or("");
    if last.contains('.') {
        path.to_string()
    } else {
        format!("{path}.zip")
    }
}

/// Resolve each entry's archive path to an id, and say so when what the archive holds
/// at an id does not look like what the page says is there. The pages do get this
/// wrong: Cacowards 2010 and 2009 give two different WADs the same id.
fn check_against_archive(
    draft: &mut DraftPage,
    lookup: impl Fn(&IdgamesRef) -> Result<FileEntry, String>,
) {
    for entry in &mut draft.entries {
        let Some(reference) = entry.idgames.clone() else {
            continue;
        };
        match lookup(&reference) {
            Ok(file) => {
                entry.idgames = Some(IdgamesRef::Id(file.id));
                if !titles_match(&entry.title, &file.title) {
                    entry.notes.push(format!(
                        "idgames {} is {:?} ({}), by {:?}",
                        file.id, file.title, file.filename, file.author
                    ));
                }
            }
            Err(e) => {
                entry
                    .notes
                    .push(format!("could not look up {reference:?} on idgames: {e}"));
                // A path is not an identifier, so it cannot stay. The id might
                // still be right, so it can, and the note says it went unverified.
                if matches!(reference, IdgamesRef::File(_)) {
                    entry.idgames = None;
                }
            }
        }
    }
}

/// For each entry the page gives no idgames link, say what the archive has under
/// that title. Only ever a note, since a shared title is no proof: "Corruption" is
/// three WADs.
fn suggest_from_archive(
    draft: &mut DraftPage,
    search: impl Fn(&str) -> Result<Vec<FileEntry>, String>,
) {
    for entry in &mut draft.entries {
        if entry.idgames.is_some() {
            continue;
        }
        // A search that fails is no worse than not having looked.
        let Ok(found) = search(&entry.title) else {
            continue;
        };
        for file in found
            .iter()
            .filter(|file| titles_match(&entry.title, &file.title))
        {
            entry.notes.push(format!(
                "idgames may have this: {} {:?} ({}/{}), by {:?}, dated {}",
                file.id, file.title, file.dir, file.filename, file.author, file.date
            ));
        }
    }
}

/// A TOML string. JSON's escapes are a subset of TOML's.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).expect("a string always serializes")
}

/// Print a page as a data file, in the layout the checked-in files have.
fn render(year: i64, draft: &DraftPage) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Facts from Doomworld's Cacowards, by way of the Doom Wiki."
    );
    let _ = writeln!(out, "year = {year}");
    if let Some(page) = &draft.page {
        let _ = writeln!(out, "page = {}", quoted(page));
    }
    if !draft.skipped.is_empty() {
        let _ = writeln!(
            out,
            "\n# Not entries, being people or tools: {}.",
            draft.skipped.join(", ")
        );
    }
    for entry in &draft.entries {
        render_entry(&mut out, year, entry);
    }
    out
}

fn render_entry(out: &mut String, year: i64, entry: &DraftEntry) {
    out.push('\n');
    for note in &entry.notes {
        let _ = writeln!(out, "# CHECK: {note}");
    }
    let _ = writeln!(out, "[[entry]]");
    let _ = writeln!(
        out,
        "key       = {}",
        quoted(&entry_key(year, &entry.category, &entry.title))
    );
    let _ = writeln!(out, "category  = {}", quoted(&entry.category));
    let _ = writeln!(out, "order     = {}", entry.order);
    let _ = writeln!(out, "title     = {}", quoted(&entry.title));
    if let Some(author) = &entry.author {
        let _ = writeln!(out, "author    = {}", quoted(author));
    }
    match &entry.idgames {
        Some(IdgamesRef::Id(id)) => {
            let _ = writeln!(out, "idgames   = {id}");
        }
        // Only an offline draft still has a path. `idgames` is a number.
        Some(IdgamesRef::File(path)) => {
            let _ = writeln!(out, "# CHECK: resolve idgames path {path}");
        }
        None => {}
    }
    if let Some(topic) = entry.doomworld {
        let _ = writeln!(out, "doomworld = {topic}");
    }
    if let Some(page) = &entry.doomwiki {
        let _ = writeln!(out, "doomwiki  = {}", quoted(page));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "=== Winners ===
* [[Eviternity]] - [[Dragonfly]] et al. - ({{ig|id=19304}})
* [[Going Down]] - [[Cyriak Harris (mouldy)]] ({{ig|file=levels/doom2/Ports/megawads/gd}})
* [[Lonesome Road]] - [[Thomas Seifert (tourniquet)]] - ({{dwforums|151858}})

=== Creator of the Year ===
* [[Bridgeburner56]]

== External links ==
* [https://www.doomworld.com/cacowards/2019/ The 2019 Cacowards]
";

    fn file(id: i64, title: &str) -> FileEntry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "title": title,
            "filename": "file.zip",
        }))
        .unwrap()
    }

    fn entry_called<'a>(draft: &'a DraftPage, title: &str) -> &'a DraftEntry {
        draft
            .entries
            .iter()
            .find(|entry| entry.title == title)
            .unwrap()
    }

    #[test]
    fn a_draft_loads_as_a_data_file() {
        let mut draft = parse_page(PAGE);
        check_against_archive(&mut draft, |reference| match reference {
            IdgamesRef::Id(id) => Ok(file(*id, "Eviternity")),
            IdgamesRef::File(_) => Ok(file(17500, "Going Down")),
        });
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("3019.toml"), render(3019, &draft)).unwrap();

        let dataset = caco_core::cacowards::load(dir.path()).unwrap();

        let going_down = dataset
            .entries
            .iter()
            .find(|entry| entry.key == "3019-winner-going-down")
            .unwrap();
        assert_eq!(going_down.year, 3019);
        assert_eq!(going_down.page_order, 2);
        assert_eq!(going_down.idgames_id, Some(17500));
        assert_eq!(going_down.doomwiki_page.as_deref(), Some("Going_Down"));
        assert_eq!(
            going_down.wad_author.as_deref(),
            Some("Cyriak Harris (mouldy)")
        );
        let lonesome = dataset
            .entries
            .iter()
            .find(|entry| entry.key == "3019-winner-lonesome-road")
            .unwrap();
        assert_eq!(lonesome.doomworld_id, Some(151858));
        assert_eq!(
            dataset.pages.get(&3019).map(String::as_str),
            Some("https://www.doomworld.com/cacowards/2019/")
        );
    }

    #[test]
    fn an_offline_draft_still_loads_with_its_paths_left_as_comments() {
        let draft = parse_page(PAGE);
        let text = render(3019, &draft);
        assert!(text.contains("# CHECK: resolve idgames path levels/doom2/Ports/megawads/gd"));

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("3019.toml"), text).unwrap();
        assert!(caco_core::cacowards::load(dir.path()).is_ok());
    }

    #[test]
    fn a_title_with_quotes_and_backslashes_stays_valid_toml() {
        let draft = parse_page("=== Winners ===\n* [[worm\\V/wood]] - \"Quoted\" Author\n");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("3000.toml"), render(3000, &draft)).unwrap();

        let dataset = caco_core::cacowards::load(dir.path()).unwrap();

        let entry = dataset
            .entries
            .iter()
            .find(|entry| entry.year == 3000)
            .unwrap();
        assert_eq!(entry.wad_title, "worm\\V/wood");
        assert_eq!(entry.wad_author.as_deref(), Some("\"Quoted\" Author"));
    }

    #[test]
    fn notes_are_printed_above_their_entry_and_people_at_the_top() {
        let draft = parse_page("=== Winners ===\n* Mohrta - Someone\n\n=== Espi award ===\n* X\n");
        let text = render(3000, &draft);
        let check = text.find("# CHECK: no link in the title").unwrap();
        let entry = text.find("[[entry]]").unwrap();
        assert!(check < entry);
        assert!(text.contains("# Not entries, being people or tools: Espi award."));
    }

    #[test]
    fn a_mismatch_with_the_archive_is_flagged_and_keeps_the_id() {
        let mut draft = parse_page(PAGE);
        check_against_archive(&mut draft, |reference| match reference {
            IdgamesRef::Id(id) => Ok(file(*id, "Cheogsh 2")),
            IdgamesRef::File(_) => Ok(file(17500, "Going Down")),
        });

        let entry = entry_called(&draft, "Eviternity");
        assert_eq!(entry.idgames, Some(IdgamesRef::Id(19304)));
        assert!(entry.notes.iter().any(|note| note.contains("Cheogsh 2")));
        assert!(entry_called(&draft, "Going Down").notes.is_empty());
    }

    #[test]
    fn a_path_the_archive_does_not_know_leaves_a_note_and_no_id() {
        let mut draft = parse_page(PAGE);
        check_against_archive(&mut draft, |reference| match reference {
            IdgamesRef::Id(id) => Ok(file(*id, "Eviternity")),
            IdgamesRef::File(_) => Err("Bad File".to_string()),
        });

        let entry = entry_called(&draft, "Going Down");
        assert_eq!(entry.idgames, None);
        assert!(entry.notes.iter().any(|note| note.contains("Bad File")));
    }

    #[test]
    fn an_id_the_archive_cannot_be_asked_about_stays_with_a_note() {
        let mut draft = parse_page(PAGE);
        check_against_archive(&mut draft, |reference| match reference {
            IdgamesRef::Id(_) => Err("Cloudflare".to_string()),
            IdgamesRef::File(_) => Ok(file(17500, "Going Down")),
        });

        let entry = entry_called(&draft, "Eviternity");
        assert_eq!(entry.idgames, Some(IdgamesRef::Id(19304)));
        assert!(entry.notes.iter().any(|note| note.contains("Cloudflare")));
    }

    #[test]
    fn an_entry_with_no_id_gets_what_the_archive_has_under_its_title() {
        let mut draft = parse_page(PAGE);
        suggest_from_archive(&mut draft, |title| {
            Ok(vec![file(21867, title), file(1, "Some Other WAD")])
        });

        let entry = entry_called(&draft, "Lonesome Road");
        assert_eq!(entry.idgames, None, "a suggestion is not a decision");
        assert_eq!(entry.notes.len(), 1);
        assert!(entry.notes[0].contains("idgames may have this: 21867"));
    }

    #[test]
    fn an_entry_with_an_id_is_not_searched_for() {
        let mut draft = parse_page(PAGE);
        let searched = std::cell::RefCell::new(Vec::new());
        suggest_from_archive(&mut draft, |title| {
            searched.borrow_mut().push(title.to_string());
            Ok(Vec::new())
        });
        assert_eq!(*searched.borrow(), ["Lonesome Road"]);
    }

    #[test]
    fn a_failed_search_leaves_the_entry_as_it_was() {
        let mut draft = parse_page(PAGE);
        suggest_from_archive(&mut draft, |_| Err("Cloudflare".to_string()));
        assert!(entry_called(&draft, "Lonesome Road").notes.is_empty());
    }

    #[test]
    fn archive_paths_get_a_zip_when_they_have_no_extension() {
        assert_eq!(
            archive_path("levels/doom2/Ports/d-f/fw"),
            "levels/doom2/Ports/d-f/fw.zip"
        );
        assert_eq!(
            archive_path("levels/doom2/Ports/megawads/tph.zip"),
            "levels/doom2/Ports/megawads/tph.zip"
        );
    }
}
