//! Per-WAD save, backup, and demo management: all one dialog, since all of it is in the
//! same folder. Destructive actions stage a confirmation instead of acting on the click, and a
//! rename asks for the name in the same place.
//!
//! A demo is listed by what it says about itself (`caco_core::demos::header`) and by what caco
//! recorded it with (`caco_core::demos::record`), not by its file: a file's time is whenever a
//! sync tool last wrote it, and its name is whatever somebody called it.

use std::path::PathBuf;

use egui_extras::Column;
use rusqlite::Connection;

use caco_core::demos::{self, DemoFile};
use caco_core::saves::{self, BackupInfo, SaveFile};
use caco_core::utils::format_size;

use crate::dialogs::{Dialog, Primary, StatusLine, count, dialog_table, form_label, tab_bar};
use crate::theme;
use crate::workers::{FileDialogReceiver, FileDialogRequest, spawn_file_dialog};

/// What a column says when there is nothing to say.
const NOTHING: &str = "\u{2014}";

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Saves,
    Backups,
    Demos,
}

/// An action awaiting confirmation: a destructive one, or a rename awaiting its name.
enum Pending {
    CleanSaves,
    RestoreBackup(usize),
    DeleteDemo(usize),
    CleanDemos,
    RenameDemo {
        idx: usize,
        name: String,
        /// The field has had focus given to it, which happens once.
        focused: bool,
    },
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
    /// Whether the WAD's maps go by episode, to read a demo's start map by.
    episode_names: bool,
    /// Add From File's picker, while it is open.
    picker: Option<FileDialogReceiver>,
    /// Add From File was pressed, and the picker opens once there is a context to open it with.
    open_picker: bool,
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
        let maps: Vec<String> = caco_core::db::get_analysis(conn, wad_id)
            .ok()
            .flatten()
            .map(|analysis| analysis.maps.into_iter().map(|map| map.lump).collect())
            .unwrap_or_default();
        let episode_names = demos::uses_episode_names(&maps, wad.custom_iwad.as_deref());

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
            episode_names,
            picker: None,
            open_picker: false,
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

    /// Reload, and select the demo at `path` if it is there.
    fn reload_selecting(&mut self, path: &std::path::Path) {
        self.reload();
        if let Some(idx) = self.demos.iter().position(|d| d.path == path) {
            self.selected_demo = Some(idx);
        }
    }

    /// Show the demos tab.
    pub fn show_demos(mut self) -> Self {
        self.tab = Tab::Demos;
        self
    }

    /// Copy a demo from somewhere else in, from what the picker chose.
    fn add_demo(&mut self, source: &std::path::Path) {
        match demos::add_demo_from_file(&self.data_dir, source) {
            Ok(added) => {
                let name = added
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.reload_selecting(&added);
                self.tab = Tab::Demos;
                self.set_info(format!("Added {name}."));
            }
            Err(e) => self.set_error(e.to_string()),
        }
    }

    /// Open the demos folder in the file manager, making it first if there are no demos yet.
    fn open_folder(&mut self) {
        let dir = demos::demos_dir(&self.data_dir);
        let opened = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| open::that(&dir).map_err(|e| e.to_string()));
        if let Err(e) = opened {
            self.set_error(format!("Couldn't open the folder: {e}"));
        }
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
                match demos::delete_demo(&path) {
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
            Pending::RenameDemo { idx, name, .. } => {
                let Some(demo) = self.demos.get(idx) else {
                    return;
                };
                let old = demo.name.clone();
                match demos::rename_demo(&demo.path.clone(), &name) {
                    Ok(renamed) => {
                        let new = renamed
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.reload_selecting(&renamed);
                        if new != old {
                            self.set_info(format!("Renamed {old} to {new}."));
                        }
                    }
                    Err(e) => {
                        // Keep asking, with what was typed, since the name is what was wrong.
                        self.pending = Some(Pending::RenameDemo {
                            idx,
                            name,
                            focused: false,
                        });
                        self.set_error(e.to_string());
                    }
                }
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
            // Asked by its field, not in words.
            Pending::RenameDemo { .. } => return None,
        })
    }

    /// What going ahead with the pending action is, on its button.
    fn pending_button(&self) -> Primary<'static> {
        match self.pending {
            Some(Pending::RestoreBackup(_)) => Primary::danger("Restore"),
            Some(Pending::RenameDemo { .. }) => Primary::new("Rename"),
            _ => Primary::danger("Delete"),
        }
    }

    /// The name a rename is asking for, in the footer. True if Enter was pressed in it.
    fn rename_field(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(Pending::RenameDemo { name, focused, .. }) = &mut self.pending else {
            return false;
        };
        let mut entered = false;
        ui.horizontal(|ui| {
            let label = form_label(ui, "New name");
            let field = ui
                .add(egui::TextEdit::singleline(name).desired_width(320.0))
                .labelled_by(label.id);

            if !*focused {
                field.request_focus();
                *focused = true;
            }
            entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
        entered
    }

    pub fn render(&mut self, ctx: &egui::Context) -> WadDataResult {
        let mut result = WadDataResult::Open;
        if std::mem::take(&mut self.open_picker) && self.picker.is_none() {
            let request = FileDialogRequest::open()
                .add_filter("Demos", &["lmp"])
                .add_filter("All files", &["*"]);
            self.picker = Some(spawn_file_dialog(Some(ctx.clone()), request));
        }
        if let Some(picker) = &self.picker
            && let Ok(picked) = picker.try_recv()
        {
            self.picker = None;
            if let Some(path) = picked {
                self.add_demo(&path);
            }
        }
        let mut entered = false;
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
                        entered = self.rename_field(ui);
                        if let Some(status) = &self.status {
                            status.show(ui);
                        }
                    })
                    .buttons(|row| {
                        if self.pending.is_some() {
                            let ended = row.end(Some("Cancel"), Some(self.pending_button()));
                            if ended.dismissed {
                                self.pending = None;
                            }
                            if ended.primary || entered {
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
                if let Some(idx) = self.selected_demo
                    && let Some(demo) = self.demos.get(idx)
                {
                    if ui.button("Play").clicked() {
                        *result = WadDataResult::PlayDemo {
                            wad_id: self.wad_id,
                            demo: demo.name.clone(),
                        };
                    }
                    if ui.button("Rename").clicked() {
                        let name = demo
                            .name
                            .strip_suffix(demos::DEMO_EXTENSION)
                            .unwrap_or(&demo.name)
                            .to_string();
                        self.status = None;
                        self.pending = Some(Pending::RenameDemo {
                            idx,
                            name,
                            focused: false,
                        });
                    }
                    if ui.button("Delete").clicked() {
                        self.pending = Some(Pending::DeleteDemo(idx));
                    }
                }
                if self.demos.len() > 1 && ui.button("Delete All").clicked() {
                    self.pending = Some(Pending::CleanDemos);
                }
                // A spinner while the picker is open, which also stops it opening twice.
                if self.picker.is_some() {
                    ui.spinner();
                } else if ui
                    .button("Add From File")
                    .on_hover_text("Copy a demo in: one from a speedrun archive, or one that came with the WAD")
                    .clicked()
                {
                    self.open_picker = true;
                }
                if ui
                    .button("Open Folder")
                    .on_hover_text("Open the folder this WAD's demos are in")
                    .clicked()
                {
                    self.open_folder();
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
                "No demos yet. Play and Record (R) records one, and Add From File brings in \
                 one from somewhere else.",
            );
            return;
        }

        let rows: Vec<DemoRow> = self
            .demos
            .iter()
            .map(|d| DemoRow::of(d, self.episode_names))
            .collect();
        demo_table(ui, height, &rows, &mut self.selected_demo);
    }
}

/// A demo as the table shows it.
struct DemoRow {
    cells: [String; 6],
    /// What there is to say about it that doesn't fit in a column.
    hint: String,
}

impl DemoRow {
    fn of(demo: &DemoFile, episode_names: bool) -> Self {
        let header = demo.header.as_ref();
        let or_nothing = |text: Option<String>| text.unwrap_or_else(|| NOTHING.to_string());
        let cells = [
            demo.name.clone(),
            or_nothing(header.and_then(|h| h.map_name(episode_names))),
            or_nothing(header.and_then(|h| h.skill_name())),
            or_nothing(
                header
                    .and_then(|h| h.duration_secs())
                    .map(|secs| caco_core::player::format_duration(i64::from(secs))),
            ),
            or_nothing(demo.sourceport()),
            or_nothing(demo.recorded_at().map(format_timestamp)),
        ];
        Self {
            cells,
            hint: demo_hint(demo),
        }
    }
}

/// What a row's tooltip says: the format, the players if there were more than one, and what a
/// dsda-family sourceport says it was recorded with, which is all there is to go on for a demo
/// from somewhere else.
fn demo_hint(demo: &DemoFile) -> String {
    let Some(header) = &demo.header else {
        return "caco can't read what this demo says about itself.".to_string();
    };
    let mut lines = vec![match header.players {
        0 | 1 => format!("{} demo", header.format.name()),
        n => format!("{} demo, {n} players", header.format.name()),
    }];
    if demo.record.is_some() {
        lines.push("caco recorded it, and plays it back the same way.".to_string());
    } else if let Some(command_line) = header
        .footer
        .as_ref()
        .and_then(|f| f.command_line.as_deref())
    {
        lines.push(format!("Recorded with {command_line}"));
    }
    lines.join("\n")
}

/// The demos, one a row, with the selected one marked.
fn demo_table(ui: &mut egui::Ui, height: f32, rows: &[DemoRow], selection: &mut Option<usize>) {
    const HEADERS: [&str; 6] = ["Demo", "Map", "Skill", "Length", "Sourceport", "Recorded"];
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let mut clicked = None;
    let selected = *selection;

    dialog_table(ui, height - row_height)
        .sense(egui::Sense::click())
        .column(Column::remainder().at_least(140.0).clip(true))
        .column(Column::initial(60.0).at_least(50.0).clip(true))
        .column(Column::initial(48.0).at_least(40.0))
        .column(Column::initial(64.0).at_least(50.0))
        .column(Column::initial(120.0).at_least(70.0).clip(true))
        .column(Column::initial(100.0).at_least(70.0).clip(true))
        .header(row_height + 2.0, |mut header| {
            for label in HEADERS {
                header.col(|ui| {
                    ui.label(theme::column_header(label));
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rows.len(), |mut row| {
                let idx = row.index();
                row.set_selected(selected == Some(idx));
                for (column, cell) in rows[idx].cells.iter().enumerate() {
                    row.col(|ui| {
                        if column == 0 || cell != NOTHING {
                            ui.label(cell);
                        } else {
                            ui.colored_label(theme::TEXT_SECONDARY, cell);
                        }
                    });
                }
                let response = row.response().on_hover_text(&rows[idx].hint);
                if response.clicked() {
                    clicked = Some(idx);
                }
            });
        });

    if let Some(idx) = clicked {
        *selection = Some(idx);
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

    dialog_table(ui, height - row_height)
        .sense(egui::Sense::click())
        .column(Column::remainder().at_least(180.0).clip(true))
        .column(Column::initial(90.0).at_least(60.0))
        .column(Column::initial(160.0).at_least(100.0))
        .header(row_height + 2.0, |mut header| {
            for label in headers {
                header.col(|ui| {
                    ui.label(theme::column_header(label));
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
