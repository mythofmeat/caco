//! Asked before launching a WAD another machine has newer saves for.
//!
//! A save is a file a sourceport rewrites, and only the person playing knows whether they
//! meant to carry on from the one here. So `caco_core::player::play` refuses rather than
//! deciding, and this is where the question is put. Going ahead is remembered as this
//! machine's latest word on its saves, so the same ones are not asked about again.

use caco_core::player::PlayOptions;
use caco_core::sync::{NewerSaves, machine};

use crate::theme;

/// How many saves to name, of however many there are.
const SHOWN: usize = 4;

/// What sits outside the scrolling part: the title bar, the WAD's title, the button row.
const CHROME: f32 = 120.0;

pub struct NewerSavesDialogState {
    wad_id: i64,
    wad_title: String,
    saves: NewerSaves,
    /// What the launch had already been told to go ahead with.
    opts: PlayOptions,
}

#[derive(Debug, Clone, Copy)]
pub enum NewerSavesResult {
    Open,
    PlayAnyway(i64, PlayOptions),
    Cancelled,
}

impl NewerSavesDialogState {
    pub fn new(wad_id: i64, wad_title: String, saves: NewerSaves, opts: PlayOptions) -> Self {
        Self {
            wad_id,
            wad_title,
            saves,
            opts,
        }
    }

    pub fn render(&self, ctx: &egui::Context) -> NewerSavesResult {
        let mut result = NewerSavesResult::Open;

        crate::dialogs::modal_window(ctx, "Newer Saves", [460.0, 250.0]).show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.colored_label(
                theme::TEXT_PRIMARY,
                egui::RichText::new(&self.wad_title).heading(),
            );
            ui.add_space(4.0);
            // Shrinks to what it holds, since this is one question with a few file names.
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(crate::dialogs::modal_body_height(ctx, CHROME))
                .show(ui, |ui| {
                    ui.colored_label(theme::TEXT_SECONDARY, explanation(&self.saves));
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        "If your sync hasn't finished, wait for it. Playing now carries on from \
                         what is on this machine, and saving could overwrite the same slot.",
                    );
                    for line in file_lines(&self.saves.files) {
                        ui.label(
                            egui::RichText::new(line)
                                .monospace()
                                .small()
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Play Anyway").clicked() {
                    result = NewerSavesResult::PlayAnyway(
                        self.wad_id,
                        PlayOptions {
                            accept_newer_saves: true,
                            ..self.opts
                        },
                    );
                }
                if ui.button("Cancel").clicked() {
                    result = NewerSavesResult::Cancelled;
                }
            });
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return NewerSavesResult::Cancelled;
        }
        result
    }
}

/// "thinkpad has 2 saves for this WAD that are not on this machine, from 3 days ago." The
/// time is shown raw if it does not parse, rather than dropping the fact that says how old
/// the saves here are by comparison.
fn explanation(saves: &NewerSaves) -> String {
    let when = chrono::DateTime::parse_from_rfc3339(&saves.recorded_at)
        .map(|at| {
            crate::relative_time::relative_time_full(
                &at.with_timezone(&chrono::Local).naive_local(),
            )
        })
        .unwrap_or_else(|_| saves.recorded_at.clone());
    let (count, verb) = match saves.files.len() {
        1 => ("a save".to_string(), "isn't"),
        n => (format!("{n} saves"), "aren't"),
    };
    format!(
        "{} has {count} for this WAD that {verb} on this machine, from {when}.",
        machine::display_name(&saves.machine)
    )
}

/// The saves in question, by file name, since the rest of the path is the same for all of
/// them and much longer. A person has one or two at most in a slot they care about.
fn file_lines(files: &[String]) -> Vec<String> {
    let name = |path: &String| {
        path.rsplit('/')
            .next()
            .map_or_else(|| path.clone(), str::to_string)
    };
    let mut lines: Vec<String> = files.iter().take(SHOWN).map(name).collect();
    if files.len() > SHOWN {
        lines.push(format!("and {} more", files.len() - SHOWN));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saves(files: &[&str]) -> NewerSaves {
        NewerSaves {
            machine: "thinkpad-3fa9c1d2".to_string(),
            // Midday UTC, so the local date has the same year in any timezone.
            recorded_at: "2024-06-15T12:00:00+00:00".to_string(),
            files: files.iter().map(|file| file.to_string()).collect(),
        }
    }

    #[test]
    fn test_the_explanation_says_who_and_when_and_how_many() {
        let one = explanation(&saves(&["saves/slot1.dsg"]));
        assert!(one.starts_with("thinkpad has a save for this WAD that isn't on this machine"));
        assert!(one.contains("2024"), "{one}");

        let two = explanation(&saves(&["a.dsg", "b.dsg"]));
        assert!(
            two.starts_with("thinkpad has 2 saves for this WAD that aren't"),
            "{two}"
        );
    }

    #[test]
    fn test_a_time_that_does_not_parse_is_still_said() {
        let mut odd = saves(&["a.dsg"]);
        odd.recorded_at = "some time ago".to_string();
        assert!(explanation(&odd).contains("some time ago"));
    }

    #[test]
    fn test_saves_are_named_by_file_and_not_all_of_them() {
        assert_eq!(
            file_lines(&["doom2/x/slot1.dsg".to_string()]),
            ["slot1.dsg"]
        );

        let many: Vec<String> = (1..=6).map(|n| format!("s/{n}.dsg")).collect();
        let lines = file_lines(&many);
        assert_eq!(lines.len(), SHOWN + 1);
        assert_eq!(lines[SHOWN], "and 2 more");
    }
}
