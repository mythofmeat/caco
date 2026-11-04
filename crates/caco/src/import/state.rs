// ---------------------------------------------------------------------------
// Import state types
// ---------------------------------------------------------------------------

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use caco_sources::idgames::sync::{ListingOutcome, SyncProgress};
use caco_sources::idgames::{IdgamesRef, IndexStatus};

use crate::message::IdgamesIndexRun;

pub const IMPORT_SOURCES: &[&str] = &["idgames", "doomwiki", "doomworld", "URL", "local"];

// ---------------------------------------------------------------------------
// Top-level import state
// ---------------------------------------------------------------------------

pub struct ImportState {
    pub active_source: usize,
    pub idgames: SearchState,
    pub doomwiki: SearchState,
    pub doomworld: FormState,
    pub url_form: FormState,
    pub local_form: FormState,
    pub index: IndexPanel,
}

impl Default for ImportState {
    fn default() -> Self {
        Self {
            active_source: 0,
            index: IndexPanel::default(),
            idgames: SearchState::default(),
            doomwiki: SearchState::default(),
            doomworld: FormState::new(FormKind::Doomworld),
            url_form: FormState::new(FormKind::Url),
            local_form: FormState::new(FormKind::Local),
        }
    }
}

impl ImportState {
    /// Get a mutable reference to the search state for the given source.
    pub fn search_state_mut(&mut self, source: SearchSource) -> &mut SearchState {
        match source {
            SearchSource::Idgames => &mut self.idgames,
            SearchSource::Doomwiki => &mut self.doomwiki,
        }
    }

    /// Get a mutable reference to the form state for the given kind.
    pub fn form_state_mut(&mut self, kind: FormKind) -> &mut FormState {
        match kind {
            FormKind::Doomworld => &mut self.doomworld,
            FormKind::Url => &mut self.url_form,
            FormKind::Local => &mut self.local_form,
        }
    }
}

// ---------------------------------------------------------------------------
// The local index of idgames
// ---------------------------------------------------------------------------

/// What the Import view knows about the local index of idgames, which a worker keeps up to
/// date whichever view is on screen.
#[derive(Default)]
pub struct IndexPanel {
    /// How far along it is, once that has been read.
    pub status: Option<IndexStatus>,
    /// Set while a worker is running, for stopping it.
    pub cancel: Option<Arc<AtomicBool>>,
    /// How far along the worker is.
    pub progress: Option<SyncProgress>,
    /// What went wrong the last time it ran, if anything did.
    pub problem: Option<String>,
}

impl IndexPanel {
    pub fn is_running(&self) -> bool {
        self.cancel.is_some() || self.progress.is_some()
    }

    /// A worker is starting. Stops when the flag it is handed is set.
    pub fn started(&mut self) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(Arc::clone(&cancel));
        self.progress = None;
        cancel
    }

    pub fn stop(&self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The worker has finished, or has given up.
    pub fn finished(&mut self, outcome: &Result<IdgamesIndexRun, String>) {
        self.cancel = None;
        self.progress = None;
        self.problem = None;
        match outcome {
            Ok(run) => {
                self.status = Some(run.status.clone());
                self.problem = problem_of(run);
            }
            Err(why) => self.problem = Some(why.clone()),
        }
    }

    /// The line the Import view shows about it.
    pub fn summary(&self) -> String {
        if let Some(progress) = &self.progress {
            return match progress {
                SyncProgress::Listing => "Reading the idgames listing...".to_string(),
                SyncProgress::AskingApi { done, total } => {
                    format!("Asking idgames about new uploads: {done} of {total}...")
                }
                SyncProgress::ReadingText { done, total } => {
                    format!("Reading text files off a mirror: {done} of {total}...")
                }
            };
        }
        let status = self.status.as_ref().filter(|status| !status.is_empty());
        let Some(status) = status else {
            return "No local index yet, so searches ask idgames itself.".to_string();
        };
        let listed = status.listing.as_ref().map_or(String::new(), |listing| {
            format!(", listing of {}", listing.generated)
        });
        if status.is_complete() {
            format!("Searching {} uploads here{listed}.", status.uploads)
        } else {
            format!(
                "{} uploads here, {} not read yet{listed}. Searches ask idgames as well.",
                status.uploads, status.waiting_for_text
            )
        }
    }
}

/// What to say about a run that did not go as it should, in the fewest words that say what
/// it costs. Nothing for what it can do without: the API being blocked is an ordinary day
/// when the text files carry on regardless.
fn problem_of(run: &IdgamesIndexRun) -> Option<String> {
    if let ListingOutcome::Failed(why) = &run.report.listing {
        return Some(format!("Could not read a new listing. {why}"));
    }
    if let Some(why) = &run.report.text_failure {
        return Some(format!("Stopped reading text files. {why}"));
    }
    match (&run.report.api_failure, run.status.is_complete()) {
        (Some(why), false) => Some(format!("Could not ask idgames. {why}")),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Search state (idgames / doomwiki)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResultEntry>,
    pub selected_row: Option<usize>,
    pub is_searching: bool,
    pub status_text: String,
    /// Why the last search failed. Not the same as empty `results`, which means
    /// the source answered and has nothing: "No results" for a search that never
    /// ran would say the WAD does not exist.
    pub error: Option<String>,
    /// Why these may not be all the results there are.
    pub note: Option<String>,
    /// The table holds the newest uploads, rather than the answer to a search.
    pub showing_newest: bool,
    /// The newest uploads have been asked for, so that opening the tab does not ask again
    /// on every frame until they come.
    pub newest_asked: bool,
}

impl SearchState {
    /// Update state after a search completes.
    pub fn set_results(&mut self, results: SearchResults) {
        let SearchResults { entries, note } = results;
        let count = entries.len();
        self.is_searching = false;
        self.showing_newest = false;
        self.status_text = format!("{count} result{}", if count == 1 { "" } else { "s" });
        self.selected_row = if entries.is_empty() { None } else { Some(0) };
        self.results = entries;
        self.error = None;
        self.note = note;
    }

    /// Fill the table with the newest uploads, unless a search has got there first.
    pub fn set_newest(&mut self, results: SearchResults) {
        if self.is_searching || !(self.results.is_empty() || self.showing_newest) {
            return;
        }
        self.showing_newest = true;
        self.status_text = "Newest on idgames".to_string();
        self.selected_row = if results.entries.is_empty() {
            None
        } else {
            Some(0)
        };
        self.results = results.entries;
        self.error = None;
        self.note = None;
    }

    /// Whether the table is free for the newest uploads and they have not been asked for.
    pub fn wants_newest(&self, index: &IndexPanel) -> bool {
        !self.newest_asked
            && !self.is_searching
            && self.query.trim().is_empty()
            && self.results.is_empty()
            && self.error.is_none()
            && index
                .status
                .as_ref()
                .is_some_and(|status| !status.is_empty())
    }

    /// Update state after a search fails.
    ///
    /// Drops the previous results too: they answered an earlier query, and left
    /// in the table they would stand in for the reason this one failed.
    pub fn set_error(&mut self, error: String) {
        self.is_searching = false;
        self.showing_newest = false;
        self.status_text = "Search failed".to_string();
        self.selected_row = None;
        self.results.clear();
        self.error = Some(error);
        self.note = None;
    }
}

// ---------------------------------------------------------------------------
// Form state (doomworld / URL / local)
// ---------------------------------------------------------------------------

pub struct FormState {
    pub kind: FormKind,
    pub fields: Vec<FormField>,
    pub is_submitting: bool,
    pub status_text: String,
    /// Async file picker for the Local-import "Browse…" button. Polled each
    /// frame; result is written into the `path` field.
    pub pending_browse: Option<crate::workers::FileDialogReceiver>,
}

impl FormState {
    pub fn new(kind: FormKind) -> Self {
        let fields = match kind {
            FormKind::Doomworld => vec![
                FormField::new("url", "Forum URL", true),
                FormField::new("title", "Title", false),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
            ],
            FormKind::Url => vec![
                FormField::new("title", "Title", true),
                FormField::new("url", "Download URL", true),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
                FormField::new("notes", "Notes", false),
            ],
            FormKind::Local => vec![
                FormField::new("path", "File Path", true),
                FormField::new("title", "Title", true),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
            ],
        };
        Self {
            kind,
            fields,
            is_submitting: false,
            status_text: String::new(),
            pending_browse: None,
        }
    }

    /// Validate required fields. Returns Ok or Err with the missing field label.
    pub fn validate(&self) -> Result<(), String> {
        for f in &self.fields {
            if f.required && f.value.trim().is_empty() {
                return Err(format!("{} is required", f.label));
            }
        }
        Ok(())
    }

    /// Collect field name/value pairs.
    pub fn collect_values(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .map(|f| (f.name.to_string(), f.value.clone()))
            .collect()
    }

    /// Reset all fields and status.
    pub fn reset(&mut self) {
        for f in &mut self.fields {
            f.value.clear();
        }
        self.status_text.clear();
        self.is_submitting = false;
        self.pending_browse = None;
    }
}

pub struct FormField {
    pub name: &'static str,
    pub label: &'static str,
    pub display_label: String,
    pub value: String,
    pub required: bool,
}

impl FormField {
    fn new(name: &'static str, label: &'static str, required: bool) -> Self {
        let display_label = if required {
            format!("{label}*:")
        } else {
            format!("{label}:")
        };
        Self {
            name,
            label,
            display_label,
            value: String::new(),
            required,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    Doomworld,
    Url,
    Local,
}

// ---------------------------------------------------------------------------
// Search result types (shared with workers + messages)
// ---------------------------------------------------------------------------

/// What a search came to.
#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub entries: Vec<SearchResultEntry>,
    /// Why this may not be everything, when that is the case.
    pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SearchResultEntry {
    pub title: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub source_data: SearchSourceData,
}

impl SearchResultEntry {
    /// Display string for the table's extra column (derived from source_data).
    pub fn extra_display(&self) -> String {
        match &self.source_data {
            SearchSourceData::Idgames { rating, date, .. } => {
                let r = rating.map(|v| format!("{v:.1}")).unwrap_or_default();
                let d = date.as_deref().unwrap_or("");
                format!("{r}  {d}")
            }
            SearchSourceData::Doomwiki { year, port, .. } => {
                let y = year.map(|v| v.to_string()).unwrap_or_default();
                let p = port.as_deref().unwrap_or("");
                format!("{y}  {p}")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum SearchSourceData {
    Idgames {
        /// How to ask for it, which is by path for what the index has no id for yet.
        target: IdgamesRef,
        rating: Option<f64>,
        date: Option<String>,
        /// Where it is in the archive, which tells apart the seven files called
        /// `castle.zip`.
        dir: Option<String>,
        filename: Option<String>,
    },
    Doomwiki {
        year: Option<i32>,
        iwad: Option<String>,
        port: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchSource {
    Idgames,
    Doomwiki,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idgames_entry(id: i64, title: &str) -> SearchResultEntry {
        SearchResultEntry {
            title: title.to_string(),
            author: None,
            description: None,
            source_data: SearchSourceData::Idgames {
                target: IdgamesRef::Id(id),
                rating: None,
                date: None,
                dir: None,
                filename: None,
            },
        }
    }

    fn results(entries: Vec<SearchResultEntry>) -> SearchResults {
        SearchResults {
            entries,
            note: None,
        }
    }

    fn ready_index() -> IndexPanel {
        IndexPanel {
            status: Some(IndexStatus {
                uploads: 100,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn set_results_selects_first_when_non_empty() {
        let mut s = SearchState::default();
        s.set_results(results(vec![idgames_entry(1, "a"), idgames_entry(2, "b")]));
        assert_eq!(s.selected_row, Some(0));
        assert!(!s.is_searching);
        assert_eq!(s.status_text, "2 results");
        assert_eq!(s.results.len(), 2);
    }

    #[test]
    fn set_results_clears_selection_when_empty() {
        let mut s = SearchState {
            selected_row: Some(3),
            is_searching: true,
            ..SearchState::default()
        };
        s.set_results(SearchResults::default());
        assert_eq!(s.selected_row, None);
        assert!(!s.is_searching);
        assert_eq!(s.status_text, "0 results");
    }

    #[test]
    fn set_results_singular_status() {
        let mut s = SearchState::default();
        s.set_results(results(vec![idgames_entry(1, "solo")]));
        assert_eq!(s.status_text, "1 result");
    }

    #[test]
    fn set_error_stops_searching_and_drops_old_results() {
        let mut s = SearchState {
            results: vec![idgames_entry(1, "from an earlier query")],
            selected_row: Some(0),
            is_searching: true,
            ..SearchState::default()
        };
        s.set_error("blocked".to_string());
        assert_eq!(s.error.as_deref(), Some("blocked"));
        assert_eq!(s.status_text, "Search failed");
        assert!(!s.is_searching);
        assert!(s.results.is_empty());
        assert_eq!(s.selected_row, None);
    }

    #[test]
    fn set_results_clears_a_previous_error() {
        let mut s = SearchState::default();
        s.set_error("blocked".to_string());
        s.set_results(SearchResults::default());
        assert_eq!(s.error, None);
        assert_eq!(s.status_text, "0 results");
    }

    #[test]
    fn form_validate_rejects_missing_required() {
        let form = FormState::new(FormKind::Doomworld);
        let err = form.validate().unwrap_err();
        assert!(err.contains("Forum URL"));
    }

    #[test]
    fn form_validate_accepts_required_filled() {
        let mut form = FormState::new(FormKind::Doomworld);
        for f in &mut form.fields {
            if f.required {
                f.value = "x".into();
            }
        }
        assert!(form.validate().is_ok());
    }

    #[test]
    fn form_reset_clears_fields_and_status() {
        let mut form = FormState::new(FormKind::Url);
        for f in &mut form.fields {
            f.value = "populated".into();
        }
        form.status_text = "boom".into();
        form.is_submitting = true;
        form.reset();
        assert!(form.fields.iter().all(|f| f.value.is_empty()));
        assert!(form.status_text.is_empty());
        assert!(!form.is_submitting);
    }

    #[test]
    fn form_collect_values_returns_all_fields() {
        let mut form = FormState::new(FormKind::Local);
        form.fields[0].value = "/tmp/a.wad".into();
        form.fields[1].value = "Title".into();
        let vals = form.collect_values();
        assert_eq!(vals[0], ("path".to_string(), "/tmp/a.wad".to_string()));
        assert_eq!(vals[1], ("title".to_string(), "Title".to_string()));
    }

    #[test]
    fn a_search_says_when_its_results_may_not_be_everything() {
        let mut s = SearchState::default();
        s.set_results(SearchResults {
            entries: vec![idgames_entry(1, "a")],
            note: Some("from the index".to_string()),
        });
        assert_eq!(s.note.as_deref(), Some("from the index"));
        s.set_results(results(vec![idgames_entry(1, "a")]));
        assert_eq!(s.note, None);
    }

    /// With nothing asked yet, a table of what is new is better than an empty one.
    #[test]
    fn the_newest_uploads_are_wanted_only_by_an_empty_table_and_only_once() {
        let mut s = SearchState::default();
        assert!(s.wants_newest(&ready_index()));

        // There is nothing to ask for until there is a listing.
        assert!(!s.wants_newest(&IndexPanel::default()));

        s.newest_asked = true;
        assert!(!s.wants_newest(&ready_index()));

        let typing = SearchState {
            query: "sunlust".to_string(),
            ..Default::default()
        };
        assert!(!typing.wants_newest(&ready_index()));

        let answered = SearchState {
            results: vec![idgames_entry(1, "a")],
            ..Default::default()
        };
        assert!(!answered.wants_newest(&ready_index()));
    }

    #[test]
    fn the_newest_uploads_fill_the_table_and_say_what_it_holds() {
        let mut s = SearchState::default();
        s.set_newest(results(vec![idgames_entry(1, "a")]));
        assert!(s.showing_newest);
        assert_eq!(s.status_text, "Newest on idgames");
        assert_eq!(s.selected_row, Some(0));
    }

    /// They can arrive after a search was started, and must not replace its answer.
    #[test]
    fn the_newest_uploads_do_not_replace_a_search() {
        let mut s = SearchState::default();
        s.set_results(results(vec![idgames_entry(1, "searched for")]));
        s.set_newest(results(vec![idgames_entry(2, "newest")]));
        assert_eq!(s.results[0].title, "searched for");
        assert!(!s.showing_newest);

        let mut s = SearchState {
            is_searching: true,
            ..Default::default()
        };
        s.set_newest(results(vec![idgames_entry(2, "newest")]));
        assert!(s.results.is_empty());
    }

    #[test]
    fn newer_newest_uploads_replace_older_ones() {
        let mut s = SearchState::default();
        s.set_newest(results(vec![idgames_entry(1, "yesterday")]));
        s.set_newest(results(vec![idgames_entry(2, "today")]));
        assert_eq!(s.results[0].title, "today");
    }

    fn status(uploads: usize, waiting_for_text: usize) -> IndexStatus {
        IndexStatus {
            uploads,
            waiting_for_text,
            ..Default::default()
        }
    }

    fn run(listing: ListingOutcome, status: IndexStatus) -> IdgamesIndexRun {
        IdgamesIndexRun {
            report: caco_sources::idgames::sync::SyncReport {
                listing,
                ..Default::default()
            },
            status,
            ids_filled_in: 0,
            asked_for: false,
        }
    }

    #[test]
    fn the_index_says_what_a_search_will_do() {
        let summary = |status| IndexPanel {
            status,
            ..Default::default()
        };
        assert!(summary(None).summary().contains("ask idgames itself"));
        assert!(
            summary(Some(status(0, 0)))
                .summary()
                .contains("ask idgames itself")
        );

        let complete = summary(Some(status(21_384, 0))).summary();
        assert_eq!(complete, "Searching 21384 uploads here.");

        let building = summary(Some(status(21_384, 9_000))).summary();
        assert!(building.contains("9000 not read yet"), "{building}");
        assert!(building.contains("ask idgames as well"), "{building}");
    }

    #[test]
    fn the_index_says_how_far_along_a_run_is() {
        let at = |progress| {
            IndexPanel {
                progress: Some(progress),
                ..Default::default()
            }
            .summary()
        };
        assert!(at(SyncProgress::Listing).contains("listing"));
        assert!(at(SyncProgress::AskingApi { done: 3, total: 9 }).contains("3 of 9"));
        assert!(at(SyncProgress::ReadingText { done: 5, total: 7 }).contains("5 of 7"));
    }

    #[test]
    fn a_run_finishing_stops_saying_it_is_running_and_keeps_what_it_learned() {
        let mut index = IndexPanel::default();
        let cancel = index.started();
        index.progress = Some(SyncProgress::Listing);
        assert!(index.is_running());

        index.stop();
        assert!(cancel.load(Ordering::Relaxed));

        index.finished(&Ok(run(ListingOutcome::Unchanged, status(10, 0))));
        assert!(!index.is_running());
        assert_eq!(index.progress, None);
        assert_eq!(index.status.as_ref().unwrap().uploads, 10);
        assert_eq!(index.problem, None);
    }

    #[test]
    fn a_run_that_could_not_start_says_why() {
        let mut index = IndexPanel::default();
        index.started();
        index.finished(&Err("no disk".to_string()));
        assert!(!index.is_running());
        assert_eq!(index.problem.as_deref(), Some("no disk"));
    }

    #[test]
    fn a_listing_nobody_could_give_is_worth_a_word() {
        let mut index = IndexPanel::default();
        index.finished(&Ok(run(
            ListingOutcome::Failed("offline".to_string()),
            status(10, 0),
        )));
        assert!(index.problem.unwrap().contains("offline"));
    }

    /// An ordinary day, with the index complete and the API behind a challenge. Going on
    /// about it would only tell the user something they can do nothing about.
    #[test]
    fn the_api_being_blocked_is_not_a_problem_unless_it_costs_something() {
        let blocked = |status| {
            let mut run = run(ListingOutcome::Unchanged, status);
            run.report.api_failure = Some("blocked".to_string());
            let mut index = IndexPanel::default();
            index.finished(&Ok(run));
            index.problem
        };
        assert_eq!(blocked(status(100, 0)), None);
        assert!(blocked(status(100, 90)).unwrap().contains("blocked"));
    }

    #[test]
    fn active_source_state_mut_routes_correctly() {
        let mut state = ImportState::default();
        state.search_state_mut(SearchSource::Idgames).query = "ig".into();
        state.search_state_mut(SearchSource::Doomwiki).query = "dw".into();
        assert_eq!(state.idgames.query, "ig");
        assert_eq!(state.doomwiki.query, "dw");
    }
}
