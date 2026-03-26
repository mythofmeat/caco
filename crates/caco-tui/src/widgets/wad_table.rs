use std::collections::HashMap;
use std::time::Instant;

use caco_core::db::models::WadRecord;
use caco_core::db::query::search_wads;
use caco_core::db::sessions::{WadStats, get_wad_stats_batch};
use caco_core::player::format_duration;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Cell, Row, Table, TableState};
use ratatui::Frame;
use rusqlite::Connection;

use crate::theme;

/// State for the WAD table widget.
pub struct WadTableState {
    pub wads: Vec<WadRecord>,
    pub stats_map: HashMap<i64, WadStats>,
    pub table_state: TableState,
    id_to_index: HashMap<i64, usize>,
    /// For gg (double-g to go to top): tracks when first g was pressed.
    pub g_pressed: Option<Instant>,
}

impl Default for WadTableState {
    fn default() -> Self {
        Self::new()
    }
}

impl WadTableState {
    pub fn new() -> Self {
        Self {
            wads: Vec::new(),
            stats_map: HashMap::new(),
            table_state: TableState::default(),
            id_to_index: HashMap::new(),
            g_pressed: None,
        }
    }

    /// Load WADs from the database with the given query and sort parameters.
    pub fn load_wads(
        &mut self,
        conn: &Connection,
        query: Option<&str>,
        sort_by: Option<&str>,
        sort_desc: bool,
        include_deleted: bool,
    ) -> usize {
        match search_wads(conn, query, sort_by, sort_desc, include_deleted, 0) {
            Ok(wads) => {
                let ids: Vec<i64> = wads.iter().map(|w| w.id).collect();
                self.stats_map = get_wad_stats_batch(conn, &ids).unwrap_or_default();
                self.id_to_index = ids
                    .iter()
                    .enumerate()
                    .map(|(i, &id)| (id, i))
                    .collect();
                let count = wads.len();
                self.wads = wads;

                // Preserve selection or select first
                if count > 0 {
                    let current = self.table_state.selected().unwrap_or(0);
                    if current >= count {
                        self.table_state.select(Some(count - 1));
                    } else {
                        self.table_state.select(Some(current));
                    }
                } else {
                    self.table_state.select(None);
                }

                count
            }
            Err(_) => {
                self.wads.clear();
                self.stats_map.clear();
                self.id_to_index.clear();
                self.table_state.select(None);
                0
            }
        }
    }

    /// Refresh a single WAD's data in-place.
    pub fn update_row(&mut self, conn: &Connection, wad_id: i64) -> bool {
        if let Some(&idx) = self.id_to_index.get(&wad_id) {
            if let Ok(Some(mut wad)) = caco_core::db::wads::get_wad(conn, wad_id, true) {
                let _ = caco_core::db::connection::attach_tags(conn, &mut wad);
                self.wads[idx] = wad;
                if let Ok(stats) = get_wad_stats_batch(conn, &[wad_id]) {
                    if let Some(s) = stats.into_iter().next() {
                        self.stats_map.insert(s.0, s.1);
                    }
                }
                return true;
            }
        }
        false
    }

    /// Get the currently selected WAD ID.
    pub fn selected_wad_id(&self) -> Option<i64> {
        self.table_state
            .selected()
            .and_then(|i| self.wads.get(i))
            .map(|w| w.id)
    }

    /// Get the currently selected WAD record.
    pub fn selected_wad(&self) -> Option<&WadRecord> {
        self.table_state
            .selected()
            .and_then(|i| self.wads.get(i))
    }

    /// Get stats for the currently selected WAD.
    pub fn selected_stats(&self) -> Option<&WadStats> {
        self.selected_wad_id()
            .and_then(|id| self.stats_map.get(&id))
    }

    /// Move selection down by one.
    pub fn next(&mut self) {
        if self.wads.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => (i + 1).min(self.wads.len() - 1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Move selection up by one.
    pub fn previous(&mut self) {
        if self.wads.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Move to first row.
    pub fn first(&mut self) {
        if !self.wads.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    /// Move to last row.
    pub fn last(&mut self) {
        if !self.wads.is_empty() {
            self.table_state.select(Some(self.wads.len() - 1));
        }
    }

    /// Page down (half screen).
    pub fn page_down(&mut self, visible_rows: usize) {
        if self.wads.is_empty() {
            return;
        }
        let half = visible_rows / 2;
        let i = match self.table_state.selected() {
            Some(i) => (i + half).min(self.wads.len() - 1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Page up (half screen).
    pub fn page_up(&mut self, visible_rows: usize) {
        if self.wads.is_empty() {
            return;
        }
        let half = visible_rows / 2;
        let i = match self.table_state.selected() {
            Some(i) => i.saturating_sub(half),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Get WAD record by ID from the cached list.
    pub fn get_wad_by_id(&self, wad_id: i64) -> Option<&WadRecord> {
        self.id_to_index
            .get(&wad_id)
            .and_then(|&i| self.wads.get(i))
    }

    /// Get stats for a WAD by ID.
    pub fn get_wad_stats(&self, wad_id: i64) -> Option<&WadStats> {
        self.stats_map.get(&wad_id)
    }
}

/// Render the WAD table.
pub fn render_wad_table(state: &mut WadTableState, frame: &mut Frame, area: Rect) {
    let header = Row::new(vec![
        Cell::from("ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Author").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Playtime").style(Style::default().add_modifier(Modifier::BOLD)),
    ])
    .height(1);

    let rows: Vec<Row> = state
        .wads
        .iter()
        .map(|wad| {
            let stats = state.stats_map.get(&wad.id);
            let playtime = stats
                .map(|s| {
                    if s.playtime > 0 {
                        format_duration(s.playtime)
                    } else {
                        String::new()
                    }
                })
                .unwrap_or_default();

            let status_str = theme::status_display(&wad.status);
            let status_style = theme::status_style(&wad.status);

            let deleted_style = if wad.deleted_at.is_some() {
                Style::default().add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(wad.id.to_string()).style(deleted_style),
                Cell::from(wad.title.clone()).style(deleted_style),
                Cell::from(wad.author.clone().unwrap_or_default()).style(deleted_style),
                Cell::from(status_str).style(status_style),
                Cell::from(playtime).style(deleted_style),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(5),
        Constraint::Min(20),
        Constraint::Length(20),
        Constraint::Length(16),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(theme::highlight_style());

    frame.render_stateful_widget(table, area, &mut state.table_state);
}
