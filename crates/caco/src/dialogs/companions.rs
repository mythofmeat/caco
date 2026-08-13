//! Library-wide companion file registry.
//!
//! The GUI answer to `caco companion ls` with no query. Per-WAD linking lives
//! in the edit dialog's Companions tab; this is the other half — what is in
//! managed storage, who still uses it, and what can be cleaned up.
//!
//! Companion files are deduplicated by MD5, so one managed file can serve
//! several WADs. That is why every row shows its users: deleting is only safe
//! once nothing points at it.

use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use caco_core::companion_service;
use caco_core::db;
use caco_core::utils::format_size;

use crate::theme;

/// One registry row, with the WADs that link it resolved up front.
struct Entry {
    companion_id: i64,
    filename: String,
    size: i64,
    md5: String,
    users: Vec<String>,
}

impl Entry {
    fn is_orphan(&self) -> bool {
        self.users.is_empty()
    }
}

struct StatusLine {
    text: String,
    is_error: bool,
}

/// Height of everything this tab draws outside its table: the summary line,
/// the "used by" line, the separator, the button row and the status line.
const FURNITURE: f32 = 115.0;

pub struct CompanionsDialogState {
    entries: Vec<Entry>,
    selected: Option<usize>,
    /// Index staged for deletion, awaiting confirmation.
    pending_delete: Option<usize>,
    /// Set when the "delete every orphan" action is awaiting confirmation.
    pending_purge: bool,
    status: Option<StatusLine>,
    /// Whether anything changed, so the parent knows to reload.
    pub modified: bool,
}

impl CompanionsDialogState {
    pub fn new(conn: &Connection) -> Self {
        let mut state = Self {
            entries: Vec::new(),
            selected: None,
            pending_delete: None,
            pending_purge: false,
            status: None,
            modified: false,
        };
        state.reload(conn);
        state
    }

    fn reload(&mut self, conn: &Connection) {
        self.entries = db::get_all_companions(conn)
            .unwrap_or_default()
            .into_iter()
            .map(|c| {
                let users = db::get_wads_for_companion(conn, c.id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(_, title)| title)
                    .collect();
                Entry {
                    companion_id: c.id,
                    filename: c.filename,
                    size: c.size,
                    md5: c.md5,
                    users,
                }
            })
            .collect();

        self.pending_delete = None;
        self.pending_purge = false;
        self.selected = (!self.entries.is_empty()).then_some(0);
    }

    fn orphan_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_orphan()).count()
    }

    fn set_error(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine {
            text: text.into(),
            is_error: true,
        });
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine {
            text: text.into(),
            is_error: false,
        });
    }

    fn delete_selected(&mut self, conn: &Connection, idx: usize) {
        let Some(entry) = self.entries.get(idx) else {
            return;
        };
        let (id, name) = (entry.companion_id, entry.filename.clone());
        match companion_service::delete_orphan(conn, id) {
            Ok(true) => {
                self.modified = true;
                self.reload(conn);
                self.set_info(format!("Deleted {name}."));
            }
            // Only reachable if something linked it since the dialog loaded.
            Ok(false) => self.set_error(format!("'{name}' is still in use.")),
            Err(e) => self.set_error(e.to_string()),
        }
    }

    fn purge_orphans(&mut self, conn: &Connection) {
        let ids: Vec<i64> = self
            .entries
            .iter()
            .filter(|e| e.is_orphan())
            .map(|e| e.companion_id)
            .collect();

        let mut deleted = 0;
        let mut failures = Vec::new();
        for id in ids {
            match companion_service::delete_orphan(conn, id) {
                Ok(true) => deleted += 1,
                Ok(false) => {}
                Err(e) => failures.push(e.to_string()),
            }
        }

        self.modified = deleted > 0;
        self.reload(conn);
        if failures.is_empty() {
            self.set_info(format!("Deleted {deleted} orphaned file(s)."));
        } else {
            self.set_error(format!(
                "Deleted {deleted}, {} failed: {}",
                failures.len(),
                failures.join("; ")
            ));
        }
    }

    /// Render the Files tab inside `avail` points of vertical space.
    pub fn render_body(&mut self, ui: &mut egui::Ui, conn: &Connection, avail: f32) {
        let total: i64 = self.entries.iter().map(|e| e.size).sum();
        let orphans = self.orphan_count();
        ui.colored_label(
            theme::TEXT_SECONDARY,
            format!(
                "{} registered, {} — {} orphaned",
                self.entries.len(),
                format_size(total.max(0) as u64),
                orphans
            ),
        );
        ui.add_space(6.0);

        if self.entries.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No companion files registered. Add one from a WAD's Edit dialog.",
            );
        } else {
            self.render_table(ui, avail - FURNITURE);
            self.render_users(ui);
        }

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);

        if let Some(idx) = self.pending_delete {
            let name = self
                .entries
                .get(idx)
                .map(|e| e.filename.as_str())
                .unwrap_or("file");
            ui.colored_label(
                theme::COLOR_ERROR,
                format!("Delete the managed copy of {name}?"),
            );
            ui.horizontal(|ui| {
                if ui.button("Delete").clicked() {
                    self.pending_delete = None;
                    self.delete_selected(conn, idx);
                }
                if ui.button("Cancel").clicked() {
                    self.pending_delete = None;
                }
            });
        } else if self.pending_purge {
            ui.colored_label(
                theme::COLOR_ERROR,
                format!("Delete all {orphans} orphaned companion file(s)?"),
            );
            ui.horizontal(|ui| {
                if ui.button("Delete All").clicked() {
                    self.pending_purge = false;
                    self.purge_orphans(conn);
                }
                if ui.button("Cancel").clicked() {
                    self.pending_purge = false;
                }
            });
        } else {
            self.render_actions(ui);
        }

        if let Some(status) = &self.status {
            ui.add_space(4.0);
            let color = if status.is_error {
                theme::COLOR_ERROR
            } else {
                theme::TEXT_SECONDARY
            };
            ui.colored_label(color, &status.text);
        }
    }

    /// Escape. Dismisses a staged confirmation before it closes Storage.
    pub fn escape(&mut self) -> bool {
        if self.pending_delete.is_some() || self.pending_purge {
            self.pending_delete = None;
            self.pending_purge = false;
            return false;
        }
        true
    }

    fn render_table(&mut self, ui: &mut egui::Ui, height: f32) {
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        let mut clicked = None;
        let selected = self.selected;

        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .sense(egui::Sense::click())
            .column(Column::remainder().at_least(180.0)) // Filename
            .column(Column::initial(80.0).at_least(60.0)) // Size
            .column(Column::initial(110.0).at_least(80.0)) // MD5
            .column(Column::initial(110.0).at_least(70.0)) // Used by
            .max_scroll_height(height.max(80.0))
            .header(row_height + 2.0, |mut header| {
                for label in ["Filename", "Size", "MD5", "Used by"] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, self.entries.len(), |mut row| {
                    let idx = row.index();
                    row.set_selected(selected == Some(idx));
                    let entry = &self.entries[idx];

                    row.col(|ui| {
                        ui.label(&entry.filename);
                    });
                    row.col(|ui| {
                        ui.label(format_size(entry.size.max(0) as u64));
                    });
                    row.col(|ui| {
                        ui.colored_label(
                            theme::TEXT_SECONDARY,
                            &entry.md5[..12.min(entry.md5.len())],
                        );
                    });
                    row.col(|ui| {
                        if entry.is_orphan() {
                            ui.colored_label(theme::COLOR_ERROR, "orphan");
                        } else {
                            ui.label(format!(
                                "{} WAD{}",
                                entry.users.len(),
                                if entry.users.len() == 1 { "" } else { "s" }
                            ));
                        }
                    });

                    if row.response().clicked() {
                        clicked = Some(idx);
                    }
                });
            });

        if let Some(idx) = clicked {
            self.selected = Some(idx);
        }
    }

    /// Show who links the selected file — the thing you need to know before
    /// deciding whether deleting it is safe.
    fn render_users(&mut self, ui: &mut egui::Ui) {
        let Some(entry) = self.selected.and_then(|i| self.entries.get(i)) else {
            return;
        };
        ui.add_space(4.0);
        if entry.is_orphan() {
            ui.colored_label(theme::TEXT_SECONDARY, "Not linked to any WAD.");
            return;
        }
        ui.colored_label(
            theme::TEXT_SECONDARY,
            format!("Used by: {}", entry.users.join(", ")),
        );
    }

    fn render_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let selected_is_orphan = self
                .selected
                .and_then(|i| self.entries.get(i))
                .is_some_and(Entry::is_orphan);

            if ui
                .add_enabled(selected_is_orphan, egui::Button::new("Delete Selected"))
                .on_hover_text("Only files no WAD links can be deleted")
                .clicked()
            {
                self.pending_delete = self.selected;
            }

            if ui
                .add_enabled(
                    self.orphan_count() > 0,
                    egui::Button::new("Delete All Orphans"),
                )
                .clicked()
            {
                self.pending_purge = true;
            }
        });
    }
}
