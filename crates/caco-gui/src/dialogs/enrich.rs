//! Re-run metadata detection over the library, and refresh Cacowards years.
//!
//! The GUI answer to `caco enrich`. Enrichment is network-bound — one Doom
//! Wiki lookup per WAD file detection cannot settle — so the run happens on a
//! worker thread and this dialog only holds the request, the live progress,
//! and the report. `caco_sources::enrich_service` does the actual work for
//! both frontends.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::message::EnrichReport;
use crate::theme;

/// Which WADs a run should visit.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum EnrichScope {
    /// Every WAD in the library.
    All,
    /// Only WADs with no complevel set — the common "fill the gaps" case,
    /// and far cheaper, since WADs that need nothing skip the wiki entirely.
    MissingComplevel,
}

/// A run the dialog is asking the app to start.
pub enum EnrichRequest {
    Wads {
        scope: EnrichScope,
        dry_run: bool,
        cancel: Arc<AtomicBool>,
    },
    Cacowards {
        year: i64,
        dry_run: bool,
    },
}

pub enum EnrichResult {
    Open,
    Closed,
    Start(EnrichRequest),
}

pub struct EnrichDialogState {
    scope: EnrichScope,
    dry_run: bool,
    year_input: String,
    /// Set while a run is in flight; cleared when the report arrives.
    running: bool,
    /// Shared with the worker so Cancel can stop it between WADs.
    cancel: Arc<AtomicBool>,
    progress: Option<(usize, usize, String)>,
    report: Option<EnrichReport>,
    error: Option<String>,
}

impl EnrichDialogState {
    pub fn new(default_year: i64) -> Self {
        Self {
            scope: EnrichScope::MissingComplevel,
            dry_run: false,
            year_input: default_year.to_string(),
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: None,
            report: None,
            error: None,
        }
    }

    /// Called by the app when a progress message arrives.
    pub fn set_progress(&mut self, done: usize, total: usize, title: String) {
        self.progress = Some((done, total, title));
    }

    /// Called by the app when the run finishes (or fails).
    pub fn finish(&mut self, outcome: Result<EnrichReport, String>) {
        self.running = false;
        self.progress = None;
        match outcome {
            Ok(report) => {
                self.report = Some(report);
                self.error = None;
            }
            Err(e) => {
                self.report = None;
                self.error = Some(e);
            }
        }
    }

    fn start_wads(&mut self) -> EnrichRequest {
        // A fresh flag per run: reusing a cancelled one would abort instantly.
        self.cancel = Arc::new(AtomicBool::new(false));
        self.running = true;
        self.report = None;
        self.error = None;
        self.progress = Some((0, 0, String::new()));
        EnrichRequest::Wads {
            scope: self.scope,
            dry_run: self.dry_run,
            cancel: Arc::clone(&self.cancel),
        }
    }

    pub fn render(&mut self, ctx: &egui::Context) -> EnrichResult {
        let mut result = EnrichResult::Open;

        egui::Window::new("Enrich")
            .collapsible(false)
            .resizable(true)
            .default_size([640.0, 460.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                self.render_library_section(ui, &mut result);
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
                self.render_cacowards_section(ui, &mut result);

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                self.render_output(ui);

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.running, egui::Button::new("Close"))
                        .clicked()
                    {
                        result = EnrichResult::Closed;
                    }
                });
            });

        // Escape must not close mid-run: the worker holds a DB connection and
        // the report would be lost with nowhere to display it.
        if !self.running && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return EnrichResult::Closed;
        }

        result
    }

    fn render_library_section(&mut self, ui: &mut egui::Ui, result: &mut EnrichResult) {
        ui.strong("Library metadata");
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "Fills in complevel, IWAD and zdoom-required from the WAD file, \
             falling back to the Doom Wiki. Existing values are never overwritten.",
        );
        ui.add_space(4.0);

        ui.add_enabled_ui(!self.running, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.scope,
                    EnrichScope::MissingComplevel,
                    "Missing complevel",
                );
                ui.selectable_value(&mut self.scope, EnrichScope::All, "All WADs");
                ui.checkbox(&mut self.dry_run, "Dry run")
                    .on_hover_text("Detect and report, but write nothing");
            });
        });
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.running, egui::Button::new("Enrich Library"))
                .clicked()
            {
                *result = EnrichResult::Start(self.start_wads());
            }
            if ui
                .add_enabled(self.running, egui::Button::new("Cancel"))
                .on_hover_text("Stops after the WAD in flight; work already done is kept")
                .clicked()
            {
                self.cancel.store(true, Ordering::Relaxed);
            }
        });
    }

    fn render_cacowards_section(&mut self, ui: &mut egui::Ui, result: &mut EnrichResult) {
        ui.strong("Cacowards");
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "Re-scrapes a year's Doom Wiki page and auto-links entries to \
             library WADs. Manually pinned links survive.",
        );
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.add_enabled(
                !self.running,
                egui::TextEdit::singleline(&mut self.year_input)
                    .desired_width(70.0)
                    .hint_text("YYYY"),
            );

            let year = self.year_input.trim().parse::<i64>().ok();
            if ui
                .add_enabled(
                    !self.running && year.is_some(),
                    egui::Button::new("Fetch Year"),
                )
                .clicked()
                && let Some(year) = year
            {
                self.running = true;
                self.report = None;
                self.error = None;
                self.progress = None;
                *result = EnrichResult::Start(EnrichRequest::Cacowards {
                    year,
                    dry_run: self.dry_run,
                });
            }
        });
    }

    fn render_output(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(theme::COLOR_ERROR, error);
            return;
        }

        if self.running {
            match &self.progress {
                Some((done, total, title)) if *total > 0 => {
                    ui.add(
                        egui::ProgressBar::new(*done as f32 / *total as f32)
                            .text(format!("{done}/{total}")),
                    );
                    ui.colored_label(theme::TEXT_SECONDARY, title);
                }
                _ => {
                    ui.spinner();
                    ui.colored_label(theme::TEXT_SECONDARY, "Working\u{2026}");
                }
            }
            return;
        }

        let Some(report) = &self.report else {
            ui.colored_label(theme::TEXT_SECONDARY, "Idle.");
            return;
        };

        match report {
            EnrichReport::Wads {
                examined,
                findings,
                wiki_lookups,
                cancelled,
                dry_run,
            } => {
                let verb = if *dry_run { "would change" } else { "changed" };
                let mut headline = format!("{} of {examined} WAD(s) {verb}", findings.len());
                if *cancelled {
                    headline.push_str(" (cancelled)");
                }
                if *wiki_lookups > 0 {
                    headline.push_str(&format!(", {wiki_lookups} wiki lookup(s)"));
                }
                ui.colored_label(theme::TEXT_SECONDARY, headline);
                ui.add_space(4.0);
                scroll_lines(ui, findings, "Nothing new detected.");
            }
            EnrichReport::Cacowards {
                year,
                scraped,
                upserted,
                linked,
                previews,
                dry_run,
            } => {
                let suffix = if *dry_run { " (dry run)" } else { "" };
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    format!(
                        "Cacowards {year}: scraped {scraped}, upserted {upserted}, \
                         auto-linked {linked}{suffix}"
                    ),
                );
                ui.add_space(4.0);
                scroll_lines(ui, previews, "");
            }
        }
    }
}

/// Render a scrollable list of report lines, or `empty` when there are none.
fn scroll_lines(ui: &mut egui::Ui, lines: &[String], empty: &str) {
    if lines.is_empty() {
        if !empty.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, empty);
        }
        return;
    }
    egui::ScrollArea::vertical()
        .max_height(200.0)
        .show(ui, |ui| {
            for line in lines {
                ui.label(line);
            }
        });
}
