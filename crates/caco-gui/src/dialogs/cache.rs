use std::fs;

use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use crate::theme;

/// A single cache entry for display.
struct CacheEntry {
    wad_id: i64,
    title: String,
    path: String,
    size: Option<u64>,
}

/// State for the cache management dialog.
pub struct CacheDialogState {
    entries: Vec<CacheEntry>,
    total_size: u64,
    selected_row: Option<usize>,
    pub modified: bool,
}

/// Result of showing the cache dialog.
pub enum CacheResult {
    Open,
    Closed,
}

impl CacheDialogState {
    /// Create a new cache dialog, loading cached WADs from the DB.
    pub fn new(conn: &Connection) -> Self {
        let mut state = Self {
            entries: Vec::new(),
            total_size: 0,
            selected_row: None,
            modified: false,
        };
        state.load(conn);
        state
    }

    fn load(&mut self, conn: &Connection) {
        let wads = caco_core::db::sessions::get_cached_wads(conn).unwrap_or_default();

        self.entries = wads
            .into_iter()
            .filter_map(|w| {
                let path = w.cached_path?;
                let size = fs::metadata(&path).ok().map(|m| m.len());
                Some(CacheEntry {
                    wad_id: w.id,
                    title: w.title,
                    path,
                    size,
                })
            })
            .collect();

        self.total_size = self.entries.iter().filter_map(|e| e.size).sum();
        self.selected_row = if self.entries.is_empty() { None } else { Some(0) };
    }

    /// Render the cache dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> CacheResult {
        let mut result = CacheResult::Open;

        egui::Window::new("Cache Management")
            .collapsible(false)
            .resizable(true)
            .default_size([700.0, 450.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Summary line
                ui.horizontal(|ui| {
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        format!(
                            "{} cached file{}, {}",
                            self.entries.len(),
                            if self.entries.len() == 1 { "" } else { "s" },
                            caco_core::utils::format_size(self.total_size),
                        ),
                    );
                });
                ui.add_space(4.0);

                if self.entries.is_empty() {
                    ui.colored_label(theme::TEXT_SECONDARY, "No cached files.");
                } else {
                    let text_height = ui.text_style_height(&egui::TextStyle::Body);
                    let row_height = text_height + 6.0;

                    let table = TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .sense(egui::Sense::click())
                        .column(Column::exact(50.0))                     // ID
                        .column(Column::initial(200.0).at_least(100.0))  // Title
                        .column(Column::remainder().at_least(150.0))     // Path
                        .column(Column::initial(80.0).at_least(60.0));   // Size

                    table
                        .header(row_height + 2.0, |mut header| {
                            for label in ["ID", "Title", "Path", "Size"] {
                                header.col(|ui| { ui.strong(label); });
                            }
                        })
                        .body(|body| {
                            let count = self.entries.len();
                            body.rows(row_height, count, |mut row| {
                                let idx = row.index();
                                let is_selected = self.selected_row == Some(idx);
                                row.set_selected(is_selected);

                                let entry = &self.entries[idx];

                                row.col(|ui| {
                                    ui.label(entry.wad_id.to_string());
                                });
                                row.col(|ui| {
                                    ui.label(&entry.title);
                                });
                                row.col(|ui| {
                                    let color = if entry.size.is_some() {
                                        theme::TEXT_SECONDARY
                                    } else {
                                        crate::theme::COLOR_ERROR
                                    };
                                    ui.colored_label(color, &entry.path);
                                });
                                row.col(|ui| {
                                    let size_str = match entry.size {
                                        Some(s) => caco_core::utils::format_size(s),
                                        None => "missing".to_string(),
                                    };
                                    ui.label(size_str);
                                });

                                if row.response().clicked() {
                                    self.selected_row = Some(idx);
                                }
                            });
                        });
                }

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Button row
                ui.horizontal(|ui| {
                    let has_selection = self.selected_row.is_some() && !self.entries.is_empty();
                    if ui.add_enabled(has_selection, egui::Button::new("Delete Selected")).clicked()
                        && let Some(idx) = self.selected_row
                        && idx < self.entries.len()
                    {
                        let entry = &self.entries[idx];
                        let _ = fs::remove_file(&entry.path);
                        let _ = caco_core::db::sessions::clear_cached_path(conn, entry.wad_id);
                        self.modified = true;
                        self.load(conn);
                    }

                    if ui.add_enabled(!self.entries.is_empty(), egui::Button::new("Clear All")).clicked() {
                        for entry in &self.entries {
                            let _ = fs::remove_file(&entry.path);
                        }
                        let _ = caco_core::db::sessions::clear_all_cached_paths(conn);
                        self.modified = true;
                        self.load(conn);
                    }

                    if ui.button("Close").clicked() {
                        result = CacheResult::Closed;
                    }
                });
            });

        // Escape closes
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return CacheResult::Closed;
        }

        result
    }
}
