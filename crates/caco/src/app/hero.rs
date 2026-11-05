//! The banner over the library: what is being played, or what to carry on with.
//!
//! At full height it has room for the picture and the progress, and below
//! `theme::SHORT_WINDOW` it collapses to a line, since at the 800x400 minimum it used to take
//! half the window and leave the grid the tops of two cards.

use caco_core::db::sessions::WadStats;
use caco_core::db::{Status, WadRecord};
use std::collections::HashMap;

use crate::panels;
use crate::state::{ActionRequest, AppState, MapProgress, PlayState};
use crate::theme;
use crate::thumbnails::ThumbnailManager;

const FULL_THUMB: egui::Vec2 = egui::vec2(96.0, 72.0);
const SHORT_THUMB: egui::Vec2 = egui::vec2(48.0, 36.0);
const THUMB_ROUNDING: u8 = 8;

/// Render the banner. Returns an action request if the user interacted with it.
pub(super) fn render_now_playing_hero(
    ui: &mut egui::Ui,
    state: &AppState,
    thumbnails: &ThumbnailManager,
) -> Option<ActionRequest> {
    let (wad_id, title, is_active) = match &state.play_state {
        PlayState::Playing { wad_id, wad_title } => (*wad_id, wad_title.clone(), true),
        PlayState::Idle => {
            let wad = continue_playing(&state.wads, &state.stats_map)?;
            (wad.id, wad.title.clone(), false)
        }
    };
    let wad = state.wads.iter().find(|wad| wad.id == wad_id);
    let author = wad.and_then(|wad| wad.author.as_deref()).unwrap_or("");
    let playtime = state
        .stats_map
        .get(&wad_id)
        .filter(|stats| stats.playtime > 0)
        .map(|stats| caco_core::player::format_duration(stats.playtime));
    let progress = wad.and_then(|wad| state.map_progress(wad));
    let heading = if is_active {
        "NOW PLAYING"
    } else {
        "CONTINUE PLAYING"
    };

    let short = theme::is_short_window(ui.ctx());
    let (margin_y, thumb_size) = if short {
        (6, SHORT_THUMB)
    } else {
        (12, FULL_THUMB)
    };

    let mut action = None;
    ui.add_space(if short { 8.0 } else { 16.0 });
    let frame = egui::Frame::new()
        .fill(theme::BG_HERO)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(14, margin_y))
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_MED))
        .outer_margin(egui::Margin::symmetric(20, 0))
        .show(ui, |ui| {
            let (row, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), thumb_size.y),
                egui::Sense::hover(),
            );
            let thumb = egui::Rect::from_min_size(row.min, thumb_size);
            theme::paint_thumbnail(
                ui.painter(),
                thumb,
                egui::CornerRadius::same(THUMB_ROUNDING),
                thumbnails.get(wad_id),
            );

            // From the right first, so that the details get what is left of the row, whatever
            // the two take: nesting them in layouts leaves the details no width at all.
            let mut right = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(row)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            if !is_active && right.add(theme::primary_button("Play")).clicked() {
                action = Some(ActionRequest::Play(wad_id));
            }
            if let Some(playtime) = &playtime {
                right.add_space(8.0);
                playtime_block(&mut right, playtime, short);
            }
            let details = egui::Rect::from_min_max(
                egui::pos2(thumb.max.x + 12.0, row.min.y),
                egui::pos2(right.min_rect().min.x - 12.0, row.max.y),
            );
            let mut details_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(details)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            details_ui.spacing_mut().item_spacing.y = 2.0;
            if short {
                short_details(&mut details_ui, heading, &title, progress.as_ref());
            } else {
                full_details(&mut details_ui, heading, &title, author, progress.as_ref());
            }
        });

    // The whole banner stands for the WAD: double-click to play it, right-click for the rest.
    let banner = frame.response.interact(egui::Sense::click());
    if banner.double_clicked() && !is_active {
        action = Some(ActionRequest::Play(wad_id));
    }
    if let Some(picked) = wad.and_then(|wad| panels::wad_context_menu(&banner, wad)) {
        action = Some(picked);
    }

    action
}

/// Which WAD to offer: the one in progress that was played last, rather than whichever
/// comes first in the sort order, which is not what "continue" means.
fn continue_playing<'a>(
    wads: &'a [WadRecord],
    stats: &HashMap<i64, WadStats>,
) -> Option<&'a WadRecord> {
    wads.iter()
        .rev()
        .filter(|wad| wad.status == Status::InProgress)
        .max_by_key(|wad| {
            stats
                .get(&wad.id)
                .and_then(|stats| stats.last_played.as_deref())
        })
}

fn heading_label(ui: &mut egui::Ui, heading: &str) {
    ui.colored_label(
        theme::status_color(Status::InProgress),
        egui::RichText::new(heading).size(11.0).strong(),
    );
}

fn full_details(
    ui: &mut egui::Ui,
    heading: &str,
    title: &str,
    author: &str,
    progress: Option<&MapProgress>,
) {
    heading_label(ui, heading);
    ui.add(
        egui::Label::new(
            egui::RichText::new(title)
                .size(18.0)
                .strong()
                .color(theme::TEXT_PRIMARY),
        )
        .truncate(),
    );
    if !author.is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new(author)
                    .size(12.0)
                    .color(theme::TEXT_SECONDARY),
            )
            .truncate(),
        );
    }
    if let Some(progress) = progress {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let (bar, _) = ui.allocate_exact_size(egui::vec2(160.0, 5.0), egui::Sense::hover());
            theme::paint_progress(ui.painter(), bar, progress.fraction());
            ui.colored_label(
                theme::TEXT_SECONDARY,
                egui::RichText::new(progress.summary()).size(11.0),
            );
        });
    }
}

fn short_details(ui: &mut egui::Ui, heading: &str, title: &str, progress: Option<&MapProgress>) {
    ui.horizontal(|ui| {
        heading_label(ui, heading);
        if let Some(progress) = progress {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                egui::RichText::new(progress.summary()).size(11.0),
            );
        }
    });
    ui.add(
        egui::Label::new(
            egui::RichText::new(title)
                .size(14.0)
                .strong()
                .color(theme::TEXT_PRIMARY),
        )
        .truncate(),
    );
}

/// Painted to its own measure rather than in a right-aligned layout, which would take all the
/// width it was offered and leave none for the details.
fn playtime_block(ui: &mut egui::Ui, playtime: &str, short: bool) {
    let painter = ui.painter();
    let value = painter.layout_no_wrap(
        playtime.to_string(),
        egui::FontId::proportional(if short { 14.0 } else { 20.0 }),
        theme::TEXT_PRIMARY,
    );
    let caption = painter.layout_no_wrap(
        "played".to_string(),
        egui::FontId::proportional(11.0),
        theme::TEXT_MUTED,
    );
    let size = egui::vec2(
        value.size().x.max(caption.size().x),
        value.size().y + caption.size().y,
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{playtime} played"))
    });
    let right = rect.right();
    let caption_top = rect.top() + value.size().y;
    let value_left = right - value.size().x;
    let caption_left = right - caption.size().x;
    let painter = ui.painter();
    painter.galley(
        egui::pos2(value_left, rect.top()),
        value,
        theme::TEXT_PRIMARY,
    );
    painter.galley(
        egui::pos2(caption_left, caption_top),
        caption,
        theme::TEXT_MUTED,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{self, NewWad, SourceType};

    #[test]
    fn it_offers_the_wad_in_progress_that_was_played_last() {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        let add = |title: &str, status: Status| {
            let id =
                db::add_wad(&conn, &NewWad::new(title, SourceType::Local).status(status)).unwrap();
            db::get_wad(&conn, id, false).unwrap().unwrap()
        };
        let played = |at: &str| WadStats {
            playtime: 100,
            last_played: Some(at.to_string()),
            session_count: 1,
            times_beaten: 0,
        };

        let wads = vec![
            add("Older", Status::InProgress),
            add("Finished", Status::Completed),
            add("Newer", Status::InProgress),
            add("Never", Status::InProgress),
        ];
        let stats = HashMap::from([
            (wads[0].id, played("2026-01-01T00:00:00+00:00")),
            (wads[1].id, played("2026-09-01T00:00:00+00:00")),
            (wads[2].id, played("2026-02-01T00:00:00+00:00")),
        ]);

        assert_eq!(continue_playing(&wads, &stats).unwrap().title, "Newer");

        // Nothing to go by, so the first it is.
        let unplayed = HashMap::new();
        assert_eq!(continue_playing(&wads, &unplayed).unwrap().title, "Older");

        assert!(continue_playing(&wads[1..2], &stats).is_none());
    }
}
