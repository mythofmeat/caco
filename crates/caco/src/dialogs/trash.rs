//! Browse, restore, and permanently delete soft-deleted WADs.
//!
//! Deleting a WAD sets `deleted_at` rather than dropping the row, so the
//! record — play history, completions, ratings — survives the mistake. This
//! dialog is the only way back: without it a soft delete would be
//! indistinguishable from a permanent one.

use rusqlite::Connection;

use caco_core::db::{self, WadRecord};

use crate::dialogs::{FooterRow, Primary, StatusLine, count, scroll_body};
use crate::theme;

/// A destructive action awaiting confirmation.
enum Pending {
    PurgeAll,
}

/// What the line above the list takes from the tab's height.
const SUMMARY: f32 = 24.0;

pub struct TrashDialogState {
    wads: Vec<WadRecord>,
    selected: Option<usize>,
    pending: Option<Pending>,
    status: Option<StatusLine>,
    /// Whether anything was restored or purged, so the parent reloads.
    pub modified: bool,
}

impl TrashDialogState {
    pub fn new(conn: &Connection) -> Self {
        let mut state = Self {
            wads: Vec::new(),
            selected: None,
            pending: None,
            status: None,
            modified: false,
        };
        state.reload(conn);
        state
    }

    fn reload(&mut self, conn: &Connection) {
        // The fifth argument flips `search_wads` into trash-only mode.
        self.wads = db::search_wads(conn, None, None, false, true, 0).unwrap_or_default();
        self.pending = None;
        self.selected = (!self.wads.is_empty()).then_some(0);
    }

    fn set_error(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine::error(text));
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine::info(text));
    }

    fn restore(&mut self, conn: &Connection, idx: usize) {
        let Some(wad) = self.wads.get(idx) else {
            return;
        };
        let (id, title) = (wad.id, wad.title.clone());
        match db::restore_wad(conn, id) {
            Ok(true) => {
                self.modified = true;
                self.reload(conn);
                self.set_info(format!("Restored '{title}'."));
            }
            Ok(false) => self.set_error(format!("'{title}' was not in the trash.")),
            Err(e) => self.set_error(e.to_string()),
        }
    }

    fn run_pending(&mut self, conn: &Connection) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match pending {
            Pending::PurgeAll => match db::purge_all_deleted(conn) {
                Ok(deleted) => {
                    self.modified = true;
                    self.reload(conn);
                    self.set_info(format!(
                        "Permanently deleted {}.",
                        crate::dialogs::count(deleted, "WAD", "WADs")
                    ));
                }
                Err(e) => self.set_error(e.to_string()),
            },
        }
    }

    fn pending_prompt(&self) -> Option<String> {
        Some(match self.pending.as_ref()? {
            Pending::PurgeAll => format!(
                "Permanently delete {}? Their play history goes too.",
                count(self.wads.len(), "WAD", "WADs")
            ),
        })
    }

    /// The Trash tab: what has been deleted and can still come back.
    pub fn render_body(&mut self, ui: &mut egui::Ui, height: f32) {
        if self.wads.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "The Trash is empty.");
            return;
        }
        ui.colored_label(
            theme::TEXT_SECONDARY,
            format!(
                "{}, which you can restore with their play history.",
                count(self.wads.len(), "WAD", "WADs")
            ),
        );
        ui.add_space(4.0);
        self.render_list(ui, height - SUMMARY);
    }

    /// What is being asked, and how the last thing went.
    pub fn render_notes(&self, ui: &mut egui::Ui) {
        if let Some(prompt) = self.pending_prompt() {
            ui.colored_label(theme::COLOR_ERROR, prompt);
        }
        if let Some(status) = &self.status {
            status.show(ui);
        }
    }

    /// Restore or delete what is selected, or the lot. True if Close was pressed.
    pub fn render_footer(&mut self, row: &mut FooterRow<'_>, conn: &Connection) -> bool {
        if self.pending.is_some() {
            let ended = row.end(Some("Cancel"), Some(Primary::danger("Delete Permanently")));
            if ended.dismissed {
                self.pending = None;
            }
            if ended.primary {
                self.run_pending(conn);
            }
            return false;
        }
        row.left(|ui| self.render_actions(ui, conn));
        row.close()
    }

    /// Escape. Backs out of a staged confirmation before it closes Storage.
    pub fn escape(&mut self) -> bool {
        if self.pending.is_some() {
            self.pending = None;
            return false;
        }
        true
    }

    fn render_list(&mut self, ui: &mut egui::Ui, height: f32) {
        let rows: Vec<(usize, String)> = self
            .wads
            .iter()
            .enumerate()
            .map(|(idx, wad)| match &wad.author {
                Some(author) => (idx, format!("{}  \u{00b7}  {author}", wad.title)),
                None => (idx, wad.title.clone()),
            })
            .collect();

        scroll_body(ui, height, |ui| {
            for (idx, label) in rows {
                if ui
                    .selectable_label(self.selected == Some(idx), label)
                    .clicked()
                {
                    self.selected = Some(idx);
                }
            }
        });
    }

    fn render_actions(&mut self, ui: &mut egui::Ui, conn: &Connection) {
        if self.wads.is_empty() {
            return;
        }
        if let Some(idx) = self.selected {
            if ui.button("Restore").clicked() {
                self.restore(conn, idx);
            }
            if ui
                .button(egui::RichText::new("Delete Permanently").color(theme::COLOR_ERROR))
                .on_hover_text("Its play history goes too")
                .clicked()
                && let Some(wad) = self.wads.get(idx)
            {
                let (id, title) = (wad.id, wad.title.clone());
                match db::delete_wad(conn, id, true) {
                    Ok(_) => {
                        self.modified = true;
                        self.reload(conn);
                        self.set_info(format!("Permanently deleted '{title}'."));
                    }
                    Err(e) => self.set_error(e.to_string()),
                }
            }
        }
        if ui.button("Empty Trash").clicked() {
            self.pending = Some(Pending::PurgeAll);
        }
    }
}
