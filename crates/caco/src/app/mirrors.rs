//! Talking to the user's mirrors off the UI thread, and what that tells the user.

use std::path::PathBuf;

use caco_core::config;
use caco_sources::mirror::{self, SyncReport, Uploaded};

use crate::message::{AppMessage, Notification};
use crate::workers::BackgroundSender;

/// Read every mirror's index, on a worker thread: one request per mirror, and however
/// long one that has gone takes to fail. So the window opens on what each was last
/// known to hold, and the library reloads if that has changed. With no mirrors
/// configured this asks nothing of anyone, and only forgets the mirrors that have been
/// taken out of the config.
pub(super) fn spawn_sync(sender: BackgroundSender, db_path: PathBuf) {
    std::thread::spawn(move || {
        let outcome = caco_core::db::open_connection(&db_path)
            .and_then(|conn| mirror::sync_mirrors(&conn, &config::load_config().mirrors));
        match outcome {
            Ok(report) => sender.send(AppMessage::MirrorsSynced(report)),
            Err(e) => sender.send(AppMessage::Notify(Notification::error(format!(
                "Could not read mirrors: {e}"
            )))),
        }
    });
}

/// Upload a WAD's file to a mirror, on a worker thread. It can take as long as a
/// large file over a home connection does, and the user can go on using the app.
pub(super) fn spawn_upload(
    sender: BackgroundSender,
    db_path: PathBuf,
    wad_id: i64,
    title: String,
    mirror_name: String,
) {
    std::thread::spawn(move || {
        let outcome = (|| -> Result<Uploaded, String> {
            let conn = caco_core::db::open_connection(&db_path)
                .map_err(|e| format!("DB open failed: {e}"))?;
            let config = config::load_config();
            let target = config
                .mirrors
                .iter()
                .find(|mirror| mirror.name == mirror_name)
                .ok_or_else(|| format!("There is no mirror called {mirror_name} any more"))?;
            let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            mirror::upload_wad(&conn, wad_id, target, &now).map_err(|e| e.to_string())
        })();
        sender.send(AppMessage::MirrorUploaded { title, outcome });
    });
}

/// What a sync has to tell the user, if anything. The status bar holds one line, and
/// the reason is most of what a first-time setup needs from it.
pub(super) fn sync_notification(report: &SyncReport) -> Option<Notification> {
    match report.failed.as_slice() {
        [] if report.skipped > 0 => Some(Notification::warning(format!(
            "Left out {} from a mirror's index: the {} could not be read.",
            plural(report.skipped, "entry", "entries"),
            if report.skipped == 1 {
                "entry"
            } else {
                "entries"
            },
        ))),
        [] => None,
        [(name, why)] => Some(Notification::warning(format!(
            "Could not read mirror {name}: {why}"
        ))),
        many => {
            let names: Vec<&str> = many.iter().map(|(name, _)| name.as_str()).collect();
            Some(Notification::warning(format!(
                "Could not read {} mirrors: {}",
                many.len(),
                names.join(", ")
            )))
        }
    }
}

/// What a finished upload has to tell the user.
pub(super) fn upload_notification(outcome: Result<&Uploaded, &String>) -> Notification {
    match outcome {
        Ok(uploaded) if uploaded.already_there => Notification::info(format!(
            "{} already has {}.",
            uploaded.mirror, uploaded.filename
        )),
        Ok(uploaded) => Notification::info(format!(
            "Uploaded {} to {}.",
            uploaded.filename, uploaded.mirror
        )),
        Err(why) => Notification::error(format!("Upload failed. {why}")),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Severity;

    fn said(report: SyncReport) -> Option<(Severity, String)> {
        sync_notification(&report).map(|n| (n.severity, n.text))
    }

    /// The ordinary launch, with or without anything new.
    #[test]
    fn test_a_sync_that_went_fine_says_nothing() {
        assert_eq!(said(SyncReport::default()), None);
        assert_eq!(
            said(SyncReport {
                changed: true,
                ..Default::default()
            }),
            None
        );
    }

    #[test]
    fn test_one_failure_says_why() {
        let report = SyncReport {
            failed: vec![("home".to_string(), "HTTP 401".to_string())],
            ..Default::default()
        };
        assert_eq!(
            said(report),
            Some((
                Severity::Warning,
                "Could not read mirror home: HTTP 401".to_string()
            ))
        );
    }

    #[test]
    fn test_several_failures_say_which() {
        let report = SyncReport {
            failed: vec![
                ("home".to_string(), "could not connect".to_string()),
                ("nas".to_string(), "/mnt/nas is not there".to_string()),
            ],
            ..Default::default()
        };
        assert_eq!(
            said(report),
            Some((
                Severity::Warning,
                "Could not read 2 mirrors: home, nas".to_string()
            ))
        );
    }

    #[test]
    fn test_entries_left_out_are_mentioned() {
        let skipped = |n| SyncReport {
            skipped: n,
            ..Default::default()
        };
        assert_eq!(
            said(skipped(1)).unwrap().1,
            "Left out 1 entry from a mirror's index: the entry could not be read."
        );
        assert_eq!(
            said(skipped(3)).unwrap().1,
            "Left out 3 entries from a mirror's index: the entries could not be read."
        );
    }

    #[test]
    fn test_an_upload_says_what_happened() {
        let uploaded = |already_there| Uploaded {
            mirror: "home".to_string(),
            filename: "road.zip".to_string(),
            already_there,
        };
        assert_eq!(
            upload_notification(Ok(&uploaded(false))).text,
            "Uploaded road.zip to home."
        );
        assert_eq!(
            upload_notification(Ok(&uploaded(true))).text,
            "home already has road.zip."
        );
        let failed = upload_notification(Err(&"Mirror home: HTTP 405".to_string()));
        assert_eq!(failed.severity, Severity::Error);
        assert_eq!(failed.text, "Upload failed. Mirror home: HTTP 405");
    }
}
