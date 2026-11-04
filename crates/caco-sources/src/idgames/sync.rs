//! Keeping the local index up to date, in three stages that each ask for less the next
//! time. The listing is where rows come from, so the others have nothing to do until one
//! has been read. After that any of them can fail and the next still runs:
//!
//! 1. **The listing**, from the first mirror that has one: a conditional `GET` of about
//!    400 KB, at most once a day. Every upload is searchable by file name and fetchable
//!    from here on.
//! 2. **The API**, for ids, which the listing cannot give. Each directory is one call,
//!    so the first run is about 300 of them, at most one a second. After that it is at
//!    most one call per new upload, and up to five directories a day to keep ratings
//!    current. The same answer has the title, author and description, so it saves about
//!    19,000 requests to someone's volunteer mirror when it works. It is behind
//!    Cloudflare, and it is allowed to fail.
//! 3. **The text files**, from the mirrors, for whatever the API did not describe, which
//!    is everything on a day it is blocked. Throttled, since these are volunteers.
//!
//! Each row is written as it comes, so a run that is stopped, or an app that is closed, is
//! just a shorter run, and the next one picks up where it left off. Nothing here is on a
//! thread of its own: the app's worker calls [`sync_index`] and gets on with it.

use std::thread::sleep;
use std::time::Duration;

use caco_core::config::MirrorConfig;
use chrono::{DateTime, NaiveDate, Utc};

use super::client::IdgamesClient;
use super::index::{DirectoryJob, Index, ListingChanges, ListingState};
use super::listing::{self, LISTING_PATH, ListedUpload};
use super::public_mirrors;
use super::textfile;
use crate::error::SourceError;
use crate::mirror::{Conditional, Store, Validators};

/// Between one call to the API and the next.
const API_PAUSE: Duration = Duration::from_secs(1);

/// Between one text file and the next, from a mirror on the network. At most four small
/// requests a second to somebody's server, then, and two in practice from the other side of
/// the world. A first run with the API blocked is a few hours of the app being open at
/// that rate, which nobody is waiting for, and which can be spread over any number of runs.
const TEXT_PAUSE: Duration = Duration::from_millis(250);

/// How big a compressed listing can be. It is 420 KB.
const MAX_LISTING_BYTES: u64 = 64 << 20;

/// How big a text file can be. The largest on the archive is 126 KB.
const MAX_TEXT_BYTES: u64 = 4 << 20;

/// A listing older than this is from a mirror that has stopped keeping up, and a newer
/// one is worth looking for. Uploads stop appearing without anything else going wrong.
const STALE_LISTING_DAYS: i64 = 14;

/// When this few uploads are new to a directory the index already knows, asking about each
/// is cheaper than asking for the whole directory, which can be 700 KB. Which is the usual
/// day: one call per new upload.
const FEW_ENOUGH_TO_ASK_ONE_BY_ONE: usize = 5;

/// How many directories to ask about again each day, so that ratings keep up: two months
/// to get round the archive. A new upload has no votes on the day it is first asked about,
/// and nothing else would ever ask about it again.
const DIRECTORIES_TO_REFRESH: usize = 5;

/// A directory asked about more recently than this is not asked about again.
const REFRESH_AFTER_DAYS: i64 = 30;

/// How many text files in a row can be missing before it is the mirrors that are wrong,
/// rather than a few files between syncs.
const MOST_MISSING_IN_A_ROW: usize = 20;

/// Somewhere the archive can be read from.
struct Source {
    /// How it is named when something goes wrong, and which listing came from it.
    label: String,
    store: Store,
}

/// What a sync should talk to, and how fast. Explicit, because a test that could reach
/// the real mirrors could ask volunteers for 21,000 files.
pub struct SyncOptions {
    /// Where the archive can be read from, in the order to try: the user's copies of it
    /// if they have any, then the public mirrors.
    pub sources: Vec<MirrorConfig>,
    pub api: IdgamesClient,
    /// How many days a listing is good for. `0` means it is not looked for.
    pub check_days: i64,
    /// Ask for a new listing, however recently the last one came.
    pub force: bool,
    pub now: DateTime<Utc>,
    pub api_pause: Duration,
    pub text_pause: Duration,
}

impl SyncOptions {
    /// The real thing, starting with the mirror the user has picked for downloads.
    pub fn for_config(config: &caco_core::config::Config, force: bool, now: DateTime<Utc>) -> Self {
        let copies = config
            .mirrors
            .iter()
            .filter(|mirror| mirror.is_idgames_copy())
            .cloned();
        Self {
            sources: copies
                .chain(public_mirrors(config.download_mirror))
                .collect(),
            api: IdgamesClient::new(),
            check_days: config.idgames_index_check_days,
            force,
            now,
            api_pause: API_PAUSE,
            text_pause: TEXT_PAUSE,
        }
    }

    /// Everything that can be opened. A mirror whose config is wrong gets its say in the
    /// Mirrors tool, and is no reason to stop reading from the others.
    fn sources(&self) -> Vec<Source> {
        self.sources
            .iter()
            .filter_map(|config| {
                Some(Source {
                    label: config.name.clone(),
                    store: Store::open(config).ok()?,
                })
            })
            .collect()
    }

    fn today(&self) -> NaiveDate {
        self.now.date_naive()
    }

    /// Whether it is time to ask for a listing.
    fn listing_due(&self, state: Option<&ListingState>) -> bool {
        if self.force {
            return true;
        }
        let checked = state.and_then(|state| DateTime::parse_from_rfc3339(&state.checked).ok());
        match checked {
            Some(checked) => {
                self.now - checked.with_timezone(&Utc) >= chrono::Duration::days(self.check_days)
            }
            None => true,
        }
    }

    fn is_stale(&self, generated: NaiveDate) -> bool {
        self.today() - generated > chrono::Duration::days(STALE_LISTING_DAYS)
    }
}

/// How far a sync has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncProgress {
    Listing,
    AskingApi { done: usize, total: usize },
    ReadingText { done: usize, total: usize },
}

/// What came of asking for a listing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ListingOutcome {
    /// The last one is recent enough.
    #[default]
    NotDue,
    Unchanged,
    Updated(ListingChanges),
    /// None could be had. What the index already holds still stands.
    Failed(String),
}

/// What a sync did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncReport {
    pub listing: ListingOutcome,
    /// How many uploads the API gave an id for.
    pub ids_learned: usize,
    /// How many text files were read.
    pub text_read: usize,
    /// Why the API stopped being asked. Nothing that stops anything else.
    pub api_failure: Option<String>,
    /// Why the text files stopped being read.
    pub text_failure: Option<String>,
    pub cancelled: bool,
}

impl SyncReport {
    /// Whether the index says anything it did not before.
    pub fn changed(&self) -> bool {
        matches!(self.listing, ListingOutcome::Updated(_))
            || self.ids_learned > 0
            || self.text_read > 0
    }
}

/// Bring the index up to date. Takes as long as it takes, which on a first run with the API
/// blocked is over an hour, so run it on a worker. With nothing to do it costs one row read.
pub fn sync_index(
    index: &Index,
    options: &SyncOptions,
    progress: &mut dyn FnMut(SyncProgress),
    cancelled: &dyn Fn() -> bool,
) -> caco_core::Result<SyncReport> {
    let mut report = SyncReport::default();
    if options.check_days <= 0 && !options.force {
        return Ok(report);
    }
    let sources = options.sources();

    let due = options.listing_due(index.listing_state()?.as_ref());
    if due {
        progress(SyncProgress::Listing);
        report.listing = read_listing(index, &sources, options)?;
    }

    ask_api(index, options, due, &mut report, progress, cancelled)?;
    read_text_files(index, &sources, options, &mut report, progress, cancelled)?;
    Ok(report)
}

// ---------------------------------------------------------------------------
// The listing
// ---------------------------------------------------------------------------

/// A listing that has been fetched and read, and not applied yet.
struct FetchedListing {
    source: String,
    uploads: Vec<ListedUpload>,
    validators: Validators,
    generated: NaiveDate,
}

fn read_listing(
    index: &Index,
    sources: &[Source],
    options: &SyncOptions,
) -> caco_core::Result<ListingOutcome> {
    let held = index.listing_state()?;
    let checked = options.now.to_rfc3339();

    let mut first_failure: Option<String> = None;
    // The newest of the listings too old to be happy with, for if nothing beats it.
    let mut stale: Option<FetchedListing> = None;
    let mut holding_a_stale_one = false;

    for source in sources {
        let known = held
            .as_ref()
            .filter(|held| held.source == source.label)
            .map(|held| Validators {
                etag: held.etag.clone(),
                last_modified: held.last_modified.clone(),
            });
        let answer = source
            .store
            .read_if_changed(LISTING_PATH, known.as_ref(), MAX_LISTING_BYTES);
        let failure = match answer {
            Ok(Conditional::NotModified) => {
                let generated = held.as_ref().and_then(|held| parse_date(&held.generated));
                if generated.is_some_and(|generated| options.is_stale(generated)) {
                    holding_a_stale_one = true;
                    continue;
                }
                index.listing_unchanged(&checked)?;
                return Ok(ListingOutcome::Unchanged);
            }
            Ok(Conditional::NotThere) => format!("{} has no {LISTING_PATH}", source.label),
            Ok(Conditional::Changed { bytes, validators }) => {
                match parse_fetched(source, &bytes, validators, options) {
                    Ok(fetched) if !options.is_stale(fetched.generated) => {
                        match apply(index, &fetched, &checked)? {
                            Ok(changes) => return Ok(ListingOutcome::Updated(changes)),
                            Err(why) => why,
                        }
                    }
                    Ok(fetched) => {
                        let newer = stale
                            .as_ref()
                            .is_none_or(|kept| fetched.generated > kept.generated);
                        if newer {
                            stale = Some(fetched);
                        }
                        continue;
                    }
                    Err(why) => why,
                }
            }
            Err(why) => format!("{}: {why}", source.label),
        };
        first_failure.get_or_insert(failure);
    }

    // Nothing recent. An old listing is still better than none, or than an older one.
    let held_generated = held.as_ref().and_then(|held| parse_date(&held.generated));
    match stale {
        Some(fetched)
            if !holding_a_stale_one
                || held_generated.is_none_or(|held| fetched.generated > held) =>
        {
            Ok(match apply(index, &fetched, &checked)? {
                Ok(changes) => ListingOutcome::Updated(changes),
                Err(why) => ListingOutcome::Failed(why),
            })
        }
        _ if holding_a_stale_one => {
            index.listing_unchanged(&checked)?;
            Ok(ListingOutcome::Unchanged)
        }
        _ => Ok(ListingOutcome::Failed(first_failure.unwrap_or_else(|| {
            "there is nowhere to read a listing from".to_string()
        }))),
    }
}

/// Read a listing's bytes, or say what is wrong with them, in words about the mirror.
fn parse_fetched(
    source: &Source,
    bytes: &[u8],
    validators: Validators,
    options: &SyncOptions,
) -> Result<FetchedListing, String> {
    let label = &source.label;
    let text = listing::decompress(bytes)
        .map_err(|e| format!("{label}: could not read its {LISTING_PATH}: {e}"))?;
    let generated = validators
        .last_modified
        .as_deref()
        .and_then(|when| DateTime::parse_from_rfc2822(when).ok())
        .map_or(options.today(), |when| when.date_naive());
    let uploads = listing::parse_listing(&text, generated);
    if uploads.is_empty() {
        return Err(format!("{label}: its {LISTING_PATH} lists no uploads"));
    }
    Ok(FetchedListing {
        source: label.clone(),
        uploads,
        validators,
        generated,
    })
}

/// Make the index say what a listing says. The inner error is a listing not to be believed,
/// which is that mirror's problem and no reason not to try the next. The outer one is the
/// index's own.
fn apply(
    index: &Index,
    fetched: &FetchedListing,
    checked: &str,
) -> caco_core::Result<Result<ListingChanges, String>> {
    if !index.is_believable(fetched.uploads.len())? {
        return Ok(Err(format!(
            "{} lists only {} uploads, which is most of the archive gone",
            fetched.source,
            fetched.uploads.len()
        )));
    }
    let state = ListingState {
        source: fetched.source.clone(),
        etag: fetched.validators.etag.clone(),
        last_modified: fetched.validators.last_modified.clone(),
        generated: ymd(fetched.generated),
        checked: checked.to_string(),
    };
    index.apply_listing(&fetched.uploads, &state).map(Ok)
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// A date as the index stores it, and as a date with a time would compare with it.
fn ymd(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

// ---------------------------------------------------------------------------
// The API
// ---------------------------------------------------------------------------

/// Ask the API about whatever is new, and when a new listing has come in, about a few
/// directories asked about longest ago.
fn ask_api(
    index: &Index,
    options: &SyncOptions,
    listing_was_due: bool,
    report: &mut SyncReport,
    progress: &mut dyn FnMut(SyncProgress),
    cancelled: &dyn Fn() -> bool,
) -> caco_core::Result<()> {
    let mut jobs = index.directory_jobs()?;
    if listing_was_due {
        let cutoff = options.today() - chrono::Duration::days(REFRESH_AFTER_DAYS);
        let cutoff = ymd(cutoff);
        for dir in index.directories_to_refresh(&cutoff, DIRECTORIES_TO_REFRESH)? {
            if !jobs.iter().any(|job| job.dir == dir) {
                jobs.push(DirectoryJob {
                    dir,
                    waiting: Vec::new(),
                    uploads: 0,
                });
            }
        }
    }

    let today = ymd(options.today());
    let total = jobs.len();
    for (done, job) in jobs.iter().enumerate() {
        if cancelled() {
            report.cancelled = true;
            return Ok(());
        }
        progress(SyncProgress::AskingApi { done, total });

        // Not when these are all the directory has, as on a first run, when one call for the
        // lot costs no more than one for the first of them.
        let few = !job.waiting.is_empty()
            && job.waiting.len() <= FEW_ENOUGH_TO_ASK_ONE_BY_ONE
            && job.waiting.len() < job.uploads;
        let answer = if few {
            ask_one_by_one(index, options, job, report, cancelled)
        } else {
            options
                .api
                .list_directory(&job.dir)
                .map_err(ApiStop::from)
                .and_then(|entries| {
                    index
                        .record_directory(&job.dir, &entries, &today)
                        .map_err(ApiStop::Index)
                })
                .map(|learned| report.ids_learned += learned)
        };
        match answer {
            Ok(()) => {}
            // It answered, with something that says it has nothing here.
            Err(ApiStop::NoAnswer) => {
                if !few {
                    index.record_directory(&job.dir, &[], &today)?;
                }
            }
            Err(ApiStop::Failed(why)) => {
                report.api_failure = Some(why);
                return Ok(());
            }
            Err(ApiStop::Index(e)) => return Err(e),
        }
        if done + 1 < total {
            sleep(options.api_pause);
        }
    }
    Ok(())
}

/// Why asking the API about something went no further.
enum ApiStop {
    /// It answered with an error about this one thing, so the next may be fine.
    NoAnswer,
    /// It did not answer, and nothing else is going to go better.
    Failed(String),
    Index(caco_core::Error),
}

impl From<SourceError> for ApiStop {
    fn from(error: SourceError) -> Self {
        match error {
            SourceError::Api(_) => Self::NoAnswer,
            other => Self::Failed(other.to_string()),
        }
    }
}

fn ask_one_by_one(
    index: &Index,
    options: &SyncOptions,
    job: &DirectoryJob,
    report: &mut SyncReport,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), ApiStop> {
    for (position, path) in job.waiting.iter().enumerate() {
        if cancelled() {
            return Ok(());
        }
        if position > 0 {
            sleep(options.api_pause);
        }
        match options.api.get(None, Some(path)) {
            Ok(entry) => {
                let learned = index.record_entry(&entry).map_err(ApiStop::Index)?;
                report.ids_learned += usize::from(learned);
            }
            Err(error) => match ApiStop::from(error) {
                ApiStop::NoAnswer => {
                    index.record_unknown_to_api(path).map_err(ApiStop::Index)?;
                }
                stop => return Err(stop),
            },
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The text files
// ---------------------------------------------------------------------------

fn read_text_files(
    index: &Index,
    sources: &[Source],
    options: &SyncOptions,
    report: &mut SyncReport,
    progress: &mut dyn FnMut(SyncProgress),
    cancelled: &dyn Fn() -> bool,
) -> caco_core::Result<()> {
    let jobs = index.text_jobs()?;
    let total = jobs.len();
    let mut alive = vec![true; sources.len()];
    let mut first_failure: Option<String> = None;
    let mut missing_in_a_row = 0;

    for (done, job) in jobs.iter().enumerate() {
        if cancelled() {
            report.cancelled = true;
            return Ok(());
        }
        progress(SyncProgress::ReadingText { done, total });

        let mut found = None;
        let mut asked_the_network = false;
        for (source, alive) in sources.iter().zip(alive.iter_mut()) {
            if !*alive {
                continue;
            }
            asked_the_network |= !source.store.is_local();
            match source.store.read_bytes(&job.path, MAX_TEXT_BYTES) {
                Ok(Some(bytes)) => {
                    found = Some(bytes);
                    break;
                }
                Ok(None) => {}
                // It is not coming back during this run. A local copy with a dead mount
                // is the same thing, more slowly.
                Err(why) => {
                    *alive = false;
                    first_failure.get_or_insert(format!("{}: {why}", source.label));
                }
            }
        }

        match found {
            Some(bytes) => {
                index.record_text(job, &textfile::parse(&textfile::decode(&bytes)))?;
                report.text_read += 1;
                missing_in_a_row = 0;
            }
            None if !alive.contains(&true) => {
                report.text_failure = first_failure.or(Some("nothing to read from".to_string()));
                return Ok(());
            }
            // Between one mirror's sync and the next. It is tried again on the next run,
            // when it will have come or the listing will have stopped naming it.
            None => {
                missing_in_a_row += 1;
                if missing_in_a_row >= MOST_MISSING_IN_A_ROW {
                    report.text_failure = Some(format!(
                        "{MOST_MISSING_IN_A_ROW} text files in a row were not where the listing \
                         says they are"
                    ));
                    return Ok(());
                }
            }
        }
        if asked_the_network && done + 1 < total {
            sleep(options.text_pause);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
