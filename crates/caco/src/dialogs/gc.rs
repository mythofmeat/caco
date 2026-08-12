//! Reclaim disk space from finished WADs and orphaned files.
//!
//! The GUI answer to `caco gc`. The CLI asks y/n per section because a
//! terminal can only ask one question at a time; here the whole plan is a
//! checkbox list, so a run can keep one WAD's saves while dropping another's.
//!
//! Nothing is measured or deleted in this file — `caco_core::gc` produces the
//! plan and executes the subset that is still ticked when Clean is pressed.

use std::collections::HashSet;

use rusqlite::Connection;

use caco_core::gc::{self, GcOptions, GcPaths, GcPlan, GcSelection, OrphanEntry, WadPlanEntry};
use caco_core::utils::format_size;

use crate::theme;

/// Which list a row belongs to, for the "select all in section" controls.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Wads,
    DataDirs,
    Companions,
    Backups,
}

impl Section {
    const ALL: [Section; 4] = [
        Section::Wads,
        Section::DataDirs,
        Section::Companions,
        Section::Backups,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Wads => "Finished & abandoned WADs",
            Section::DataDirs => "Orphaned data directories",
            Section::Companions => "Orphaned companion files",
            Section::Backups => "Orphaned backups",
        }
    }

    /// Why the rows in this section are safe to remove.
    fn blurb(self) -> &'static str {
        match self {
            Section::Wads => {
                "Completed or abandoned. Cached downloads can be re-fetched; \
                 saves and demos cannot."
            }
            Section::DataDirs | Section::Companions | Section::Backups => {
                "No WAD in the library points at these any more."
            }
        }
    }
}

struct StatusLine {
    text: String,
    is_error: bool,
}

pub struct GcDialogState {
    opts: GcOptions,
    plan: GcPlan,
    /// Ticked rows, keyed by section. Rebuilt whenever the plan is.
    selected_wads: HashSet<i64>,
    selected_orphans: [HashSet<usize>; 3],
    confirming: bool,
    /// Opt-in that lets "All" sweep in WADs whose only copy is in the cache.
    /// Reset by `rescan`, so it never survives the plan it was ticked against.
    include_at_risk: bool,
    status: Option<StatusLine>,
    /// Whether anything was deleted, so the parent knows to reload.
    pub modified: bool,
}

pub enum GcResult {
    Open,
    Closed,
}

impl GcDialogState {
    pub fn new(conn: &Connection) -> Self {
        let mut state = Self {
            opts: GcOptions::default(),
            plan: GcPlan::default(),
            selected_wads: HashSet::new(),
            selected_orphans: Default::default(),
            confirming: false,
            include_at_risk: false,
            status: None,
            modified: false,
        };
        state.rescan(conn);
        state
    }

    /// Re-measure with the current options.
    ///
    /// Rows start ticked, except those whose cache file cannot be re-fetched:
    /// deleting one of those costs the WAD permanently rather than a download,
    /// so reclaiming space must never take one without the user saying so. A
    /// manual WAD with nothing at stake in the cache still starts ticked —
    /// only its irreplaceable file is protected, not the whole entry.
    fn rescan(&mut self, conn: &Connection) {
        self.confirming = false;
        self.include_at_risk = false;
        match gc::plan(conn, self.opts, &GcPaths::from_config()) {
            Ok(plan) => {
                self.selected_wads = plan
                    .wads
                    .iter()
                    .filter(|w| !Self::is_at_risk(w))
                    .map(|w| w.wad_id)
                    .collect();
                self.selected_orphans = [
                    (0..plan.orphan_data_dirs.len()).collect(),
                    (0..plan.orphan_companions.len()).collect(),
                    (0..plan.orphan_backups.len()).collect(),
                ];
                self.plan = plan;
            }
            Err(e) => {
                self.plan = GcPlan::default();
                self.status = Some(StatusLine {
                    text: e.to_string(),
                    is_error: true,
                });
            }
        }
    }

    fn orphan_list(&self, section: Section) -> &[OrphanEntry] {
        match section {
            Section::DataDirs => &self.plan.orphan_data_dirs,
            Section::Companions => &self.plan.orphan_companions,
            Section::Backups => &self.plan.orphan_backups,
            Section::Wads => &[],
        }
    }

    /// Index into `selected_orphans` for an orphan section.
    fn orphan_slot(section: Section) -> usize {
        match section {
            Section::DataDirs => 0,
            Section::Companions => 1,
            Section::Backups => 2,
            Section::Wads => unreachable!("wads are tracked by id, not index"),
        }
    }

    /// Bytes the current ticks would reclaim.
    fn selected_size(&self) -> u64 {
        let wads: u64 = self
            .plan
            .wads
            .iter()
            .filter(|w| self.selected_wads.contains(&w.wad_id))
            .map(|w| w.total_size)
            .sum();

        let orphans: u64 = [Section::DataDirs, Section::Companions, Section::Backups]
            .into_iter()
            .map(|section| {
                let picked = &self.selected_orphans[Self::orphan_slot(section)];
                self.orphan_list(section)
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| picked.contains(i))
                    .map(|(_, o)| o.size)
                    .sum::<u64>()
            })
            .sum();

        wads + orphans
    }

    fn selected_count(&self) -> usize {
        self.selected_wads.len()
            + self
                .selected_orphans
                .iter()
                .map(HashSet::len)
                .sum::<usize>()
    }

    /// Build the selection and delete it.
    ///
    /// Entries are filtered into owned vectors first because `GcSelection`
    /// borrows contiguous slices, and the ticked rows need not be contiguous.
    fn clean(&mut self, conn: &Connection) {
        let wads: Vec<WadPlanEntry> = self
            .plan
            .wads
            .iter()
            .filter(|w| self.selected_wads.contains(&w.wad_id))
            .cloned()
            .collect();

        let picked = |section: Section| -> Vec<OrphanEntry> {
            let chosen = &self.selected_orphans[Self::orphan_slot(section)];
            self.orphan_list(section)
                .iter()
                .enumerate()
                .filter(|(i, _)| chosen.contains(i))
                .map(|(_, o)| o.clone())
                .collect()
        };
        let data_dirs = picked(Section::DataDirs);
        let companions = picked(Section::Companions);
        let backups = picked(Section::Backups);

        let selection = GcSelection {
            wads: &wads,
            orphan_data_dirs: &data_dirs,
            orphan_companions: &companions,
            orphan_backups: &backups,
        };

        match gc::execute(conn, &selection, self.opts) {
            Ok(freed) => {
                self.modified = freed > 0;
                self.status = Some(StatusLine {
                    text: format!("Freed {}.", format_size(freed)),
                    is_error: false,
                });
                self.rescan(conn);
            }
            Err(e) => {
                self.status = Some(StatusLine {
                    text: e.to_string(),
                    is_error: true,
                });
                self.confirming = false;
            }
        }
    }

    pub fn render_body(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, conn: &Connection) {
        self.render_options(ui, conn);
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        if self.plan.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "Nothing to clean up.");
        } else {
            // Chrome: title bar, the keep-toggles row above, and the
            // Clean/Rescan/Close row plus status line below. A constant
            // 340 here was taller than the whole 800x400 minimum window.
            crate::dialogs::modal_body(ctx, ui, 170.0, |ui| {
                for section in Section::ALL {
                    self.render_section(ui, section);
                }
            });
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
        self.render_actions(ui, conn);

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

    /// Escape. Backs out of the staged confirmation before it closes Storage.
    pub fn escape(&mut self) -> bool {
        if self.confirming {
            self.confirming = false;
            return false;
        }
        true
    }

    fn render_options(&mut self, ui: &mut egui::Ui, conn: &Connection) {
        ui.strong("Keep");
        ui.add_space(2.0);

        let before = self.opts;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.opts.keep_saves, "Saves")
                .on_hover_text("Leave .dsg / .zds / .hsg files in place");
            ui.checkbox(&mut self.opts.keep_demos, "Demos");
            ui.checkbox(&mut self.opts.keep_data, "All data dirs");
            ui.checkbox(&mut self.opts.keep_cache, "Cached downloads");
            ui.checkbox(&mut self.opts.keep_companions, "Companions");
            ui.separator();
            ui.checkbox(&mut self.opts.orphans_only, "Orphans only")
                .on_hover_text("Leave finished WADs alone; only sweep unreachable files");
        });

        // Options change what is reclaimable, so the plan has to be re-measured.
        if self.opts != before {
            self.rescan(conn);
        }
    }

    fn render_section(&mut self, ui: &mut egui::Ui, section: Section) {
        let is_wads = section == Section::Wads;
        let len = if is_wads {
            self.plan.wads.len()
        } else {
            self.orphan_list(section).len()
        };
        if len == 0 {
            return;
        }

        let total: u64 = if is_wads {
            self.plan.wads.iter().map(|w| w.total_size).sum()
        } else {
            self.orphan_list(section).iter().map(|o| o.size).sum()
        };

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.strong(section.title());
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!("{len} · {}", format_size(total)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("None").clicked() {
                    self.set_all(section, false);
                }
                if ui.small_button("All").clicked() {
                    self.set_all(section, true);
                }
            });
        });
        ui.colored_label(theme::TEXT_MUTED, section.blurb());
        if is_wads {
            let at_risk = self
                .plan
                .wads
                .iter()
                .filter(|w| Self::is_at_risk(w))
                .count();
            let before = self.include_at_risk;
            crate::dialogs::include_at_risk_toggle(ui, &mut self.include_at_risk, at_risk);
            // Untick what the toggle no longer permits, so the count above the
            // Clean button cannot claim rows the rule now excludes.
            if before && !self.include_at_risk {
                let doomed: Vec<i64> = self
                    .plan
                    .wads
                    .iter()
                    .filter(|w| Self::is_at_risk(w))
                    .map(|w| w.wad_id)
                    .collect();
                for id in doomed {
                    self.selected_wads.remove(&id);
                }
            }
        }
        ui.add_space(2.0);

        if is_wads {
            self.render_wad_rows(ui);
        } else {
            self.render_orphan_rows(ui, section);
        }
    }

    /// Would cleaning this row destroy the only copy of a WAD?
    ///
    /// Only true when there is actually a cache file queued for deletion —
    /// `redownloadable` alone describes the WAD, not what this plan does to it.
    fn is_at_risk(w: &caco_core::gc::WadPlanEntry) -> bool {
        !w.redownloadable && w.cache_path.is_some()
    }

    fn render_wad_rows(&mut self, ui: &mut egui::Ui) {
        // Collect first: the rows borrow `self.plan` while the ticks mutate
        // `self.selected_wads`.
        let rows: Vec<(i64, String, bool)> = self
            .plan
            .wads
            .iter()
            .map(|w| {
                let mut detail = Vec::new();
                if w.data_size > 0 {
                    detail.push(format!("data {}", format_size(w.data_size)));
                }
                if w.cache_size > 0 {
                    detail.push(format!("cache {}", format_size(w.cache_size)));
                }
                if w.companion_size > 0 {
                    detail.push(format!("companions {}", format_size(w.companion_size)));
                }
                (
                    w.wad_id,
                    format!(
                        "{}{}  ·  {}  ·  {}",
                        if Self::is_at_risk(w) { "\u{26a0} " } else { "" },
                        w.title,
                        format_size(w.total_size),
                        detail.join(", ")
                    ),
                    Self::is_at_risk(w),
                )
            })
            .collect();

        for (wad_id, label, at_risk) in rows {
            let mut checked = self.selected_wads.contains(&wad_id);
            let response = if at_risk {
                ui.checkbox(
                    &mut checked,
                    egui::RichText::new(label).color(theme::COLOR_WARNING),
                )
            } else {
                ui.checkbox(&mut checked, label)
            };
            if at_risk {
                response.on_hover_text(
                    "No idgames source: caco cannot download this WAD again. \
                     Deleting the cached file destroys the only copy.",
                );
            }
            if checked {
                self.selected_wads.insert(wad_id);
            } else {
                self.selected_wads.remove(&wad_id);
            }
        }
    }

    fn render_orphan_rows(&mut self, ui: &mut egui::Ui, section: Section) {
        let rows: Vec<(usize, String)> = self
            .orphan_list(section)
            .iter()
            .enumerate()
            .map(|(i, o)| {
                (
                    i,
                    format!("{}  ·  {}", o.display_name(), format_size(o.size)),
                )
            })
            .collect();

        let slot = Self::orphan_slot(section);
        for (idx, label) in rows {
            let mut checked = self.selected_orphans[slot].contains(&idx);
            ui.checkbox(&mut checked, label);
            if checked {
                self.selected_orphans[slot].insert(idx);
            } else {
                self.selected_orphans[slot].remove(&idx);
            }
        }
    }

    fn set_all(&mut self, section: Section, on: bool) {
        if section == Section::Wads {
            // "All" used to mean literally all, sweeping in rows that `rescan`
            // had deliberately left unticked — one click, no warning, and the
            // only copy of a WAD gone. It now means "all the ones this plan is
            // allowed to touch", which the toggle beside it decides.
            let include_at_risk = self.include_at_risk;
            self.selected_wads = if on {
                self.plan
                    .wads
                    .iter()
                    .filter(|w| include_at_risk || !Self::is_at_risk(w))
                    .map(|w| w.wad_id)
                    .collect()
            } else {
                HashSet::new()
            };
            return;
        }
        let len = self.orphan_list(section).len();
        let slot = Self::orphan_slot(section);
        self.selected_orphans[slot] = if on {
            (0..len).collect()
        } else {
            HashSet::new()
        };
    }

    fn render_actions(&mut self, ui: &mut egui::Ui, conn: &Connection) {
        let count = self.selected_count();
        let size = self.selected_size();

        if self.confirming {
            ui.colored_label(
                theme::COLOR_ERROR,
                format!(
                    "Permanently delete {count} item(s), freeing {}?",
                    format_size(size)
                ),
            );
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "Save games and demos cannot be recovered.",
            );
            ui.horizontal(|ui| {
                if ui.button("Delete").clicked() {
                    self.confirming = false;
                    self.clean(conn);
                }
                if ui.button("Cancel").clicked() {
                    self.confirming = false;
                }
            });
            return;
        }

        ui.horizontal(|ui| {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!("{count} selected · {}", format_size(size)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Rescan").clicked() {
                    self.rescan(conn);
                }
                if ui
                    .add_enabled(count > 0, egui::Button::new("Clean"))
                    .clicked()
                {
                    self.confirming = true;
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::Status;
    use std::path::PathBuf;

    fn entry(redownloadable: bool, cache_path: Option<&str>) -> WadPlanEntry {
        WadPlanEntry {
            wad_id: 1,
            title: "T".into(),
            status: Status::Completed,
            redownloadable,
            data_dir: None,
            data_size: 0,
            cache_path: cache_path.map(PathBuf::from),
            cache_size: if cache_path.is_some() { 100 } else { 0 },
            companion_ids: Vec::new(),
            companion_size: 0,
            total_size: 100,
            has_stats_snapshot: false,
        }
    }

    fn state(wads: Vec<WadPlanEntry>) -> GcDialogState {
        GcDialogState {
            opts: GcOptions::default(),
            plan: GcPlan {
                wads,
                ..GcPlan::default()
            },
            selected_wads: HashSet::new(),
            selected_orphans: Default::default(),
            confirming: false,
            include_at_risk: false,
            status: None,
            modified: false,
        }
    }

    /// "All" means "all the rows this plan is allowed to touch", not literally
    /// all of them. Sweeping in an at-risk row on one unarmed click is how the
    /// only copy of a WAD gets deleted.
    #[test]
    fn test_select_all_skips_at_risk_until_armed() {
        let safe = WadPlanEntry {
            wad_id: 1,
            ..entry(true, Some("/cache/a.zip"))
        };
        let risky = WadPlanEntry {
            wad_id: 2,
            ..entry(false, Some("/cache/b.zip"))
        };
        let mut s = state(vec![safe, risky]);

        s.set_all(Section::Wads, true);
        assert_eq!(s.selected_wads, HashSet::from([1]));

        s.include_at_risk = true;
        s.set_all(Section::Wads, true);
        assert_eq!(s.selected_wads, HashSet::from([1, 2]));

        s.set_all(Section::Wads, false);
        assert!(s.selected_wads.is_empty());
    }

    /// A manual WAD with nothing queued in the cache is an ordinary row, so
    /// "All" takes it whether or not the opt-in is ticked.
    #[test]
    fn test_select_all_takes_manual_wads_with_no_cached_file() {
        let mut s = state(vec![WadPlanEntry {
            wad_id: 7,
            ..entry(false, None)
        }]);
        s.set_all(Section::Wads, true);
        assert_eq!(s.selected_wads, HashSet::from([7]));
    }

    /// The only row worth protecting is one where this plan would delete an
    /// irreplaceable file. A manual WAD with no cached file queued is an
    /// ordinary row.
    #[test]
    fn test_at_risk_needs_both_manual_and_a_cache_file() {
        assert!(GcDialogState::is_at_risk(&entry(
            false,
            Some("/cache/a.zip")
        )));
        assert!(!GcDialogState::is_at_risk(&entry(
            true,
            Some("/cache/a.zip")
        )));
        assert!(!GcDialogState::is_at_risk(&entry(false, None)));
        assert!(!GcDialogState::is_at_risk(&entry(true, None)));
    }
}
