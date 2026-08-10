//! Modal picker for linking a Cacoward entry to a library WAD.
//!
//! The cacoward auto-linker (idgames id + normalized title) catches most
//! matches, but anything with a renamed/abbreviated/aliased title needs a
//! manual link. This dialog shows the user's library with a filter input
//! and lets them pick a WAD to attach to the entry. Linking sets
//! `manual_override=true` so re-enrich won't clobber the choice.

use rusqlite::Connection;

use crate::theme;

#[derive(Debug, PartialEq, Eq)]
pub enum CacowardLinkResult {
    /// User picked a library WAD for the entry (cacoward pk, wad id).
    Linked(i64, i64),
    Cancelled,
    Open,
}

pub struct CacowardLinkDialogState {
    pub cacoward_pk: i64,
    pub cacoward_title: String,
    pub filter: String,
    /// Cached `(wad_id, title, author)` list — small library so we load it
    /// once on dialog open and filter client-side.
    candidates: Vec<(i64, String, Option<String>)>,
    selected: Option<i64>,
}

impl CacowardLinkDialogState {
    pub fn new(conn: &Connection, cacoward_pk: i64) -> Option<Self> {
        let record = caco_core::db::cacowards::get_cacoward(conn, cacoward_pk).ok()??;
        let wads = caco_core::db::search_wads(conn, None, Some("title"), false, false, 0).ok()?;
        let candidates: Vec<(i64, String, Option<String>)> = wads
            .into_iter()
            .map(|w| (w.id, w.title, w.author))
            .collect();
        Some(Self {
            cacoward_pk,
            cacoward_title: record.wad_title,
            filter: String::new(),
            candidates,
            selected: None,
        })
    }

    pub fn render(&mut self, ctx: &egui::Context) -> CacowardLinkResult {
        let mut result = CacowardLinkResult::Open;
        let mut close_with_cancel = false;

        egui::Window::new("Link cacoward entry")
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .default_height(540.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("Link “{}” to:", self.cacoward_title))
                        .size(13.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Filter").color(theme::TEXT_MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text("title or author")
                            .desired_width(f32::INFINITY),
                    );
                });

                ui.add_space(6.0);

                let needle = self.filter.to_lowercase();
                let matches: Vec<&(i64, String, Option<String>)> = if needle.is_empty() {
                    self.candidates.iter().collect()
                } else {
                    self.candidates
                        .iter()
                        .filter(|(_, t, a)| {
                            t.to_lowercase().contains(&needle)
                                || a.as_deref()
                                    .map(|s| s.to_lowercase().contains(&needle))
                                    .unwrap_or(false)
                        })
                        .collect()
                };

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(380.0)
                    .show(ui, |ui| {
                        for (id, title, author) in &matches {
                            let label = match author {
                                Some(a) if !a.is_empty() => format!("{title} — {a}"),
                                _ => title.clone(),
                            };
                            let selected = self.selected == Some(*id);
                            let row = ui.add_sized(
                                [ui.available_width(), 24.0],
                                egui::SelectableLabel::new(selected, label),
                            );
                            if row.clicked() {
                                self.selected = Some(*id);
                            }
                            if row.double_clicked() {
                                result = CacowardLinkResult::Linked(self.cacoward_pk, *id);
                            }
                        }
                        if matches.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.colored_label(theme::TEXT_MUTED, "No matching WADs.");
                            });
                        }
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close_with_cancel = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let confirm = ui.add_enabled(
                            self.selected.is_some(),
                            egui::Button::new(
                                egui::RichText::new("Link")
                                    .color(egui::Color32::from_rgb(0x1a, 0x0a, 0x04))
                                    .strong(),
                            )
                            .fill(theme::TEXT_ACCENT),
                        );
                        if confirm.clicked()
                            && let Some(wad_id) = self.selected
                        {
                            result = CacowardLinkResult::Linked(self.cacoward_pk, wad_id);
                        }
                    });
                });
            });

        if close_with_cancel {
            CacowardLinkResult::Cancelled
        } else {
            result
        }
    }
}
