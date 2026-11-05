//! Per-WAD save, backup, and demo management: all one dialog, since all of it is in the
//! same folder. Destructive actions stage a confirmation instead of acting on the click.

use std::path::PathBuf;

use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use caco_core::demos::{self, DemoFile};
use caco_core::saves::{self, BackupInfo, SaveFile};
use caco_core::utils::format_size;

use crate::dialogs::{Dialog, Primary, StatusLine, count, tab_bar};
use crate::theme;

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Saves,
    Backups,
    Demos,
}

/// A destructive action awaiting confirmation.
enum Pending {
    CleanSaves,
    RestoreBackup(usize),
    DeleteDemo(usize),
    CleanDemos,
}

pub struct WadDataDialogState {
    wad_id: i64,
    wad_title: String,
    data_key: String,
    data_dir: PathBuf,
    tab: Tab,
    saves: Vec<SaveFile>,
    backups: Vec<BackupInfo>,
    demos: Vec<DemoFile>,
    selected_backup: Option<usize>,
    selected_demo: Option<usize>,
    pending: Option<Pending>,
    status: Option<StatusLine>,
}

pub enum WadDataResult {
    Open,
    Closed,
    /// Play back a demo. Blocking, so the app dispatches it to a worker
    /// thread rather than the dialog running it inline.
    PlayDemo {
        wad_id: i64,
        demo: String,
    },
}

impl WadDataDialogState {
    /// Returns `None` when the WAD row is gone.
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let wad = caco_core::db::get_wad(conn, wad_id, false).ok()??;
        let data_dir = caco_core::config::find_wad_data_dir(&wad.data_key, &wad.title)
            .unwrap_or_else(|| caco_core::config::get_wad_data_dir(&wad.data_key, &wad.title));

        let mut state = Self {
            wad_id,
            wad_title: wad.title,
            data_key: wad.data_key,
            data_dir,
            tab: Tab::Saves,
            saves: Vec::new(),
            backups: Vec::new(),
            demos: Vec::new(),
            selected_backup: None,
            selected_demo: None,
            pending: None,
            status: None,
        };
        state.reload();
        Some(state)
    }

    fn reload(&mut self) {
        self.saves = saves::find_save_files(&self.data_dir);
        self.backups = saves::list_backups(&self.data_key);
        self.demos = demos::find_demo_files(&self.data_dir);
        self.pending = None;
        self.selected_backup = (!self.backups.is_empty()).then_some(0);
        self.selected_demo = (!self.demos.is_empty()).then_some(0);
    }

    fn set_error(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine::error(text));
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine::info(text));
    }

    fn backup_now(&mut self) {
        if !self.data_dir.is_dir() {
            self.set_error(format!("'{}' has nothing to back up yet.", self.wad_title));
            return;
        }
        match saves::create_backup(&self.data_key, &self.wad_title, &self.data_dir) {
            Ok(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.reload();
                self.set_info(format!("Backed up as {name}."));
            }
            Err(e) => self.set_error(e.to_string()),
        }
    }

    fn run_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match pending {
            Pending::CleanSaves => {
                let deleted = saves::clean_save_files(&self.data_dir).len();
                self.reload();
                self.set_info(format!(
                    "Deleted {}.",
                    count(deleted, "save file", "save files")
                ));
            }
            Pending::RestoreBackup(idx) => {
                let Some(backup) = self.backups.get(idx) else {
                    return;
                };
                let (path, name) = (backup.path.clone(), backup.name.clone());
                match saves::restore_backup(&path, &self.data_dir) {
                    Ok(restored) => {
                        self.reload();
                        self.set_info(format!(
                            "Restored {} from {name}.",
                            count(restored, "file", "files")
                        ));
                    }
                    Err(e) => self.set_error(e.to_string()),
                }
            }
            Pending::DeleteDemo(idx) => {
                let Some(demo) = self.demos.get(idx) else {
                    return;
                };
                let (path, name) = (demo.path.clone(), demo.name.clone());
                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        self.reload();
                        self.set_info(format!("Deleted {name}."));
                    }
                    Err(e) => self.set_error(e.to_string()),
                }
            }
            Pending::CleanDemos => {
                let deleted = demos::clean_demo_files(&self.data_dir).len();
                self.reload();
                self.set_info(format!("Deleted {}.", count(deleted, "demo", "demos")));
            }
        }
    }

    /// What the pending action is about to do, as a question.
    fn pending_prompt(&self) -> Option<String> {
        Some(match self.pending.as_ref()? {
            Pending::CleanSaves => format!(
                "Delete {}? Back up first if you might want them back.",
                count(self.saves.len(), "save file", "save files")
            ),
            Pending::RestoreBackup(idx) => format!(
                "Restore from {}? This replaces the saves there are now.",
                self.backups
                    .get(*idx)
                    .map(|b| b.name.as_str())
                    .unwrap_or("backup")
            ),
            Pending::DeleteDemo(idx) => format!(
                "Delete {}?",
                self.demos
                    .get(*idx)
                    .map(|d| d.name.as_str())
                    .unwrap_or("this demo")
            ),
            Pending::CleanDemos => {
                format!("Delete {}?", count(self.demos.len(), "demo", "demos"))
            }
        })
    }

    /// What going ahead with the pending action is called, on its button.
    fn pending_verb(&self) -> &'static str {
        match self.pending {
            Some(Pending::RestoreBackup(_)) => "Restore",
            _ => "Delete",
        }
    }

    pub fn render(&mut self, ctx: &egui::Context) -> WadDataResult {
        let mut result = WadDataResult::Open;
        let tabs = [
            (Tab::Saves, format!("Saves ({})", self.saves.len())),
            (Tab::Backups, format!("Backups ({})", self.backups.len())),
            (Tab::Demos, format!("Demos ({})", self.demos.len())),
        ];

        Dialog::new("Saves & Demos", [720.0, 480.0])
            .about(&self.wad_title)
            .show(ctx, |dialog| {
                dialog
                    .pinned(|ui| tab_bar(ui, &mut self.tab, &tabs))
                    .fill(|ui, height| match self.tab {
                        Tab::Saves => self.render_saves(ui, height),
                        Tab::Backups => self.render_backups(ui, height),
                        Tab::Demos => self.render_demos(ui, height),
                    })
                    .above(|ui| {
                        if let Some(prompt) = self.pending_prompt() {
                            ui.colored_label(theme::COLOR_ERROR, prompt);
                        }
                        if let Some(status) = &self.status {
                            status.show(ui);
                        }
                    })
                    .buttons(|row| {
                        if self.pending.is_some() {
                            let verb = self.pending_verb();
                            let ended = row.end(Some("Cancel"), Some(Primary::danger(verb)));
                            if ended.dismissed {
                                self.pending = None;
                            }
                            if ended.primary {
                                self.run_pending();
                            }
                            return;
                        }
                        row.left(|ui| self.render_actions(ui, &mut result));
                        if row.close() {
                            result = WadDataResult::Closed;
                        }
                    })
            });

        // Escape dismisses a pending confirmation before it closes the dialog.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.pending.is_some() {
                self.pending = None;
            } else {
                return WadDataResult::Closed;
            }
        }

        result
    }

    /// What can be done on this tab, of which there is nothing to do to what isn't there.
    fn render_actions(&mut self, ui: &mut egui::Ui, result: &mut WadDataResult) {
        match self.tab {
            Tab::Saves => {
                if ui
                    .button("Back Up Now")
                    .on_hover_text("Zip this WAD's saves and stats into the backups folder")
                    .clicked()
                {
                    self.backup_now();
                }
                if !self.saves.is_empty() && ui.button("Delete Saves").clicked() {
                    self.pending = Some(Pending::CleanSaves);
                }
            }
            Tab::Backups => {
                if let Some(idx) = self.selected_backup
                    && ui.button("Restore Selected").clicked()
                {
                    self.pending = Some(Pending::RestoreBackup(idx));
                }
            }
            Tab::Demos => {
                if let Some(idx) = self.selected_demo {
                    if ui.button("Play").clicked()
                        && let Some(demo) = self.demos.get(idx)
                    {
                        *result = WadDataResult::PlayDemo {
                            wad_id: self.wad_id,
                            demo: demo.name.clone(),
                        };
                    }
                    if ui.button("Delete").clicked() {
                        self.pending = Some(Pending::DeleteDemo(idx));
                    }
                }
                if !self.demos.is_empty() && ui.button("Delete All").clicked() {
                    self.pending = Some(Pending::CleanDemos);
                }
            }
        }
    }

    fn render_saves(&mut self, ui: &mut egui::Ui, height: f32) {
        if self.saves.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "No saves yet.");
            return;
        }

        let total: u64 = self.saves.iter().map(|s| s.size).sum();
        ui.colored_label(theme::TEXT_SECONDARY, format_size(total));
        ui.add_space(2.0);

        let rows: Vec<[String; 3]> = self
            .saves
            .iter()
            .map(|s| {
                [
                    s.rel_path.clone(),
                    format_size(s.size),
                    format_timestamp(&s.mtime_iso),
                ]
            })
            .collect();
        file_table(ui, height - 20.0, ["File", "Size", "Modified"], &rows, None);
    }

    fn render_backups(&mut self, ui: &mut egui::Ui, height: f32) {
        if self.backups.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No backups yet. Use Back Up Now on the Saves tab.",
            );
            return;
        }

        let rows: Vec<[String; 3]> = self
            .backups
            .iter()
            .map(|b| {
                [
                    b.name.clone(),
                    format_size(b.size),
                    format_timestamp(&b.created_iso),
                ]
            })
            .collect();
        file_table(
            ui,
            height,
            ["Backup", "Size", "Created"],
            &rows,
            Some(&mut self.selected_backup),
        );
    }

    fn render_demos(&mut self, ui: &mut egui::Ui, height: f32) {
        if self.demos.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No demos yet. Record one by playing with demo recording turned on.",
            );
            return;
        }

        let rows: Vec<[String; 3]> = self
            .demos
            .iter()
            .map(|d| {
                [
                    d.name.clone(),
                    format_size(d.size),
                    format_timestamp(&d.mtime_iso),
                ]
            })
            .collect();
        file_table(
            ui,
            height,
            ["Demo", "Size", "Recorded"],
            &rows,
            Some(&mut self.selected_demo),
        );
    }
}

/// Render a three-column file listing. Passing `selection` makes rows
/// clickable; passing `None` renders a read-only list.
fn file_table(
    ui: &mut egui::Ui,
    height: f32,
    headers: [&str; 3],
    rows: &[[String; 3]],
    selection: Option<&mut Option<usize>>,
) {
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let mut clicked = None;
    let selected = selection.as_ref().and_then(|s| **s);

    TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .sense(egui::Sense::click())
        .column(Column::remainder().at_least(180.0).clip(true))
        .column(Column::initial(90.0).at_least(60.0))
        .column(Column::initial(160.0).at_least(100.0))
        .max_scroll_height((height - row_height).max(60.0))
        .header(row_height + 2.0, |mut header| {
            for label in headers {
                header.col(|ui| {
                    ui.strong(label);
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rows.len(), |mut row| {
                let idx = row.index();
                row.set_selected(selected == Some(idx));
                for cell in &rows[idx] {
                    row.col(|ui| {
                        ui.label(cell);
                    });
                }
                if row.response().clicked() {
                    clicked = Some(idx);
                }
            });
        });

    if let (Some(slot), Some(idx)) = (selection, clicked) {
        *slot = Some(idx);
    }
}

/// Render an RFC3339 timestamp the way the rest of the GUI shows times,
/// falling back to the raw string when it can't be parsed.
fn format_timestamp(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|dt| {
            crate::relative_time::relative_time(&dt.with_timezone(&chrono::Local).naive_local())
        })
        .unwrap_or_else(|_| iso.to_string())
}
