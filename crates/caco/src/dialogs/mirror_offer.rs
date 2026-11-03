//! Offers to upload a WAD caco just got to one of the user's mirrors.
//!
//! How a mirror fills itself as you play, rather than being a project of its own.
//! Asked only about a WAD that nothing could fetch again, because that is the only
//! case a copy on a mirror changes anything, and asked once the game has closed:
//! `AppState::mirror_offers` holds the question until nothing else is going on.

use caco_core::config::{self, MirrorConfig};
use caco_core::mirrors::offer_targets;
use rusqlite::Connection;

use crate::theme;

pub struct MirrorOfferDialogState {
    wad_id: i64,
    title: String,
    filename: String,
    /// The mirrors it can go to: one button each.
    targets: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MirrorOfferResult {
    Open,
    Upload { wad_id: i64, mirror: String },
    Declined,
}

impl MirrorOfferDialogState {
    /// `None` when there is nothing to offer. Decided here rather than when the WAD
    /// was queued, because the answer can change while it waits: the file may have
    /// gone, or the WAD been trashed.
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        Self::new_with(conn, wad_id, &config::load_config().mirrors)
    }

    fn new_with(conn: &Connection, wad_id: i64, mirrors: &[MirrorConfig]) -> Option<Self> {
        let wad = caco_core::db::get_wad(conn, wad_id, false).ok()??;
        let targets = offer_targets(&wad, mirrors);
        if targets.is_empty() {
            return None;
        }
        let filename = wad
            .cached_path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some(Self {
            wad_id,
            title: wad.title,
            filename,
            targets,
        })
    }

    pub fn render(&self, ctx: &egui::Context) -> MirrorOfferResult {
        let mut result = MirrorOfferResult::Open;

        crate::dialogs::modal_window(ctx, "Keep a Copy on Your Mirror?", [440.0, 190.0]).show(
            ctx,
            |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                ui.colored_label(
                    theme::TEXT_PRIMARY,
                    egui::RichText::new(&self.title).heading(),
                );
                ui.add_space(4.0);
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    format!(
                        "caco can't download {} again if this copy goes. A copy on your \
                         mirror means any machine with this library can get it back.",
                        self.filename
                    ),
                );

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // Wrapped, because nothing limits how many mirrors there are.
                ui.horizontal_wrapped(|ui| {
                    for mirror in &self.targets {
                        if ui.button(format!("Upload to {mirror}")).clicked() {
                            result = MirrorOfferResult::Upload {
                                wad_id: self.wad_id,
                                mirror: mirror.clone(),
                            };
                        }
                    }
                    if ui.button("Not Now").clicked() {
                        result = MirrorOfferResult::Declined;
                    }
                });
            },
        );

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return MirrorOfferResult::Declined;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{self, NewWad, SourceType};

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        let id = db::add_wad(
            &conn,
            &NewWad::new("Road", SourceType::Doomworld).cached_path("/keep/road.zip"),
        )
        .unwrap();
        (conn, id)
    }

    fn mirror(name: &str, write: bool) -> MirrorConfig {
        MirrorConfig {
            name: name.to_string(),
            path: Some("/mnt/nas".to_string()),
            write,
            ..Default::default()
        }
    }

    #[test]
    fn test_the_offer_names_the_file_and_each_mirror_it_can_go_to() {
        let (conn, id) = setup();
        let mirrors = [
            mirror("home", true),
            mirror("read-only", false),
            mirror("nas", true),
        ];

        let dialog = MirrorOfferDialogState::new_with(&conn, id, &mirrors).unwrap();

        assert_eq!(dialog.title, "Road");
        assert_eq!(dialog.filename, "road.zip");
        assert_eq!(dialog.targets, ["home", "nas"]);
    }

    #[test]
    fn test_nothing_is_offered_without_a_writable_mirror() {
        let (conn, id) = setup();
        assert!(MirrorOfferDialogState::new_with(&conn, id, &[]).is_none());
        assert!(
            MirrorOfferDialogState::new_with(&conn, id, &[mirror("read-only", false)]).is_none()
        );
    }

    /// The answer can change while the WAD waits in the queue.
    #[test]
    fn test_a_wad_trashed_while_it_waited_is_not_offered() {
        let (conn, id) = setup();
        db::delete_wad(&conn, id, false).unwrap();
        assert!(MirrorOfferDialogState::new_with(&conn, id, &[mirror("home", true)]).is_none());
    }
}
