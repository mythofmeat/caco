//! Re-run metadata detection over the library.
//!
//! Enrichment is network-bound — one Doom Wiki lookup per WAD file detection
//! cannot settle — so the run happens on a worker thread and this dialog only
//! holds the request, the live progress, and the report.
//! `caco_sources::enrich_service` does the actual work.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::dialogs::{Dialog, Primary, count};
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
pub struct EnrichRequest {
    pub scope: EnrichScope,
    pub dry_run: bool,
    pub cancel: Arc<AtomicBool>,
}

pub enum EnrichResult {
    Open,
    Closed,
    Start(EnrichRequest),
}

pub struct EnrichDialogState {
    scope: EnrichScope,
    dry_run: bool,
    /// Set while a run is in flight; cleared when the report arrives.
    running: bool,
    /// Shared with the worker so Cancel can stop it between WADs.
    cancel: Arc<AtomicBool>,
    progress: Option<(usize, usize, String)>,
    report: Option<EnrichReport>,
    error: Option<String>,
}

impl Default for EnrichDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl EnrichDialogState {
    pub fn new() -> Self {
        Self {
            scope: EnrichScope::MissingComplevel,
            dry_run: false,
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

    fn start(&mut self) -> EnrichRequest {
        // A fresh flag per run: reusing a cancelled one would abort instantly.
        self.cancel = Arc::new(AtomicBool::new(false));
        self.running = true;
        self.report = None;
        self.error = None;
        self.progress = Some((0, 0, String::new()));
        EnrichRequest {
            scope: self.scope,
            dry_run: self.dry_run,
            cancel: Arc::clone(&self.cancel),
        }
    }

    pub fn render(&mut self, ctx: &egui::Context) -> EnrichResult {
        let mut result = EnrichResult::Open;

        Dialog::new("Enrich", [640.0, 460.0]).show(ctx, |dialog| {
            dialog
                .scroll(|ui| {
                    self.render_controls(ui);
                    self.render_output(ui);
                })
                .buttons(|row| {
                    if self.running {
                        // No way out but stopping: the worker holds a connection, and the
                        // report would have nowhere to go.
                        let ended = row.end(Some("Stop"), None);
                        if ended.dismissed {
                            self.cancel.store(true, Ordering::Relaxed);
                        }
                    } else {
                        let ended = row.end(Some("Close"), Some(Primary::new("Enrich Library")));
                        if ended.dismissed {
                            result = EnrichResult::Closed;
                        }
                        if ended.primary {
                            result = EnrichResult::Start(self.start());
                        }
                    }
                })
        });

        // Escape must not close mid-run: the worker holds a DB connection and
        // the report would be lost with nowhere to display it.
        if !self.running && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return EnrichResult::Closed;
        }

        result
    }

    fn render_controls(&mut self, ui: &mut egui::Ui) {
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "Looks up the complevel, IWAD and sourceport each WAD needs, from its file and then \
             the Doom Wiki, and fills in the ones you haven't set.",
        );
        ui.add_space(8.0);

        ui.add_enabled_ui(!self.running, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.scope,
                    EnrichScope::MissingComplevel,
                    "WADs with no complevel",
                );
                ui.selectable_value(&mut self.scope, EnrichScope::All, "All WADs");
                ui.add_space(8.0);
                ui.checkbox(&mut self.dry_run, "Only show what would change");
            });
        });
    }

    /// How it is going, or how it went. Nothing at all before it has been run.
    fn render_output(&mut self, ui: &mut egui::Ui) {
        if self.error.is_none() && !self.running && self.report.is_none() {
            return;
        }
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        if let Some(error) = &self.error {
            ui.colored_label(theme::COLOR_ERROR, error);
            return;
        }

        if self.running {
            match &self.progress {
                Some((done, total, title)) if *total > 0 => {
                    ui.add(
                        egui::ProgressBar::new(*done as f32 / *total as f32)
                            .text(format!("{done} of {total}")),
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
            return;
        };
        let wiki_blocked = report.wiki_blocked.as_deref();
        ui.colored_label(theme::TEXT_SECONDARY, report_headline(report));
        if let Some(blocked) = wiki_blocked {
            ui.colored_label(theme::COLOR_WARNING, blocked);
        }
        ui.add_space(4.0);
        // "Nothing new" would be false of a wiki that never got asked.
        let empty = if wiki_blocked.is_some() {
            ""
        } else {
            "Nothing new detected."
        };
        report_lines(ui, &report.findings, empty);
    }
}

/// "Changed 3 of 116 WADs, looking up 12 on the Doom Wiki."
fn report_headline(report: &EnrichReport) -> String {
    let verb = if report.dry_run {
        "Would change"
    } else {
        "Changed"
    };
    let mut headline = format!(
        "{verb} {} of {}",
        report.findings.len(),
        count(report.examined, "WAD", "WADs")
    );
    if report.wiki_lookups > 0 {
        headline.push_str(&format!(
            ", looking up {} on the Doom Wiki",
            report.wiki_lookups
        ));
    }
    if report.cancelled {
        headline.push_str(", then stopped");
    }
    headline.push('.');
    headline
}

/// Render a list of report lines, or `empty` when there are none.
fn report_lines(ui: &mut egui::Ui, lines: &[String], empty: &str) {
    if lines.is_empty() {
        if !empty.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, empty);
        }
        return;
    }
    for line in lines {
        ui.label(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> EnrichReport {
        EnrichReport {
            examined: 116,
            findings: vec!["Road \u{2192} complevel 9 (Boom)".to_string()],
            wiki_lookups: 0,
            wiki_blocked: None,
            cancelled: false,
            dry_run: false,
        }
    }

    #[test]
    fn the_headline_reads_as_a_sentence() {
        assert_eq!(report_headline(&report()), "Changed 1 of 116 WADs.");

        let one = EnrichReport {
            examined: 1,
            ..report()
        };
        assert_eq!(report_headline(&one), "Changed 1 of 1 WAD.");

        let everything = EnrichReport {
            wiki_lookups: 12,
            cancelled: true,
            dry_run: true,
            ..report()
        };
        assert_eq!(
            report_headline(&everything),
            "Would change 1 of 116 WADs, looking up 12 on the Doom Wiki, then stopped."
        );
    }

    /// There is nothing to report before anything has been run, and it says nothing.
    #[test]
    fn an_idle_dialog_has_no_output_to_show() {
        let state = EnrichDialogState::new();
        assert!(!state.running);
        assert!(state.report.is_none());
        assert!(state.error.is_none());
    }
}
