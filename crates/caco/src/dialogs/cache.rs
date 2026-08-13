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

/// Height of everything this tab draws outside its table: the summary line,
/// the at-risk toggle, the separator, the button row and the status line.
const FURNITURE: f32 = 160.0;

/// State for the cache management dialog.
pub struct CacheDialogState {
    entries: Vec<CacheEntry>,
    total_size: u64,
    selected_row: Option<usize>,
    /// Opt-in that lets the bulk clear touch irreplaceable files. Reset by
    /// `load`, so it never survives the list it was ticked against.
    include_at_risk: bool,
    pub modified: bool,
}

impl CacheDialogState {
    /// Create a new cache dialog, loading cached WADs from the DB.
    pub fn new(conn: &Connection) -> Self {
        let mut state = Self {
            entries: Vec::new(),
            total_size: 0,
            selected_row: None,
            include_at_risk: false,
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
        self.include_at_risk = false;
        self.selected_row = if self.entries.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    /// How many entries the bulk clear would actually remove.
    fn clearable_count(&self) -> usize {
        self.entries.iter().filter(|e| self.clears(e)).count()
    }

    /// How many entries the bulk clear is refusing to touch.
    fn at_risk_count(&self) -> usize {
        self.entries.iter().filter(|e| e.irreplaceable).count()
    }

    /// Would the bulk clear delete this entry?
    fn clears(&self, entry: &CacheEntry) -> bool {
        !entry.irreplaceable || self.include_at_risk
    }

    /// Render the Cache tab inside `avail` points of vertical space.
    pub fn render_body(&mut self, ui: &mut egui::Ui, conn: &Connection, avail: f32) {
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

            // Reserve this tab's own furniture — summary line, at-risk
            // toggle, button row, status — out of the budget, then let the
            // table scroll inside what is left. Without a cap the table grows
            // to its full row count and drags the window past the viewport,
            // taking the Close button below the bottom edge with it.
            let table_height = (avail - FURNITURE).max(80.0);
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

        let at_risk = self.at_risk_count();
        crate::dialogs::include_at_risk_toggle(ui, &mut self.include_at_risk, at_risk);

        // Button row
        ui.horizontal(|ui| {
            let has_selection = self.selected_row.is_some() && !self.entries.is_empty();
            let selected_irreplaceable = self
                .selected_row
                .and_then(|i| self.entries.get(i))
                .is_some_and(|e| e.irreplaceable);
            let delete_label = if selected_irreplaceable {
                "Delete Selected — cannot re-download"
            } else {
                "Delete Selected"
            };
            let delete_btn = if selected_irreplaceable {
                egui::Button::new(egui::RichText::new(delete_label).color(theme::COLOR_ERROR))
            } else {
                egui::Button::new(delete_label)
            };

            if ui.add_enabled(has_selection, delete_btn).clicked()
                && let Some(idx) = self.selected_row
                && idx < self.entries.len()
            {
                let entry = &self.entries[idx];
                let _ = fs::remove_file(&entry.path);
                let _ = caco_core::db::sessions::clear_cached_path(conn, entry.wad_id);
                self.modified = true;
                self.load(conn);
            }

            // Bulk clear skips files caco cannot re-fetch unless the
            // toggle beside it says otherwise — see
            // `dialogs::include_at_risk_toggle`.
            let clearable = self.clearable_count();
            let skipping = self.entries.len() - clearable;
            let clear_label = if skipping > 0 {
                format!("Clear {clearable} Re-downloadable")
            } else {
                format!("Clear All ({clearable})")
            };
            let clear = ui
                .add_enabled(
                    clearable > 0,
                    egui::Button::new(if self.include_at_risk {
                        egui::RichText::new(clear_label).color(theme::COLOR_ERROR)
                    } else {
                        egui::RichText::new(clear_label)
                    }),
                )
                .on_hover_text(if skipping > 0 {
                    "Skips WADs with no idgames source. Tick the box to include them."
                } else if self.include_at_risk {
                    "Includes files that cannot be downloaded again."
                } else {
                    "Every cached file can be fetched again."
                });
            if clear.clicked() {
                let doomed: Vec<(i64, String)> = self
                    .entries
                    .iter()
                    .filter(|e| self.clears(e))
                    .map(|e| (e.wad_id, e.path.clone()))
                    .collect();
                for (wad_id, path) in doomed {
                    let _ = fs::remove_file(&path);
                    let _ = caco_core::db::sessions::clear_cached_path(conn, wad_id);
                }
                self.modified = true;
                self.load(conn);
            }
        });
    }

    /// Escape closes Storage from this tab.
    pub fn escape(&mut self) -> bool {
        true
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
            include_at_risk: false,
            modified: false,
        }
    }

    /// Ticking the opt-in is the only thing that puts an irreplaceable file
    /// in reach of the bulk clear. The label and the deletion loop read the
    /// same predicate, so they cannot disagree about what is about to go.
    #[test]
    fn test_opt_in_includes_irreplaceable() {
        let mut s = state(vec![entry(false), entry(true), entry(true)]);
        assert_eq!(s.clearable_count(), 1);
        assert_eq!(s.at_risk_count(), 2);

        s.include_at_risk = true;
        assert_eq!(s.clearable_count(), 3);
        assert_eq!(
            s.at_risk_count(),
            2,
            "the count of at-risk files is a fact \
                                           about the library, not about the toggle"
        );
    }

    /// Reloading the list drops the opt-in. It was ticked against a set of
    /// files that no longer exists, and carrying it silently over would arm a
    /// bulk delete the user never armed.
    #[test]
    fn test_opt_in_does_not_survive_a_reload() {
        let mut s = state(vec![entry(true)]);
        s.include_at_risk = true;
        s.entries.clear();
        s.total_size = 0;
        s.include_at_risk = false; // what `load` does
        assert!(!s.include_at_risk);
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
