//! Everything about what caco is keeping on disk, in one dialog.
//!
//! Cache, Clean, Trash and Files were four sidebar entries and four modals,
//! which read as four unrelated features. They are one question — what is on
//! disk and how do I get space back — and they overlap: Cache and Clean delete
//! the same `wads.cached_path` files, and Files and Clean both remove orphaned
//! companions. Four entry points to overlapping deletions is how the two
//! dialogs came to disagree about irreplaceable files in the first place; see
//! [`super::include_at_risk_toggle`].
//!
//! Each tab keeps its own module and its own state. This file owns only the
//! window, the tab strip, and the routing of Escape. A tab says what goes in the
//! one footer they share, with its own way out, because whether that is Close or
//! Cancel depends on whether the tab is in the middle of asking something.

use rusqlite::Connection;

use crate::dialogs::cache::CacheDialogState;
use crate::dialogs::companions::CompanionsDialogState;
use crate::dialogs::gc::GcDialogState;
use crate::dialogs::trash::TrashDialogState;
use crate::dialogs::{Dialog, FooterRow, tab_bar};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StorageTab {
    Cache,
    Clean,
    Trash,
    Files,
}

impl StorageTab {
    const ALL: [(StorageTab, &'static str); 4] = [
        (StorageTab::Cache, "Cache"),
        (StorageTab::Clean, "Clean"),
        (StorageTab::Trash, "Trash"),
        (StorageTab::Files, "Files"),
    ];

    fn blurb(self) -> &'static str {
        match self {
            StorageTab::Cache => "The WAD files on this machine",
            StorageTab::Clean => "Get space back from finished WADs and what was left behind",
            StorageTab::Trash => "WADs you deleted, which can still be restored",
            StorageTab::Files => "Companion files, and the WADs using them",
        }
    }
}

pub enum StorageResult {
    Open,
    Closed,
}

/// Sized for the widest tab (Clean's row list). Every tab gets the same body height whatever
/// it holds, so switching tabs cannot resize the window under the pointer.
const DESIRED: [f32; 2] = [860.0, 560.0];

/// What the strip and the blurb above the tab take from its height.
const STRIP: f32 = 34.0;

pub struct StorageDialogState {
    tab: StorageTab,
    modified: bool,
    // Built on first visit, not up front: `GcDialogState::new` runs a full
    // `gc::plan`, which walks the data dir, the backup dir and every cached
    // file. Opening Storage to look at Trash must not pay for that.
    cache: Option<CacheDialogState>,
    gc: Option<Box<GcDialogState>>,
    trash: Option<TrashDialogState>,
    files: Option<CompanionsDialogState>,
}

impl StorageDialogState {
    pub fn new(tab: StorageTab) -> Self {
        Self {
            tab,
            modified: false,
            cache: None,
            gc: None,
            trash: None,
            files: None,
        }
    }

    /// Did any tab change something the library view needs to re-read?
    ///
    /// Asked once when the dialog closes, so an untouched tab that was never
    /// opened cannot report a change it did not make.
    pub fn modified(&self) -> bool {
        self.modified
            || self.cache.as_ref().is_some_and(|s| s.modified)
            || self.gc.as_ref().is_some_and(|s| s.modified)
            || self.trash.as_ref().is_some_and(|s| s.modified)
            || self.files.as_ref().is_some_and(|s| s.modified)
    }

    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> StorageResult {
        let mut result = StorageResult::Open;

        Dialog::new("Storage", DESIRED).show(ctx, |dialog| {
            dialog
                .pinned(|ui| {
                    tab_bar(ui, &mut self.tab, &StorageTab::ALL);
                })
                .fill(|ui, height| {
                    ui.colored_label(theme::TEXT_SECONDARY, self.tab.blurb());
                    ui.add_space(6.0);
                    self.render_active_tab(ui, conn, height - STRIP);
                })
                .above(|ui| self.render_tab_notes(ui))
                .buttons(|row| {
                    if self.render_tab_footer(row, conn) {
                        result = StorageResult::Closed;
                    }
                })
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.escape() {
            return StorageResult::Closed;
        }

        result
    }

    fn render_active_tab(&mut self, ui: &mut egui::Ui, conn: &Connection, height: f32) {
        match self.tab {
            StorageTab::Cache => self
                .cache
                .get_or_insert_with(|| CacheDialogState::new(conn))
                .render_body(ui, height),
            StorageTab::Clean => self
                .gc
                .get_or_insert_with(|| Box::new(GcDialogState::new(conn)))
                .render_body(ui, conn, height),
            StorageTab::Trash => self
                .trash
                .get_or_insert_with(|| TrashDialogState::new(conn))
                .render_body(ui, height),
            StorageTab::Files => self
                .files
                .get_or_insert_with(|| CompanionsDialogState::new(conn))
                .render_body(ui, height),
        }
    }

    /// What has to stay in view above the buttons: how the last action went, or what is
    /// being asked.
    fn render_tab_notes(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            StorageTab::Cache => {
                if let Some(state) = &mut self.cache {
                    state.render_notes(ui);
                }
            }
            StorageTab::Clean => {
                if let Some(state) = &mut self.gc {
                    state.render_notes(ui);
                }
            }
            StorageTab::Trash => {
                if let Some(state) = &self.trash {
                    state.render_notes(ui);
                }
            }
            StorageTab::Files => {
                if let Some(state) = &self.files {
                    state.render_notes(ui);
                }
            }
        }
    }

    /// The buttons for this tab. True if one of them closed Storage.
    fn render_tab_footer(&mut self, row: &mut FooterRow<'_>, conn: &Connection) -> bool {
        let (closed, changed) = match self.tab {
            StorageTab::Cache => self.cache.as_mut().map(|state| {
                let closed = state.render_footer(row, conn);
                (closed, std::mem::take(&mut state.modified))
            }),
            StorageTab::Clean => self.gc.as_mut().map(|state| {
                let closed = state.render_footer(row, conn);
                (closed, std::mem::take(&mut state.modified))
            }),
            StorageTab::Trash => self.trash.as_mut().map(|state| {
                let closed = state.render_footer(row, conn);
                (closed, std::mem::take(&mut state.modified))
            }),
            StorageTab::Files => self.files.as_mut().map(|state| {
                let closed = state.render_footer(row, conn);
                (closed, std::mem::take(&mut state.modified))
            }),
        }
        // The tab builds its state the frame it is first shown, and that is before this.
        .unwrap_or((false, false));
        if changed {
            self.record_change();
        }
        closed
    }

    fn record_change(&mut self) {
        self.modified = true;
        // Every tab is a cached view over overlapping files and rows. Drop
        // the inactive snapshots now so returning to one rebuilds it from the
        // post-mutation state instead of showing stale entries.
        match self.tab {
            StorageTab::Cache => {
                self.gc = None;
                self.trash = None;
                self.files = None;
            }
            StorageTab::Clean => {
                self.cache = None;
                self.trash = None;
                self.files = None;
            }
            StorageTab::Trash => {
                self.cache = None;
                self.gc = None;
                self.files = None;
            }
            StorageTab::Files => {
                self.cache = None;
                self.gc = None;
                self.trash = None;
            }
        }
    }

    /// Route Escape to the active tab first.
    ///
    /// Bulk destructive actions are staged behind a confirmation, and the key
    /// that dismisses one must not also close the dialog around it — otherwise
    /// backing out of "Empty Trash?" throws away the whole view.
    /// Returns true when the tab had nothing staged and Storage should close.
    fn escape(&mut self) -> bool {
        match self.tab {
            StorageTab::Cache => self.cache.as_mut().is_none_or(CacheDialogState::escape),
            StorageTab::Clean => self.gc.as_mut().is_none_or(|s| s.escape()),
            StorageTab::Trash => self.trash.as_mut().is_none_or(TrashDialogState::escape),
            StorageTab::Files => self
                .files
                .as_mut()
                .is_none_or(CompanionsDialogState::escape),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_change_discards_inactive_tab_snapshots() {
        let conn = caco_core::db::open_memory().unwrap();
        caco_core::db::init_db(&conn).unwrap();
        let mut state = StorageDialogState::new(StorageTab::Cache);
        state.cache = Some(CacheDialogState::new(&conn));
        state.trash = Some(TrashDialogState::new(&conn));
        state.files = Some(CompanionsDialogState::new(&conn));

        state.record_change();

        assert!(state.modified());
        assert!(state.cache.is_some());
        assert!(state.trash.is_none());
        assert!(state.files.is_none());
    }
}
