//! Per-WAD save, backup, and demo management.
//!
//! The GUI answer to `caco saves` and `caco demos`. Both commands operate on
//! the same managed data directory, so they share one dialog here rather than
//! two — the CLI only split them because a subcommand can't have tabs.
//!
//! Destructive actions stage a confirmation instead of acting on the click.

use std::path::PathBuf;

use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use caco_core::demos::{self, DemoFile};
use caco_core::saves::{self, BackupInfo, SaveFile};
use caco_core::utils::format_size;

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

struct StatusLine {
    text: String,
    is_error: bool,
}

pub struct WadDataDialogState {
    wad_id: i64,
    wad_title: String,
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
        let data_dir = caco_core::config::find_wad_data_dir(wad_id)
            .unwrap_or_else(|| caco_core::config::get_wad_data_dir(wad_id, &wad.title));

        let mut state = Self {
            wad_id,
            wad_title: wad.title,
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
        self.backups = saves::list_backups(self.wad_id);
        self.demos = demos::find_demo_files(&self.data_dir);
        self.pending = None;
        self.selected_backup = (!self.backups.is_empty()).then_some(0);
        self.selected_demo = (!self.demos.is_empty()).then_some(0);
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

    fn backup_now(&mut self) {
        if !self.data_dir.is_dir() {
            self.set_error(format!("No data directory for '{}'.", self.wad_title));
            return;
        }
        match saves::create_backup(self.wad_id, &self.wad_title, &self.data_dir) {
            Ok(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.reload();
                self.set_info(format!("Backup created: {name}"));
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
                self.set_info(format!("Deleted {deleted} save file(s)."));
            }
            Pending::RestoreBackup(idx) => {
                let Some(backup) = self.backups.get(idx) else {
                    return;
                };
                let (path, name) = (backup.path.clone(), backup.name.clone());
                match saves::restore_backup(&path, &self.data_dir) {
                    Ok(count) => {
                        self.reload();
                        self.set_info(format!("Restored {count} file(s) from {name}."));
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
                self.set_info(format!("Deleted {deleted} demo file(s)."));
            }
        }
    }

    /// Human-readable description of what the pending action will do.
    fn pending_prompt(&self) -> Option<String> {
        Some(match self.pending.as_ref()? {
            Pending::CleanSaves => format!(
                "Delete {} save file(s) for '{}'? Back up first if you might want them back.",
                self.saves.len(),
                self.wad_title
            ),
            Pending::RestoreBackup(idx) => format!(
                "Restore from {}? This overwrites the current data directory.",
                self.backups
                    .get(*idx)
                    .map(|b| b.name.as_str())
                    .unwrap_or("backup")
            ),
            Pending::DeleteDemo(idx) => format!(
                "Delete demo {}?",
                self.demos
                    .get(*idx)
                    .map(|d| d.name.as_str())
                    .unwrap_or("file")
            ),
            Pending::CleanDemos => {
                format!("Delete all {} demo file(s)?", self.demos.len())
            }
        })
    }

    pub fn render(&mut self, ctx: &egui::Context) -> WadDataResult {
        let mut result = WadDataResult::Open;

        crate::dialogs::modal_window(ctx, "Saves & Demos", [720.0, 460.0]).show(ctx, |ui| {
            ui.strong(&self.wad_title);
            ui.colored_label(theme::TEXT_SECONDARY, self.data_dir.display().to_string());
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Saves,
                    format!("Saves ({})", self.saves.len()),
                );
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Backups,
                    format!("Backups ({})", self.backups.len()),
                );
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Demos,
                    format!("Demos ({})", self.demos.len()),
                );
            });
            ui.add_space(6.0);

            match self.tab {
                Tab::Saves => self.render_saves(ui),
                Tab::Backups => self.render_backups(ui),
                Tab::Demos => self.render_demos(ui),
            }

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            if let Some(prompt) = self.pending_prompt() {
                self.render_confirmation(ui, &prompt);
            } else {
                self.render_actions(ui, &mut result);
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

    fn render_confirmation(&mut self, ui: &mut egui::Ui, prompt: &str) {
        ui.colored_label(theme::COLOR_ERROR, prompt);
        ui.horizontal(|ui| {
            if ui.button("Confirm").clicked() {
                self.run_pending();
            }
            if ui.button("Cancel").clicked() {
                self.pending = None;
            }
        });
    }

    fn render_actions(&mut self, ui: &mut egui::Ui, result: &mut WadDataResult) {
        ui.horizontal(|ui| {
            match self.tab {
                Tab::Saves => {
                    if ui
                        .button("Back Up Now")
                        .on_hover_text("Zip the whole data directory into the backups folder")
                        .clicked()
                    {
                        self.backup_now();
                    }
                    if ui
                        .add_enabled(!self.saves.is_empty(), egui::Button::new("Delete Saves"))
                        .clicked()
                    {
                        self.pending = Some(Pending::CleanSaves);
                    }
                }
                Tab::Backups => {
                    let has_selection = self.selected_backup.is_some();
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Restore Selected"))
                        .clicked()
                        && let Some(idx) = self.selected_backup
                    {
                        self.pending = Some(Pending::RestoreBackup(idx));
                    }
                }
                Tab::Demos => {
                    let has_selection = self.selected_demo.is_some();
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Play"))
                        .clicked()
                        && let Some(demo) = self.selected_demo.and_then(|i| self.demos.get(i))
                    {
                        *result = WadDataResult::PlayDemo {
                            wad_id: self.wad_id,
                            demo: demo.name.clone(),
                        };
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Delete"))
                        .clicked()
                        && let Some(idx) = self.selected_demo
                    {
                        self.pending = Some(Pending::DeleteDemo(idx));
                    }
                    if ui
                        .add_enabled(!self.demos.is_empty(), egui::Button::new("Delete All"))
                        .clicked()
                    {
                        self.pending = Some(Pending::CleanDemos);
                    }
                }
            }

            if ui.button("Close").clicked() {
                *result = WadDataResult::Closed;
            }
        });
    }

    fn render_saves(&mut self, ui: &mut egui::Ui) {
        if self.saves.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "No save files.");
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
        file_table(ui, ["File", "Size", "Modified"], &rows, None);
    }

    fn render_backups(&mut self, ui: &mut egui::Ui) {
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
            ["Backup", "Size", "Created"],
            &rows,
            Some(&mut self.selected_backup),
        );
    }

    fn render_demos(&mut self, ui: &mut egui::Ui) {
        if self.demos.is_empty() {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "No demos. Record one by playing with demo recording enabled.",
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
    headers: [&str; 3],
    rows: &[[String; 3]],
    selection: Option<&mut Option<usize>>,
) {
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let mut clicked = None;
    let selected = selection.as_ref().and_then(|s| **s);

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .sense(egui::Sense::click())
        .column(Column::remainder().at_least(180.0))
        .column(Column::initial(90.0).at_least(60.0))
        .column(Column::initial(160.0).at_least(100.0))
        .max_scroll_height(280.0)
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
