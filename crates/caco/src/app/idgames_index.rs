//! Keeping the local index of idgames up to date off the UI thread, and what that tells
//! the user, which is as little as possible.
//!
//! It runs at startup, and a first run can take as long as the app is open: see
//! `caco_sources::idgames::sync`. Closing the app is just a shorter run. A launch with
//! nothing to do reads one row and asks nothing of anyone.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use caco_core::config;
use caco_sources::idgames::Index;
use caco_sources::idgames::index::ListingChanges;
use caco_sources::idgames::sync::{ListingOutcome, SyncOptions, SyncReport, sync_index};
use caco_sources::import_service::backfill_idgames_ids;

use crate::message::{AppMessage, IdgamesIndexRun, Notification};
use crate::workers::BackgroundSender;

/// How often a run says how many uploads it has got to, which a person can read at about
/// this speed anyway, and which costs a query.
const STATUS_EVERY: Duration = Duration::from_secs(2);

const NOTHING_NEW: &str = "idgames has nothing new.";

/// Run it, on a worker thread. `asked_for` is someone pressing Update, which goes ahead
/// whatever the setting says, and is told how it went.
pub(super) fn spawn(
    sender: BackgroundSender,
    db_path: PathBuf,
    asked_for: bool,
    cancel: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let outcome = run(&sender, &db_path, asked_for, &cancel);
        sender.send(AppMessage::IdgamesIndexFinished(outcome));
    });
}

fn run(
    sender: &BackgroundSender,
    db_path: &std::path::Path,
    asked_for: bool,
    cancel: &AtomicBool,
) -> Result<IdgamesIndexRun, String> {
    let index = Index::open_default().map_err(|e| format!("Could not open the index: {e}"))?;
    let status = |index: &Index| index.status().map_err(|e| e.to_string());
    sender.send(AppMessage::IdgamesIndexStatus(status(&index)?));

    let options = SyncOptions::for_config(&config::load_config(), asked_for, chrono::Utc::now());
    let mut said_at = Instant::now();
    let report = sync_index(
        &index,
        &options,
        &mut |progress| {
            sender.send(AppMessage::IdgamesIndexProgress(progress));
            if said_at.elapsed() >= STATUS_EVERY
                && let Ok(status) = index.status()
            {
                sender.send(AppMessage::IdgamesIndexStatus(status));
                said_at = Instant::now();
            }
        },
        &|| cancel.load(Ordering::Relaxed),
    )
    .map_err(|e| e.to_string())?;

    // The library's own rows, for what was imported before the index knew an id. Nothing
    // to do on nearly every run, and worth nothing if it fails, since it runs again.
    let ids_filled_in = caco_core::db::open_connection(db_path)
        .and_then(|conn| backfill_idgames_ids(&conn, &index))
        .unwrap_or(0);

    Ok(IdgamesIndexRun {
        report,
        status: status(&index)?,
        ids_filled_in,
        asked_for,
    })
}

/// What to say when a run someone asked for finishes. Nothing about one nobody did: it is
/// meant to be a thing that happens quietly, and a laptop away from any network should not
/// hear about it every time it starts. What went wrong is on the Import view.
pub(super) fn notification(run: &IdgamesIndexRun) -> Option<Notification> {
    if !run.asked_for {
        return None;
    }
    let SyncReport {
        listing, cancelled, ..
    } = &run.report;
    if *cancelled {
        return Some(Notification::info(
            "Stopped updating the idgames index.".to_string(),
        ));
    }
    Some(match listing {
        ListingOutcome::Failed(why) => {
            Notification::warning(format!("Could not read a new idgames listing: {why}"))
        }
        // From another mirror, for one, whose listing has to be read to find that out.
        ListingOutcome::Updated(changes) if *changes == ListingChanges::default() => {
            Notification::info(NOTHING_NEW.to_string())
        }
        ListingOutcome::Updated(changes) => Notification::info(format!(
            "idgames index updated: {} new, {} changed, {} gone.",
            changes.added, changes.changed, changes.removed
        )),
        ListingOutcome::Unchanged | ListingOutcome::NotDue => {
            Notification::info(NOTHING_NEW.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use caco_sources::idgames::IndexStatus;

    use super::*;
    use crate::message::Severity;

    fn run_with(listing: ListingOutcome, asked_for: bool) -> IdgamesIndexRun {
        IdgamesIndexRun {
            report: SyncReport {
                listing,
                ..Default::default()
            },
            status: IndexStatus::default(),
            ids_filled_in: 0,
            asked_for,
        }
    }

    #[test]
    fn test_a_run_nobody_asked_for_says_nothing_however_it_went() {
        for listing in [
            ListingOutcome::NotDue,
            ListingOutcome::Unchanged,
            ListingOutcome::Failed("offline".to_string()),
        ] {
            assert!(notification(&run_with(listing, false)).is_none());
        }
    }

    #[test]
    fn test_a_run_that_was_stopped_says_so_rather_than_that_there_is_nothing_new() {
        let mut run = run_with(ListingOutcome::Unchanged, true);
        run.report.cancelled = true;
        assert!(notification(&run).unwrap().text.contains("Stopped"));
    }

    #[test]
    fn test_a_run_someone_asked_for_says_how_it_went() {
        let said = |listing| notification(&run_with(listing, true)).unwrap();

        assert_eq!(said(ListingOutcome::Unchanged).text, NOTHING_NEW);
        assert_eq!(
            said(ListingOutcome::Updated(ListingChanges::default())).text,
            NOTHING_NEW
        );
        let updated = said(ListingOutcome::Updated(ListingChanges {
            added: 3,
            changed: 1,
            removed: 0,
        }));
        assert_eq!(
            updated.text,
            "idgames index updated: 3 new, 1 changed, 0 gone."
        );
        let failed = said(ListingOutcome::Failed("offline".to_string()));
        assert_eq!(failed.severity, Severity::Warning);
        assert!(failed.text.contains("offline"));
    }
}
