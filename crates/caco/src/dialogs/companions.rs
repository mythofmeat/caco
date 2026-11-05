//! Library-wide companion file registry. Per-WAD linking lives in the edit dialog's
//! Companions tab; this is the other half: what is in managed storage, who still uses it,
//! and what can be cleaned up.
//!
//! Companion files are deduplicated by MD5, so one managed file can serve
//! several WADs. That is why every row shows its users: deleting is only safe
//! once nothing points at it.

use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use caco_core::companion_service;
use caco_core::db;
use caco_core::utils::format_size;

use crate::dialogs::{FooterRow, Primary, StatusLine, count};
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
    fn is_unused(&self) -> bool {
        self.users.is_empty()
    }
}

/// What this tab draws outside its table: the summary line and the "used by" line.
const FURNITURE: f32 = 56.0;

pub struct CompanionsDialogState {
    entries: Vec<Entry>,
    selected: Option<usize>,
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

        self.pending_purge = false;
        self.selected = (!self.entries.is_empty()).then_some(0);
    }

    fn unused_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_unused()).count()
    }

    fn set_error(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine::error(text));
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine::info(text));
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

    fn purge_unused(&mut self, conn: &Connection) {
        let ids: Vec<i64> = self
            .entries
            .iter()
            .filter(|e| e.is_unused())
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
            self.set_info(format!(
                "Deleted {}.",
                count(deleted, "unused file", "unused files")
            ));
        } else {
            self.set_error(format!(
                "Deleted {deleted}, {} failed: {}",
                failures.len(),
                failures.join("; ")
            ));
        }
    }

    /// The Files tab: every companion file, and who is using it.
    pub fn render_body(&mut self, ui: &mut egui::Ui, height: f32) {
        if self.entries.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No companion files yet. Add one from a WAD's Edit dialog.",
            );
            return;
        }

        let total: i64 = self.entries.iter().map(|e| e.size).sum();
        ui.colored_label(
            theme::TEXT_SECONDARY,
            format!(
                "{}, {}, {} unused",
                count(self.entries.len(), "file", "files"),
                format_size(total.max(0) as u64),
                self.unused_count()
            ),
        );
        ui.add_space(6.0);
        self.render_table(ui, height - FURNITURE);
        self.render_users(ui);
    }

    /// What is being asked, and how the last thing went.
    pub fn render_notes(&self, ui: &mut egui::Ui) {
        if self.pending_purge {
            ui.colored_label(
                theme::COLOR_ERROR,
                format!(
                    "Delete {}?",
                    count(self.unused_count(), "unused file", "unused files")
                ),
            );
        }
        if let Some(status) = &self.status {
            status.show(ui);
        }
    }

    /// Delete what nothing uses. True if Close was pressed.
    pub fn render_footer(&mut self, row: &mut FooterRow<'_>, conn: &Connection) -> bool {
        if self.pending_purge {
            let ended = row.end(Some("Cancel"), Some(Primary::danger("Delete All")));
            if ended.dismissed {
                self.pending_purge = false;
            }
            if ended.primary {
                self.pending_purge = false;
                self.purge_unused(conn);
            }
            return false;
        }
        row.left(|ui| self.render_actions(ui, conn));
        row.close()
    }

    /// Escape. Dismisses a staged confirmation before it closes Storage.
    pub fn escape(&mut self) -> bool {
        if self.pending_purge {
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
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .sense(egui::Sense::click())
            .column(Column::remainder().at_least(180.0).clip(true)) // Filename
            .column(Column::initial(80.0).at_least(60.0)) // Size
            .column(Column::initial(110.0).at_least(70.0)) // Used by
            .max_scroll_height(height.max(60.0))
            .header(row_height + 2.0, |mut header| {
                for label in ["File", "Size", "Used by"] {
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
                        if entry.is_unused() {
                            ui.colored_label(theme::TEXT_SECONDARY, "Nothing");
                        } else {
                            ui.label(count(entry.users.len(), "WAD", "WADs"));
                        }
                    });

                    let response = row.response();
                    if response.clicked() {
                        clicked = Some(idx);
                    }
                    response.on_hover_text(format!("MD5 {}", entry.md5));
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
        if entry.is_unused() {
            ui.colored_label(theme::TEXT_SECONDARY, "No WAD is using it.");
            return;
        }
        ui.colored_label(
            theme::TEXT_SECONDARY,
            format!("Used by: {}", entry.users.join(", ")),
        );
    }

    /// Only what nothing uses can be deleted, so that is all that is offered.
    fn render_actions(&mut self, ui: &mut egui::Ui, conn: &Connection) {
        if let Some(idx) = self
            .selected
            .filter(|i| self.entries.get(*i).is_some_and(Entry::is_unused))
            && ui.button("Delete Selected").clicked()
        {
            self.delete_selected(conn, idx);
        }
        if self.unused_count() > 0 && ui.button("Delete Unused Files").clicked() {
            self.pending_purge = true;
        }
    }
}
