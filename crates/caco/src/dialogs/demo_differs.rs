//! Asked before playing a demo back in a way it wasn't recorded.
//!
//! A demo is a list of inputs, and plays back as recorded only with what it was recorded with.
//! caco keeps a note of that beside each demo it records, and `caco_core::player::play_demo`
//! refuses rather than decide when this machine can't match it: the sourceport, the build, the
//! WAD's file, the IWAD, a companion. Playing anyway is the user's call, and usually ends with
//! the player walking into walls, which is why it says so here instead of letting them find out.

use caco_core::demos::DemoDifference;
use caco_core::player::DemoOptions;

use crate::dialogs::{Dialog, Primary};
use crate::theme;

pub struct DemoDiffersDialogState {
    wad_id: i64,
    wad_title: String,
    demo: String,
    differences: Vec<DemoDifference>,
}

#[derive(Debug, Clone)]
pub enum DemoDiffersResult {
    Open,
    PlayAnyway {
        wad_id: i64,
        demo: String,
        opts: DemoOptions,
    },
    Cancelled,
}

impl DemoDiffersDialogState {
    pub fn new(
        wad_id: i64,
        wad_title: String,
        demo: String,
        differences: Vec<DemoDifference>,
    ) -> Self {
        Self {
            wad_id,
            wad_title,
            demo,
            differences,
        }
    }

    pub fn render(&self, ctx: &egui::Context) -> DemoDiffersResult {
        let mut result = DemoDiffersResult::Open;

        Dialog::new("Not As Recorded", [520.0, 340.0])
            .about(&self.wad_title)
            .show(ctx, |dialog| {
                dialog
                    .scroll(|ui| {
                        ui.colored_label(
                            theme::TEXT_SECONDARY,
                            format!("{} can't be played the way it was recorded:", self.demo),
                        );
                        for difference in &self.differences {
                            ui.horizontal_top(|ui| {
                                ui.colored_label(theme::TEXT_SECONDARY, "\u{2022}");
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(difference.to_string())
                                            .color(theme::TEXT_PRIMARY),
                                    )
                                    .wrap(),
                                );
                            });
                        }
                        ui.colored_label(
                            theme::TEXT_SECONDARY,
                            "Played anyway, it will probably go out of step with what was \
                             recorded.",
                        );
                    })
                    .buttons(|row| {
                        let ended = row.end(Some("Cancel"), Some(Primary::new("Play Anyway")));
                        if ended.dismissed {
                            result = DemoDiffersResult::Cancelled;
                        }
                        if ended.primary {
                            result = DemoDiffersResult::PlayAnyway {
                                wad_id: self.wad_id,
                                demo: self.demo.clone(),
                                opts: DemoOptions {
                                    accept_differences: true,
                                },
                            };
                        }
                    })
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return DemoDiffersResult::Cancelled;
        }
        result
    }
}
