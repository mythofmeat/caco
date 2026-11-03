//! Getting a WAD's file through the user's browser: which Gets are waiting, the
//! worker watching the Downloads folder for them, and the questions that leads to.
//!
//! A browser passes the challenges caco's client cannot, and everything after the
//! download is bookkeeping caco can do. So Get opens the WAD's page, watches the folder
//! for a while, and asks about whatever lands there. It asks rather than assumes,
//! because that folder is the user's and they may be downloading something else.
//!
//! A listing every second, rather than a filesystem watcher, because it only runs for
//! as long as a Get is waiting and costs one directory read each time. There is
//! nothing to gain from an event: a file then has to hold still for two seconds anyway.
//! And what it avoids is real, since inotify is silent about changes made from
//! another machine or through a FUSE mount, and fails outright once a machine has
//! used up its instances, both of which would look like caco ignoring the download.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use caco_core::db;
use caco_core::downloads::{Arrival, Arrivals, list_wad_shaped, name_likeness};
use rusqlite::Connection;

use crate::message::{AppMessage, Notification};
use crate::workers::BackgroundSender;

/// How long a Get waits for its file. Long enough for a slow download started some
/// minutes in. It is here so that a Get forgotten about does not ask about an unrelated
/// zip days later, and a question too many only costs a click.
const GET_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// How the worker paces itself.
#[derive(Debug, Clone, Copy)]
struct WatchTiming {
    /// How often it looks.
    scan_every: Duration,
    /// How long a file has to hold still to have arrived.
    settle: Duration,
    /// How long a page waits for the first listing. Long enough for any disk that is
    /// answering, and short enough not to look like nothing happened if one is not.
    first_listing_wait: Duration,
}

const WATCH_TIMING: WatchTiming = WatchTiming {
    scan_every: Duration::from_secs(1),
    settle: Duration::from_secs(2),
    first_listing_wait: Duration::from_secs(2),
};

/// What hands a page to a browser. Replaceable because `tests/screenshots.rs` presses
/// Get for real, and must not open a browser on the machine running it.
pub type PageOpener = Arc<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

/// What a Get is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GetTarget {
    /// A WAD in the library with no file here.
    Wad(i64),
    /// A Cacoward entry with no WAD in the library yet, by its pk.
    Cacoward(i64),
}

/// A Get waiting for its file.
#[derive(Debug, Clone)]
pub struct PendingGet {
    pub target: GetTarget,
    pub title: String,
    /// What the file was called the last time caco had it.
    filename: Option<String>,
    /// Every page known for it, best first. The first has been opened.
    pub pages: Vec<String>,
    armed_at: Instant,
    expires_at: Instant,
}

impl PendingGet {
    /// How much a file's name suggests it is this one's. A tie-break, not a test.
    fn likeness(&self, filename: &str) -> f32 {
        let known = self
            .filename
            .as_deref()
            .map_or(0.0, |known| name_likeness(filename, known));
        known.max(name_likeness(filename, &self.title))
    }
}

/// A file that has landed, and the Gets it could be for.
#[derive(Debug)]
struct Question {
    arrival: Arrival,
    /// The likeliest first, which is the one being asked about.
    candidates: VecDeque<(GetTarget, String)>,
}

/// What to ask the user about a file that has landed.
#[derive(Debug, PartialEq, Eq)]
pub struct Asking<'a> {
    pub path: &'a Path,
    pub size: u64,
    pub title: &'a str,
}

/// A file the user said to link, which the app now has to do.
#[derive(Debug, PartialEq, Eq)]
pub struct LinkRequest {
    pub target: GetTarget,
    pub title: String,
    pub path: PathBuf,
}

/// What linking a landed file came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetOutcome {
    /// The file is now this WAD's.
    Linked { wad_id: i64 },
    /// The entry belongs to a WAD that already has its file. The award is linked to it,
    /// and the download is left where it was, for the user to do what they like with.
    AlreadyHad { title: String },
}

/// Every Get in progress.
pub struct Gets {
    pending: Vec<PendingGet>,
    /// One at a time, in the order the files landed.
    questions: VecDeque<Question>,
    /// Titles of the WADs whose file is being placed right now, for the status bar.
    linking: Vec<String>,
    /// Running while anything is waiting, and dropping it stops the worker.
    watch: Option<DownloadWatch>,
    opener: PageOpener,
    /// Where to watch. A function so that a test can say somewhere else: reading the real
    /// Downloads folder is a permission prompt on a Mac, in the middle of `cargo test`.
    folder: fn() -> PathBuf,
}

impl Default for Gets {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            questions: VecDeque::new(),
            linking: Vec::new(),
            watch: None,
            opener: Arc::new(|url| open::that_detached(url)),
            folder: caco_core::config::downloads_dir,
        }
    }
}

impl Gets {
    /// One that opens no browser and reads no real folder, for a test to arm.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self {
            opener: Arc::new(|_| Ok(())),
            folder: || PathBuf::from("/nonexistent/caco-test-downloads"),
            ..Self::default()
        }
    }

    /// Hand pages to this rather than to a browser.
    pub fn open_pages_with(&mut self, opener: PageOpener) {
        self.opener = opener;
    }

    /// Start waiting for a file, or start the wait over for one already waited for.
    pub fn arm(
        &mut self,
        sender: &BackgroundSender,
        target: GetTarget,
        title: String,
        filename: Option<String>,
        pages: Vec<String>,
        now: Instant,
    ) {
        self.pending.retain(|get| get.target != target);
        self.pending.push(PendingGet {
            target,
            title,
            filename,
            pages,
            armed_at: now,
            expires_at: now + GET_TIMEOUT,
        });
        if self.watch.is_none() {
            self.watch = Some(DownloadWatch::start(
                sender.clone(),
                Arc::clone(&self.opener),
                self.folder,
                WATCH_TIMING,
            ));
        }
    }

    /// Open a page in the browser, off the UI thread.
    pub fn open_page(&self, url: String) {
        if let Some(watch) = &self.watch {
            watch.open(url);
        }
    }

    pub fn waiting(&self) -> &[PendingGet] {
        &self.pending
    }

    pub fn is_waiting_for(&self, target: GetTarget) -> bool {
        self.pending.iter().any(|get| get.target == target)
    }

    pub fn linking(&self) -> &[String] {
        &self.linking
    }

    /// Stop waiting for a file. A question already asked about it stands.
    pub fn stop(&mut self, target: GetTarget) {
        self.pending.retain(|get| get.target != target);
        self.retire_idle_watch();
    }

    /// Give up on the Gets whose time is up, and return their titles.
    pub fn expire(&mut self, now: Instant) -> Vec<String> {
        let mut expired = Vec::new();
        self.pending.retain(|get| {
            let keep = get.expires_at > now;
            if !keep {
                expired.push(get.title.clone());
            }
            keep
        });
        self.retire_idle_watch();
        expired
    }

    /// When the next Get's time is up, for a repaint to notice.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.iter().map(|get| get.expires_at).min()
    }

    fn retire_idle_watch(&mut self) {
        if self.pending.is_empty() {
            self.watch = None;
        }
    }

    /// A file has landed: ask about it if any Get could want it, and say whether that
    /// makes a new question to bring to the user's attention.
    ///
    /// The Gets it could be for are worked out now, because one that runs out of time
    /// while the question waits its turn was still waiting when the file came.
    pub fn landed(&mut self, conn: &Connection, arrival: Arrival) -> bool {
        let filename = arrival
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut ranked: Vec<&PendingGet> = self
            .pending
            .iter()
            .filter(|get| still_wanted(conn, get.target))
            .collect();
        // The nearest name, and among equals the one asked for most recently: which is
        // the only one, most of the time, since a person gets one WAD and then another.
        ranked.sort_by(|a, b| {
            b.likeness(&filename)
                .total_cmp(&a.likeness(&filename))
                .then(b.armed_at.cmp(&a.armed_at))
        });
        if ranked.is_empty() {
            return false;
        }
        let candidates = ranked
            .into_iter()
            .map(|get| (get.target, get.title.clone()))
            .collect();
        self.questions.push_back(Question {
            arrival,
            candidates,
        });
        true
    }

    /// What to ask now, if anything.
    pub fn asking(&self) -> Option<Asking<'_>> {
        let question = self.questions.front()?;
        let (_, title) = question.candidates.front()?;
        Some(Asking {
            path: &question.arrival.path,
            size: question.arrival.size,
            title,
        })
    }

    /// The user said the file is not for the Get being asked about. It may still be
    /// for another that is waiting, and if not, it is forgotten.
    pub fn not_this_one(&mut self, conn: &Connection) {
        if let Some(question) = self.questions.front_mut() {
            question.candidates.pop_front();
        }
        self.drop_unwanted(conn);
    }

    /// The user said to link the file. A Get is finished by the link succeeding, not by
    /// this: if the file turns out to have gone, the Get is still waiting for one.
    pub fn take_link(&mut self, conn: &Connection) -> Option<LinkRequest> {
        let mut question = self.questions.pop_front()?;
        self.drop_unwanted(conn);
        let (target, title) = question.candidates.pop_front()?;
        self.linking.push(title.clone());
        Some(LinkRequest {
            target,
            title,
            path: question.arrival.path,
        })
    }

    /// A link has finished, one way or the other.
    pub fn link_finished(&mut self, target: GetTarget, title: &str, succeeded: bool) {
        if let Some(position) = self.linking.iter().position(|linking| linking == title) {
            self.linking.remove(position);
        }
        if !succeeded {
            return;
        }
        self.pending.retain(|get| get.target != target);
        for question in &mut self.questions {
            question.candidates.retain(|(other, _)| *other != target);
        }
        self.questions
            .retain(|question| !question.candidates.is_empty());
        self.retire_idle_watch();
    }

    /// Clear out what no longer needs asking, which can have changed while a question
    /// waited its turn: the WAD trashed, or given a file another way. Decided when the
    /// question comes up, as the mirror offer is.
    fn drop_unwanted(&mut self, conn: &Connection) {
        while let Some(question) = self.questions.front_mut() {
            question
                .candidates
                .retain(|(target, _)| still_wanted(conn, *target));
            if !question.candidates.is_empty() {
                return;
            }
            self.questions.pop_front();
        }
    }
}

/// Whether the thing a Get is for has still got no file to link one to.
fn still_wanted(conn: &Connection, target: GetTarget) -> bool {
    let wad_id = match target {
        GetTarget::Wad(id) => id,
        GetTarget::Cacoward(pk) => match db::get_cacoward(conn, pk) {
            Ok(Some(record)) => match record.wad_id {
                Some(id) => id,
                None => return true,
            },
            _ => return false,
        },
    };
    match db::get_wad(conn, wad_id, false) {
        Ok(Some(wad)) => wad.cached_path.as_deref().unwrap_or("").is_empty(),
        // A Cacoward entry can outlive the WAD it was linked to, and it reads as absent.
        Ok(None) => matches!(target, GetTarget::Cacoward(_)),
        Err(_) => false,
    }
}

/// The worker watching the Downloads folder. It goes when this does.
struct DownloadWatch {
    /// Never sent on. Dropping it is what stops the worker, and at once, where a flag
    /// would have to wait out its sleep.
    _stop: mpsc::Sender<()>,
    sender: BackgroundSender,
    opener: PageOpener,
    /// Whether the worker has been through the folder once, whatever it found.
    has_looked: Arc<AtomicBool>,
    timing: WatchTiming,
}

impl DownloadWatch {
    fn start(
        sender: BackgroundSender,
        opener: PageOpener,
        folder: impl Fn() -> PathBuf + Send + 'static,
        timing: WatchTiming,
    ) -> Self {
        let (stop, stopped) = mpsc::channel();
        let has_looked = Arc::new(AtomicBool::new(false));
        {
            let sender = sender.clone();
            let has_looked = Arc::clone(&has_looked);
            std::thread::spawn(move || watch(&stopped, &sender, &has_looked, &folder, timing));
        }
        Self {
            _stop: stop,
            sender,
            opener,
            has_looked,
            timing,
        }
    }

    /// Open a page once the worker has seen what is in the folder, so that a file the page
    /// brings cannot be taken for one that was already there.
    ///
    /// But no longer than that takes on a disk that is answering. A Downloads folder on a
    /// mount that has stopped responding must not cost the user the page as well, with
    /// nothing to say why: the worst that can come of opening it early is a question not
    /// asked about a file so quick that it beat the listing.
    fn open(&self, url: String) {
        let sender = self.sender.clone();
        let opener = Arc::clone(&self.opener);
        let has_looked = Arc::clone(&self.has_looked);
        let give_up_at = Instant::now() + self.timing.first_listing_wait;
        std::thread::spawn(move || {
            while !has_looked.load(Ordering::Acquire) && Instant::now() < give_up_at {
                std::thread::sleep(Duration::from_millis(10));
            }
            if let Err(e) = opener(&url) {
                sender.send(AppMessage::Notify(Notification::error(format!(
                    "Could not open a browser: {e}. The page is {url}"
                ))));
            }
        });
    }
}

/// What the worker does until it is told to stop.
///
/// Asks where the folder is every time round, so a change in Settings applies at once,
/// and starts afresh on a new one.
fn watch(
    stopped: &mpsc::Receiver<()>,
    sender: &BackgroundSender,
    has_looked: &AtomicBool,
    folder: &dyn Fn() -> PathBuf,
    timing: WatchTiming,
) {
    let mut watching: Option<(PathBuf, Arrivals)> = None;
    let mut complained_about: Option<PathBuf> = None;
    loop {
        let dir = folder();
        match list_wad_shaped(&dir) {
            Ok(listing) => {
                complained_about = None;
                match &mut watching {
                    Some((watched, arrivals)) if *watched == dir => {
                        for arrival in arrivals.observe(&listing, Instant::now()) {
                            sender.send(AppMessage::DownloadLanded(arrival));
                        }
                    }
                    _ => watching = Some((dir, Arrivals::new(listing, timing.settle))),
                }
            }
            // A folder that cannot be read is worth saying once, and what was known
            // about it still holds. It may just be a drive that is not mounted yet.
            Err(e) => {
                if complained_about.as_ref() != Some(&dir) {
                    sender.send(AppMessage::Notify(Notification::warning(format!(
                        "Cannot read {} to watch for your download: {e}. \
                         Set the folder in Settings.",
                        dir.display()
                    ))));
                    complained_about = Some(dir);
                }
            }
        }
        has_looked.store(true, Ordering::Release);

        if let Err(mpsc::RecvTimeoutError::Disconnected) = stopped.recv_timeout(timing.scan_every) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Mutex;

    use caco_core::db::{NewCacoward, NewWad, SourceType, WadUpdate};

    use super::*;
    use crate::workers::BackgroundChannel;

    fn setup() -> Connection {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        conn
    }

    fn add_wad(conn: &Connection, title: &str) -> GetTarget {
        let id = db::add_wad(conn, &NewWad::new(title, SourceType::Doomworld)).unwrap();
        GetTarget::Wad(id)
    }

    /// Arms Gets with no worker behind them, which only some tests want.
    fn gets_with(pending: &[(GetTarget, &str, Option<&str>)], start: Instant) -> Gets {
        let mut gets = Gets::for_tests();
        for (index, (target, title, filename)) in pending.iter().enumerate() {
            let armed_at = start + Duration::from_secs(index as u64);
            gets.pending.push(PendingGet {
                target: *target,
                title: title.to_string(),
                filename: filename.map(str::to_string),
                pages: vec!["https://example.com/".to_string()],
                armed_at,
                expires_at: armed_at + GET_TIMEOUT,
            });
        }
        gets
    }

    fn arrival(name: &str) -> Arrival {
        Arrival {
            path: PathBuf::from("/downloads").join(name),
            size: 1024,
        }
    }

    /// What is being asked, as `(file name, title)`.
    fn asked(gets: &Gets) -> Option<(String, String)> {
        gets.asking().map(|asking| {
            let name = asking.path.file_name().unwrap().to_string_lossy();
            (name.into_owned(), asking.title.to_string())
        })
    }

    fn question(file: &str, title: &str) -> Option<(String, String)> {
        Some((file.to_string(), title.to_string()))
    }

    /// The issue's whole point about matching: the user just asked for exactly one WAD,
    /// so a name that looks nothing like it is still worth asking about.
    #[test]
    fn test_with_one_get_waiting_any_file_is_asked_about() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let mut gets = gets_with(&[(road, "Lonesome Road", None)], Instant::now());

        assert!(gets.landed(&conn, arrival("download.zip")));

        assert_eq!(asked(&gets), question("download.zip", "Lonesome Road"));
    }

    #[test]
    fn test_with_nothing_waiting_nothing_is_asked() {
        let conn = setup();
        let mut gets = Gets::for_tests();

        assert!(!gets.landed(&conn, arrival("road.zip")));

        assert_eq!(gets.asking(), None);
    }

    #[test]
    fn test_with_several_waiting_the_nearest_name_is_asked_about_first() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let sunder = add_wad(&conn, "Sunder");
        let pending = [(road, "Lonesome Road", None), (sunder, "Sunder", None)];

        let mut gets = gets_with(&pending, Instant::now());
        gets.landed(&conn, arrival("LonesomeRoad_v1.2.pk3"));
        assert_eq!(
            asked(&gets),
            question("LonesomeRoad_v1.2.pk3", "Lonesome Road")
        );

        let mut gets = gets_with(&pending, Instant::now());
        gets.landed(&conn, arrival("sunder.zip"));
        assert_eq!(asked(&gets), question("sunder.zip", "Sunder"));
    }

    /// A WAD that has had a file before is known by what it was called, which can look
    /// nothing like its title.
    #[test]
    fn test_a_files_old_name_counts_for_as_much_as_its_title() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let sunder = add_wad(&conn, "Sunder");
        let mut gets = gets_with(
            &[
                (sunder, "Sunder", None),
                (road, "Lonesome Road", Some("lr_rc3.pk3")),
            ],
            Instant::now(),
        );

        gets.landed(&conn, arrival("LR_RC3.PK3"));

        assert_eq!(asked(&gets), question("LR_RC3.PK3", "Lonesome Road"));
    }

    #[test]
    fn test_when_the_name_says_nothing_the_latest_get_is_asked_about_first() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let sunder = add_wad(&conn, "Sunder");
        let mut gets = gets_with(
            &[(road, "Lonesome Road", None), (sunder, "Sunder", None)],
            Instant::now(),
        );

        gets.landed(&conn, arrival("attachment.7z"));

        assert_eq!(asked(&gets), question("attachment.7z", "Sunder"));
    }

    #[test]
    fn test_not_this_one_moves_on_to_the_next_candidate_then_lets_it_go() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let sunder = add_wad(&conn, "Sunder");
        let mut gets = gets_with(
            &[(road, "Lonesome Road", None), (sunder, "Sunder", None)],
            Instant::now(),
        );
        gets.landed(&conn, arrival("attachment.7z"));

        gets.not_this_one(&conn);
        assert_eq!(asked(&gets), question("attachment.7z", "Lonesome Road"));

        gets.not_this_one(&conn);
        assert_eq!(gets.asking(), None);
        // A file it was not is not what either was waiting for.
        assert!(gets.is_waiting_for(road));
        assert!(gets.is_waiting_for(sunder));
    }

    #[test]
    fn test_files_landing_together_are_asked_about_in_turn() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let mut gets = gets_with(&[(road, "Lonesome Road", None)], Instant::now());
        gets.landed(&conn, arrival("one.zip"));
        gets.landed(&conn, arrival("two.zip"));

        assert_eq!(asked(&gets), question("one.zip", "Lonesome Road"));
        gets.not_this_one(&conn);
        assert_eq!(asked(&gets), question("two.zip", "Lonesome Road"));
    }

    #[test]
    fn test_linking_finishes_the_get_only_once_it_has_worked() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let mut gets = gets_with(&[(road, "Lonesome Road", None)], Instant::now());
        gets.landed(&conn, arrival("road.zip"));

        let request = gets.take_link(&conn).unwrap();
        assert_eq!(
            request,
            LinkRequest {
                target: road,
                title: "Lonesome Road".to_string(),
                path: PathBuf::from("/downloads/road.zip"),
            }
        );
        assert_eq!(gets.asking(), None);
        assert_eq!(gets.linking(), ["Lonesome Road"]);
        assert!(gets.is_waiting_for(road));

        // The file had gone, say. The Get is still after one.
        gets.link_finished(road, "Lonesome Road", false);
        assert_eq!(gets.linking(), Vec::<String>::new());
        assert!(gets.is_waiting_for(road));

        gets.landed(&conn, arrival("road.zip"));
        gets.take_link(&conn);
        gets.link_finished(road, "Lonesome Road", true);
        assert!(!gets.is_waiting_for(road));
        assert_eq!(gets.linking(), Vec::<String>::new());
    }

    /// Two files came for one WAD, and the first was it. The second has nothing left to
    /// be for, and the same question should not be asked of a WAD that has its file.
    #[test]
    fn test_a_get_that_succeeds_is_dropped_from_questions_waiting_their_turn() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let sunder = add_wad(&conn, "Sunder");
        let mut gets = gets_with(
            &[(road, "Lonesome Road", None), (sunder, "Sunder", None)],
            Instant::now(),
        );
        gets.landed(&conn, arrival("lonesomeroad.zip"));
        gets.landed(&conn, arrival("lonesomeroad2.zip"));

        let link = gets.take_link(&conn).unwrap();
        assert_eq!(link.target, road);
        gets.link_finished(link.target, &link.title, true);

        assert_eq!(asked(&gets), question("lonesomeroad2.zip", "Sunder"));
    }

    #[test]
    fn test_a_wad_given_a_file_some_other_way_is_not_asked_about() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let mut gets = gets_with(&[(road, "Lonesome Road", None)], Instant::now());
        let GetTarget::Wad(id) = road else {
            unreachable!()
        };
        let linked = WadUpdate::new().set_text("cached_path", Some("/keep/road.zip".to_string()));
        db::update_wad(&conn, id, &linked).unwrap();

        assert!(!gets.landed(&conn, arrival("road.zip")));

        assert_eq!(gets.asking(), None);
    }

    #[test]
    fn test_a_wad_trashed_while_a_question_waits_is_not_asked_about() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let mut gets = gets_with(&[(road, "Lonesome Road", None)], Instant::now());
        gets.landed(&conn, arrival("one.zip"));
        gets.landed(&conn, arrival("two.zip"));
        let GetTarget::Wad(id) = road else {
            unreachable!()
        };
        db::delete_wad(&conn, id, false).unwrap();

        gets.not_this_one(&conn);

        assert_eq!(gets.asking(), None);
    }

    fn add_entry(conn: &Connection) -> i64 {
        db::upsert_cacoward(
            conn,
            &NewCacoward {
                key: "k".to_string(),
                year: 2025,
                category: "winner".to_string(),
                page_order: 1,
                wad_title: "Lonesome Road".to_string(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn test_an_absent_entry_is_wanted_until_its_wad_has_a_file() {
        let conn = setup();
        let entry = add_entry(&conn);
        let target = GetTarget::Cacoward(entry);
        assert!(still_wanted(&conn, target));

        // An import can give it a row with no file, and a file is still what it needs.
        let wad = db::add_wad(&conn, &NewWad::new("Road", SourceType::Doomworld)).unwrap();
        db::link_wad(&conn, entry, wad, false).unwrap();
        assert!(still_wanted(&conn, target));

        // A link surviving the WAD being trashed reads as absent, as the view has it.
        db::delete_wad(&conn, wad, false).unwrap();
        assert!(still_wanted(&conn, target));
        db::restore_wad(&conn, wad).unwrap();

        let linked = WadUpdate::new().set_text("cached_path", Some("/keep/road.zip".to_string()));
        db::update_wad(&conn, wad, &linked).unwrap();
        assert!(!still_wanted(&conn, target));
    }

    #[test]
    fn test_a_get_gives_up_when_its_time_is_up() {
        let start = Instant::now();
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let sunder = add_wad(&conn, "Sunder");
        let mut gets = gets_with(
            &[(road, "Lonesome Road", None), (sunder, "Sunder", None)],
            start,
        );
        assert_eq!(gets.next_deadline(), Some(start + GET_TIMEOUT));

        assert_eq!(gets.expire(start), Vec::<String>::new());
        assert_eq!(
            gets.expire(start + GET_TIMEOUT + Duration::from_millis(500)),
            ["Lonesome Road"]
        );
        assert!(!gets.is_waiting_for(road));
        assert!(gets.is_waiting_for(sunder));
    }

    /// The file came before its time was up, so the answer is still wanted.
    #[test]
    fn test_a_question_outlives_the_get_it_is_about() {
        let start = Instant::now();
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let mut gets = gets_with(&[(road, "Lonesome Road", None)], start);
        gets.landed(&conn, arrival("road.zip"));

        gets.expire(start + GET_TIMEOUT * 2);

        assert_eq!(asked(&gets), question("road.zip", "Lonesome Road"));
    }

    // -- The worker --

    /// What the worker told the app, with a time limit, since a failure here is a
    /// message that never comes.
    struct Worker {
        bg: BackgroundChannel,
        opened: Arc<Mutex<Vec<String>>>,
        dir: tempfile::TempDir,
        watch: Option<DownloadWatch>,
    }

    const TIMING: WatchTiming = WatchTiming {
        scan_every: Duration::from_millis(5),
        settle: Duration::from_millis(20),
        first_listing_wait: Duration::from_secs(5),
    };

    impl Worker {
        fn start() -> Self {
            Self::with_folder(tempfile::tempdir().unwrap())
        }

        fn with_folder(dir: tempfile::TempDir) -> Self {
            let bg = BackgroundChannel::new();
            let opened: Arc<Mutex<Vec<String>>> = Arc::default();
            let recording = Arc::clone(&opened);
            let opener: PageOpener = Arc::new(move |url| {
                recording.lock().unwrap().push(url.to_string());
                Ok(())
            });
            let folder = dir.path().to_path_buf();
            let watch = DownloadWatch::start(bg.sender(), opener, move || folder.clone(), TIMING);
            Self {
                bg,
                opened,
                dir,
                watch: Some(watch),
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        /// Make sure the worker has taken its first listing, by what it does right after.
        fn wait_until_watching(&self) {
            self.watch.as_ref().unwrap().open("about:blank".to_string());
            for _ in 0..500 {
                if !self.opened.lock().unwrap().is_empty() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("the worker never got as far as opening a page");
        }

        fn next_arrival(&self) -> Arrival {
            match self.bg.rx.recv_timeout(Duration::from_secs(5)) {
                Ok(AppMessage::DownloadLanded(arrival)) => arrival,
                Ok(_) => panic!("expected a download to land, got something else"),
                Err(e) => panic!("no download landed: {e}"),
            }
        }

        fn stays_quiet(&self) {
            let wait = TIMING.settle * 4;
            if let Ok(AppMessage::DownloadLanded(arrival)) = self.bg.rx.recv_timeout(wait) {
                panic!("{} should not have landed", arrival.path.display());
            }
        }
    }

    /// The whole of what a browser does, on a real folder: a temporary name, renamed
    /// when the download is done.
    #[test]
    fn test_a_finished_download_lands_under_its_final_name() {
        let worker = Worker::start();
        worker.wait_until_watching();

        let part = worker.path("road.zip.crdownload");
        fs::write(&part, b"half").unwrap();
        worker.stays_quiet();

        fs::write(&part, b"all of it").unwrap();
        let done = worker.path("road.zip");
        fs::rename(&part, &done).unwrap();

        let arrival = worker.next_arrival();
        assert_eq!(arrival.path, done);
        assert_eq!(arrival.size, 9);
        worker.stays_quiet();
    }

    #[test]
    fn test_what_was_in_the_folder_before_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("old.zip"), b"old").unwrap();
        let worker = Worker::with_folder(dir);
        worker.wait_until_watching();

        worker.stays_quiet();
        fs::write(worker.path("new.pk3"), b"new").unwrap();

        assert_eq!(worker.next_arrival().path, worker.path("new.pk3"));
    }

    #[test]
    fn test_only_what_a_wad_could_be_lands() {
        let worker = Worker::start();
        worker.wait_until_watching();

        fs::write(worker.path("invoice.pdf"), b"pdf").unwrap();
        fs::write(worker.path("road.wad.part"), b"part").unwrap();
        worker.stays_quiet();
    }

    /// A folder that stops answering until `release` is dropped, as a dead mount does,
    /// and a list of the pages that have opened.
    fn watch_a_stuck_folder(
        timing: WatchTiming,
    ) -> (DownloadWatch, Arc<Mutex<Vec<String>>>, mpsc::Sender<()>) {
        let (release, stuck) = mpsc::channel::<()>();
        let stuck = Mutex::new(stuck);
        let opened: Arc<Mutex<Vec<String>>> = Arc::default();
        let recording = Arc::clone(&opened);
        let opener: PageOpener = Arc::new(move |url| {
            recording.lock().unwrap().push(url.to_string());
            Ok(())
        });
        let bg = BackgroundChannel::new();
        let watch = DownloadWatch::start(
            bg.sender(),
            opener,
            move || {
                let _ = stuck.lock().unwrap().recv();
                PathBuf::from("/nonexistent/caco-test-downloads")
            },
            timing,
        );
        (watch, opened, release)
    }

    fn wait_for_a_page(opened: &Arc<Mutex<Vec<String>>>) {
        for _ in 0..1000 {
            if !opened.lock().unwrap().is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the page never opened");
    }

    /// If the page opened first, what it brought could land before the first listing,
    /// and be taken for something that was already there.
    #[test]
    fn test_a_page_waits_for_the_first_listing() {
        let (watch, opened, release) = watch_a_stuck_folder(TIMING);

        watch.open("https://example.com/".to_string());
        std::thread::sleep(TIMING.settle * 4);
        assert_eq!(*opened.lock().unwrap(), Vec::<String>::new());

        drop(release);
        wait_for_a_page(&opened);
    }

    /// The Downloads folder being on a mount that has gone must not cost the user the page
    /// as well, with nothing to say why.
    #[test]
    fn test_a_page_does_not_wait_for_a_folder_that_is_not_answering() {
        let timing = WatchTiming {
            first_listing_wait: Duration::from_millis(20),
            ..TIMING
        };
        let (watch, opened, _release) = watch_a_stuck_folder(timing);

        watch.open("https://example.com/".to_string());

        wait_for_a_page(&opened);
    }

    #[test]
    fn test_a_browser_that_will_not_open_says_where_the_page_is() {
        let dir = tempfile::tempdir().unwrap();
        let bg = BackgroundChannel::new();
        let opener: PageOpener = Arc::new(|_| Err(io::Error::other("no browser")));
        let folder = dir.path().to_path_buf();
        let watch = DownloadWatch::start(bg.sender(), opener, move || folder.clone(), TIMING);

        watch.open("https://example.com/road".to_string());

        match bg.rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            AppMessage::Notify(notification) => {
                assert!(notification.text.contains("no browser"));
                assert!(notification.text.contains("https://example.com/road"));
            }
            _ => panic!("expected to be told"),
        }
    }

    /// Said once, or the status bar would repeat it every second for an hour.
    #[test]
    fn test_a_folder_that_cannot_be_read_is_mentioned_once() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("not-mounted");
        let bg = BackgroundChannel::new();
        let opener: PageOpener = Arc::new(|_| Ok(()));
        let folder = missing.clone();
        let _watch = DownloadWatch::start(bg.sender(), opener, move || folder.clone(), TIMING);

        match bg.rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            AppMessage::Notify(notification) => {
                assert!(
                    notification.text.contains("not-mounted"),
                    "{}",
                    notification.text
                );
                assert!(
                    notification.text.contains("Settings"),
                    "{}",
                    notification.text
                );
            }
            _ => panic!("expected to be told"),
        }
        assert!(bg.rx.recv_timeout(TIMING.settle * 4).is_err());

        // A drive mounted later is watched from then on, and what is on it has not
        // arrived, because caco has never seen it to know.
        let mounted = root.path().join("being-mounted");
        fs::create_dir(&mounted).unwrap();
        fs::write(mounted.join("was-already-there.zip"), b"old").unwrap();
        fs::rename(&mounted, &missing).unwrap();
        assert!(bg.rx.recv_timeout(TIMING.settle * 4).is_err());
        fs::write(missing.join("new.zip"), b"new").unwrap();
        match bg.rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            AppMessage::DownloadLanded(arrival) => {
                assert_eq!(arrival.path, missing.join("new.zip"));
            }
            _ => panic!("expected a download to land"),
        }
    }

    /// Changing the folder in Settings applies straight away, and what is already in
    /// the new one has not arrived either.
    #[test]
    fn test_the_folder_can_change_while_it_is_watched() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(second.join("old.zip"), b"old").unwrap();
        let current = Arc::new(Mutex::new(first.clone()));

        let bg = BackgroundChannel::new();
        let opener: PageOpener = Arc::new(|_| Ok(()));
        let folder = Arc::clone(&current);
        let _watch = DownloadWatch::start(
            bg.sender(),
            opener,
            move || folder.lock().unwrap().clone(),
            TIMING,
        );
        std::thread::sleep(TIMING.scan_every * 4);

        *current.lock().unwrap() = second.clone();
        assert!(bg.rx.recv_timeout(TIMING.settle * 4).is_err());

        fs::write(first.join("in-the-old-folder.zip"), b"x").unwrap();
        fs::write(second.join("new.zip"), b"new").unwrap();
        match bg.rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            AppMessage::DownloadLanded(arrival) => {
                assert_eq!(arrival.path, second.join("new.zip"));
            }
            _ => panic!("expected a download to land"),
        }
    }

    /// Nothing is left running once nothing is waiting.
    #[test]
    fn test_the_worker_stops_when_nothing_is_waiting() {
        let mut worker = Worker::start();
        worker.wait_until_watching();

        worker.watch = None;
        std::thread::sleep(TIMING.scan_every * 4);

        fs::write(worker.path("road.zip"), b"road").unwrap();
        worker.stays_quiet();
    }

    #[test]
    fn test_a_second_get_shares_the_worker_and_stopping_the_last_one_ends_it() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let sunder = add_wad(&conn, "Sunder");
        let bg = BackgroundChannel::new();
        let mut gets = Gets::for_tests();
        let sender = bg.sender();

        let now = Instant::now();
        gets.arm(&sender, road, "Lonesome Road".into(), None, vec![], now);
        assert!(gets.watch.is_some());
        gets.arm(&sender, sunder, "Sunder".into(), None, vec![], now);
        assert_eq!(gets.waiting().len(), 2);

        gets.stop(road);
        assert!(gets.watch.is_some());
        gets.stop(sunder);
        assert!(gets.watch.is_none());
    }

    #[test]
    fn test_asking_again_starts_the_wait_over_rather_than_adding_a_second() {
        let conn = setup();
        let road = add_wad(&conn, "Lonesome Road");
        let bg = BackgroundChannel::new();
        let mut gets = Gets::for_tests();
        let sender = bg.sender();

        let start = Instant::now();
        gets.arm(&sender, road, "Road".into(), None, vec![], start);
        let later = start + Duration::from_secs(600);
        gets.arm(&sender, road, "Road".into(), None, vec![], later);

        assert_eq!(gets.waiting().len(), 1);
        assert_eq!(gets.next_deadline(), Some(later + GET_TIMEOUT));
    }
}
