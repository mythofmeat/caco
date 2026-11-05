use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use caco_core::db::cacowards::{CacowardRecord, EffectiveStatus};
use caco_core::db::collections::{self, CollectionRecord};
use caco_core::db::models::WadRecord;
use caco_core::db::sessions::WadStats;
use caco_core::wad_analysis::WadAnalysis;
use rusqlite::Connection;

use crate::dialogs::cacoward_link::CacowardLinkDialogState;
use crate::dialogs::collections::CollectionsDialogState;
use crate::dialogs::delete::DeleteDialogState;
use crate::dialogs::edit::EditDialogState;
use crate::dialogs::enrich::EnrichDialogState;
use crate::dialogs::file_changed::FileChangedDialogState;
use crate::dialogs::link::LinkDialogState;
use crate::dialogs::mirror_offer::MirrorOfferDialogState;
use crate::dialogs::mirrors::MirrorsDialogState;
use crate::dialogs::newer_saves::NewerSavesDialogState;
use crate::dialogs::profiles::ProfilesDialogState;
use crate::dialogs::resources::ResourcesDialogState;
use crate::dialogs::sessions::SessionsDialogState;
use crate::dialogs::settings::SettingsDialogState;
use crate::dialogs::sourceports::SourceportsDialogState;
use crate::dialogs::storage::{StorageDialogState, StorageTab};
use crate::dialogs::sync::SyncDialogState;
use crate::dialogs::wad_data::WadDataDialogState;
use crate::dialogs::wad_stats::WadStatsDialogState;
use crate::filter_query::{FilterCheck, FilterQuery};
use crate::get::{GetTarget, Gets};
use crate::import::state::ImportState;
use crate::message::Notification;
use crate::persist;

// ---------------------------------------------------------------------------
// View mode (Library vs Import)
// ---------------------------------------------------------------------------

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum ViewMode {
    #[default]
    Library,
    Import,
    /// Magazine-style view over the `cacowards` table — see
    /// [`CacowardsState`] for the data backing it.
    Cacowards,
    /// Library-wide playtime and completion figures. A view rather than a
    /// dialog because nothing in it is an action — you go there to read.
    Stats,
}

// ---------------------------------------------------------------------------
// View layout (List table vs Grid cards)
// ---------------------------------------------------------------------------

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum ViewLayout {
    List,
    #[default]
    Grid,
}

// ---------------------------------------------------------------------------
// Action requests (returned by panels/widgets to trigger app-level actions)
// ---------------------------------------------------------------------------

pub enum ActionRequest {
    Play(i64),
    /// Play a WAD the launch stopped to ask about, once the user has been told and
    /// chose to go ahead.
    PlayWith(i64, caco_core::player::PlayOptions),
    StartNewPlaythrough(i64),
    Edit(i64),
    Delete(i64),
    Sessions(i64),
    MapStats(i64),
    /// Open the per-WAD saves / backups / demos dialog.
    WadData(i64),
    /// Play back a demo file (by name) from a WAD's demos directory.
    PlayDemo(i64, String),
    /// Open the disk-management dialog on a given tab — cached files, cleanup,
    /// trash and the companion registry all live in there.
    Storage(StorageTab),
    Settings,
    Help,
    Resources,
    Profiles,
    Collections,
    /// Open the metadata enrichment dialog.
    Enrich,
    /// Open the sourceport build manager.
    Sourceports,
    /// Build a sourceport the sourceports dialog has selected, on a worker thread.
    StartSourceportBuild(Box<crate::dialogs::sourceports::SourceportBuildRequest>),
    /// Lazily discover a sourceport's release tags and branches.
    CheckSourceportVersions(crate::dialogs::sourceports::SourceportVersionsRequest),
    /// Run an enrichment the dialog has configured, on a worker thread.
    StartEnrich(Box<crate::dialogs::enrich::EnrichRequest>),
    EditCollection(String),
    DeleteCollection(String),
    /// Import the WAD referenced by a Cacoward entry (by DB pk). The
    /// dispatcher spawns a background worker that resolves the entry,
    /// fetches the underlying idgames or doomwiki source, and links the
    /// new wad row back to the cacoward entry.
    ImportCacoward(i64),

    /// Open the modal that lets the user pick a library WAD to link to
    /// this cacoward entry (cacoward pk).
    LinkCacoward(i64),

    /// Clear the wad_id on a cacoward entry. It stays clear until a WAD it
    /// matches is imported or a data update gives it new evidence.
    UnlinkCacoward(i64),

    /// Flip the `supported` flag on a cacoward entry. `true` makes the
    /// entry playable / counted toward completion; `false` parks it as
    /// "not yet supported by caco".
    SetCacowardSupported(i64, bool),

    /// Open the mirrors tool, which starts on the setup wizard if there are none.
    Mirrors,

    /// Open the Sync tool.
    Sync,

    /// Write out this machine's log and read everyone else's, now.
    SyncNow,

    /// Read every mirror's index again, because the list of mirrors may have changed.
    SyncMirrors,

    /// Try a mirror the wizard is setting up, on a worker thread.
    ProbeMirror(Box<caco_core::config::MirrorConfig>),

    /// Make a mirror the wizard is setting up ready to use, on a worker thread.
    SetUpMirror(Box<caco_core::config::MirrorConfig>),

    /// Upload a WAD's file to a mirror, on a worker thread.
    UploadToMirror {
        wad_id: i64,
        mirror: String,
    },

    /// Upload everything worth uploading to a mirror, on a worker thread.
    UploadAllToMirror(String),

    /// Open the page for something caco cannot fetch, and watch the Downloads folder
    /// for what comes of it.
    Get(GetTarget),
}

// ---------------------------------------------------------------------------
// Dialog / play state
// ---------------------------------------------------------------------------

pub enum ActiveDialog {
    Edit(Box<EditDialogState>),
    Delete(DeleteDialogState),
    Sessions(SessionsDialogState),
    /// Cache / Clean / Trash / Files, tabbed — see [`StorageDialogState`].
    Storage(Box<StorageDialogState>),
    Profiles(Box<ProfilesDialogState>),
    Collections(CollectionsDialogState),
    Enrich(Box<EnrichDialogState>),
    Sourceports(Box<SourceportsDialogState>),
    Resources(ResourcesDialogState),
    WadStats(WadStatsDialogState),
    WadData(Box<WadDataDialogState>),
    Link(LinkDialogState),
    /// Asked once a WAD nothing could fetch again has a file here.
    MirrorOffer(MirrorOfferDialogState),
    /// The mirrors tool, with its setup wizard.
    Mirrors(Box<MirrorsDialogState>),
    /// Asked before launching a WAD whose file has changed since it was last
    /// played.
    FileChanged(FileChangedDialogState),
    /// Asked before launching a WAD another machine has newer saves for.
    NewerSaves(NewerSavesDialogState),
    /// Which machines this library has heard from.
    Sync(Box<SyncDialogState>),
    /// Modal picker for choosing a library WAD to link to a Cacoward entry.
    CacowardLink(CacowardLinkDialogState),
    Settings(SettingsDialogState),
    Help,
}

pub enum PlayState {
    Idle,
    Playing { wad_id: i64, wad_title: String },
}

// ---------------------------------------------------------------------------
// Sort definitions
// ---------------------------------------------------------------------------

/// The ways to sort the library, by the key `db::search_wads` knows them by. The first is the
/// default: what you played last comes first, and a library that has never been played falls
/// back to the order it was added in.
pub const SORT_FIELDS: &[(&str, &str)] = &[
    ("last_played", "Last Played"),
    ("title", "Title"),
    ("author", "Author"),
    ("playtime", "Playtime"),
    ("year", "Year"),
    ("rating", "Rating"),
    ("created", "Date Added"),
    ("random", "Random"),
];

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    // View mode
    pub view_mode: ViewMode,

    // Status filters (empty = all; multiple entries produce OR query)
    pub status_filters: HashSet<String>,

    // Filter (debounced)
    pub filter: FilterQuery,

    // Sort
    pub sort_field_index: usize,
    pub sort_desc: bool,

    // WAD data
    pub wads: Vec<WadRecord>,
    pub stats_map: HashMap<i64, WadStats>,
    /// Per-WAD `WadAnalysis` keyed by id. Single source of truth for "what
    /// counts as a map for completion": the hero counter, grid progress bar,
    /// and Map Stats dialog all pull from here.
    ///
    /// Stale rows (`version < ANALYSIS_VERSION`) are filtered out by
    /// `db::get_analyses_batch` and refreshed in the background by the
    /// re-analysis worker so the UI converges on the same set the auto-
    /// completion verdict uses.
    pub analyses_map: HashMap<i64, WadAnalysis>,

    /// Which WAD is chosen. The only record of it: where that is in the list is worked out
    /// when it is wanted, since a second copy is how Escape once left a card highlighted with
    /// nothing selected, and every key that acts on the selection doing nothing.
    pub selected_wad_id: Option<i64>,
    /// Nothing is selected because that was asked for, so a reload should not pick something.
    pub selection_dismissed: bool,
    /// The selection moved somewhere that may be off the screen, and the next frame should
    /// bring it into view. Not set by a click, which is already looking at it.
    pub reveal_selection: bool,
    /// The query the list was last loaded with, to tell a selection that went because the
    /// list is now a different list from one that went because it was deleted.
    pub loaded_query: Option<String>,

    // UI flags
    pub view_layout: ViewLayout,
    pub notification: Option<Notification>,
    pub needs_reload: bool,

    // Dialogs & play
    pub active_dialog: Option<ActiveDialog>,
    pub play_state: PlayState,
    pub db_path: PathBuf,

    // Keyboard state (for gg detection)
    pub last_g_press: Option<Instant>,

    // Import
    pub import: ImportState,

    // Sidebar status counts (status → count)
    pub status_counts: HashMap<String, usize>,
    pub total_wad_count: usize,

    // Sidebar collections (cached for display)
    pub sidebar_collections: Vec<CollectionRecord>,
    /// Name of the currently active collection (acts like a playlist selection).
    pub active_collection: Option<String>,

    /// Magazine-style Cacowards view state. Independent of `wads`/library
    /// state so switching views doesn't churn either side's cache.
    pub cacowards: CacowardsState,

    /// Library statistics view state.
    pub stats: StatsState,

    /// WADs that just got a file, to be offered to a mirror once the user is free to
    /// answer. A queue rather than a dialog opened on the spot, because linking a file
    /// launches the WAD straight after, and the question should wait for the game to
    /// close, not sit behind it or be replaced by what the launch asks.
    pub mirror_offers: VecDeque<i64>,

    /// Titles of the WADs being uploaded to a mirror right now, for the status bar.
    pub uploads: Vec<String>,

    /// A bulk upload in progress. Here rather than in the mirrors dialog, because it
    /// outlives the dialog being closed.
    pub mirror_bulk: Option<MirrorBulk>,

    /// What the last read of every mirror's index found, for the mirrors dialog to say.
    /// `None` until the first one, at startup, has finished.
    pub mirror_report: Option<caco_sources::mirror::SyncReport>,
    /// What the last pass of the sync worker that had anything to say found.
    pub sync_report: Option<caco_core::sync::SyncReport>,

    /// Files being fetched by the user in their browser, and what has landed for them.
    pub gets: Gets,
}

/// How far through a WAD's maps a playthrough has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapProgress {
    pub finished: usize,
    pub required: usize,
    pub secret_finished: usize,
    pub secret_total: usize,
}

impl MapProgress {
    /// How full a bar should be. Tracks required maps only.
    pub fn fraction(&self) -> f32 {
        (self.finished as f32 / self.required.max(1) as f32).clamp(0.0, 1.0)
    }

    /// "3 of 8 maps, 1 of 2 secret"
    pub fn summary(&self) -> String {
        let mut text = format!("{} of {} maps", self.finished, self.required);
        if self.secret_total > 0 {
            text.push_str(&format!(
                ", {} of {} secret",
                self.secret_finished, self.secret_total
            ));
        }
        text
    }
}

/// Where a bulk upload to a mirror has got to.
pub struct MirrorBulk {
    pub mirror: String,
    pub done: usize,
    pub total: usize,
    /// The WAD going up now.
    pub title: String,
    /// Shared with the worker, which stops between WADs when this is set.
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl MirrorBulk {
    pub fn stop(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Backing data for [`ViewMode::Stats`].
///
/// The snapshot is one aggregate query over every session, so it is pulled on
/// entry to the view rather than per frame, and re-pulled when something that
/// feeds it changes — a finished session, an import, a delete.
#[derive(Default)]
pub struct StatsState {
    pub snapshot: Option<caco_core::db::sessions::StatsSnapshot>,
    pub needs_reload: bool,
}

// ---------------------------------------------------------------------------
// Cacowards view state
// ---------------------------------------------------------------------------

/// Backs the magazine-style `ViewMode::Cacowards` central panel.
///
/// Strategy: load every cacoward row + its effective status into memory up
/// front (the table is tiny — under a thousand rows across all the years of
/// awards). Year navigation and category sectioning are then pure filters
/// over [`all_entries`], so flicking between years stays render-only with
/// no DB hits.
pub struct CacowardsState {
    /// All Cacoward entries joined with their linked-WAD effective status.
    /// Sorted year DESC, then canonical category order, then page order.
    pub all_entries: Vec<(CacowardRecord, EffectiveStatus)>,

    /// Library WAD records for every entry with a linked `wad_id`. Loaded
    /// once per reload so card rendering can pull thumbnail hints
    /// (source type / URL / cached path) without per-frame DB lookups.
    pub linked_wads: HashMap<i64, WadRecord>,

    /// Doomworld's page for each year, where the write-ups are. From the dataset
    /// rather than the table, which holds entries.
    pub pages: std::collections::BTreeMap<i64, String>,

    /// Currently focused year. `None` only when the table is empty, which takes
    /// a dataset that could not be loaded on a library that has never had one.
    pub selected_year: Option<i64>,

    /// Currently selected cacoward entry (by DB pk), used to draw the
    /// highlight border on the magazine card grid. `None` means nothing
    /// is selected — pressing Esc clears the selection.
    pub selected_entry_pk: Option<i64>,

    /// Set true when the underlying table may have changed (initial load,
    /// after import). The app's update loop drains this flag
    /// and re-runs `db::search_cacowards`.
    pub needs_reload: bool,

    /// A list to go through, or posters to look at.
    pub layout: ViewLayout,
}

impl Default for CacowardsState {
    fn default() -> Self {
        Self {
            all_entries: Vec::new(),
            linked_wads: HashMap::new(),
            pages: std::collections::BTreeMap::new(),
            selected_year: None,
            selected_entry_pk: None,
            // Eager-load on first view-mode entry so the magazine has
            // something to render the moment the user clicks the nav item.
            needs_reload: true,
            layout: ViewLayout::List,
        }
    }
}

impl CacowardsState {
    /// Distinct years present in the loaded entries, newest first.
    pub fn years(&self) -> Vec<i64> {
        let mut years: Vec<i64> = self.all_entries.iter().map(|(r, _)| r.year).collect();
        years.dedup();
        years
    }

    /// (total, completed) for `year` — the per-year completion ratio shown
    /// in the year strip. "Completed" means any linked-WAD status of
    /// `completed`; absent and in-progress don't count. Unsupported
    /// entries are excluded from both numerator and denominator so the
    /// year reads "x of N playable entries completed", and so is anything
    /// outside the core categories: see [`caco_core::db::CORE_CATEGORIES`].
    pub fn year_summary(&self, year: i64) -> (usize, usize) {
        let mut total = 0;
        let mut done = 0;
        for (record, status) in &self.all_entries {
            if record.year != year
                || !record.supported
                || !caco_core::db::is_core_category(&record.category)
            {
                continue;
            }
            total += 1;
            if matches!(
                status,
                EffectiveStatus::Library(caco_core::db::Status::Completed)
            ) {
                done += 1;
            }
        }
        (total, done)
    }
}

fn layout_from(saved: &str) -> ViewLayout {
    match saved {
        "grid" => ViewLayout::Grid,
        _ => ViewLayout::List,
    }
}

fn layout_name(layout: ViewLayout) -> String {
    match layout {
        ViewLayout::List => "list".to_string(),
        ViewLayout::Grid => "grid".to_string(),
    }
}

impl AppState {
    pub fn new(db_path: PathBuf) -> Self {
        let persisted = persist::load();
        let view_layout = layout_from(&persisted.view_layout);
        let cacowards_layout = layout_from(&persisted.cacowards_layout);
        let sort_field_index = persisted
            .sort_field
            .as_deref()
            .and_then(|field| SORT_FIELDS.iter().position(|(key, _)| *key == field))
            .unwrap_or(0);

        Self {
            view_mode: ViewMode::default(),
            view_layout,
            status_filters: persisted.status_filters.into_iter().collect(),
            filter: FilterQuery::new(),
            sort_field_index,
            sort_desc: persisted.sort_desc,
            wads: Vec::new(),
            stats_map: HashMap::new(),
            analyses_map: HashMap::new(),
            selected_wad_id: None,
            selection_dismissed: false,
            reveal_selection: false,
            loaded_query: None,
            notification: None,
            needs_reload: true,
            active_dialog: None,
            play_state: PlayState::Idle,
            db_path,
            import: ImportState::default(),
            last_g_press: None,
            status_counts: HashMap::new(),
            total_wad_count: 0,
            sidebar_collections: Vec::new(),
            active_collection: None,
            cacowards: CacowardsState {
                layout: cacowards_layout,
                ..CacowardsState::default()
            },
            stats: StatsState::default(),
            mirror_offers: VecDeque::new(),
            uploads: Vec::new(),
            mirror_bulk: None,
            mirror_report: None,
            sync_report: None,
            gets: Gets::default(),
        }
    }

    /// Open the next waiting mirror offer, if the user is free to answer one.
    pub fn open_next_mirror_offer(&mut self, conn: &Connection) {
        if self.has_dialog() || self.is_playing() {
            return;
        }
        while let Some(wad_id) = self.mirror_offers.pop_front() {
            if let Some(dialog) = MirrorOfferDialogState::new(conn, wad_id) {
                self.active_dialog = Some(ActiveDialog::MirrorOffer(dialog));
                return;
            }
        }
    }

    /// Returns true if a dialog is open (used to suppress keyboard shortcuts).
    pub fn has_dialog(&self) -> bool {
        self.active_dialog.is_some()
    }

    /// Returns true if a WAD is currently being played.
    pub fn is_playing(&self) -> bool {
        matches!(self.play_state, PlayState::Playing { .. })
    }

    /// Mark every projection derived from WAD rows or play history stale.
    ///
    /// Library, Cacowards and Stats deliberately keep independent snapshots
    /// so switching views is cheap. The corresponding cost is that a write
    /// must invalidate all three: linked Cacoward cards embed `WadRecord`s,
    /// while Stats aggregates statuses, sessions and completions.
    pub fn invalidate_wad_views(&mut self) {
        self.needs_reload = true;
        self.cacowards.needs_reload = true;
        self.stats.needs_reload = true;
    }

    /// How far through a WAD's maps it has got, by `analyses_map`, the same list of what
    /// counts as a map that the verdict on whether it is finished goes by. `None` without a
    /// fresh analysis, rather than a misleading number from the stats alone, which only
    /// ever list maps that have been played.
    pub fn map_progress(&self, wad: &WadRecord) -> Option<MapProgress> {
        use caco_core::wad_analysis::MapClassification;

        let analysis = self.analyses_map.get(&wad.id)?;
        if analysis.required_maps == 0 {
            return None;
        }
        let stats: caco_core::wad_stats::WadStats =
            serde_json::from_str(wad.stats_snapshot.as_deref()?).ok()?;
        let exited: HashSet<&str> = stats
            .maps
            .iter()
            .filter(|map| map.total_exits >= 1)
            .map(|map| map.lump.as_str())
            .collect();
        let finished = analysis
            .maps
            .iter()
            .filter(|map| map.classification == MapClassification::Required)
            .filter(|map| exited.contains(map.lump.as_str()))
            .count();
        let secret_finished = analysis
            .secret_maps
            .iter()
            .filter(|lump| exited.contains(lump.as_str()))
            .count();
        Some(MapProgress {
            finished,
            required: analysis.required_maps,
            secret_finished,
            secret_total: analysis.secret_maps.len(),
        })
    }

    /// Refresh sidebar status counts from the database.
    pub fn refresh_status_counts(&mut self, conn: &Connection) {
        self.status_counts.clear();
        self.total_wad_count = 0;
        if let Ok(counts) = caco_core::db::get_status_counts(conn) {
            for (status, count) in counts {
                let count = count as usize;
                self.total_wad_count += count;
                self.status_counts.insert(status, count);
            }
        }
    }

    /// Exit the currently active collection: drop both the collection
    /// state and the query that was loaded from it. Library/Import nav
    /// items use this so navigating away from a collection cleanly
    /// undoes its filter rather than leaving a stale collection scope
    /// applied with no visual indicator.
    ///
    /// Returns `true` if a collection was actually cleared, so callers
    /// can decide whether a reload is warranted.
    pub fn clear_active_collection(&mut self) -> bool {
        if self.active_collection.is_some() {
            self.active_collection = None;
            self.filter.set_both(String::new());
            true
        } else {
            false
        }
    }

    /// Apply a saved collection to the library's active query and sort.
    /// Returns false when the named collection no longer exists.
    pub fn activate_collection(&mut self, name: &str) -> bool {
        let Some(collection) = self
            .sidebar_collections
            .iter()
            .find(|collection| collection.name == name)
            .cloned()
        else {
            return false;
        };

        self.active_collection = Some(collection.name);
        self.filter.set_both(collection.query);
        if let Some(sort_by) = collection.sort_by
            && let Some(idx) = SORT_FIELDS
                .iter()
                .position(|(key, _)| *key == sort_by.as_str())
        {
            self.sort_field_index = idx;
            self.sort_desc = collection.sort_desc;
        }
        true
    }

    /// Refresh the sidebar collections list from the database.
    pub fn refresh_collections(&mut self, conn: &Connection) {
        self.sidebar_collections = collections::get_all_collections(conn).unwrap_or_default();
        let Some(name) = self.active_collection.clone() else {
            return;
        };

        // The manager can edit or delete the collection currently scoping the
        // library. Keep that live scope in lockstep with the refreshed row;
        // merely reloading the WAD query would otherwise reuse the old text.
        if !self.activate_collection(&name) {
            self.active_collection = None;
            self.filter.set_both(String::new());
        }
    }

    /// Get count for a unified status (for sidebar display).
    pub fn status_count(&self, status: Option<&str>) -> usize {
        match status {
            None => self.total_wad_count,
            Some(s) => self.status_counts.get(s).copied().unwrap_or(0),
        }
    }

    /// Reload WAD list and stats from the database.
    pub fn reload(&mut self, conn: &Connection) {
        // Build combined query from status filters + user filter. Both
        // sides can contain OR-groups (multi-select status pills, or a
        // collection query that uses `,`), so we compose via
        // `compose_and` which distributes the Cartesian product — beets
        // grammar has no parentheses, and a naive space-join would
        // leave trailing OR clauses unbound and leak to the full
        // library (see compose_and docs).
        let status_q: String = self
            .status_filters
            .iter()
            .filter_map(|s| s.parse::<caco_core::db::Status>().ok())
            .map(crate::theme::status_query)
            .collect::<Vec<_>>()
            .join(" , ");
        let user_q = self.filter.applied.as_str();

        let combined = caco_core::db::compose_and(&status_q, user_q);
        let query = if combined.is_empty() {
            None
        } else {
            Some(combined)
        };

        let sort_field = SORT_FIELDS[self.sort_field_index].0;

        match caco_core::db::search_wads(
            conn,
            query.as_deref(),
            Some(sort_field),
            self.sort_desc,
            false,
            0,
        ) {
            Ok(wads) => {
                // Batch fetch stats
                let ids: Vec<i64> = wads.iter().map(|w| w.id).collect();
                self.stats_map =
                    caco_core::db::sessions::get_wad_stats_batch(conn, &ids).unwrap_or_default();
                self.analyses_map =
                    caco_core::db::get_analyses_batch(conn, &ids).unwrap_or_default();

                let same_list = self.loaded_query == query;
                self.selected_wad_id = self.selection_after_reload(&wads, same_list);
                self.loaded_query = query;
                self.wads = wads;
            }
            Err(e) => {
                self.notification = Some(Notification::error(format!("Query failed: {e}")));
            }
        }

        // Refresh sidebar counts and collections
        self.refresh_status_counts(conn);
        self.refresh_collections(conn);

        self.needs_reload = false;
    }

    /// Pull the library-wide stats snapshot for [`ViewMode::Stats`].
    pub fn reload_stats(&mut self, conn: &Connection) {
        self.stats.snapshot = caco_core::db::sessions::get_stats_snapshot(conn).ok();
        self.stats.needs_reload = false;
    }

    /// Reload the magazine-style Cacowards view's data. Pulls every entry
    /// plus its effective status in one query, then picks the newest year
    /// as the focused tab if the previous focus is gone.
    pub fn reload_cacowards(&mut self, conn: &Connection) {
        match caco_core::db::cacowards::search_cacowards(conn) {
            Ok(entries) => {
                self.cacowards.all_entries = entries;
            }
            Err(e) => {
                self.notification = Some(Notification::error(format!(
                    "Could not load Cacowards: {e}"
                )));
                self.cacowards.all_entries.clear();
            }
        }
        // Hydrate library-WAD records for every linked entry — the
        // magazine renderer needs source_type / cached_path / source_url
        // to ask the thumbnail manager for a TITLEPIC.
        self.cacowards.linked_wads.clear();
        let wad_ids: Vec<i64> = self
            .cacowards
            .all_entries
            .iter()
            .filter_map(|(r, _)| r.wad_id)
            .collect();
        for id in wad_ids {
            if self.cacowards.linked_wads.contains_key(&id) {
                continue;
            }
            if let Ok(Some(wad)) = caco_core::db::wads::get_wad(conn, id, false) {
                self.cacowards.linked_wads.insert(id, wad);
            }
        }
        // Pick the newest year by default; preserve the user's selection
        // if it still exists in the loaded set.
        let years = self.cacowards.years();
        if let Some(current) = self.cacowards.selected_year
            && !years.contains(&current)
        {
            self.cacowards.selected_year = None;
        }
        if self.cacowards.selected_year.is_none() {
            self.cacowards.selected_year = years.first().copied();
        }
        // Drop a stale selection if the entry vanished.
        if let Some(pk) = self.cacowards.selected_entry_pk
            && !self.cacowards.all_entries.iter().any(|(r, _)| r.id == pk)
        {
            self.cacowards.selected_entry_pk = None;
        }
        self.cacowards.needs_reload = false;
    }

    /// Check if filter debounce has elapsed and apply if so.
    pub fn check_filter_debounce(&mut self, ctx: &egui::Context, conn: &Connection) {
        match self.filter.poll(Instant::now()) {
            FilterCheck::Idle => {}
            FilterCheck::Pending { remaining } => ctx.request_repaint_after(remaining),
            FilterCheck::Apply => self.reload(conn),
        }
    }

    /// What should be selected once `wads` replaces the list.
    ///
    /// What was selected, if it is still there. Then, if the same list lost it, which means it
    /// was deleted or filtered out by a status it no longer has, whatever is now in its place:
    /// the next thing to look at, rather than a jump to the top. A different list starts at
    /// the top, as does a list nothing was selected in, unless that was asked for.
    fn selection_after_reload(&self, wads: &[WadRecord], same_list: bool) -> Option<i64> {
        if let Some(id) = self.selected_wad_id
            && wads.iter().any(|wad| wad.id == id)
        {
            return Some(id);
        }
        if self.selection_dismissed && self.selected_wad_id.is_none() {
            return None;
        }
        let place = self
            .selected_index()
            .filter(|_| same_list)
            .unwrap_or(0)
            .min(wads.len().saturating_sub(1));
        wads.get(place).map(|wad| wad.id)
    }

    /// Where the selected WAD is in the list.
    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected_wad_id?;
        self.wads.iter().position(|wad| wad.id == id)
    }

    /// Select what was clicked on.
    pub fn select_wad(&mut self, wad_id: i64) {
        self.selected_wad_id = Some(wad_id);
        self.selection_dismissed = false;
    }

    /// Nothing selected, and it stays that way until something is asked for.
    pub fn deselect(&mut self) {
        self.selected_wad_id = None;
        self.selection_dismissed = true;
    }

    /// Select the WAD at `index` and bring it into view.
    fn select_index(&mut self, index: usize) {
        if let Some(wad) = self.wads.get(index) {
            self.selected_wad_id = Some(wad.id);
            self.selection_dismissed = false;
            self.reveal_selection = true;
        }
    }

    /// Move the selection by `step` places, stopping at the ends. With nothing selected, any
    /// move starts from the first.
    pub fn move_selection(&mut self, step: isize) {
        let Some(from) = self.selected_index() else {
            self.select_first();
            return;
        };
        let last = self.wads.len().saturating_sub(1);
        self.select_index(from.saturating_add_signed(step).min(last));
    }

    /// Jump selection to the first WAD.
    pub fn select_first(&mut self) {
        self.select_index(0);
    }

    /// Jump selection to the last WAD.
    pub fn select_last(&mut self) {
        self.select_index(self.wads.len().saturating_sub(1));
    }

    /// Handle a 'g' keypress for vim-style gg (jump to top).
    /// Returns true if gg was triggered.
    pub fn handle_g_press(&mut self) -> bool {
        if self
            .last_g_press
            .is_some_and(|t| t.elapsed().as_millis() < 500)
        {
            self.select_first();
            self.last_g_press = None;
            true
        } else {
            self.last_g_press = Some(Instant::now());
            false
        }
    }

    /// Move up a row of `columns`, staying put in the top one.
    pub fn select_up(&mut self, columns: usize) {
        match self.selected_index() {
            Some(from) if from >= columns => self.select_index(from - columns),
            Some(_) => {}
            None => self.select_first(),
        }
    }

    /// Move down a row of `columns`. A last row that stops short of this column still counts,
    /// or the last few WADs could only be reached from the column they happen to be in.
    pub fn select_down(&mut self, columns: usize) {
        let Some(from) = self.selected_index() else {
            self.select_first();
            return;
        };
        let last = self.wads.len().saturating_sub(1);
        if from + columns <= last {
            self.select_index(from + columns);
        } else if from / columns < last / columns {
            self.select_index(last);
        }
    }

    /// Go to a view, the way its entry in the sidebar does. Leaving the library leaves a
    /// collection too, and each view asks for what it shows to be read again, because a
    /// figure that predates the session you just finished is worse than a brief wait.
    pub fn show_view(&mut self, mode: ViewMode) {
        let library = mode == ViewMode::Library;
        if self.clear_active_collection() || (library && self.wads.is_empty()) {
            self.needs_reload = true;
        }
        self.view_mode = mode;
        match mode {
            ViewMode::Cacowards => self.cacowards.needs_reload = true,
            ViewMode::Stats => self.stats.needs_reload = true,
            ViewMode::Library | ViewMode::Import => {}
        }
    }

    /// Produce a snapshot of persistent GUI state for saving.
    pub fn to_persisted(&self) -> persist::GuiState {
        persist::GuiState {
            view_layout: layout_name(self.view_layout),
            cacowards_layout: layout_name(self.cacowards.layout),
            sort_field: Some(SORT_FIELDS[self.sort_field_index].0.to_string()),
            sort_field_index: None,
            sort_desc: self.sort_desc,
            status_filters: self.status_filters.iter().cloned().collect(),
            status_filter: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Connection, AppState) {
        let conn = caco_core::db::open_memory().unwrap();
        caco_core::db::init_db(&conn).unwrap();
        let state = AppState::new(PathBuf::from("test-library.db"));
        (conn, state)
    }

    /// Five WADs, first selected, read back as they would be from the database.
    fn five_wads() -> (Connection, AppState) {
        let (conn, mut state) = setup();
        for title in ["A", "B", "C", "D", "E"] {
            caco_core::db::wads::add_wad(
                &conn,
                &caco_core::db::wads::NewWad::new(title, caco_core::db::models::SourceType::Local),
            )
            .unwrap();
        }
        // `AppState::new` starts from whatever this machine last had on the screen.
        state.status_filters.clear();
        state.sort_desc = true;
        state.sort_field_index = SORT_FIELDS.iter().position(|(k, _)| *k == "title").unwrap();
        state.reload(&conn);
        (conn, state)
    }

    fn selected_title(state: &AppState) -> Option<&str> {
        let id = state.selected_wad_id?;
        state
            .wads
            .iter()
            .find(|wad| wad.id == id)
            .map(|wad| wad.title.as_str())
    }

    #[test]
    fn the_first_load_selects_the_first_wad() {
        let (_conn, state) = five_wads();
        assert_eq!(selected_title(&state), Some("A"));
    }

    /// The bug this replaced: the highlight came from one field and the keys went by another,
    /// so after Escape a card still looked selected and Enter did nothing.
    #[test]
    fn nothing_is_selected_after_it_was_dismissed_until_something_is_asked_for() {
        let (conn, mut state) = five_wads();
        state.deselect();
        assert_eq!(state.selected_index(), None);

        // A sync finishing does this, and it should not put the selection back.
        state.reload(&conn);
        assert_eq!(state.selected_wad_id, None);

        // Any move starts again from the first.
        state.move_selection(1);
        assert_eq!(selected_title(&state), Some("A"));
        assert!(!state.selection_dismissed);
    }

    #[test]
    fn a_deleted_wad_leaves_the_selection_on_whatever_took_its_place() {
        let (conn, mut state) = five_wads();
        state.move_selection(2);
        assert_eq!(selected_title(&state), Some("C"));
        let id = state.selected_wad_id.unwrap();
        caco_core::db::wads::delete_wad(&conn, id, false).unwrap();

        state.reload(&conn);

        assert_eq!(selected_title(&state), Some("D"));
    }

    #[test]
    fn deleting_the_last_wad_selects_the_new_last_one() {
        let (conn, mut state) = five_wads();
        state.select_last();
        let id = state.selected_wad_id.unwrap();
        caco_core::db::wads::delete_wad(&conn, id, false).unwrap();

        state.reload(&conn);

        assert_eq!(selected_title(&state), Some("D"));
    }

    #[test]
    fn a_different_list_starts_at_the_top() {
        let (conn, mut state) = five_wads();
        state.select_last();
        state.filter.set_both("title:B , title:C".to_string());

        state.reload(&conn);

        assert_eq!(selected_title(&state), Some("B"));
    }

    #[test]
    fn moving_in_a_list_stops_at_the_ends() {
        let (_conn, mut state) = five_wads();
        state.move_selection(-1);
        assert_eq!(selected_title(&state), Some("A"));
        state.move_selection(3);
        assert_eq!(selected_title(&state), Some("D"));
        state.move_selection(100);
        assert_eq!(selected_title(&state), Some("E"));
        state.move_selection(-100);
        assert_eq!(selected_title(&state), Some("A"));
    }

    #[test]
    fn moving_with_the_keyboard_brings_the_selection_into_view_and_clicking_does_not() {
        let (_conn, mut state) = five_wads();
        state.reveal_selection = false;
        let second = state.wads[1].id;

        state.select_wad(second);
        assert!(!state.reveal_selection);

        state.move_selection(1);
        assert!(state.reveal_selection);
    }

    /// A grid of two columns: A B / C D / E.
    #[test]
    fn moving_down_a_grid_reaches_a_last_row_that_stops_short() {
        let (_conn, mut state) = five_wads();
        state.select_down(2);
        assert_eq!(selected_title(&state), Some("C"));
        // From D there is nothing directly below, but E is still the next row down.
        state.move_selection(1);
        assert_eq!(selected_title(&state), Some("D"));
        state.select_down(2);
        assert_eq!(selected_title(&state), Some("E"));
        // And from the last row there is nowhere to go.
        state.select_down(2);
        assert_eq!(selected_title(&state), Some("E"));
        state.select_up(2);
        assert_eq!(selected_title(&state), Some("C"));
        state.select_up(2);
        assert_eq!(selected_title(&state), Some("A"));
        state.select_up(2);
        assert_eq!(selected_title(&state), Some("A"));
    }

    #[test]
    fn progress_reads_as_a_person_would_say_it() {
        let progress = MapProgress {
            finished: 3,
            required: 8,
            secret_finished: 0,
            secret_total: 0,
        };
        assert_eq!(progress.summary(), "3 of 8 maps");
        assert_eq!(progress.fraction(), 3.0 / 8.0);

        let with_secrets = MapProgress {
            secret_finished: 1,
            secret_total: 2,
            ..progress
        };
        assert_eq!(with_secrets.summary(), "3 of 8 maps, 1 of 2 secret");
    }

    #[test]
    fn wad_mutation_invalidates_every_dependent_view() {
        let (_, mut state) = setup();
        state.needs_reload = false;
        state.cacowards.needs_reload = false;
        state.stats.needs_reload = false;

        state.invalidate_wad_views();

        assert!(state.needs_reload);
        assert!(state.cacowards.needs_reload);
        assert!(state.stats.needs_reload);
    }

    #[test]
    fn a_years_total_counts_the_core_categories_that_can_be_played() {
        let (conn, mut state) = setup();
        let wad = caco_core::db::wads::add_wad(
            &conn,
            &caco_core::db::wads::NewWad::new("Done", caco_core::db::models::SourceType::Local),
        )
        .unwrap();
        conn.execute("UPDATE wads SET status = 'completed' WHERE id = ?1", [wad])
            .unwrap();
        let add = |title: &str, category: &str, supported: bool, linked: bool| {
            let id = caco_core::db::upsert_cacoward(
                &conn,
                &caco_core::db::NewCacoward {
                    key: format!("2025-{category}-{title}"),
                    year: 2025,
                    category: category.to_string(),
                    page_order: 1,
                    wad_title: title.to_string(),
                    supported,
                    ..Default::default()
                },
            )
            .unwrap();
            if linked {
                caco_core::db::link_wad(&conn, id, wad, false).unwrap();
            }
        };
        add("beaten", "winner", true, true);
        add("not yet", "runner-up", true, false);
        add("cannot run", "mordeth", false, false);
        add("beaten extra", "multiplayer", true, true);
        add("unbeaten extra", "doom-moma", true, false);

        state.reload_cacowards(&conn);

        assert_eq!(state.cacowards.year_summary(2025), (2, 1));
    }

    #[test]
    fn refreshing_collections_updates_the_active_scope() {
        let (conn, mut state) = setup();
        collections::create_collection(&conn, "active", "status:queued", Some("title"), true)
            .unwrap();
        state.active_collection = Some("active".to_string());
        state.filter.set_both("status:old".to_string());

        collections::update_collection(
            &conn,
            "active",
            Some("status:playing"),
            Some(Some("year")),
            Some(false),
        )
        .unwrap();
        state.refresh_collections(&conn);

        assert_eq!(state.filter.input, "status:playing");
        assert_eq!(state.filter.applied, "status:playing");
        assert_eq!(SORT_FIELDS[state.sort_field_index].0, "year");
        assert!(!state.sort_desc);
    }

    #[test]
    fn refreshing_collections_clears_a_deleted_active_scope() {
        let (conn, mut state) = setup();
        collections::create_collection(&conn, "active", "status:queued", None, true).unwrap();
        state.active_collection = Some("active".to_string());
        state.filter.set_both("status:queued".to_string());
        collections::delete_collection(&conn, "active").unwrap();

        state.refresh_collections(&conn);

        assert!(state.active_collection.is_none());
        assert!(state.filter.input.is_empty());
        assert!(state.filter.applied.is_empty());
    }
}
