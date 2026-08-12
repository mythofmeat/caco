//! Browse, restore, and permanently delete soft-deleted WADs.
//!
//! Deleting a WAD sets `deleted_at` rather than dropping the row, so the
//! record — play history, completions, ratings — survives the mistake. This
//! dialog is the only way back: without it a soft delete would be
//! indistinguishable from a permanent one.

use rusqlite::Connection;

use caco_core::db::{self, WadRecord};

use crate::theme;

/// A destructive action awaiting confirmation.
enum Pending {
    Purge(usize),
    PurgeAll,
}

struct StatusLine {
    text: String,
    is_error: bool,
}

pub struct TrashDialogState {
    wads: Vec<WadRecord>,
    selected: Option<usize>,
    pending: Option<Pending>,
    status: Option<StatusLine>,
    /// Whether anything was restored or purged, so the parent reloads.
    pub modified: bool,
}

pub enum TrashResult {
    Open,
    Closed,
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
            Pending::Purge(idx) => {
                let Some(wad) = self.wads.get(idx) else {
                    return;
                };
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
            Pending::PurgeAll => match db::purge_all_deleted(conn) {
                Ok(count) => {
                    self.modified = true;
                    self.reload(conn);
                    self.set_info(format!("Permanently deleted {count} WAD(s)."));
                }
                Err(e) => self.set_error(e.to_string()),
            },
        }
    }

    fn pending_prompt(&self) -> Option<String> {
        Some(match self.pending.as_ref()? {
            Pending::Purge(idx) => format!(
                "Permanently delete '{}'? Its play history and completions go too.",
                self.wads.get(*idx).map(|w| w.title.as_str()).unwrap_or("")
            ),
            Pending::PurgeAll => format!(
                "Permanently delete all {} trashed WAD(s)? Play history and completions go too.",
                self.wads.len()
            ),
        })
    }

    pub fn render_body(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, conn: &Connection) {
        if self.wads.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "Trash is empty.");
        } else {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!(
                    "{} deleted WAD{} — the rows are kept until purged.",
                    self.wads.len(),
                    if self.wads.len() == 1 { "" } else { "s" }
                ),
            );
            ui.add_space(4.0);
            self.render_list(ui);
        }

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);

        if let Some(prompt) = self.pending_prompt() {
            ui.colored_label(theme::COLOR_ERROR, prompt);
            ui.horizontal(|ui| {
                if ui.button("Delete Permanently").clicked() {
                    self.run_pending(conn);
                }
                if ui.button("Cancel").clicked() {
                    self.pending = None;
                }
            });
        } else {
            self.render_actions(ui, conn);
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

    /// Escape. Backs out of a staged confirmation before it closes Storage.
    pub fn escape(&mut self) -> bool {
        if self.pending.is_some() {
            self.pending = None;
            return false;
        }
        true
    }

    fn render_list(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<(usize, String)> = self
            .wads
            .iter()
            .enumerate()
            .map(|(idx, wad)| {
                let author = wad.author.as_deref().unwrap_or("unknown");
                (idx, format!("{}  ·  {}  ·  {}", wad.id, wad.title, author))
            })
            .collect();

        egui::ScrollArea::vertical()
            .max_height(280.0)
            .show(ui, |ui| {
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
        ui.horizontal(|ui| {
            let has_selection = self.selected.is_some();

            if ui
                .add_enabled(has_selection, egui::Button::new("Restore"))
                .clicked()
                && let Some(idx) = self.selected
            {
                self.restore(conn, idx);
            }

            if ui
                .add_enabled(has_selection, egui::Button::new("Delete Permanently"))
                .clicked()
            {
                self.pending = self.selected.map(Pending::Purge);
            }

            if ui
                .add_enabled(!self.wads.is_empty(), egui::Button::new("Empty Trash"))
                .clicked()
            {
                self.pending = Some(Pending::PurgeAll);
            }
        });
    }
}
