//! Modal picker for linking a Cacoward entry to a library WAD.
//!
//! The cacoward auto-linker catches what an identifier or a unique title can
//! prove, but a WAD imported some other way, under another title, needs a manual
//! link. This dialog shows the user's library with a filter input and lets them
//! pick a WAD to attach to the entry. Linking sets `manual_override=true` so a data
//! update won't clobber the choice.

use rusqlite::Connection;

use crate::dialogs::{Dialog, Primary, form_label, scroll_body};
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
        let cacoward_pk = self.cacoward_pk;
        let about = self.cacoward_title.clone();

        Dialog::new("Link to a WAD in Your Library", [520.0, 540.0])
            .about(about)
            .show(ctx, |dialog| {
                dialog
                    .pinned(|ui| {
                        ui.horizontal(|ui| {
                            let label = form_label(ui, "Filter");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.filter)
                                    .hint_text("title or author")
                                    .desired_width(f32::INFINITY),
                            )
                            .labelled_by(label.id);
                        });
                    })
                    .fill(|ui, height| {
                        if let Some(wad_id) = self.render_matches(ui, height) {
                            result = CacowardLinkResult::Linked(cacoward_pk, wad_id);
                        }
                    })
                    .buttons(|row| {
                        let primary =
                            Primary::new("Link").when(self.selected.is_some(), "Pick a WAD first");
                        let ended = row.end(Some("Cancel"), Some(primary));
                        if ended.dismissed {
                            result = CacowardLinkResult::Cancelled;
                        }
                        if ended.primary
                            && let Some(wad_id) = self.selected
                        {
                            result = CacowardLinkResult::Linked(cacoward_pk, wad_id);
                        }
                    })
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return CacowardLinkResult::Cancelled;
        }
        result
    }

    /// The WADs the filter leaves. Returns one that was double-clicked, which means link it.
    fn render_matches(&mut self, ui: &mut egui::Ui, height: f32) -> Option<i64> {
        let needle = self.filter.to_lowercase();
        let matches: Vec<&(i64, String, Option<String>)> = self
            .candidates
            .iter()
            .filter(|(_, title, author)| {
                needle.is_empty()
                    || title.to_lowercase().contains(&needle)
                    || author
                        .as_deref()
                        .is_some_and(|author| author.to_lowercase().contains(&needle))
            })
            .collect();

        let mut clicked = None;
        let mut linked = None;
        scroll_body(ui, height, |ui| {
            let justified = egui::Layout::top_down_justified(egui::Align::LEFT);
            ui.with_layout(justified, |ui| {
                for (id, title, author) in &matches {
                    let label = match author {
                        Some(author) if !author.is_empty() => format!("{title} \u{2014} {author}"),
                        _ => title.clone(),
                    };
                    let row = ui.selectable_label(self.selected == Some(*id), label);
                    if row.clicked() {
                        clicked = Some(*id);
                    }
                    if row.double_clicked() {
                        linked = Some(*id);
                    }
                }
                if matches.is_empty() {
                    ui.colored_label(theme::TEXT_SECONDARY, "No matching WADs.");
                }
            });
        });
        if clicked.is_some() {
            self.selected = clicked;
        }
        linked
    }
}
