use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use caco_core::db::models::WadRecord;
use caco_core::db::sessions::WadStats;
use rusqlite::Connection;

use crate::dialogs::cache::CacheDialogState;
use crate::dialogs::delete::DeleteDialogState;
use crate::dialogs::edit::EditDialogState;
use crate::dialogs::link::LinkDialogState;
use crate::dialogs::resources::ResourcesDialogState;
use crate::dialogs::sessions::SessionsDialogState;
use crate::dialogs::stats::StatsDialogState;
use crate::dialogs::wad_stats::WadStatsDialogState;
use crate::import::state::ImportState;
use crate::message::Notification;
use crate::persist::{self, SavedSearch};

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
    Edit(i64),
    Delete(i64),
    Sessions(i64),
    MapStats(i64),
    BeatenAdd(i64),
    BeatenRemove(i64),
    Stats,
    Cache,
    Resources,
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
// Tab definitions
// ---------------------------------------------------------------------------

pub struct TabDef {
    pub label: &'static str,
    pub query_filter: Option<&'static str>,
}

pub const TABS: &[TabDef] = &[
    TabDef { label: "All",     query_filter: None },
    TabDef { label: "Inbox",   query_filter: Some("intent:inbox") },
    TabDef { label: "Queued",  query_filter: Some("intent:queued") },
    TabDef { label: "Playing", query_filter: Some("play:started") },
    TabDef { label: "Shelved", query_filter: Some("intent:shelved") },
    TabDef { label: "Dropped", query_filter: Some("intent:dropped") },
];

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
];

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    // View mode
    pub view_mode: ViewMode,

    // Tab
    pub active_tab: usize,

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
    pub show_detail_panel: bool,
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

    // Saved searches
    pub saved_searches: Vec<SavedSearch>,
    pub save_search_pending: bool,
    pub save_search_name: String,

    // Sidebar status counts (total library, not filtered)
    pub status_counts: HashMap<String, usize>,
    pub total_wad_count: usize,

    // Hidden sidebar status filter tabs (indices into TABS)
    pub hidden_tabs: std::collections::HashSet<usize>,
}

impl AppState {
    pub fn new(db_path: PathBuf) -> Self {
        let persisted = persist::load();
        let view_layout = match persisted.view_layout.as_str() {
            "grid" => ViewLayout::Grid,
            _ => ViewLayout::List,
        };
        let active_tab = if persisted.active_tab < TABS.len() {
            persisted.active_tab
        } else {
            0
        };
        let sort_field_index = if persisted.sort_field_index < SORT_FIELDS.len() {
            persisted.sort_field_index
        } else {
            0
        };

        Self {
            view_mode: ViewMode::default(),
            view_layout,
            active_tab,
            filter_text: String::new(),
            filter_changed_at: None,
            applied_filter: String::new(),
            sort_field_index,
            sort_desc: persisted.sort_desc,
            wads: Vec::new(),
            stats_map: HashMap::new(),
            selected_wad_id: None,
            selected_row: 0,
            show_detail_panel: persisted.show_detail_panel,
            notification: None,
            needs_reload: true,
            active_dialog: None,
            play_state: PlayState::Idle,
            db_path,
            import: ImportState::default(),
            last_g_press: None,
            saved_searches: persisted.saved_searches,
            save_search_pending: false,
            save_search_name: String::new(),
            status_counts: HashMap::new(),
            total_wad_count: 0,
            hidden_tabs: persisted.hidden_tabs.iter().copied().collect(),
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

    /// Refresh sidebar tab counts from the database.
    pub fn refresh_tab_counts(&mut self, conn: &Connection) {
        self.status_counts.clear();
        self.total_wad_count = 0;
        if let Ok(all_wads) =
            caco_core::db::search_wads(conn, None, Some("id"), false, false, 0)
        {
            self.total_wad_count = all_wads.len();
            // Count by intent and play_state for the new tab queries
            for wad in &all_wads {
                *self.status_counts.entry(format!("intent:{}", wad.intent)).or_insert(0) += 1;
                *self.status_counts.entry(format!("play:{}", wad.play_state)).or_insert(0) += 1;
                // Keep legacy status counts for any remaining usage
                *self.status_counts.entry(wad.status.clone()).or_insert(0) += 1;
            }
        }
    }

    /// Get count for a tab's query filter (for sidebar display).
    pub fn tab_count(&self, query_filter: Option<&str>) -> usize {
        match query_filter {
            None => self.total_wad_count,
            Some(q) => self.status_counts.get(q).copied().unwrap_or(0),
        }
    }

    /// Reload WAD list and stats from the database.
    pub fn reload(&mut self, conn: &Connection) {
        // Build combined query from tab filter + user filter
        let tab = &TABS[self.active_tab];
        let mut query_parts: Vec<String> = Vec::new();

        if let Some(qf) = tab.query_filter {
            query_parts.push(qf.to_string());
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

        // Refresh sidebar counts
        self.refresh_tab_counts(conn);

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
            show_detail_panel: self.show_detail_panel,
            sort_field_index: self.sort_field_index,
            sort_desc: self.sort_desc,
            active_tab: self.active_tab,
            saved_searches: self.saved_searches.clone(),
            hidden_tabs: self.hidden_tabs.iter().copied().collect(),
        }
    }
}
