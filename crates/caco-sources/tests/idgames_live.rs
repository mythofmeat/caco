//! The local index against the real archive: what the fakes in the unit tests were modelled
//! on, and what they could be wrong about. `#[ignore]`d, because `cargo test --workspace`
//! must not need a network, and these ask real people's servers for real files, though not
//! many: one listing per mirror, one directory of the API, and a few dozen text files.
//!
//! ```bash
//! cargo test -p caco-sources --test idgames_live -- --ignored --nocapture
//! ```
//!
//! A blocked API is not a failure here, since it is the day the index exists for: those
//! tests say so and stop. What they find out about the text files is printed, as the rate
//! at which they read as the API says they do is worth looking at.

use std::thread::sleep;
use std::time::Duration;

use caco_sources::SourceError;
use caco_sources::http::{USER_AGENT, build_client};
use caco_sources::idgames::listing::{self, LISTING_PATH};
use caco_sources::idgames::{FileEntry, IdgamesClient, MIRRORS, textfile};
use reqwest::header::{ETAG, IF_NONE_MATCH};

/// quaddicted has the files and not the listing.
const MIRRORS_WITH_NO_LISTING: &[&str] = &["https://www.quaddicted.com/files/idgames/"];

fn compared(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn listing_uploads(mirror: &str) -> Vec<listing::ListedUpload> {
    let client = build_client();
    let response = client
        .get(format!("{mirror}{LISTING_PATH}"))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let bytes = response.bytes().unwrap();
    let text = listing::decompress(&bytes).unwrap();
    listing::parse_listing(&text, chrono::Utc::now().date_naive())
}

#[test]
#[ignore = "asks the real mirrors"]
fn each_mirror_has_the_listing_the_index_expects() {
    for mirror in MIRRORS {
        if MIRRORS_WITH_NO_LISTING.contains(mirror) {
            continue;
        }
        let uploads = listing_uploads(mirror);
        println!("{mirror}: {} uploads", uploads.len());
        assert!(uploads.len() > 20_000, "{mirror}: {}", uploads.len());

        let sunlust = uploads
            .iter()
            .find(|upload| upload.path() == "levels/doom2/Ports/megawads/sunlust.zip")
            .unwrap_or_else(|| panic!("{mirror} has no sunlust.zip"));
        assert_eq!(sunlust.date, "2015-08-09");
        assert!(sunlust.size > 1_000_000);
        assert!(sunlust.text.is_some(), "{mirror} has no text file for it");

        assert!(
            !uploads
                .iter()
                .any(|upload| upload.dir.starts_with("newstuff/")
                    || upload.dir.starts_with("incoming/")),
            "{mirror} has uploads passing through"
        );
        // What a year-less date resolved to is in the listing's past, give or take a day.
        let tomorrow = chrono::Utc::now().date_naive() + chrono::Duration::days(1);
        let tomorrow = tomorrow.format("%Y-%m-%d").to_string();
        for upload in &uploads {
            assert!(
                upload.date <= tomorrow,
                "{}: {}",
                upload.path(),
                upload.date
            );
            assert!(
                upload.date.as_str() > "1993",
                "{}: {}",
                upload.path(),
                upload.date
            );
        }
    }
}

/// That a mirror answers a conditional request with a 304 and nothing in the body is the
/// whole of why a daily check costs nothing.
#[test]
#[ignore = "asks the real mirrors"]
fn each_mirror_answers_a_conditional_get() {
    let client = build_client();
    for mirror in MIRRORS {
        if MIRRORS_WITH_NO_LISTING.contains(mirror) {
            continue;
        }
        let url = format!("{mirror}{LISTING_PATH}");
        let head = client
            .head(&url)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
        let etag = head
            .headers()
            .get(ETAG)
            .unwrap_or_else(|| panic!("{mirror} sends no ETag"))
            .clone();
        let again = client.get(&url).header(IF_NONE_MATCH, etag).send().unwrap();
        assert_eq!(again.status(), 304, "{mirror}");
    }
}

/// The sweep, for one directory, and the two ways the real API has of being odd.
#[test]
#[ignore = "asks the real API"]
fn a_directory_of_the_api_has_every_upload_the_listing_has() {
    let dir = "levels/doom2/Ports/v-z/";
    let entries = match IdgamesClient::new().list_directory(dir) {
        Ok(entries) => entries,
        Err(blocked @ SourceError::WafBlocked { .. }) => {
            println!("the API is blocked today: {blocked}");
            return;
        }
        Err(other) => panic!("{other}"),
    };
    // Has `"title": 2014`, which used to cost the whole directory.
    let vtm: &FileEntry = entries
        .iter()
        .find(|entry| entry.filename == "vtm_2014.zip")
        .expect("vtm_2014.zip, whose title is a number to the API");
    assert_eq!(vtm.title, "2014");

    let listed = listing_uploads(MIRRORS[0]);
    let here: Vec<&str> = listed
        .iter()
        .filter(|upload| upload.dir == dir)
        .map(|upload| upload.filename.as_str())
        .collect();
    let answered = entries
        .iter()
        .filter(|entry| here.contains(&entry.filename.as_str()))
        .count();
    println!("{answered} of {} in the listing", here.len());
    assert!(
        answered * 100 >= here.len() * 95,
        "{answered} of {}",
        here.len()
    );
}

/// How well the text files read: the same thing the API's own parser made of them, as a
/// share of a sample from all eras of the archive. Not all of them should match. The API
/// has been corrected by hand here and there, and the two take different views of a field
/// with several people in it. This is here to notice when the reading gets worse.
#[test]
#[ignore = "asks the real API and the real mirrors"]
fn text_files_read_about_the_way_the_api_read_them() {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .unwrap();
    let api = IdgamesClient::new();

    let (mut compared_titles, mut same_titles) = (0, 0);
    let (mut compared_authors, mut same_authors) = (0, 0);
    for dir in [
        "levels/doom/a-c/",
        "levels/doom2/m-o/",
        "levels/doom2/Ports/megawads/",
    ] {
        let entries = match api.list_directory(dir) {
            Ok(entries) => entries,
            Err(blocked @ SourceError::WafBlocked { .. }) => {
                println!("the API is blocked today: {blocked}");
                return;
            }
            Err(other) => panic!("{other}"),
        };
        for entry in entries.iter().step_by(entries.len().div_ceil(15)) {
            let stem = entry.filename.trim_end_matches(".zip");
            let url = format!("{}{}{stem}.txt", MIRRORS[0], entry.dir);
            sleep(Duration::from_millis(250));
            let Ok(bytes) = client.get(&url).send().and_then(|r| r.error_for_status()) else {
                continue;
            };
            let found = textfile::parse(&textfile::decode(&bytes.bytes().unwrap()));
            if !entry.title.is_empty() {
                compared_titles += 1;
                if compared(&found.title) == compared(&entry.title) {
                    same_titles += 1;
                } else {
                    println!("title:  {:?} / the API: {:?}", found.title, entry.title);
                }
            }
            if !entry.author.is_empty() {
                compared_authors += 1;
                if compared(&found.author) == compared(&entry.author) {
                    same_authors += 1;
                } else {
                    println!("author: {:?} / the API: {:?}", found.author, entry.author);
                }
            }
        }
        sleep(Duration::from_secs(1));
    }
    println!(
        "titles: {same_titles} of {compared_titles}; authors: {same_authors} of {compared_authors}"
    );
    assert!(
        same_titles * 100 >= compared_titles * 75,
        "{same_titles} of {compared_titles}"
    );
    assert!(
        same_authors * 100 >= compared_authors * 65,
        "{same_authors} of {compared_authors}"
    );
}
