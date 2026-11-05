//! The Sync tool: which machines this library has heard from, and how it is going.
//!
//! There is nothing to set up inside caco but where the folder is, which is in Settings,
//! because moving the files is left to whatever already moves the rest of the data dir. So
//! this mostly says what to do with the folder, and that the library must not go with it.

use std::path::PathBuf;

use egui_extras::{Column, TableBuilder};
use rusqlite::Connection;

use caco_core::config;
use caco_core::sync::{self, MachineStatus, SyncReport, machine};

use crate::dialogs::{Dialog, Primary};
use crate::theme;

pub struct SyncDialogState {
    this_machine: Option<String>,
    log_dir: PathBuf,
    machines: Vec<MachineStatus>,
    waiting_for_parent: i64,
    waiting_for_upgrade: i64,
    /// Why the folder could not be opened, which is usually that it is not there yet.
    open_failed: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SyncResult {
    Open,
    Closed,
    SyncNow,
}

impl SyncDialogState {
    pub fn new(conn: &Connection, db_path: &std::path::Path) -> Self {
        let mut state = Self {
            this_machine: machine::this_machine().ok(),
            log_dir: config::sync_dir_for(db_path),
            machines: Vec::new(),
            waiting_for_parent: 0,
            waiting_for_upgrade: 0,
            open_failed: None,
        };
        state.reload(conn);
        state
    }

    /// Read what a pass has changed.
    pub fn reload(&mut self, conn: &Connection) {
        self.machines = sync::machines(conn).unwrap_or_default();
        let (parent, upgrade) = sync::parked_counts(conn).unwrap_or_default();
        self.waiting_for_parent = parent;
        self.waiting_for_upgrade = upgrade;
    }

    pub fn render(&mut self, ctx: &egui::Context, report: Option<&SyncReport>) -> SyncResult {
        let mut result = SyncResult::Open;

        Dialog::new("Sync", [620.0, 460.0]).show(ctx, |dialog| {
            dialog
                .scroll(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    self.intro(ui);
                    ui.add_space(8.0);
                    self.machines_table(ui);
                    ui.add_space(8.0);
                    self.status_lines(ui, report);
                })
                .buttons(|row| {
                    row.left(|ui| {
                        if ui
                            .button("Open Folder")
                            .on_hover_text("Open the folder the logs are in")
                            .clicked()
                        {
                            self.open_failed =
                                open::that(&self.log_dir).err().map(|e| e.to_string());
                        }
                    });
                    let ended = row.end(Some("Close"), Some(Primary::new("Sync Now")));
                    if ended.dismissed {
                        result = SyncResult::Closed;
                    }
                    if ended.primary {
                        result = SyncResult::SyncNow;
                    }
                })
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return SyncResult::Closed;
        }
        result
    }

    fn intro(&self, ui: &mut egui::Ui) {
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "Each machine keeps a log of what happens on it, in this folder. Sync the folder \
             with git, Syncthing, a shared drive or anything else that moves files, and every \
             machine ends up with the same library.",
        );
        ui.label(
            egui::RichText::new(self.log_dir.display().to_string())
                .monospace()
                .small()
                .color(theme::TEXT_PRIMARY),
        );
        ui.colored_label(
            theme::COLOR_WARNING,
            "Leave library.db out of what you sync. Each machine builds its own from the logs.",
        );
        if let Some(id) = &self.this_machine {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!("This machine is {}.", machine::display_name(id)),
            );
        }
    }

    fn machines_table(&self, ui: &mut egui::Ui) {
        if self.machines.is_empty() {
            ui.colored_label(theme::TEXT_MUTED, "Nothing has been logged yet.");
            return;
        }
        TableBuilder::new(ui)
            .striped(true)
            .auto_shrink([false, true])
            .column(Column::remainder().at_least(160.0))
            .column(Column::exact(150.0))
            .column(Column::exact(70.0))
            .header(20.0, |mut header| {
                for title in ["Machine", "Last active", "Changes"] {
                    header.col(|ui| {
                        ui.colored_label(theme::TEXT_MUTED, egui::RichText::new(title).small());
                    });
                }
            })
            .body(|mut body| {
                for status in &self.machines {
                    body.row(20.0, |mut row| {
                        row.col(|ui| {
                            let name = machine::display_name(&status.machine);
                            let here = self.this_machine.as_deref() == Some(&status.machine);
                            let text = if here {
                                format!("{name} (this machine)")
                            } else {
                                name.to_string()
                            };
                            ui.label(text).on_hover_text(&status.machine);
                        });
                        row.col(|ui| {
                            ui.colored_label(theme::TEXT_SECONDARY, last_active(&status.last_at));
                        });
                        row.col(|ui| {
                            ui.colored_label(theme::TEXT_SECONDARY, status.events.to_string());
                        });
                    });
                }
            });
    }

    fn status_lines(&self, ui: &mut egui::Ui, report: Option<&SyncReport>) {
        if let Some(why) = &self.open_failed {
            ui.colored_label(
                theme::COLOR_WARNING,
                format!("Can't open the folder: {why}."),
            );
        }
        if let Some(why) = report.and_then(|report| report.write_error.as_ref()) {
            ui.colored_label(
                theme::COLOR_WARNING,
                format!("Can't write to the folder: {why}. Changes are kept until it can."),
            );
        }
        if let Some(why) = report.and_then(|report| report.read_error.as_ref()) {
            ui.colored_label(
                theme::COLOR_WARNING,
                format!("Can't read the folder: {why}."),
            );
        }
        if self.waiting_for_parent > 0 {
            ui.colored_label(theme::TEXT_SECONDARY, waiting_line(self.waiting_for_parent));
        }
        if self.waiting_for_upgrade > 0 {
            ui.colored_label(theme::COLOR_WARNING, upgrade_line(self.waiting_for_upgrade));
        }
        if let Some(unreadable) = report
            .map(|report| report.merge.unreadable)
            .filter(|n| *n > 0)
        {
            ui.colored_label(theme::TEXT_SECONDARY, unreadable_line(unreadable));
        }
    }
}

fn waiting_line(n: i64) -> String {
    match n {
        1 => {
            "1 change is waiting for something it refers to, which hasn't arrived yet.".to_string()
        }
        n => {
            format!("{n} changes are waiting for things they refer to, which haven't arrived yet.")
        }
    }
}

fn upgrade_line(n: i64) -> String {
    match n {
        1 => {
            "1 change is from a newer version of caco. Update this machine to apply it.".to_string()
        }
        n => format!(
            "{n} changes are from a newer version of caco. Update this machine to apply them."
        ),
    }
}

fn unreadable_line(n: usize) -> String {
    match n {
        1 => "Passed over 1 line in the logs that can't be read.".to_string(),
        n => format!("Passed over {n} lines in the logs that can't be read."),
    }
}

/// When a machine last did anything, as a person says it. Shown raw if it does not parse,
/// since a log can say anything.
fn last_active(at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(at)
        .map(|at| {
            crate::relative_time::relative_time_full(
                &at.with_timezone(&chrono::Local).naive_local(),
            )
        })
        .unwrap_or_else(|_| at.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_counts_read_as_a_person_would_say_them() {
        assert_eq!(
            waiting_line(1),
            "1 change is waiting for something it refers to, which hasn't arrived yet."
        );
        assert_eq!(
            waiting_line(3),
            "3 changes are waiting for things they refer to, which haven't arrived yet."
        );
        assert!(upgrade_line(1).ends_with("to apply it."));
        assert!(upgrade_line(2).ends_with("to apply them."));
        assert!(unreadable_line(1).starts_with("Passed over 1 line "));
        assert!(unreadable_line(2).starts_with("Passed over 2 lines "));
    }

    #[test]
    fn test_a_time_that_does_not_parse_is_shown_as_it_is() {
        assert_eq!(last_active("whenever"), "whenever");
        assert_ne!(
            last_active("2024-06-15T12:00:00+00:00"),
            "2024-06-15T12:00:00+00:00"
        );
    }

    #[test]
    fn test_what_a_pass_found_is_read_from_the_library() {
        let conn = caco_core::db::open_memory().unwrap();
        caco_core::db::init_db(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO sync_machines (machine, last_at, events)
                 VALUES ('thinkpad-00000000', '2026-11-05T00:00:00Z', 4);
             INSERT INTO sync_parked (id, clock, line, reason)
                 VALUES ('a', 'c', 'l', 'parent'), ('b', 'c', 'l', 'parent'),
                        ('c', 'c', 'l', 'upgrade');",
        )
        .unwrap();

        let mut state = SyncDialogState {
            this_machine: None,
            log_dir: PathBuf::from("/sync"),
            machines: Vec::new(),
            waiting_for_parent: 0,
            waiting_for_upgrade: 0,
            open_failed: None,
        };
        state.reload(&conn);
        assert_eq!(state.machines.len(), 1);
        assert_eq!(state.machines[0].events, 4);
        assert_eq!(
            (state.waiting_for_parent, state.waiting_for_upgrade),
            (2, 1)
        );
    }
}
