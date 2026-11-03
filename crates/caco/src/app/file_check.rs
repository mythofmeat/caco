//! The startup check of every recorded WAD file, and what it tells the user.

use std::path::PathBuf;

use caco_core::presence::{self, CheckReport, StorePaths};

use crate::message::{AppMessage, Notification};
use crate::workers::BackgroundSender;

/// Check every recorded WAD file against this machine, on a worker thread:
/// first that it is there, then that it is the file caco hashed.
///
/// One `stat` per row with a path, which is nothing on a local disk and
/// unbounded on a network mount that has stopped answering, and then a full
/// read of every file that is new or has been replaced: the whole library, the
/// first time. So the window opens on the rows as they were, the presence
/// answer is sent the moment it exists rather than after the hashing, and the
/// library reloads when either answer changes it. A check that changed nothing
/// says nothing.
pub(super) fn spawn(sender: BackgroundSender, db_path: PathBuf) {
    std::thread::spawn(move || {
        let conn = match caco_core::db::open_connection(&db_path) {
            Ok(conn) => conn,
            Err(e) => {
                sender.send(could_not_check(e));
                return;
            }
        };
        match presence::check_library(&conn, &StorePaths::from_config()) {
            Ok(report) if report.changed() => sender.send(AppMessage::FilesChecked(report)),
            Ok(_) => {}
            Err(e) => {
                // Hashing would only trip over the same dangling paths.
                sender.send(could_not_check(e));
                return;
            }
        }
        match caco_core::identity::check_library(&conn) {
            Ok(hashed) if hashed > 0 => sender.send(AppMessage::FilesIdentified),
            Ok(_) => {}
            Err(e) => sender.send(could_not_check(e)),
        }
    });
}

fn could_not_check(e: caco_core::Error) -> AppMessage {
    AppMessage::Notify(Notification::error(format!(
        "Could not check WAD files: {e}"
    )))
}

/// What a check that changed something has to tell the user, if anything.
///
/// Only files caco cannot fetch again are worth a warning: an idgames file
/// that has gone downloads again on Play, so its absence is not news. The
/// status bar holds one line, so a warning does not also report relinks.
pub(super) fn notification(report: &CheckReport) -> Option<Notification> {
    let files = |n: usize| match n {
        1 => "1 WAD file".to_string(),
        n => format!("{n} WAD files"),
    };
    match (report.lost, report.relinked) {
        (0, 0) => None,
        (0, relinked) => Some(Notification::info(format!("Relinked {}.", files(relinked)))),
        (lost, _) => {
            let (verb, them) = if lost == 1 {
                ("is", "it")
            } else {
                ("are", "them")
            };
            Some(Notification::warning(format!(
                "{} {verb} no longer on this machine, and caco can't download {them}.",
                files(lost)
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Severity;

    fn said(report: CheckReport) -> Option<(Severity, String)> {
        notification(&report).map(|n| (n.severity, n.text))
    }

    #[test]
    fn test_lost_files_warn() {
        let lost = |n| CheckReport {
            relinked: 2,
            missing: n + 3,
            lost: n,
        };
        assert_eq!(
            said(lost(1)),
            Some((
                Severity::Warning,
                "1 WAD file is no longer on this machine, and caco can't download it.".into()
            ))
        );
        assert_eq!(
            said(lost(6)),
            Some((
                Severity::Warning,
                "6 WAD files are no longer on this machine, and caco can't download them.".into()
            ))
        );
    }

    #[test]
    fn test_relinks_alone_are_info() {
        let report = CheckReport {
            relinked: 1,
            missing: 0,
            lost: 0,
        };
        assert_eq!(
            said(report),
            Some((Severity::Info, "Relinked 1 WAD file.".into()))
        );
    }

    /// An idgames file that has gone downloads again on Play.
    #[test]
    fn test_missing_downloadable_files_say_nothing() {
        let report = CheckReport {
            relinked: 0,
            missing: 4,
            lost: 0,
        };
        assert_eq!(said(report), None);
    }
}
