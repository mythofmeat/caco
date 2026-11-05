use egui::{CornerRadius, Rect, StrokeKind, Vec2};

use caco_core::db::{Status, WadRecord};

use crate::state::{ActionRequest, AppState, MapProgress};
use crate::theme;

/// Card dimensions. The minimum is what fits three across at the 800x400 window and five at the
/// default one, with the whole of a row in view at either.
const CARD_MIN_WIDTH: f32 = 176.0;
const CARD_MAX_WIDTH: f32 = 260.0;
const CARD_SPACING: f32 = 14.0;
const CARD_ROUNDING: u8 = 10;
const THUMB_ASPECT: f32 = 0.75; // height = width * aspect (4:3)
/// Title, author and the row with the status in it, and the gaps around them.
const CARD_BODY_HEIGHT: f32 = 76.0;
const CARD_PADDING: f32 = 10.0;
const PROGRESS_HEIGHT: f32 = 4.0;

/// Five stars at this size take about this much.
const STARS_WIDTH: f32 = 60.0;
/// A star and a number do.
const SHORT_RATING_WIDTH: f32 = 26.0;

/// Calculate responsive card width.
fn card_width(available_width: f32) -> f32 {
    let columns = ((available_width + CARD_SPACING) / (CARD_MIN_WIDTH + CARD_SPACING))
        .floor()
        .max(1.0);
    let width = (available_width - (columns - 1.0) * CARD_SPACING) / columns;
    width.clamp(CARD_MIN_WIDTH, CARD_MAX_WIDTH)
}

/// Render the WAD library as a grid of cards.
/// Returns an action request if a keyboard shortcut was pressed.
///
/// `view_top` is where what scrolls with it starts, above the grid.
pub fn render(
    ui: &mut egui::Ui,
    state: &mut AppState,
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
    view_top: f32,
) -> Option<ActionRequest> {
    // Add horizontal margin
    let margin = egui::Margin::symmetric(20, 0);
    let mut action = None;

    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        let available_width = ui.available_width();
        let card_w = card_width(available_width);
        let columns = ((available_width + CARD_SPACING) / (card_w + CARD_SPACING))
            .floor()
            .max(1.0) as usize;

        let thumb_height = (card_w * THUMB_ASPECT).round();
        let card_size = Vec2::new(card_w, thumb_height + CARD_BODY_HEIGHT);

        // Rows on the screen, for how far a page is.
        let rows_in_view = (ui.clip_rect().height() / (card_size.y + CARD_SPACING))
            .floor()
            .max(1.0) as usize;
        action = super::library_keys(
            ui,
            state,
            super::LibraryShape {
                columns,
                page: rows_in_view * columns,
            },
        );

        let wad_count = state.wads.len();
        let rows = wad_count.div_ceil(columns);

        for row in 0..rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CARD_SPACING;
                for col in 0..columns {
                    let idx = row * columns + col;
                    if idx >= wad_count {
                        break;
                    }

                    let wad = &state.wads[idx];
                    let wad_id = wad.id;

                    let selected = state.selected_wad_id == Some(wad_id);

                    // Allocate card space
                    let (rect, response) = ui.allocate_exact_size(card_size, egui::Sense::click());

                    // Before the virtualization below, since off the screen is the point.
                    if selected && state.reveal_selection {
                        state.reveal_selection = false;
                        let with_gap = rect.expand2(Vec2::new(0.0, CARD_SPACING));
                        super::reveal(ui, with_gap, row == 0, view_top);
                    }

                    // Virtualization: skip painting + event handling for cards
                    // entirely outside the scroll viewport. Layout (scrollbar
                    // extents, selection indices) still lines up because
                    // allocate_exact_size ran.
                    let clip = ui.clip_rect();
                    if rect.max.y < clip.min.y || rect.min.y > clip.max.y {
                        continue;
                    }

                    if response.double_clicked() {
                        action = Some(ActionRequest::Play(wad_id));
                    }

                    // Context menu on right-click
                    if let Some(a) =
                        super::wad_context_menu(&response, wad_id, wad.status, wad.is_lost())
                    {
                        action = Some(a);
                    }

                    let card = Card {
                        wad,
                        stats: state.stats_map.get(&wad_id),
                        progress: if wad.status == Status::InProgress {
                            state.map_progress(wad)
                        } else {
                            None
                        },
                        picture: thumbnails.and_then(|tm| tm.get(wad_id)),
                        selected,
                    };
                    card.paint(ui, rect, thumb_height, &response);

                    if response.clicked() || response.secondary_clicked() {
                        state.select_wad(wad_id);
                    }
                }
            });

            // Spacing between rows
            ui.add_space(CARD_SPACING);
        }
    });

    action
}

/// What one card shows.
struct Card<'a> {
    wad: &'a WadRecord,
    stats: Option<&'a caco_core::db::sessions::WadStats>,
    progress: Option<MapProgress>,
    picture: Option<&'a egui::TextureHandle>,
    selected: bool,
}

impl Card<'_> {
    fn paint(&self, ui: &egui::Ui, rect: Rect, thumb_height: f32, response: &egui::Response) {
        let painter = ui.painter_at(rect);
        let rounding = CornerRadius::same(CARD_ROUNDING);
        let word = theme::status_display(self.wad.status);

        // A painted card is pixels otherwise, and says nothing to a screen reader.
        let name = format!("{}, {word}", self.wad.title);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        response.clone().on_hover_ui(|ui| {
            ui.label(self.hover_text());
        });

        painter.rect_filled(rect, rounding, theme::BG_MEDIUM);

        let thumb = Rect::from_min_size(rect.min, Vec2::new(rect.width(), thumb_height));
        let thumb_rounding = CornerRadius {
            nw: CARD_ROUNDING,
            ne: CARD_ROUNDING,
            sw: 0,
            se: 0,
        };
        theme::paint_thumbnail(&painter, thumb, thumb_rounding, self.picture);

        // Along the bottom of the picture, where it costs a card no height.
        if let Some(progress) = &self.progress {
            let bar = Rect::from_min_max(
                egui::pos2(thumb.min.x, thumb.max.y - PROGRESS_HEIGHT),
                thumb.max,
            );
            painter.rect_filled(bar, 0.0, theme::BG_DARK);
            let fill = Rect::from_min_size(
                bar.min,
                Vec2::new(bar.width() * progress.fraction(), PROGRESS_HEIGHT),
            );
            painter.rect_filled(fill, 0.0, theme::status_color(Status::InProgress));
        }

        // Body
        let left = rect.min.x + CARD_PADDING;
        let right = rect.max.x - CARD_PADDING;
        let width = right - left;
        let top = thumb.max.y + 8.0;

        theme::paint_truncated(
            &painter,
            egui::pos2(left, top),
            &self.wad.title,
            egui::FontId::proportional(14.0),
            theme::TEXT_PRIMARY,
            width,
        );
        theme::paint_truncated(
            &painter,
            egui::pos2(left, top + 19.0),
            self.wad.author.as_deref().unwrap_or(""),
            egui::FontId::proportional(12.0),
            theme::TEXT_SECONDARY,
            width,
        );

        self.paint_footer(ui, &painter, left, right, rect.max.y - 18.0);

        // Border last — painted inside the rect so the parent scroll area doesn't clip the
        // outer pixels, and on top of the thumbnail so the ring doesn't get hidden by the
        // image.
        if self.selected {
            painter.rect_stroke(
                rect,
                rounding,
                egui::Stroke::new(2.0_f32, theme::TEXT_ACCENT),
                StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_stroke(
                rect,
                rounding,
                egui::Stroke::new(1.0_f32, theme::BORDER_MED),
                StrokeKind::Inside,
            );
        }
    }

    /// The row along the bottom: status, rating and playtime, or as much of that as fits, in
    /// the order of what matters. The word is the mark for having finished it: there used
    /// to be a check mark over the picture as well.
    fn paint_footer(
        &self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        left: f32,
        right: f32,
        center_y: f32,
    ) {
        let style = theme::status_style(self.wad.status);
        let extra = replays(self.wad.status, self.stats.map_or(0, |s| s.times_beaten));
        let pill = theme::paint_status_pill(painter, left, center_y, style, &extra);
        let mut free = right - pill.max.x - 6.0;

        let font = egui::FontId::proportional(11.0);
        if let Some(stats) = self.stats.filter(|stats| stats.playtime > 0) {
            let text = caco_core::player::format_duration(stats.playtime);
            let width = ui
                .painter()
                .layout_no_wrap(text.clone(), font.clone(), theme::TEXT_MUTED)
                .size()
                .x;
            if width <= free {
                painter.text(
                    egui::pos2(right, center_y),
                    egui::Align2::RIGHT_CENTER,
                    text,
                    font.clone(),
                    theme::TEXT_MUTED,
                );
                free -= width + 6.0;
            }
        }

        let stars = match self.wad.rating {
            Some(rating) if rating > 0 && free >= STARS_WIDTH => theme::rating_stars(Some(rating)),
            Some(rating) if rating > 0 && free >= SHORT_RATING_WIDTH => {
                format!("\u{2605} {rating}")
            }
            _ => return,
        };
        painter.text(
            egui::pos2(pill.max.x + 6.0, center_y),
            egui::Align2::LEFT_CENTER,
            stars,
            font,
            theme::GOLD,
        );
    }

    /// All of what a card might have had to leave out.
    fn hover_text(&self) -> String {
        let mut lines = vec![self.wad.title.clone()];
        if let Some(author) = &self.wad.author {
            lines.push(author.clone());
        }
        if let Some(progress) = &self.progress {
            lines.push(progress.summary());
        }
        if let Some(stats) = self.stats {
            match stats.times_beaten {
                0 => {}
                1 => lines.push("Finished once".to_string()),
                n => lines.push(format!("Finished {n} times")),
            }
            if let Some(when) = stats
                .last_played
                .as_deref()
                .and_then(crate::relative_time::parse_timestamp)
            {
                lines.push(format!(
                    "Last played {}",
                    crate::relative_time::relative_time(&when)
                ));
            }
        }
        lines.join("\n")
    }
}

/// What goes after a status's word on a card: how many times over, for what has been finished
/// more than once.
fn replays(status: Status, times_beaten: i64) -> String {
    if status == Status::Completed && times_beaten > 1 {
        format!(" \u{00d7}{times_beaten}")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the two window sizes that matter get: 200 of the sidebar, and 20 of margin a side.
    #[test]
    fn cards_fit_three_across_at_the_smallest_window_and_five_at_the_default() {
        let across = |window: f32| {
            let available = window - 200.0 - 40.0;
            let width = card_width(available);
            ((available + CARD_SPACING) / (width + CARD_SPACING)).floor() as usize
        };
        assert_eq!(across(800.0), 3);
        assert_eq!(across(1200.0), 5);
    }

    /// The whole of a row has to be in view at the smallest window, or it is a grid of tops.
    #[test]
    fn a_card_fits_in_what_the_smallest_window_leaves() {
        let width = card_width(800.0 - 200.0 - 40.0);
        let height = (width * THUMB_ASPECT).round() + CARD_BODY_HEIGHT;
        // 400, less the top and status bars, a one-line banner and the row of filters.
        assert!(height <= 400.0 - 38.0 - 30.0 - 60.0 - 40.0, "{height}");
    }

    #[test]
    fn a_second_playthrough_shows_only_once_it_is_finished() {
        assert_eq!(replays(Status::Completed, 1), "");
        assert_eq!(replays(Status::Completed, 3), " \u{00d7}3");
        assert_eq!(replays(Status::InProgress, 3), "");
    }
}
