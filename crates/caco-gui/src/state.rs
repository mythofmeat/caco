use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use caco_core::db::models::WadRecord;
use caco_core::db::sessions::WadStats;
use rusqlite::Connection;

use crate::dialogs::cache::CacheDialogState;
use crate::dialogs::delete::DeleteDialogState;
use crate::dialogs::edit::EditDialogState;
use crate::dialogs::resources::ResourcesDialogState;
use crate::dialogs::sessions::SessionsDialogState;
use crate::dialogs::stats::StatsDialogState;
use crate::dialogs::wad_stats::WadStatsDialogState;
use crate::import::state::ImportState;
use crate::message::Notification;

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
    #[default]
    List,
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
    pub status_filter: Option<&'static [&'static str]>,
}

pub const TABS: &[TabDef] = &[
    TabDef { label: "All", status_filter: None },
    TabDef { label: "Playing", status_filter: Some(&["playing"]) },
    TabDef { label: "To Play", status_filter: Some(&["to-play"]) },
    TabDef { label: "Finished", status_filter: Some(&["finished"]) },
    TabDef { label: "Backlog", status_filter: Some(&["backlog"]) },
    TabDef { label: "Other", status_filter: Some(&["abandoned", "awaiting-update"]) },
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

    // Import
    pub import: ImportState,
}

impl AppState {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            view_mode: ViewMode::default(),
            view_layout: ViewLayout::default(),
            active_tab: 0,
            filter_text: String::new(),
            filter_changed_at: None,
            applied_filter: String::new(),
            sort_field_index: 0,
            sort_desc: true,
            wads: Vec::new(),
            stats_map: HashMap::new(),
            selected_wad_id: None,
            selected_row: 0,
            show_detail_panel: true,
            notification: None,
            needs_reload: true,
            active_dialog: None,
            play_state: PlayState::Idle,
            db_path,
            import: ImportState::default(),
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

    /// Reload WAD list and stats from the database.
    pub fn reload(&mut self, conn: &Connection) {
        // Build combined query from tab filter + user filter
        let tab = &TABS[self.active_tab];
        let mut query_parts: Vec<String> = Vec::new();

        if let Some(statuses) = tab.status_filter {
            if statuses.len() == 1 {
                query_parts.push(format!("status:{}", statuses[0]));
            } else {
                // OR query: "status:a , status:b"
                let parts: Vec<String> =
                    statuses.iter().map(|s| format!("status:{s}")).collect();
                query_parts.push(parts.join(" , "));
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
}
