//! Keeping this machine's log written and the other machines' logs read, off the UI thread,
//! and what that tells the user, which is only what they would want to hear.
//!
//! The folder can be on a network mount that has stopped answering, and this is the one
//! thread that would notice: nothing here holds a lock while it waits on a file. See
//! `caco_core::sync`.
//!
//! A pass every so often, because a sync tool delivers files whenever it likes, and writes
//! are worth getting out soon, since going back to another machine's `library.db` loses
//! whatever has not been. A folder that has not changed costs a `stat` per log file.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use caco_core::presence::StorePaths;
use caco_core::sync::{self, SyncPaths, machine};

use crate::message::{AppMessage, Notification, SyncRun};
use crate::workers::BackgroundSender;

/// How often to look when nothing has asked for a pass.
const POLL: Duration = Duration::from_secs(30);

/// How long closing the app waits for the last writes to go out. Not forever, since the
/// folder may be somewhere that will never answer, and what does not get written now is
/// still in the outbox for next time.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

enum Request {
    /// Do a pass now, because someone asked.
    Asked,
    /// Do a pass now, because something has just happened that is worth writing down.
    Nudged,
    /// Do a pass and say when it has finished, because the app is closing.
    Closing(mpsc::Sender<()>),
}

/// The thread doing the passes. Dropping this ends it, at the end of whatever pass it is in.
pub(super) struct SyncWorker {
    requests: mpsc::Sender<Request>,
}

impl SyncWorker {
    pub(super) fn asked(&self) {
        let _ = self.requests.send(Request::Asked);
    }

    pub(super) fn nudge(&self) {
        let _ = self.requests.send(Request::Nudged);
    }

    /// Get the last writes out, waiting only so long for them.
    pub(super) fn finish(&self) {
        let (done, wait) = mpsc::channel();
        if self.requests.send(Request::Closing(done)).is_ok() {
            let _ = wait.recv_timeout(CLOSE_TIMEOUT);
        }
    }
}

/// Requests made of a worker that has not started yet.
pub(super) struct Inbox(mpsc::Receiver<Request>);

/// A worker that can be asked for things, and the other end of that to start it with.
pub(super) fn worker() -> (SyncWorker, Inbox) {
    let (requests, incoming) = mpsc::channel();
    (SyncWorker { requests }, Inbox(incoming))
}

/// Start the passes. After the Cacowards dataset has been written, on a new library, or what
/// is said about its rows would have nowhere to go until the second one.
pub(super) fn spawn(inbox: Inbox, sender: BackgroundSender, db_path: PathBuf) {
    std::thread::spawn(move || {
        run_worker(inbox, sender, &db_path, POLL, &|db_path| {
            Ok((SyncPaths::from_config(db_path)?, StorePaths::from_config()))
        });
    });
}

/// Where a pass reads and writes. Asked each time, so that a change in Settings applies at
/// once, and passed in for the reason `SyncPaths` is explicit: a test of the worker must not
/// be able to reach the real folders.
type FindPaths<'a> = &'a dyn Fn(&Path) -> caco_core::Result<(SyncPaths, StorePaths)>;

fn run_worker(
    inbox: Inbox,
    sender: BackgroundSender,
    db_path: &Path,
    poll: Duration,
    find_paths: FindPaths<'_>,
) {
    let Inbox(incoming) = inbox;
    let conn = match caco_core::db::open_connection(db_path) {
        Ok(conn) => conn,
        Err(e) => {
            sender.send(AppMessage::Notify(Notification::error(format!(
                "Could not open the library to sync it: {e}"
            ))));
            return;
        }
    };

    {
        let mut said = None;
        let mut request = None;
        // A library that has just been rebuilt, or whose keep dir has just arrived.
        let mut look_for_files = true;
        loop {
            let asked_for = matches!(request, Some(Request::Asked));
            let run = pass(&conn, db_path, asked_for, look_for_files, find_paths);
            look_for_files = false;

            // Only what has changed since it was last said, or someone with an unreachable
            // folder would have the same failure put in front of them every half a minute.
            let quiet = SyncRun {
                asked_for: false,
                ..run.clone()
            };
            let news = run.report.changed()
                || run.adopted > 0
                || run.report.written > 0
                || asked_for
                || said.as_ref() != Some(&quiet);
            if news {
                sender.send(AppMessage::SyncFinished(run));
            }
            said = Some(quiet);

            if let Some(Request::Closing(done)) = request.take() {
                let _ = done.send(());
                return;
            }
            match incoming.recv_timeout(poll) {
                Ok(next) => request = Some(next),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// One pass.
fn pass(
    conn: &rusqlite::Connection,
    db_path: &Path,
    asked_for: bool,
    look_for_files: bool,
    find_paths: FindPaths<'_>,
) -> SyncRun {
    let (paths, stores) = match find_paths(db_path) {
        Ok(paths) => paths,
        Err(e) => {
            return SyncRun {
                report: sync::SyncReport {
                    read_error: Some(format!("Could not tell which machine this is: {e}")),
                    ..Default::default()
                },
                adopted: 0,
                this_machine: String::new(),
                asked_for,
            };
        }
    };

    let report = sync::run(conn, &paths);
    // Worth nothing if it fails, since all that means is a file not found until next time.
    let adopted = if look_for_files || report.changed() {
        caco_core::identity::adopt_files(conn, &stores).unwrap_or(0)
    } else {
        0
    };
    SyncRun {
        report,
        adopted,
        this_machine: paths.machine,
        asked_for,
    }
}

/// What a pass has to tell the user, if anything.
///
/// Another machine's history arriving is news, once. This machine writing down its own is
/// not. Being unable to write is, because the log is what going back to another machine's
/// database relies on. The worker only sends that when it changes.
pub(super) fn notification(run: &SyncRun) -> Option<Notification> {
    if let Some(why) = &run.report.write_error {
        return Some(Notification::warning(format!(
            "Could not write to the sync folder: {why}"
        )));
    }
    if let Some(why) = &run.report.read_error {
        return Some(Notification::warning(format!(
            "Could not read the sync folder: {why}"
        )));
    }

    let others: Vec<(&str, usize)> = run
        .report
        .merge
        .applied
        .iter()
        .filter(|(machine, _)| **machine != run.this_machine)
        .map(|(machine, events)| (machine::display_name(machine), *events))
        .collect();
    let changes = |n: usize| match n {
        1 => "1 change".to_string(),
        n => format!("{n} changes"),
    };
    match others.as_slice() {
        [] if run.asked_for => Some(Notification::info(
            "Nothing new from other machines.".to_string(),
        )),
        [] => None,
        [(machine, events)] => Some(Notification::info(format!(
            "Merged {} from {machine}.",
            changes(*events)
        ))),
        several => {
            let total = several.iter().map(|(_, events)| events).sum();
            Some(Notification::info(format!(
                "Merged {} from {} machines.",
                changes(total),
                several.len()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Severity;
    use crate::workers::BackgroundChannel;
    use caco_core::db;
    use caco_core::sync::{MergeReport, SyncReport};

    /// A library on disk, for the worker to open, and folders of its own to work in.
    struct Fixture {
        dir: tempfile::TempDir,
        db_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("library.db");
            let conn = db::open_connection(&db_path).unwrap();
            db::init_db(&conn).unwrap();
            Self { dir, db_path }
        }

        fn add_wad(&self, title: &str) {
            let conn = db::open_connection(&self.db_path).unwrap();
            db::add_wad(&conn, &db::NewWad::new(title, db::SourceType::Local)).unwrap();
        }

        /// The worker, on a thread, with a poll too long to do anything by itself.
        fn start(&self) -> (SyncWorker, BackgroundChannel, std::thread::JoinHandle<()>) {
            let channel = BackgroundChannel::new();
            let sender = channel.sender();
            let (worker, inbox) = worker();
            let db_path = self.db_path.clone();
            let root = self.dir.path().to_path_buf();
            let thread = std::thread::spawn(move || {
                run_worker(inbox, sender, &db_path, Duration::from_secs(3600), &|_| {
                    Ok((
                        SyncPaths {
                            log_dir: root.join("sync"),
                            machine: "here-00000000".to_string(),
                            wad_data_dir: root.join("data"),
                            companion_dir: root.join("companions"),
                        },
                        StorePaths {
                            cache_dir: root.join("cache"),
                            keep_dir: root.join("keep"),
                        },
                    ))
                });
            });
            (worker, channel, thread)
        }

        fn log_has(&self, text: &str) -> bool {
            let folder = self.dir.path().join("sync").join("here-00000000");
            std::fs::read_dir(folder).is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    std::fs::read_to_string(entry.path())
                        .unwrap()
                        .contains(text)
                })
            })
        }
    }

    /// Wait for a message from the worker, however long a loaded machine takes.
    fn next_run(channel: &BackgroundChannel) -> SyncRun {
        for _ in 0..500 {
            for message in channel.drain() {
                if let AppMessage::SyncFinished(run) = message {
                    return run;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the worker said nothing");
    }

    /// What closing the app relies on: nothing it has done since the last pass is left
    /// unwritten once this returns, and the thread is done.
    #[test]
    fn test_finishing_gets_the_last_writes_out() {
        let f = Fixture::new();
        let (worker, channel, thread) = f.start();
        next_run(&channel);

        f.add_wad("Added Just Before Closing");
        worker.finish();

        assert!(f.log_has("Added Just Before Closing"));
        thread.join().unwrap();
    }

    #[test]
    fn test_a_nudge_gets_a_session_out_without_waiting_for_the_poll() {
        let f = Fixture::new();
        let (worker, channel, _thread) = f.start();
        next_run(&channel);

        f.add_wad("Just Played");
        worker.nudge();
        let run = next_run(&channel);

        assert_eq!(run.report.written, 1);
        assert!(!run.asked_for);
        assert!(f.log_has("Just Played"));
    }

    /// Pressing the button gets an answer, even if the answer is nothing.
    #[test]
    fn test_being_asked_always_gets_an_answer() {
        let f = Fixture::new();
        let (worker, channel, _thread) = f.start();
        next_run(&channel);

        worker.asked();
        let run = next_run(&channel);
        assert!(run.asked_for);
        assert!(!run.report.changed());
    }

    /// Dropping the worker is how the app ending looks to it, if `finish` never ran.
    #[test]
    fn test_the_thread_ends_when_nothing_can_ask_it_anything() {
        let f = Fixture::new();
        let (worker, _channel, thread) = f.start();
        drop(worker);
        thread.join().unwrap();
    }

    /// The second pass finds what a machine's first log left, and says nothing about it.
    #[test]
    fn test_a_pass_that_finds_nothing_says_nothing() {
        let f = Fixture::new();
        let (worker, channel, thread) = f.start();
        next_run(&channel);

        worker.nudge();
        worker.nudge();
        worker.finish();
        thread.join().unwrap();

        // At most the one a change from the first pass's report makes, and no news in it.
        let later: Vec<SyncRun> = channel
            .drain()
            .into_iter()
            .filter_map(|message| match message {
                AppMessage::SyncFinished(run) => Some(run),
                _ => None,
            })
            .collect();
        assert!(later.len() <= 1, "{later:?}");
        assert!(later.iter().all(|run| !run.report.changed()));
        assert!(later.iter().all(|run| notification(run).is_none()));
    }

    fn run(applied: &[(&str, usize)], asked_for: bool) -> SyncRun {
        SyncRun {
            report: SyncReport {
                merge: MergeReport {
                    applied: applied
                        .iter()
                        .map(|(machine, events)| (machine.to_string(), *events))
                        .collect(),
                    ..Default::default()
                },
                ..Default::default()
            },
            adopted: 0,
            this_machine: "here-00000000".to_string(),
            asked_for,
        }
    }

    fn said(run: &SyncRun) -> Option<(Severity, String)> {
        notification(run).map(|n| (n.severity, n.text))
    }

    #[test]
    fn test_another_machines_history_arriving_is_news() {
        assert_eq!(
            said(&run(&[("laptop-3fa9c1d2", 1)], false)),
            Some((Severity::Info, "Merged 1 change from laptop.".into()))
        );
        assert_eq!(
            said(&run(&[("laptop-3fa9c1d2", 12)], false)),
            Some((Severity::Info, "Merged 12 changes from laptop.".into()))
        );
        assert_eq!(
            said(&run(&[("laptop-3fa9c1d2", 2), ("deck-00000000", 3)], false)),
            Some((Severity::Info, "Merged 5 changes from 2 machines.".into()))
        );
    }

    /// A library rebuilt from its own log is no one else's doing, and a quiet pass is not
    /// worth a word unless someone pressed the button.
    #[test]
    fn test_nothing_is_said_about_nothing_unless_asked() {
        assert_eq!(said(&run(&[], false)), None);
        assert_eq!(said(&run(&[("here-00000000", 40)], false)), None);
        assert_eq!(
            said(&run(&[], true)),
            Some((Severity::Info, "Nothing new from other machines.".into()))
        );
    }

    #[test]
    fn test_a_folder_that_cannot_be_reached_is_worth_a_warning() {
        let mut failed = run(&[], false);
        failed.report.write_error = Some("read-only file system".to_string());
        assert_eq!(
            said(&failed),
            Some((
                Severity::Warning,
                "Could not write to the sync folder: read-only file system".into()
            ))
        );
    }
}
