use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use caco_core::db::cacowards::{CacowardRecord, EffectiveStatus};
use caco_core::db::collections::{self, CollectionRecord};
use caco_core::db::models::WadRecord;
use caco_core::db::sessions::WadStats;
use caco_core::wad_analysis::WadAnalysis;
use rusqlite::Connection;

use crate::dialogs::cache::CacheDialogState;
use crate::dialogs::cacoward_link::CacowardLinkDialogState;
use crate::dialogs::collections::CollectionsDialogState;
use crate::dialogs::delete::DeleteDialogState;
use crate::dialogs::edit::EditDialogState;
use crate::dialogs::link::LinkDialogState;
use crate::dialogs::resources::ResourcesDialogState;
use crate::dialogs::sessions::SessionsDialogState;
use crate::dialogs::settings::SettingsDialogState;
use crate::dialogs::stats::StatsDialogState;
use crate::dialogs::wad_stats::WadStatsDialogState;
use crate::filter_query::{FilterCheck, FilterQuery};
use crate::import::state::ImportState;
use crate::message::Notification;
use crate::persist;

// ---------------------------------------------------------------------------
// View mode (Library vs Import)
// ---------------------------------------------------------------------------

#[derive(Default, PartialEq, Eq)]
pub enum ViewMode {
    #[default]
    Library,
    Import,
    /// Magazine-style view over the `cacowards` table — see
    /// [`CacowardsState`] for the data backing it.
    Cacowards,
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
    StartNewPlaythrough(i64),
    Edit(i64),
    Delete(i64),
    Sessions(i64),
    MapStats(i64),
    Stats,
    Cache,
    Settings,
    Resources,
    Collections,
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

    /// Clear the wad_id on a cacoward entry, restoring it to the
    /// auto-linker's reach on the next enrich.
    UnlinkCacoward(i64),

    /// Flip the `supported` flag on a cacoward entry. `true` makes the
    /// entry playable / counted toward completion; `false` parks it as
    /// "not yet supported by caco".
    SetCacowardSupported(i64, bool),
}

// ---------------------------------------------------------------------------
// Dialog / play state
// ---------------------------------------------------------------------------

pub enum ActiveDialog {
    Edit(Box<EditDialogState>),
    Delete(DeleteDialogState),
    Sessions(SessionsDialogState),
    Stats(StatsDialogState),
    Cache(CacheDialogState),
    Collections(CollectionsDialogState),
    Resources(ResourcesDialogState),
    WadStats(WadStatsDialogState),
    Link(LinkDialogState),
    /// Modal picker for choosing a library WAD to link to a Cacoward entry.
    CacowardLink(CacowardLinkDialogState),
    Settings(SettingsDialogState),
    Help,
    About,
}

pub enum PlayState {
    Idle,
    Playing { wad_id: i64, wad_title: String },
}

// ---------------------------------------------------------------------------
// Sort definitions
// ---------------------------------------------------------------------------

pub const SORT_FIELDS: &[(&str, &str)] = &[
    ("id", "ID"),
    ("title", "Title"),
    ("author", "Author"),
    ("playtime", "Playtime"),
    ("last_played", "Last Played"),
    ("year", "Year"),
    ("rating", "Rating"),
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

    // Selection
    pub selected_wad_id: Option<i64>,
    pub selected_row: usize,

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
}

// ---------------------------------------------------------------------------
// Cacowards view state
// ---------------------------------------------------------------------------

/// Backs the magazine-style `ViewMode::Cacowards` central panel.
///
/// Strategy: load every cacoward row + its effective status into memory up
/// front (the table is tiny — low thousands of rows across 30+ years of
/// awards). Year navigation and category sectioning are then pure filters
/// over [`all_entries`], so flicking between years stays render-only with
/// no DB hits.
pub struct CacowardsState {
    /// All Cacoward entries joined with their linked-WAD effective status.
    /// Sorted year DESC, then canonical category order, then rank.
    pub all_entries: Vec<(CacowardRecord, EffectiveStatus)>,

    /// Library WAD records for every entry with a linked `wad_id`. Loaded
    /// once per reload so card rendering can pull thumbnail hints
    /// (source type / URL / cached path) without per-frame DB lookups.
    pub linked_wads: HashMap<i64, WadRecord>,

    /// Currently focused year. `None` only when the table is empty (i.e.
    /// the user hasn't run `caco enrich --cacowards` yet).
    pub selected_year: Option<i64>,

    /// Currently selected cacoward entry (by DB pk), used to draw the
    /// highlight border on the magazine card grid. `None` means nothing
    /// is selected — pressing Esc clears the selection.
    pub selected_entry_pk: Option<i64>,

    /// Set true when the underlying table may have changed (initial load,
    /// after import, after enrich). The app's update loop drains this flag
    /// and re-runs `db::search_cacowards`.
    pub needs_reload: bool,
}

impl Default for CacowardsState {
    fn default() -> Self {
        Self {
            all_entries: Vec::new(),
            linked_wads: HashMap::new(),
            selected_year: None,
            selected_entry_pk: None,
            // Eager-load on first view-mode entry so the magazine has
            // something to render the moment the user clicks the nav item.
            needs_reload: true,
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
    /// year reads "x of N playable entries completed".
    pub fn year_summary(&self, year: i64) -> (usize, usize) {
        let mut total = 0;
        let mut done = 0;
        for (record, status) in &self.all_entries {
            if record.year != year || !record.supported {
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

impl AppState {
    pub fn new(db_path: PathBuf) -> Self {
        let persisted = persist::load();
        let view_layout = match persisted.view_layout.as_str() {
            "grid" => ViewLayout::Grid,
            _ => ViewLayout::List,
        };
        let sort_field_index = if persisted.sort_field_index < SORT_FIELDS.len() {
            persisted.sort_field_index
        } else {
            0
        };

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
            selected_row: 0,
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
            cacowards: CacowardsState::default(),
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

    /// Refresh the sidebar collections list from the database.
    pub fn refresh_collections(&mut self, conn: &Connection) {
        self.sidebar_collections = collections::get_all_collections(conn).unwrap_or_default();
        // Clear active collection if it was deleted
        if let Some(ref name) = self.active_collection
            && !self.sidebar_collections.iter().any(|c| c.name == *name)
        {
            self.active_collection = None;
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

                // Preserve selection if still valid
                if let Some(sel_id) = self.selected_wad_id {
                    if let Some(pos) = wads.iter().position(|w| w.id == sel_id) {
                        self.selected_row = pos;
                    } else {
                        self.selected_wad_id = None;
                        self.selected_row = 0;
                    }
                } else {
                    self.selected_row = 0;
                }

                // Auto-select first if nothing selected
                if self.selected_wad_id.is_none() {
                    self.selected_wad_id = wads.first().map(|w| w.id);
                }

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

    /// Reload the magazine-style Cacowards view's data. Pulls every entry
    /// plus its effective status in one query, then picks the newest year
    /// as the focused tab if the previous focus is gone (e.g. after a
    /// re-enrich removed entries for that year).
    pub fn reload_cacowards(&mut self, conn: &Connection) {
        match caco_core::db::cacowards::search_cacowards(
            conn,
            &caco_core::db::cacowards::CacowardFilters::default(),
        ) {
            Ok(entries) => {
                self.cacowards.all_entries = entries;
            }
            Err(e) => {
                eprintln!("cacowards reload failed: {e}");
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

    /// Move selection up by one row.
    pub fn select_prev(&mut self) {
        if !self.wads.is_empty() && self.selected_row > 0 {
            self.selected_row -= 1;
            self.selected_wad_id = Some(self.wads[self.selected_row].id);
        }
    }

    /// Move selection down by one row.
    pub fn select_next(&mut self) {
        if !self.wads.is_empty() && self.selected_row + 1 < self.wads.len() {
            self.selected_row += 1;
            self.selected_wad_id = Some(self.wads[self.selected_row].id);
        }
    }

    /// Jump selection to the first WAD.
    pub fn select_first(&mut self) {
        if !self.wads.is_empty() {
            self.selected_row = 0;
            self.selected_wad_id = Some(self.wads[0].id);
        }
    }

    /// Jump selection to the last WAD.
    pub fn select_last(&mut self) {
        if !self.wads.is_empty() {
            self.selected_row = self.wads.len() - 1;
            self.selected_wad_id = Some(self.wads[self.selected_row].id);
        }
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

    /// Get the currently selected WAD record, if any.
    pub fn selected_wad(&self) -> Option<&WadRecord> {
        self.selected_wad_id
            .and_then(|id| self.wads.iter().find(|w| w.id == id))
    }

    /// Move selection left by one position (grid mode).
    pub fn select_left(&mut self, columns: usize) {
        if columns == 0 || self.wads.is_empty() {
            return;
        }
        let col = self.selected_row % columns;
        if col > 0 {
            self.selected_row -= 1;
            self.selected_wad_id = Some(self.wads[self.selected_row].id);
        }
    }

    /// Move selection right by one position (grid mode).
    pub fn select_right(&mut self, columns: usize) {
        if columns == 0 || self.wads.is_empty() {
            return;
        }
        let col = self.selected_row % columns;
        if col + 1 < columns && self.selected_row + 1 < self.wads.len() {
            self.selected_row += 1;
            self.selected_wad_id = Some(self.wads[self.selected_row].id);
        }
    }

    /// Move selection up by one row in grid mode.
    pub fn select_up_grid(&mut self, columns: usize) {
        if columns == 0 || self.wads.is_empty() {
            return;
        }
        if self.selected_row >= columns {
            self.selected_row -= columns;
            self.selected_wad_id = Some(self.wads[self.selected_row].id);
        }
    }

    /// Move selection down by one row in grid mode.
    pub fn select_down_grid(&mut self, columns: usize) {
        if columns == 0 || self.wads.is_empty() {
            return;
        }
        let new_row = self.selected_row + columns;
        if new_row < self.wads.len() {
            self.selected_row = new_row;
            self.selected_wad_id = Some(self.wads[self.selected_row].id);
        }
    }

    /// Get stats for the currently selected WAD.
    pub fn selected_stats(&self) -> Option<&WadStats> {
        self.selected_wad_id.and_then(|id| self.stats_map.get(&id))
    }

    /// Produce a snapshot of persistent GUI state for saving.
    pub fn to_persisted(&self) -> persist::GuiState {
        persist::GuiState {
            view_layout: match self.view_layout {
                ViewLayout::List => "list".to_string(),
                ViewLayout::Grid => "grid".to_string(),
            },
            sort_field_index: self.sort_field_index,
            sort_desc: self.sort_desc,
            status_filters: self.status_filters.iter().cloned().collect(),
            status_filter: None,
        }
    }
}
