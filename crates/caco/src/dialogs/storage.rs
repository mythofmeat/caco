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
//! window, the tab strip, the shared Close button, and the routing of Escape —
//! chrome that was previously copy-pasted four times.

use rusqlite::Connection;

use crate::dialogs::cache::CacheDialogState;
use crate::dialogs::companions::CompanionsDialogState;
use crate::dialogs::gc::GcDialogState;
use crate::dialogs::trash::TrashDialogState;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StorageTab {
    Cache,
    Clean,
    Trash,
    Files,
}

impl StorageTab {
    const ALL: [StorageTab; 4] = [
        StorageTab::Cache,
        StorageTab::Clean,
        StorageTab::Trash,
        StorageTab::Files,
    ];

    fn label(self) -> &'static str {
        match self {
            StorageTab::Cache => "Cache",
            StorageTab::Clean => "Clean",
            StorageTab::Trash => "Trash",
            StorageTab::Files => "Files",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            StorageTab::Cache => "Downloaded WAD files on disk",
            StorageTab::Clean => "Reclaim space from finished WADs and orphans",
            StorageTab::Trash => "Restore or permanently delete removed WADs",
            StorageTab::Files => "Companion files and the WADs using them",
        }
    }
}

pub enum StorageResult {
    Open,
    Closed,
}

/// Height of what this window draws around whichever tab is showing: the title
/// bar, the tab strip, the blurb, both separators and the Close row.
const CHROME: f32 = 150.0;

/// Sized for the widest tab (Clean's row list). Every tab gets the same body
/// height whatever it holds, so switching tabs cannot resize the window under
/// the pointer — and an empty Trash does not leave a full-screen window with
/// three rows in it.
const DESIRED: [f32; 2] = [860.0, 600.0];

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

        crate::dialogs::modal_window(ctx, "Storage", DESIRED).show(ctx, |ui| {
            self.render_tab_strip(ui);
            ui.add_space(6.0);
            ui.colored_label(theme::TEXT_MUTED, self.tab.blurb());
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            // One budget for the whole tab, handed down. The tabs used to ask
            // the screen how tall they could be, which ignored everything this
            // window draws around them and overflowed the viewport by the
            // same amount on every tab.
            //
            // The tab is also wrapped in the budget rather than merely told
            // about it: a tab's own furniture — Cache's summary, at-risk
            // toggle and two-button row — can exceed the whole budget at the
            // 800x400 minimum window, and then no cap on its list saves it.
            // This scroller only engages in that case.
            let avail = crate::dialogs::modal_body_height(ctx, CHROME).min(DESIRED[1] - CHROME);
            crate::dialogs::scroll_body(ui, avail, |ui| {
                self.render_active_tab(ui, conn, avail);
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        result = StorageResult::Closed;
                    }
                });
            });
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.escape() {
            return StorageResult::Closed;
        }

        result
    }

    fn render_tab_strip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tab in StorageTab::ALL {
                let active = self.tab == tab;
                let text = egui::RichText::new(tab.label())
                    .size(13.0)
                    .color(if active {
                        theme::TEXT_ACCENT
                    } else {
                        theme::TEXT_SECONDARY
                    });
                if ui
                    .selectable_label(active, text)
                    .on_hover_text(tab.blurb())
                    .clicked()
                {
                    self.tab = tab;
                }
            }
        });
    }

    fn render_active_tab(&mut self, ui: &mut egui::Ui, conn: &Connection, avail: f32) {
        let changed = match self.tab {
            StorageTab::Cache => {
                let state = self
                    .cache
                    .get_or_insert_with(|| CacheDialogState::new(conn));
                state.render_body(ui, conn, avail);
                std::mem::take(&mut state.modified)
            }
            StorageTab::Clean => {
                let state = self
                    .gc
                    .get_or_insert_with(|| Box::new(GcDialogState::new(conn)));
                state.render_body(ui, conn, avail);
                std::mem::take(&mut state.modified)
            }
            StorageTab::Trash => {
                let state = self
                    .trash
                    .get_or_insert_with(|| TrashDialogState::new(conn));
                state.render_body(ui, conn, avail);
                std::mem::take(&mut state.modified)
            }
            StorageTab::Files => {
                let state = self
                    .files
                    .get_or_insert_with(|| CompanionsDialogState::new(conn));
                state.render_body(ui, conn, avail);
                std::mem::take(&mut state.modified)
            }
        };

        if changed {
            self.record_change();
        }
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
    /// Every tab stages its destructive actions behind a confirmation, and the
    /// key that dismisses one must not also close the dialog around it —
    /// otherwise backing out of "Empty Trash?" throws away the whole view.
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
