//! The Mirrors tool: every mirror, what each holds, and the way to add, change, fill or
//! stop using one, so that none of it needs a text editor.
//!
//! Three pages in one window: the list, the wizard (`mirror_setup.rs`), and a page for
//! filling a mirror with what is already here. It opens straight on the wizard when
//! there are no mirrors, since then there is nothing else to show. Everything that talks
//! to a mirror is a worker's, as in `dialogs/enrich.rs`: this holds what to show.

use std::collections::HashMap;

use caco_core::config::{self, MirrorConfig, MirrorLocation};
use caco_core::db::mirrors;
use caco_core::mirror_tokens::TokenFile;
use caco_sources::mirror::{MirrorProbe, SyncReport, UploadReport};
use rusqlite::Connection;

use crate::dialogs::mirror_setup::{MirrorSetup, SetupAction};
use crate::dialogs::{Dialog, FooterRow, Primary, count};
use crate::state::MirrorBulk;
use crate::theme;

pub enum MirrorsResult {
    Open,
    Closed,
    /// Try this mirror, on a worker.
    Probe(Box<MirrorConfig>),
    /// Make this mirror ready, on a worker.
    SetUp(Box<MirrorConfig>),
    /// Read every mirror's index again.
    Sync,
    /// Upload everything worth uploading to this mirror, on a worker.
    UploadAll(String),
}

/// What the list says about one mirror.
struct Row {
    config: MirrorConfig,
    /// How many files it held the last time its index was read, if ever.
    files: Option<i64>,
    /// How many of the WADs in the library those are.
    wads: i64,
    to_upload: usize,
}

enum Page {
    List,
    Setup(Box<MirrorSetup>),
    Fill(FillPage),
}

/// Filling a mirror with what is already on this machine.
struct FillPage {
    mirror: String,
    /// The titles of what would go, shown before anything does.
    titles: Vec<String>,
    report: Option<UploadReport>,
}

pub struct MirrorsDialogState {
    page: Page,
    rows: Vec<Row>,
    /// The mirror whose removal is waiting for an answer.
    removing: Option<String>,
    error: Option<String>,
}

impl MirrorsDialogState {
    pub fn new(conn: &Connection) -> Self {
        let mut state = Self {
            page: Page::List,
            rows: Vec::new(),
            removing: None,
            error: None,
        };
        state.reload(conn);
        if state.rows.is_empty() {
            state.page = Page::Setup(Box::new(MirrorSetup::new(Vec::new())));
        }
        state
    }

    /// Read what the list shows, and what a fill page would send.
    pub fn reload(&mut self, conn: &Connection) {
        let counts: HashMap<String, i64> = mirrors::file_counts(conn).unwrap_or_default();
        self.rows = config::load_config()
            .mirrors
            .iter()
            .map(|config| Row {
                files: counts.get(&config.name).copied(),
                wads: mirrors::wads_held_by(conn, &config.name).unwrap_or_default(),
                to_upload: if config.takes_uploads() {
                    mirrors::wads_to_upload(conn, &config.name).map_or(0, |wads| wads.len())
                } else {
                    0
                },
                config: config.clone(),
            })
            .collect();
        if let Page::Fill(fill) = &mut self.page {
            fill.titles = titles_to_upload(conn, &fill.mirror);
        }
    }

    /// Called by the app when a test of the mirror in the wizard finishes.
    pub fn probe_finished(&mut self, outcome: Result<MirrorProbe, String>) {
        if let Page::Setup(setup) = &mut self.page {
            setup.probe_finished(outcome);
        }
    }

    /// Called by the app when setting up the mirror in the wizard finishes. Asks for the
    /// indexes to be read again if it was saved.
    pub fn set_up_finished(
        &mut self,
        conn: &Connection,
        config: MirrorConfig,
        outcome: Result<(), String>,
    ) -> bool {
        match outcome {
            Ok(()) => self.save_from_wizard(conn, config),
            Err(why) => {
                if let Page::Setup(setup) = &mut self.page {
                    setup.set_up_failed(why);
                }
                false
            }
        }
    }

    /// Called by the app when a bulk upload to `mirror` finishes.
    pub fn bulk_finished(&mut self, conn: &Connection, mirror: &str, report: UploadReport) {
        if let Page::Fill(fill) = &mut self.page
            && fill.mirror == mirror
        {
            fill.report = Some(report);
        }
        self.reload(conn);
    }

    /// Save what the wizard holds, and go on to filling it if there's a point.
    fn save_from_wizard(&mut self, conn: &Connection, config: MirrorConfig) -> bool {
        let Page::Setup(setup) = &self.page else {
            return false;
        };
        let replacing = setup.replacing().map(str::to_string);
        let changes = setup.token_changes();
        let tokens = TokenFile::from_config();
        let saved = (|| -> caco_core::Result<()> {
            if let Some(old) = &changes.renamed_from {
                tokens.rename(old, &config.name)?;
            }
            if changes.forget {
                tokens.set(&config.name, None)?;
            }
            if let Some(token) = &changes.token {
                tokens.set(&config.name, Some(token))?;
            }
            let mut updated = (*config::load_config()).clone();
            put_mirror(&mut updated.mirrors, replacing.as_deref(), config.clone());
            config::save_config(&updated)?;
            config::reload_config();
            Ok(())
        })();
        if let Err(e) = saved {
            if let Page::Setup(setup) = &mut self.page {
                setup.set_up_failed(format!("could not save it: {e}"));
            }
            return false;
        }

        self.page = if config.takes_uploads() && replacing.is_none() {
            Page::Fill(FillPage {
                titles: titles_to_upload(conn, &config.name),
                mirror: config.name,
                report: None,
            })
        } else {
            Page::List
        };
        self.reload(conn);
        true
    }

    /// Stop using a mirror. Nothing on the mirror is touched: it is the user's, and may
    /// be the only copy of what is in it.
    fn remove(&mut self, conn: &Connection, name: &str) -> bool {
        let mut updated = (*config::load_config()).clone();
        updated.mirrors.retain(|mirror| mirror.name != name);
        let saved = config::save_config(&updated).and_then(|()| {
            config::reload_config();
            TokenFile::from_config().set(name, None)
        });
        self.removing = None;
        if let Err(e) = saved {
            self.error = Some(format!("Could not remove {name}: {e}"));
            return false;
        }
        self.reload(conn);
        true
    }

    fn other_names(&self, except: Option<&str>) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| row.config.name.clone())
            .filter(|name| Some(name.as_str()) != except)
            .collect()
    }

    pub fn render(
        &mut self,
        ctx: &egui::Context,
        conn: &Connection,
        report: Option<&SyncReport>,
        bulk: Option<&MirrorBulk>,
    ) -> MirrorsResult {
        let title = match &self.page {
            Page::List => "Mirrors".to_string(),
            Page::Setup(setup) => setup.title(),
            Page::Fill(fill) => format!("Fill {}", fill.mirror),
        };

        let mut result = MirrorsResult::Open;
        // Keyed, because the title is the page's and this is one window.
        Dialog::new(title, [680.0, 580.0])
            .keyed("Mirrors")
            .show(ctx, |dialog| {
                dialog
                    .scroll(|ui| {
                        if let Some(error) = &self.error {
                            ui.colored_label(theme::COLOR_ERROR, error);
                        }
                        match &mut self.page {
                            Page::List => {}
                            Page::Setup(setup) => setup.render(ctx, ui),
                            Page::Fill(_) => {}
                        }
                        if matches!(self.page, Page::List) {
                            self.render_list(ui, conn, report, bulk, &mut result);
                        }
                        if let Page::Fill(fill) = &self.page {
                            render_fill(ui, fill, bulk);
                        }
                    })
                    .above(|ui| {
                        if let Page::Setup(setup) = &self.page {
                            setup.render_check_area(ui);
                        }
                    })
                    .buttons(|row| self.render_footer(row, conn, bulk, &mut result))
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            match &mut self.page {
                Page::List => return MirrorsResult::Closed,
                Page::Fill(_) => self.page = Page::List,
                Page::Setup(setup) if setup.is_busy() => {}
                Page::Setup(setup) => {
                    if !setup.go_back() {
                        if self.rows.is_empty() {
                            return MirrorsResult::Closed;
                        }
                        self.page = Page::List;
                    }
                }
            }
        }
        result
    }

    fn render_footer(
        &mut self,
        row: &mut FooterRow<'_>,
        conn: &Connection,
        bulk: Option<&MirrorBulk>,
        result: &mut MirrorsResult,
    ) {
        match &mut self.page {
            Page::List => {
                row.left(|ui| {
                    if ui.button("Add a Mirror").clicked() {
                        let taken = self.other_names(None);
                        self.page = Page::Setup(Box::new(MirrorSetup::new(taken)));
                    }
                    if !self.rows.is_empty()
                        && ui
                            .button("Read Them Again")
                            .on_hover_text(
                                "Ask each mirror what it holds now, as caco does when it starts",
                            )
                            .clicked()
                    {
                        *result = MirrorsResult::Sync;
                    }
                });
                if row.close() {
                    *result = MirrorsResult::Closed;
                }
            }
            Page::Setup(setup) => match setup.render_footer(row) {
                SetupAction::None => {}
                SetupAction::Cancel => {
                    if self.rows.is_empty() {
                        *result = MirrorsResult::Closed;
                    } else {
                        self.page = Page::List;
                    }
                }
                SetupAction::Probe(config) => *result = MirrorsResult::Probe(config),
                SetupAction::SetUp(config) => *result = MirrorsResult::SetUp(config),
                SetupAction::SaveAnyway(config) => {
                    if self.save_from_wizard(conn, *config) {
                        *result = MirrorsResult::Sync;
                    }
                }
            },
            Page::Fill(fill) => {
                let running = bulk.is_some_and(|bulk| bulk.mirror == fill.mirror);
                let mirror = fill.mirror.clone();
                let pending = fill.titles.len();
                let leave;
                if running {
                    row.left(|ui| {
                        if let Some(bulk) = bulk
                            && ui.button("Stop").clicked()
                        {
                            bulk.stop();
                        }
                    });
                    leave = row.end(Some("Done"), None).dismissed;
                } else if pending > 0 {
                    // One at a time, since each rewrites the mirror's index.
                    let upload = format!("Upload {pending} to {mirror}");
                    let primary = Primary::new(&upload)
                        .when(bulk.is_none(), "Waiting for the other upload to finish");
                    let ended = row.end(Some("Not Now"), Some(primary));
                    leave = ended.dismissed;
                    if ended.primary {
                        *result = MirrorsResult::UploadAll(mirror);
                    }
                } else {
                    leave = row.end(Some("Done"), None).dismissed;
                }
                if leave {
                    self.page = Page::List;
                }
            }
        }
    }

    fn render_list(
        &mut self,
        ui: &mut egui::Ui,
        conn: &Connection,
        report: Option<&SyncReport>,
        bulk: Option<&MirrorBulk>,
        result: &mut MirrorsResult,
    ) {
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "Your own stores of WAD files. caco looks here first for a WAD that isn't on this \
             machine, in this order.",
        );
        ui.add_space(8.0);

        if self.rows.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "No mirrors yet.");
        }

        let mut open_edit = None;
        let mut open_fill = None;
        let mut confirmed_removal = None;
        let mut move_up = None;
        for (position, row) in self.rows.iter().enumerate() {
            let name = &row.config.name;
            egui::Frame::new()
                .fill(theme::BG_MEDIUM)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.colored_label(theme::TEXT_PRIMARY, egui::RichText::new(name).strong());
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(location_text(&row.config))
                                .monospace()
                                .size(11.0)
                                .color(theme::TEXT_SECONDARY),
                        )
                        .wrap(),
                    );
                    let (status, healthy) = status_line(row, report);
                    ui.colored_label(
                        if healthy {
                            theme::TEXT_SECONDARY
                        } else {
                            theme::COLOR_WARNING
                        },
                        egui::RichText::new(status).size(12.0),
                    );

                    if self.removing.as_deref() == Some(name) {
                        ui.colored_label(theme::COLOR_WARNING, removal_warning(name, row.wads));
                        ui.horizontal(|ui| {
                            if ui.button("Stop Using It").clicked() {
                                confirmed_removal = Some(name.clone());
                            }
                            if ui.button("Keep It").clicked() {
                                self.removing = None;
                            }
                        });
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            // Still there while its own upload runs, as the way back to
                            // seeing how that is going after closing the window.
                            let uploading_here = bulk.is_some_and(|bulk| bulk.mirror == *name);
                            if uploading_here {
                                if ui.button("See the Upload").clicked() {
                                    open_fill = Some(name.clone());
                                }
                            } else if row.to_upload > 0
                                && bulk.is_none()
                                && ui
                                    .button(format!(
                                        "Upload {}",
                                        count(row.to_upload, "WAD", "WADs")
                                    ))
                                    .clicked()
                            {
                                open_fill = Some(name.clone());
                            }
                            if ui.button("Edit").clicked() {
                                open_edit = Some(row.config.clone());
                            }
                            if position > 0
                                && ui
                                    .button("Move Up")
                                    .on_hover_text("Try this one before the one above it")
                                    .clicked()
                            {
                                move_up = Some(position);
                            }
                            if ui.button("Remove").clicked() {
                                self.removing = Some(name.clone());
                            }
                        });
                    }
                });
            ui.add_space(6.0);
        }

        if let Some(name) = confirmed_removal
            && self.remove(conn, &name)
        {
            *result = MirrorsResult::Sync;
        }
        if let Some(position) = move_up {
            let mut updated = (*config::load_config()).clone();
            updated.mirrors.swap(position, position - 1);
            match config::save_config(&updated) {
                Ok(()) => {
                    config::reload_config();
                    self.reload(conn);
                }
                Err(e) => self.error = Some(format!("Could not save that: {e}")),
            }
        }
        if let Some(config) = open_edit {
            let taken = self.other_names(Some(&config.name));
            self.page = Page::Setup(Box::new(MirrorSetup::editing(&config, taken)));
        }
        if let Some(mirror) = open_fill {
            self.page = Page::Fill(FillPage {
                titles: titles_to_upload(conn, &mirror),
                mirror,
                report: None,
            });
        }
    }
}

/// Put a mirror in the list: in place of the one it replaces, which keeps its place in
/// the order caco tries them, or on the end.
fn put_mirror(list: &mut Vec<MirrorConfig>, replacing: Option<&str>, mirror: MirrorConfig) {
    match replacing.and_then(|name| list.iter().position(|m| m.name == name)) {
        Some(position) => list[position] = mirror,
        None => list.push(mirror),
    }
}

fn titles_to_upload(conn: &Connection, mirror: &str) -> Vec<String> {
    mirrors::wads_to_upload(conn, mirror)
        .unwrap_or_default()
        .into_iter()
        .map(|wad| wad.title)
        .collect()
}

fn location_text(config: &MirrorConfig) -> String {
    match config.location() {
        Ok(MirrorLocation::Url(url)) => url,
        Ok(MirrorLocation::Path(path)) => path.display().to_string(),
        Err(problem) => problem,
    }
}

/// What to say about a mirror's state, and whether it is all well: it has been read, or
/// it is still being, unless something went wrong.
fn status_line(row: &Row, report: Option<&SyncReport>) -> (String, bool) {
    let name = &row.config.name;
    if row.config.is_idgames_copy() {
        return match report.and_then(|r| r.failed.iter().find(|(n, _)| n == name)) {
            Some((_, why)) => (format!("Could not read it: {why}."), false),
            None => ("A copy of idgames · read only".to_string(), true),
        };
    }
    let access = if row.config.write {
        "caco may upload"
    } else {
        "read only"
    };
    let wads = match row.wads {
        0 => String::new(),
        1 => ", for 1 WAD in your library".to_string(),
        n => format!(", for {n} WADs in your library"),
    };
    let held = |files: i64| match files {
        1 => format!("1 file{wads}"),
        n => format!("{n} files{wads}"),
    };
    if let Some((_, why)) = report.and_then(|r| r.failed.iter().find(|(n, _)| n == name)) {
        let last = row
            .files
            .map(|files| format!(" caco is going by what it last held: {}.", held(files)))
            .unwrap_or_default();
        return (format!("Could not read it: {why}.{last}"), false);
    }
    let read = report.and_then(|r| r.read.iter().find(|(n, _)| n == name));
    match read {
        Some((_, 0)) => (format!("Empty, and ready · {access}"), true),
        Some(_) => (
            format!("{} · {access}", held(row.files.unwrap_or_default())),
            true,
        ),
        None => (format!("Not read yet · {access}"), true),
    }
}

fn removal_warning(name: &str, wads: i64) -> String {
    let relying = match wads {
        0 => String::new(),
        1 => " caco won't be able to get 1 WAD from it any more.".to_string(),
        n => format!(" caco won't be able to get {n} WADs from it any more."),
    };
    format!("Stop using {name}?{relying} Nothing on the mirror is deleted.")
}

fn render_fill(ui: &mut egui::Ui, fill: &FillPage, bulk: Option<&MirrorBulk>) {
    let running = bulk.filter(|bulk| bulk.mirror == fill.mirror);
    if let Some(bulk) = running {
        ui.colored_label(
            theme::TEXT_SECONDARY,
            format!(
                "Uploading to {}. It carries on if you close this.",
                fill.mirror
            ),
        );
        ui.add_space(4.0);
        let progress = if bulk.total > 0 {
            bulk.done as f32 / bulk.total as f32
        } else {
            0.0
        };
        ui.add(egui::ProgressBar::new(progress).text(format!("{}/{}", bulk.done, bulk.total)));
        ui.colored_label(theme::TEXT_SECONDARY, &bulk.title);
        return;
    }

    if let Some(report) = &fill.report {
        let stopped = if report.cancelled {
            ", then stopped"
        } else {
            ""
        };
        ui.colored_label(
            theme::TEXT_PRIMARY,
            format!(
                "Uploaded {}{stopped}.",
                count(report.uploaded, "WAD", "WADs")
            ),
        );
        for (title, why) in &report.failed {
            ui.colored_label(theme::COLOR_ERROR, format!("{title}: {why}"));
        }
        ui.add_space(6.0);
    }

    if fill.titles.is_empty() {
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "Nothing to upload. From now on caco will offer each WAD it gets that it couldn't \
             otherwise get back.",
        );
        return;
    }

    let count = fill.titles.len();
    ui.colored_label(
        theme::TEXT_SECONDARY,
        if count == 1 {
            "This WAD has a file on this machine that nothing else could give you back. \
             Upload it to your mirror?"
                .to_string()
        } else {
            format!(
                "These {count} WADs have a file on this machine that nothing else could give \
                 you back. Upload them to your mirror?"
            )
        },
    );
    ui.add_space(6.0);
    for title in &fill.titles {
        ui.label(title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> MirrorConfig {
        MirrorConfig {
            name: name.to_string(),
            path: Some(format!("/mnt/{name}")),
            ..Default::default()
        }
    }

    fn row(name: &str, files: Option<i64>, wads: i64) -> Row {
        Row {
            config: MirrorConfig {
                write: true,
                ..named(name)
            },
            files,
            wads,
            to_upload: 0,
        }
    }

    #[test]
    fn test_an_edited_mirror_keeps_its_place_in_the_order() {
        let mut list = vec![named("a"), named("b"), named("c")];

        put_mirror(&mut list, Some("b"), named("renamed"));
        let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a", "renamed", "c"]);

        put_mirror(&mut list, None, named("new"));
        assert_eq!(list.last().unwrap().name, "new");
    }

    /// This is the line that used to be missing: an empty mirror that reads fine looked
    /// the same as one that had never been read.
    #[test]
    fn test_a_mirror_says_whether_it_is_empty_or_unread() {
        let read = |n| SyncReport {
            read: vec![("home".to_string(), n)],
            ..Default::default()
        };
        let empty = status_line(&row("home", None, 0), Some(&read(0)));
        assert_eq!(
            empty,
            ("Empty, and ready · caco may upload".to_string(), true)
        );

        let unread = status_line(&row("home", None, 0), None);
        assert_eq!(unread, ("Not read yet · caco may upload".to_string(), true));

        let holding = status_line(&row("home", Some(42), 3), Some(&read(42)));
        assert_eq!(
            holding.0,
            "42 files, for 3 WADs in your library · caco may upload"
        );

        let one = status_line(&row("home", Some(1), 1), Some(&read(1)));
        assert_eq!(one.0, "1 file, for 1 WAD in your library · caco may upload");
    }

    #[test]
    fn test_a_mirror_that_failed_says_why_and_what_is_being_used_instead() {
        let failed = SyncReport {
            failed: vec![("home".to_string(), "could not connect".to_string())],
            ..Default::default()
        };

        let (text, healthy) = status_line(&row("home", Some(2), 0), Some(&failed));
        assert!(!healthy);
        assert_eq!(
            text,
            "Could not read it: could not connect. caco is going by what it last held: 2 files."
        );

        let (never, _) = status_line(&row("home", None, 0), Some(&failed));
        assert_eq!(never, "Could not read it: could not connect.");
    }

    #[test]
    fn test_removing_says_what_it_costs_and_what_it_does_not() {
        assert_eq!(
            removal_warning("home", 0),
            "Stop using home? Nothing on the mirror is deleted."
        );
        assert_eq!(
            removal_warning("home", 3),
            "Stop using home? caco won't be able to get 3 WADs from it any more. Nothing on \
             the mirror is deleted."
        );
    }
}
