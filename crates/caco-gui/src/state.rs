use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use caco_core::db::collections::{self, CollectionRecord};
use caco_core::db::models::WadRecord;
use caco_core::db::sessions::WadStats;
use rusqlite::Connection;

use crate::dialogs::cache::CacheDialogState;
use crate::dialogs::collections::CollectionsDialogState;
use crate::dialogs::delete::DeleteDialogState;
use crate::dialogs::edit::EditDialogState;
use crate::dialogs::link::LinkDialogState;
use crate::dialogs::resources::ResourcesDialogState;
use crate::dialogs::sessions::SessionsDialogState;
use crate::dialogs::stats::StatsDialogState;
use crate::dialogs::wad_stats::WadStatsDialogState;
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
    Resources,
    Collections,
    EditCollection(String),
    DeleteCollection(String),
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

    // Filter
    pub filter_text: String,
    pub filter_changed_at: Option<Instant>,
    pub applied_filter: String,

    // Sort
    pub sort_field_index: usize,
    pub sort_desc: bool,

    // WAD data
    pub wads: Vec<WadRecord>,
    pub stats_map: HashMap<i64, WadStats>,

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
            filter_text: String::new(),
            filter_changed_at: None,
            applied_filter: String::new(),
            sort_field_index,
            sort_desc: persisted.sort_desc,
            wads: Vec::new(),
            stats_map: HashMap::new(),
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
        if let Ok(all_wads) =
            caco_core::db::search_wads(conn, None, Some("id"), false, false, 0)
        {
            self.total_wad_count = all_wads.len();
            for wad in &all_wads {
                *self.status_counts.entry(wad.status.clone()).or_insert(0) += 1;
            }
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
        // Build combined query from status filters + user filter
        let mut query_parts: Vec<String> = Vec::new();

        if !self.status_filters.is_empty() {
            // Multiple statuses use OR syntax: "status:a , status:b"
            let status_q: Vec<&str> = self
                .status_filters
                .iter()
                .filter_map(|s| {
                    let q = crate::theme::status_query(s);
                    if q.is_empty() { None } else { Some(q) }
                })
                .collect();
            if !status_q.is_empty() {
                query_parts.push(status_q.join(" , "));
            }
        }

        if !self.applied_filter.is_empty() {
            query_parts.push(self.applied_filter.clone());
        }

        let query = if query_parts.is_empty() {
            None
        } else {
            Some(query_parts.join(" "))
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

    /// Check if filter debounce has elapsed and apply if so.
    pub fn check_filter_debounce(&mut self, ctx: &egui::Context, conn: &Connection) {
        if let Some(changed_at) = self.filter_changed_at {
            let elapsed = changed_at.elapsed();
            if elapsed.as_millis() >= 150 {
                self.filter_changed_at = None;
                if self.applied_filter != self.filter_text {
                    self.applied_filter = self.filter_text.clone();
                    self.reload(conn);
                }
            } else {
                // Schedule a repaint after the remaining debounce time
                let remaining = std::time::Duration::from_millis(150) - elapsed;
                ctx.request_repaint_after(remaining);
            }
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
        self.selected_wad_id
            .and_then(|id| self.stats_map.get(&id))
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
