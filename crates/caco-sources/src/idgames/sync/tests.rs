//! A sync against [`TestArchive`], and real files for a copy of idgames.

use std::cell::Cell;
use std::fs;

use chrono::TimeZone;

use super::*;
use crate::idgames::test_archive::{TestArchive, listing_of};

const LISTING_DATE: &str = "Tue, 03 Nov 2026 06:14:01 GMT";
const SUNLUST: &str = "levels/doom2/Ports/megawads/sunlust.zip";
const SUNLUST_TEXT: &str = "levels/doom2/Ports/megawads/sunlust.txt";
const SCYTHE: &str = "levels/doom2/megawads/scythe.zip";
const SCYTHE_TEXT: &str = "levels/doom2/megawads/scythe.txt";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 3, 12, 0, 0).unwrap()
}

/// A whole archive: two WADs, each with a text file, and the API knowing about both.
struct Fixture {
    archive: TestArchive,
    index: Index,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let archive = TestArchive::start();
        let f = Self {
            index: Index::open(&dir.path().join("index.db")).unwrap(),
            archive,
            dir,
        };
        f.put_listing(LISTING_DATE, &[]);
        f.archive.put_file(
            SUNLUST_TEXT,
            b"Title : Sunlust\nAuthor : Ribbiks\nDescription : 32 maps.\n",
            LISTING_DATE,
        );
        f.archive.put_file(
            SCYTHE_TEXT,
            b"Title : Scythe\nAuthor : Erik Alm\nDescription : 32 more maps.\n",
            LISTING_DATE,
        );
        f.archive
            .put_entry(api_entry(SUNLUST, 18168, 1000, "Sunlust (API)"));
        f.archive
            .put_entry(api_entry(SCYTHE, 11742, 2000, "Scythe (API)"));
        f
    }

    /// `ls-laR.gz` as of `generated`, with the two WADs and whatever else is asked for.
    fn put_listing(&self, generated: &str, more: &[(&str, u64, &str)]) {
        self.put_listing_on(&self.archive, generated, more);
    }

    fn put_listing_on(&self, archive: &TestArchive, generated: &str, more: &[(&str, u64, &str)]) {
        let mut files = vec![
            (SUNLUST, 1000, "Aug  9  2015"),
            (SUNLUST_TEXT, 100, "Aug  9  2015"),
            (SCYTHE, 2000, "Jan  1  2003"),
            (SCYTHE_TEXT, 100, "Jan  1  2003"),
        ];
        files.extend_from_slice(more);
        archive.put_file(LISTING_PATH, &listing_of(&files), generated);
    }

    fn options(&self) -> SyncOptions {
        SyncOptions {
            sources: vec![self.archive.as_mirror()],
            api: IdgamesClient::with_api_url(self.archive.api_url()),
            check_days: 1,
            force: false,
            now: now(),
            api_pause: Duration::ZERO,
            text_pause: Duration::ZERO,
        }
    }

    fn sync(&self, options: &SyncOptions) -> SyncReport {
        sync_index(&self.index, options, &mut |_| {}, &|| false).unwrap()
    }

    /// What the archive was asked for.
    fn asked(&self) -> Vec<String> {
        self.archive.log()
    }
}

fn api_entry(path: &str, id: i64, size: i64, title: &str) -> serde_json::Value {
    let (dir, filename) = path.rsplit_once('/').unwrap();
    serde_json::json!({
        "id": id,
        "title": title,
        "dir": format!("{dir}/"),
        "filename": filename,
        "size": size,
        "date": "2015-08-09",
        "author": "From the API",
        "description": "What the API says.",
        "rating": 4.5,
        "votes": 10,
    })
}

fn mirror_requests(log: &[String]) -> Vec<&str> {
    log.iter()
        .filter_map(|line| line.strip_prefix("mirror "))
        .collect()
}

fn api_requests(log: &[String]) -> Vec<&str> {
    log.iter()
        .filter_map(|line| line.strip_prefix("api "))
        .collect()
}

fn added(n: usize) -> ListingOutcome {
    ListingOutcome::Updated(ListingChanges {
        added: n,
        changed: 0,
        removed: 0,
    })
}

// ---------------------------------------------------------------------------
// The stages working together
// ---------------------------------------------------------------------------

/// The good day: one listing, one call per directory, and nothing asked of the mirror
/// for what the API has already said.
#[test]
fn test_a_first_sync_with_the_api_up_reads_no_text_files_at_all() {
    let f = Fixture::new();

    let report = f.sync(&f.options());

    assert_eq!(report.listing, added(2));
    assert_eq!(report.ids_learned, 2);
    assert_eq!(report.text_read, 0);
    assert_eq!(report.api_failure, None);
    assert!(report.changed());

    let log = f.asked();
    assert_eq!(mirror_requests(&log), [LISTING_PATH]);
    assert_eq!(
        api_requests(&log),
        [
            "getfiles levels/doom2/Ports/megawads/",
            "getfiles levels/doom2/megawads/",
        ]
    );

    let entry = f.index.by_id(18168).unwrap().unwrap();
    assert_eq!(entry.path(), SUNLUST);
    assert_eq!(entry.title, "Sunlust (API)");
    assert_eq!((entry.rating, entry.votes), (4.5, 10));
    assert!(f.index.status().unwrap().is_complete());
}

/// One call per directory rather than per upload, which is the whole reason a first run
/// is a few hundred calls and not 21,000.
#[test]
fn test_a_directory_nothing_is_known_about_is_asked_about_in_one_call() {
    let f = Fixture::new();
    let many: Vec<(String, u64, &str)> = (0..FEW_ENOUGH_TO_ASK_ONE_BY_ONE * 3)
        .map(|n| (format!("levels/heretic/h{n}.zip"), 10, "Mar  1  2001"))
        .collect();
    let listed: Vec<(&str, u64, &str)> = many
        .iter()
        .map(|(path, size, when)| (path.as_str(), *size, *when))
        .collect();
    f.put_listing(LISTING_DATE, &listed);
    for (n, (path, size, _)) in many.iter().enumerate() {
        f.archive
            .put_entry(api_entry(path, 100 + n as i64, *size as i64, "A map"));
    }

    let report = f.sync(&f.options());

    let log = f.asked();
    let asked = api_requests(&log);
    assert!(asked.contains(&"getfiles levels/heretic/"), "{asked:?}");
    assert_eq!(
        asked.iter().filter(|line| line.contains("heretic")).count(),
        1,
        "{asked:?}"
    );
    assert_eq!(report.ids_learned, many.len() + 2);
}

/// Otherwise a small directory on a first run costs a call per upload for nothing.
#[test]
fn test_a_directory_that_is_all_new_is_asked_about_as_one_however_small() {
    let f = Fixture::new();
    f.sync(&f.options());
    let log = f.asked();
    assert!(
        api_requests(&log)
            .iter()
            .all(|line| line.starts_with("getfiles")),
        "{log:?}"
    );
}

/// The usual day: a handful of new uploads, in directories that can be 700 KB each.
#[test]
fn test_a_few_new_uploads_in_a_known_directory_are_asked_about_one_by_one() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.clear_log();
    let new: Vec<String> = (0..FEW_ENOUGH_TO_ASK_ONE_BY_ONE)
        .map(|n| format!("levels/doom2/megawads/new{n}.zip"))
        .collect();
    let listed: Vec<(&str, u64, &str)> = new
        .iter()
        .map(|path| (path.as_str(), 10, "Nov  3 09:00"))
        .collect();
    f.put_listing("Wed, 04 Nov 2026 06:14:01 GMT", &listed);
    for (n, path) in new.iter().enumerate() {
        f.archive
            .put_entry(api_entry(path, 300 + n as i64, 10, "New"));
    }

    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::days(1),
        ..f.options()
    });

    assert_eq!(report.ids_learned, new.len());
    let log = f.asked();
    let asked = api_requests(&log);
    assert_eq!(asked.len(), new.len(), "{asked:?}");
    assert!(
        asked.iter().all(|line| line.starts_with("get ")),
        "{asked:?}"
    );
}

/// Past that it costs less to ask for the directory.
#[test]
fn test_more_than_a_few_new_uploads_in_a_known_directory_are_asked_about_as_one() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.clear_log();
    let new: Vec<String> = (0..FEW_ENOUGH_TO_ASK_ONE_BY_ONE + 1)
        .map(|n| format!("levels/doom2/megawads/new{n}.zip"))
        .collect();
    let listed: Vec<(&str, u64, &str)> = new
        .iter()
        .map(|path| (path.as_str(), 10, "Nov  3 09:00"))
        .collect();
    f.put_listing("Wed, 04 Nov 2026 06:14:01 GMT", &listed);

    f.sync(&SyncOptions {
        now: now() + chrono::Duration::days(1),
        ..f.options()
    });

    assert_eq!(
        api_requests(&f.asked()),
        ["getfiles levels/doom2/megawads/"]
    );
}

/// The day this is all for. Nothing from doomworld.com, and everything still works.
#[test]
fn test_with_the_api_blocked_it_all_comes_from_the_mirror() {
    let f = Fixture::new();
    f.archive.block_api(true);

    let report = f.sync(&f.options());

    assert_eq!(report.listing, added(2));
    assert_eq!(report.ids_learned, 0);
    assert_eq!(report.text_read, 2);
    let failure = report.api_failure.as_deref().unwrap();
    assert!(failure.contains("blocked"), "{failure}");
    assert_eq!(report.text_failure, None);

    // One try, rather than one try per upload.
    assert_eq!(api_requests(&f.asked()).len(), 1);

    let found = f.index.search("erik alm", 10).unwrap();
    assert_eq!(found[0].title, "Scythe");
    assert_eq!(found[0].id, 0);
    assert!(f.index.status().unwrap().is_complete());
}

#[test]
fn test_ids_arrive_when_the_api_comes_back_and_the_text_is_not_read_again() {
    let f = Fixture::new();
    f.archive.block_api(true);
    f.sync(&f.options());
    assert_eq!(f.index.status().unwrap().with_id, 0);

    f.archive.block_api(false);
    f.archive.clear_log();
    let later = SyncOptions {
        now: now() + chrono::Duration::hours(1),
        ..f.options()
    };
    let report = f.sync(&later);

    assert_eq!(report.ids_learned, 2);
    assert_eq!(report.text_read, 0);
    assert_eq!(mirror_requests(&f.asked()), Vec::<&str>::new());
    assert_eq!(f.index.status().unwrap().with_id, 2);
}

#[test]
fn test_the_api_failing_partway_leaves_the_rest_to_the_text_files() {
    let f = Fixture::new();
    f.archive.block_api_after(1);

    let report = f.sync(&f.options());

    assert_eq!(report.ids_learned, 1);
    assert_eq!(report.text_read, 1);
    assert!(report.api_failure.is_some());
    assert!(f.index.status().unwrap().is_complete());
}

/// The API lags the archive: this is `utakata.zip` on the day after it was replaced.
#[test]
fn test_what_the_api_says_about_an_older_file_does_not_stand_in_for_this_one() {
    let f = Fixture::new();
    let api = TestArchive::start();
    api.put_entry(api_entry(SUNLUST, 18168, 1000, "Sunlust (API)"));
    api.put_entry(api_entry(SCYTHE, 11742, 1999, "Scythe (the old one)"));

    let report = f.sync(&SyncOptions {
        api: IdgamesClient::with_api_url(api.api_url()),
        ..f.options()
    });

    assert_eq!(report.text_read, 1);
    let scythe = f.index.by_id(11742).unwrap().unwrap();
    assert_eq!(scythe.title, "Scythe");
    assert_eq!(scythe.author, "Erik Alm");
    // Still the same upload, with the same rating.
    assert_eq!(scythe.rating, 4.5);
}

// ---------------------------------------------------------------------------
// Asking for less the next time
// ---------------------------------------------------------------------------

#[test]
fn test_a_listing_is_not_asked_for_again_the_same_day() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.clear_log();

    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::hours(23),
        ..f.options()
    });

    assert_eq!(report.listing, ListingOutcome::NotDue);
    assert_eq!(f.asked(), Vec::<String>::new());
    assert!(!report.changed());
}

/// A conditional `GET`, answered with a 304 and no body, and then nothing else to do.
#[test]
fn test_the_next_day_a_listing_that_has_not_changed_costs_one_empty_answer() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.clear_log();

    let tomorrow = now() + chrono::Duration::days(1);
    let report = f.sync(&SyncOptions {
        now: tomorrow,
        ..f.options()
    });

    assert_eq!(report.listing, ListingOutcome::Unchanged);
    assert_eq!(f.asked(), [format!("mirror {LISTING_PATH}")]);
    assert_eq!(
        f.index.listing_state().unwrap().unwrap().checked,
        tomorrow.to_rfc3339()
    );
}

#[test]
fn test_only_what_is_new_is_asked_about() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.clear_log();

    let new = "levels/doom2/megawads/new.zip";
    let new_text = "levels/doom2/megawads/new.txt";
    f.put_listing(
        "Wed, 04 Nov 2026 06:14:01 GMT",
        &[(new, 300, "Nov  3 09:00"), (new_text, 50, "Nov  3 09:00")],
    );
    f.archive
        .put_file(new_text, b"Title : Brand New\nAuthor : Me\n", LISTING_DATE);
    f.archive.put_entry(api_entry(new, 22200, 300, "Brand New"));

    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::days(1),
        ..f.options()
    });

    assert_eq!(report.listing, added(1));
    let log = f.asked();
    assert_eq!(mirror_requests(&log), [LISTING_PATH]);
    assert_eq!(api_requests(&log), [format!("get {new}")]);
    assert_eq!(f.index.by_id(22200).unwrap().unwrap().title, "Brand New");
    assert_eq!(f.index.newest(1).unwrap()[0].date, "2026-11-03");
}

#[test]
fn test_a_new_upload_the_api_has_not_heard_of_is_read_off_the_mirror() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.clear_log();

    let new = "levels/doom2/m-o/new.zip";
    let new_text = "levels/doom2/m-o/new.txt";
    f.put_listing(
        "Wed, 04 Nov 2026 06:14:01 GMT",
        &[(new, 300, "Nov  3 09:00"), (new_text, 50, "Nov  3 09:00")],
    );
    f.archive
        .put_file(new_text, b"Title : Brand New\nAuthor : Me\n", LISTING_DATE);

    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::days(1),
        ..f.options()
    });

    assert_eq!(report.api_failure, None);
    assert_eq!(report.text_read, 1);
    assert_eq!(f.index.by_path(new).unwrap().unwrap().title, "Brand New");
    assert_eq!(f.index.directory_jobs().unwrap(), []);
}

/// A new upload has no votes on the day it is first asked about, and nothing else would
/// ever ask again.
#[test]
fn test_directories_are_asked_about_again_when_their_turn_comes() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.index
        .record_directory("levels/doom2/megawads/", &[], "2026-08-01")
        .unwrap();
    f.index
        .record_directory("levels/doom2/Ports/megawads/", &[], "2026-11-01")
        .unwrap();
    f.archive.clear_log();

    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::days(1),
        ..f.options()
    });

    assert_eq!(report.listing, ListingOutcome::Unchanged);
    let log = f.asked();
    assert_eq!(api_requests(&log), ["getfiles levels/doom2/megawads/"]);
    assert_eq!(
        f.index.directories_to_refresh("2026-11-01", 5).unwrap(),
        Vec::<String>::new()
    );
}

/// That only happens when a listing has been asked for, or a launch would be a batch of
/// API calls however soon after the last one it came.
#[test]
fn test_directories_are_not_asked_about_again_between_listings() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.index
        .record_directory("levels/doom2/megawads/", &[], "2026-08-01")
        .unwrap();
    f.archive.clear_log();

    f.sync(&f.options());

    assert_eq!(f.asked(), Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// Which mirror
// ---------------------------------------------------------------------------

/// quaddicted, which has the files and not the listing.
#[test]
fn test_a_mirror_with_no_listing_is_passed_over() {
    let f = Fixture::new();
    let no_listing = TestArchive::start();
    let options = SyncOptions {
        sources: vec![no_listing.as_mirror(), f.archive.as_mirror()],
        ..f.options()
    };

    let report = f.sync(&options);

    assert_eq!(report.listing, added(2));
    assert_eq!(
        f.index.listing_state().unwrap().unwrap().source,
        f.archive.mirror_url()
    );
}

#[test]
fn test_a_mirror_behind_a_challenge_is_passed_over() {
    let f = Fixture::new();
    let blocked = TestArchive::start();
    blocked.block_mirror();
    f.archive.block_api(true);
    let options = SyncOptions {
        sources: vec![blocked.as_mirror(), f.archive.as_mirror()],
        ..f.options()
    };

    let report = f.sync(&options);

    assert_eq!(report.listing, added(2));
    assert_eq!(report.text_read, 2);
    // Asked once for the listing and once for a text file, and then left alone.
    assert_eq!(mirror_requests(&blocked.log()).len(), 2);
    assert_eq!(report.text_failure, None);
}

#[test]
fn test_a_listing_nobody_can_give_is_a_failure_that_leaves_things_as_they_were() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.block_mirror();

    let tomorrow = now() + chrono::Duration::days(1);
    let report = f.sync(&SyncOptions {
        now: tomorrow,
        ..f.options()
    });

    let ListingOutcome::Failed(why) = report.listing else {
        panic!("expected a failure, got {:?}", report.listing);
    };
    assert!(why.contains(&f.archive.mirror_url()), "{why}");
    assert_eq!(f.index.status().unwrap().uploads, 2);
    // Not counted as checked, so the next launch tries again rather than waiting a day.
    assert_eq!(
        f.index.listing_state().unwrap().unwrap().checked,
        now().to_rfc3339()
    );
}

#[test]
fn test_something_that_is_not_a_listing_is_passed_over() {
    for not_a_listing in [
        &b"<html>Not this.</html>"[..],
        &crate::idgames::test_archive::gzip(b"<html>Not this either.</html>"),
    ] {
        let f = Fixture::new();
        let bad = TestArchive::start();
        bad.put_file(LISTING_PATH, not_a_listing, LISTING_DATE);
        let options = SyncOptions {
            sources: vec![bad.as_mirror(), f.archive.as_mirror()],
            ..f.options()
        };

        assert_eq!(f.sync(&options).listing, added(2));
    }
}

#[test]
fn test_a_listing_not_to_be_believed_is_a_failure_if_there_is_no_other() {
    let f = Fixture::new();
    let whole: Vec<String> = (0..40).map(|n| format!("levels/doom/{n}.zip")).collect();
    let listed: Vec<(&str, u64, &str)> = whole
        .iter()
        .map(|path| (path.as_str(), 10, "Mar  1  2001"))
        .collect();
    f.put_listing(LISTING_DATE, &listed);
    f.sync(&f.options());
    f.put_listing("Wed, 04 Nov 2026 06:14:01 GMT", &[]);

    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::days(1),
        ..f.options()
    });

    let ListingOutcome::Failed(why) = report.listing else {
        panic!("{:?}", report.listing);
    };
    assert!(why.contains("lists only 2 uploads"), "{why}");
    assert_eq!(f.index.status().unwrap().uploads, 42);
}

/// One mirror partway through something is no reason to go without.
#[test]
fn test_a_listing_with_most_of_the_archive_missing_is_passed_over_for_a_whole_one() {
    let f = Fixture::new();
    let whole: Vec<String> = (0..40).map(|n| format!("levels/doom/{n}.zip")).collect();
    let listed: Vec<(&str, u64, &str)> = whole
        .iter()
        .map(|path| (path.as_str(), 10, "Mar  1  2001"))
        .collect();
    f.put_listing(LISTING_DATE, &listed);
    f.sync(&f.options());
    assert_eq!(f.index.status().unwrap().uploads, 42);

    let short = TestArchive::start();
    f.put_listing_on(&short, "Wed, 04 Nov 2026 06:14:01 GMT", &[]);
    let later = SyncOptions {
        now: now() + chrono::Duration::days(1),
        ..f.options()
    };
    let report = f.sync(&SyncOptions {
        sources: vec![short.as_mirror(), f.archive.as_mirror()],
        ..later
    });

    assert_eq!(f.index.status().unwrap().uploads, 42);
    assert_eq!(report.listing, ListingOutcome::Unchanged);
    assert_eq!(
        f.index.listing_state().unwrap().unwrap().source,
        f.archive.mirror_url()
    );
}

#[test]
fn test_a_mirror_that_has_stopped_keeping_up_is_passed_over_for_one_that_has_not() {
    let f = Fixture::new();
    let behind = TestArchive::start();
    f.put_listing_on(&behind, "Tue, 03 Mar 2026 06:14:01 GMT", &[]);
    let options = SyncOptions {
        sources: vec![behind.as_mirror(), f.archive.as_mirror()],
        ..f.options()
    };

    f.sync(&options);

    let state = f.index.listing_state().unwrap().unwrap();
    assert_eq!(state.source, f.archive.mirror_url());
    assert_eq!(state.generated, "2026-11-03");
}

/// Better an old listing than none.
#[test]
fn test_if_every_mirror_is_behind_the_newest_of_them_will_do() {
    let f = Fixture::new();
    let older = TestArchive::start();
    f.put_listing_on(&older, "Tue, 03 Mar 2026 06:14:01 GMT", &[]);
    let newer = TestArchive::start();
    f.put_listing_on(&newer, "Mon, 01 Jun 2026 06:14:01 GMT", &[]);
    let options = SyncOptions {
        sources: vec![older.as_mirror(), newer.as_mirror()],
        api: IdgamesClient::with_api_url(f.archive.api_url()),
        ..f.options()
    };

    let report = f.sync(&options);

    assert_eq!(report.listing, added(2));
    let state = f.index.listing_state().unwrap().unwrap();
    assert_eq!(state.source, newer.mirror_url());
    assert_eq!(state.generated, "2026-06-01");
}

#[test]
fn test_a_mirror_that_has_fallen_behind_since_is_looked_past() {
    let f = Fixture::new();
    f.sync(&f.options());
    let fresh = TestArchive::start();
    f.put_listing_on(
        &fresh,
        "Fri, 01 Jan 2027 06:14:01 GMT",
        &[("levels/doom2/m-o/new.zip", 300, "Dec 31 09:00")],
    );

    // Two months later, the same mirror still says its listing has not changed.
    let later = now() + chrono::Duration::days(60);
    let report = f.sync(&SyncOptions {
        now: later,
        sources: vec![f.archive.as_mirror(), fresh.as_mirror()],
        ..f.options()
    });

    assert_eq!(report.listing, added(1));
    assert_eq!(
        f.index.listing_state().unwrap().unwrap().source,
        fresh.mirror_url()
    );
}

/// What a conditional `GET` is compared with belongs to the mirror it came from, since
/// an `ETag` means nothing anywhere else. Sending it to another could only waste a request.
#[test]
fn test_another_mirror_is_not_asked_whether_the_last_ones_listing_has_changed() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.block_mirror();
    let other = TestArchive::start();
    f.put_listing_on(&other, LISTING_DATE, &[]);

    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::days(1),
        sources: vec![f.archive.as_mirror(), other.as_mirror()],
        ..f.options()
    });

    // A whole listing, as it would have to be: nothing new in it, but it came.
    assert!(matches!(report.listing, ListingOutcome::Updated(_)));
    assert_eq!(
        f.index.listing_state().unwrap().unwrap().source,
        other.mirror_url()
    );
}

// ---------------------------------------------------------------------------
// The text files
// ---------------------------------------------------------------------------

/// Offline for the API, as in every test here that reads a text file, and the bytes as
/// they would be from a file saved on Windows in 2002.
#[test]
fn test_a_text_file_is_read_whatever_it_was_saved_as() {
    let f = Fixture::new();
    f.archive.block_api(true);
    f.archive.put_file(
        SUNLUST_TEXT,
        b"Title : M\xfcnchen\r\nAuthor : Tobias M\xfcnch\r\n",
        LISTING_DATE,
    );

    f.sync(&f.options());

    let found = f.index.search("tobias munch", 10).unwrap();
    assert_eq!(found[0].title, "München");
    assert_eq!(found[0].author, "Tobias Münch");
}

#[test]
fn test_a_text_file_gone_missing_is_tried_again_on_the_next_run() {
    let f = Fixture::new();
    f.archive.block_api(true);
    f.archive.remove_file(SUNLUST_TEXT);

    let report = f.sync(&f.options());
    assert_eq!(report.text_read, 1);
    assert_eq!(report.text_failure, None);
    assert_eq!(f.index.status().unwrap().waiting_for_text, 1);

    f.archive
        .put_file(SUNLUST_TEXT, b"Title : Sunlust\n", LISTING_DATE);
    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::hours(1),
        ..f.options()
    });
    assert_eq!(report.text_read, 1);
    assert_eq!(f.index.status().unwrap().waiting_for_text, 0);
}

/// Otherwise a mirror that has lost its text files, or a server answering 404 for
/// everything, would be asked for every one of them at a quarter of a second each.
#[test]
fn test_too_many_missing_in_a_row_stops_the_reading() {
    let f = Fixture::new();
    f.archive.block_api(true);
    let many: Vec<(String, String)> = (0..MOST_MISSING_IN_A_ROW + 10)
        .map(|n| {
            (
                format!("levels/doom/m{n}.zip"),
                format!("levels/doom/m{n}.txt"),
            )
        })
        .collect();
    let mut listed: Vec<(&str, u64, &str)> = Vec::new();
    for (zip, text) in &many {
        listed.push((zip, 10, "Mar  1  2001"));
        listed.push((text, 10, "Mar  1  2001"));
    }
    f.put_listing(LISTING_DATE, &listed);
    f.archive.remove_file(SUNLUST_TEXT);
    f.archive.remove_file(SCYTHE_TEXT);

    let report = f.sync(&f.options());

    let failure = report.text_failure.unwrap();
    assert!(failure.contains("in a row"), "{failure}");
    let log = f.asked();
    assert_eq!(
        mirror_requests(&log).len(),
        1 + MOST_MISSING_IN_A_ROW,
        "{log:?}"
    );
}

#[test]
fn test_a_text_file_a_mirror_does_not_have_comes_from_the_next() {
    let f = Fixture::new();
    f.archive.block_api(true);
    f.archive.remove_file(SUNLUST_TEXT);
    let other = TestArchive::start();
    other.put_file(SUNLUST_TEXT, b"Title : From the other one\n", LISTING_DATE);

    let report = f.sync(&SyncOptions {
        sources: vec![f.archive.as_mirror(), other.as_mirror()],
        ..f.options()
    });

    assert_eq!(report.text_read, 2);
    let found = f.index.by_path(SUNLUST).unwrap().unwrap();
    assert_eq!(found.title, "From the other one");
}

#[test]
fn test_every_mirror_failing_stops_the_reading_and_says_why() {
    let f = Fixture::new();
    f.archive.block_api(true);

    let report = sync_index(
        &f.index,
        &f.options(),
        &mut |progress| {
            if matches!(progress, SyncProgress::ReadingText { .. }) {
                f.archive.block_mirror();
            }
        },
        &|| false,
    )
    .unwrap();

    assert_eq!(report.text_read, 0);
    assert!(report.text_failure.unwrap().contains("challenge"));
    assert_eq!(f.index.status().unwrap().waiting_for_text, 2);
}

// ---------------------------------------------------------------------------
// Stopping, and being told not to start
// ---------------------------------------------------------------------------

/// Each row is written as it comes, so closing the app is just a shorter run.
#[test]
fn test_a_run_that_is_stopped_is_picked_up_where_it_left_off() {
    let f = Fixture::new();
    f.archive.block_api(true);

    let reads = Cell::new(0);
    let report = sync_index(
        &f.index,
        &f.options(),
        &mut |progress| {
            if matches!(progress, SyncProgress::ReadingText { .. }) {
                reads.set(reads.get() + 1);
            }
        },
        &|| reads.get() >= 1,
    )
    .unwrap();

    assert!(report.cancelled);
    assert_eq!(report.text_read, 1);
    assert_eq!(f.index.status().unwrap().waiting_for_text, 1);

    f.archive.clear_log();
    let report = f.sync(&f.options());
    assert_eq!(report.text_read, 1);
    assert_eq!(f.index.status().unwrap().waiting_for_text, 0);
    // The one left, and not the one already read.
    let asked = f.asked();
    assert_eq!(mirror_requests(&asked).len(), 1);
}

#[test]
fn test_stopping_during_the_api_stage_stops_there() {
    let f = Fixture::new();
    let report = sync_index(&f.index, &f.options(), &mut |_| {}, &|| true).unwrap();
    assert!(report.cancelled);
    assert_eq!(report.ids_learned, 0);
    assert_eq!(report.text_read, 0);
}

#[test]
fn test_zero_days_means_nothing_is_asked_of_anyone() {
    let f = Fixture::new();
    let report = f.sync(&SyncOptions {
        check_days: 0,
        ..f.options()
    });
    assert_eq!(report, SyncReport::default());
    assert_eq!(f.asked(), Vec::<String>::new());
}

#[test]
fn test_asking_for_an_update_goes_ahead_whatever_the_setting() {
    let f = Fixture::new();
    f.sync(&f.options());
    f.archive.clear_log();

    let report = f.sync(&SyncOptions {
        check_days: 0,
        force: true,
        ..f.options()
    });

    assert_eq!(report.listing, ListingOutcome::Unchanged);
    assert_eq!(f.asked(), [format!("mirror {LISTING_PATH}")]);
}

#[test]
fn test_progress_says_how_far_along_each_stage_is() {
    let f = Fixture::new();
    f.archive.block_api_after(0);
    let mut seen = Vec::new();

    sync_index(&f.index, &f.options(), &mut |p| seen.push(p), &|| false).unwrap();

    assert_eq!(
        seen,
        [
            SyncProgress::Listing,
            SyncProgress::AskingApi { done: 0, total: 2 },
            SyncProgress::ReadingText { done: 0, total: 2 },
            SyncProgress::ReadingText { done: 1, total: 2 },
        ]
    );
}

// ---------------------------------------------------------------------------
// A copy of idgames
// ---------------------------------------------------------------------------

/// What `rsync` leaves: the tree as it is on a mirror, with the listing at its root.
fn copy_of_idgames(f: &Fixture, listing_too: bool) -> MirrorConfig {
    let root = f.dir.path().join("copy");
    let text = root.join(SUNLUST_TEXT);
    fs::create_dir_all(text.parent().unwrap()).unwrap();
    fs::write(&text, "Title : Sunlust (from disk)\n").unwrap();
    if listing_too {
        fs::write(
            root.join(LISTING_PATH),
            listing_of(&[
                (SUNLUST, 1000, "Aug  9  2015"),
                (SUNLUST_TEXT, 100, "Aug  9  2015"),
            ]),
        )
        .unwrap();
    }
    MirrorConfig {
        name: "disk".to_string(),
        path: Some(root.to_string_lossy().into_owned()),
        layout: Some("idgames".to_string()),
        ..Default::default()
    }
}

/// With a copy on a disk, nothing is asked of any mirror, and there is nothing to pause for.
#[test]
fn test_a_copy_of_idgames_is_read_before_any_mirror() {
    let f = Fixture::new();
    f.archive.block_api(true);
    let copy = copy_of_idgames(&f, true);

    let report = f.sync(&SyncOptions {
        sources: vec![copy, f.archive.as_mirror()],
        text_pause: Duration::from_secs(3600),
        ..f.options()
    });

    assert_eq!(report.listing, added(1));
    assert_eq!(report.text_read, 1);
    assert_eq!(mirror_requests(&f.asked()), Vec::<&str>::new());
    assert_eq!(
        f.index.by_path(SUNLUST).unwrap().unwrap().title,
        "Sunlust (from disk)"
    );
    assert_eq!(f.index.listing_state().unwrap().unwrap().source, "disk");
}

/// A copy of `levels/` only, say. Its listing comes from a mirror then, and what it does
/// have saves asking a mirror for that.
#[test]
fn test_a_copy_with_no_listing_still_saves_asking_for_what_it_has() {
    let f = Fixture::new();
    f.archive.block_api(true);
    let copy = copy_of_idgames(&f, false);

    let report = f.sync(&SyncOptions {
        sources: vec![copy, f.archive.as_mirror()],
        ..f.options()
    });

    assert_eq!(report.listing, added(2));
    assert_eq!(report.text_read, 2);
    assert_eq!(
        f.index.by_path(SUNLUST).unwrap().unwrap().title,
        "Sunlust (from disk)"
    );
    assert_eq!(mirror_requests(&f.asked()), [LISTING_PATH, SCYTHE_TEXT]);
}

#[test]
fn test_a_copys_listing_that_has_not_changed_is_noticed_without_reading_it() {
    let f = Fixture::new();
    let copy = copy_of_idgames(&f, true);
    let options = SyncOptions {
        sources: vec![copy],
        ..f.options()
    };
    f.sync(&options);

    let report = f.sync(&SyncOptions {
        now: now() + chrono::Duration::days(1),
        ..options
    });

    assert_eq!(report.listing, ListingOutcome::Unchanged);
}

/// The drive is not mounted, and that is no reason to stop keeping up.
#[test]
fn test_a_copy_that_is_not_there_is_passed_over() {
    let f = Fixture::new();
    let mut copy = copy_of_idgames(&f, true);
    copy.path = Some(f.dir.path().join("not-mounted").display().to_string());

    let report = f.sync(&SyncOptions {
        sources: vec![copy, f.archive.as_mirror()],
        ..f.options()
    });

    assert_eq!(report.listing, added(2));
}
