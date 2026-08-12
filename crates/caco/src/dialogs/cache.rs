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
    /// Manual-only: deleting this file destroys the WAD rather than costing a
    /// download. Kept per-row so the bulk actions can exclude them.
    irreplaceable: bool,
}

/// State for the cache management dialog.
pub struct CacheDialogState {
    entries: Vec<CacheEntry>,
    total_size: u64,
    selected_row: Option<usize>,
    /// Index awaiting a second click before an irreplaceable file is deleted.
    confirming_delete: Option<usize>,
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
            confirming_delete: None,
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
                let irreplaceable = !w.retrievability().is_automatic();
                let path = w.cached_path?;
                let size = fs::metadata(&path).ok().map(|m| m.len());
                Some(CacheEntry {
                    wad_id: w.id,
                    title: w.title,
                    path,
                    size,
                    irreplaceable,
                })
            })
            .collect();

        self.total_size = self.entries.iter().filter_map(|e| e.size).sum();
        self.confirming_delete = None;
        self.selected_row = if self.entries.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    /// How many entries the bulk clear would actually remove.
    fn clearable_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.irreplaceable).count()
    }

    /// Render the cache dialog. Returns the dialog result.
    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> CacheResult {
        let mut result = CacheResult::Open;

        crate::dialogs::modal_window(ctx, "Cache Management", [700.0, 450.0]).show(ctx, |ui| {
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

                // Reserve the button row's height, then let the table scroll
                // inside whatever is left. Without a cap the table grows to
                // its full row count and drags the window past the viewport,
                // taking the Close button below the bottom edge with it.
                let table_height = crate::dialogs::modal_body_height(ctx, 110.0);
                let table = TableBuilder::new(ui)
                    .max_scroll_height(table_height)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .sense(egui::Sense::click())
                    .column(Column::exact(50.0)) // ID
                    .column(Column::initial(200.0).at_least(100.0)) // Title
                    .column(Column::remainder().at_least(150.0)) // Path
                    .column(Column::initial(80.0).at_least(60.0)); // Size

                table
                    .header(row_height + 2.0, |mut header| {
                        for label in ["ID", "Title", "Path", "Size"] {
                            header.col(|ui| {
                                ui.strong(label);
                            });
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
                                if entry.irreplaceable {
                                    ui.colored_label(
                                        theme::COLOR_WARNING,
                                        format!("{}{}", theme::LOST_MARKER, entry.title),
                                    )
                                    .on_hover_text(
                                        "No idgames source: this file cannot be downloaded \
                                             again. Bulk clearing skips it.",
                                    );
                                } else {
                                    ui.label(&entry.title);
                                }
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
                let selected_irreplaceable = self
                    .selected_row
                    .and_then(|i| self.entries.get(i))
                    .is_some_and(|e| e.irreplaceable);
                let awaiting = self.confirming_delete == self.selected_row;

                let delete_label = if selected_irreplaceable && awaiting {
                    "Delete anyway — cannot be re-downloaded"
                } else {
                    "Delete Selected"
                };
                let delete_btn = if selected_irreplaceable && awaiting {
                    egui::Button::new(egui::RichText::new(delete_label).color(theme::COLOR_ERROR))
                } else {
                    egui::Button::new(delete_label)
                };

                if ui.add_enabled(has_selection, delete_btn).clicked()
                    && let Some(idx) = self.selected_row
                    && idx < self.entries.len()
                {
                    // An irreplaceable file takes two clicks: the first
                    // only arms the button, so the confirmation cannot be
                    // clicked through by muscle memory.
                    if selected_irreplaceable && !awaiting {
                        self.confirming_delete = Some(idx);
                    } else {
                        let entry = &self.entries[idx];
                        let _ = fs::remove_file(&entry.path);
                        let _ = caco_core::db::sessions::clear_cached_path(conn, entry.wad_id);
                        self.modified = true;
                        self.load(conn);
                    }
                }

                // Bulk clear never touches a file caco cannot re-fetch —
                // there is no selection to review, so the only safe scope
                // is the disposable one.
                let clearable = self.clearable_count();
                let clear_label = if clearable < self.entries.len() {
                    format!("Clear {clearable} Re-downloadable")
                } else {
                    "Clear All".to_string()
                };
                let clear = ui.add_enabled(clearable > 0, egui::Button::new(clear_label));
                let clear = if clearable < self.entries.len() {
                    clear.on_hover_text(
                        "Skips WADs with no idgames source. Delete those individually.",
                    )
                } else {
                    clear
                };
                if clear.clicked() {
                    for entry in self.entries.iter().filter(|e| !e.irreplaceable) {
                        let _ = fs::remove_file(&entry.path);
                        let _ = caco_core::db::sessions::clear_cached_path(conn, entry.wad_id);
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(irreplaceable: bool) -> CacheEntry {
        CacheEntry {
            wad_id: 1,
            title: "T".into(),
            path: "/cache/t.zip".into(),
            size: Some(10),
            irreplaceable,
        }
    }

    fn state(entries: Vec<CacheEntry>) -> CacheDialogState {
        CacheDialogState {
            entries,
            total_size: 0,
            selected_row: None,
            confirming_delete: None,
            modified: false,
        }
    }

    /// Bulk clear must never count a file caco cannot fetch again — the label
    /// and the loop both read this, so an off-by-one here is a deleted WAD.
    #[test]
    fn test_clearable_count_excludes_irreplaceable() {
        assert_eq!(state(vec![entry(false), entry(false)]).clearable_count(), 2);
        assert_eq!(state(vec![entry(false), entry(true)]).clearable_count(), 1);
        assert_eq!(state(vec![entry(true), entry(true)]).clearable_count(), 0);
        assert_eq!(state(Vec::new()).clearable_count(), 0);
    }
}
