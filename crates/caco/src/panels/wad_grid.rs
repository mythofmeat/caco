use egui::{CornerRadius, Rect, StrokeKind, Vec2};

use crate::state::{ActionRequest, AppState};
use crate::theme;

/// Type alias to disambiguate from caco_core::db::sessions::WadStats.
type StatsData = caco_core::wad_stats::WadStats;

/// Card dimensions.
const CARD_MIN_WIDTH: f32 = 190.0;
const CARD_MAX_WIDTH: f32 = 260.0;
const CARD_SPACING: f32 = 14.0;
const CARD_ROUNDING: u8 = 12;
const THUMB_ASPECT: f32 = 0.75; // height = width * aspect (4:3)

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
pub fn render(
    ui: &mut egui::Ui,
    state: &mut AppState,
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
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

        // Handle keyboard shortcuts
        if !state.has_dialog() && !ui.ctx().egui_wants_keyboard_input() {
            if ui.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown)) {
                state.select_down_grid(columns);
            }
            if ui.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp)) {
                state.select_up_grid(columns);
            }
            if ui.input(|i| i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::ArrowLeft)) {
                state.select_left(columns);
            }
            if ui.input(|i| i.key_pressed(egui::Key::L) || i.key_pressed(egui::Key::ArrowRight)) {
                state.select_right(columns);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Home)) {
                state.select_first();
            }
            if ui.input(|i| i.key_pressed(egui::Key::End)) {
                state.select_last();
            }
            if ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::G)) {
                state.select_last();
            } else if ui.input(|i| !i.modifiers.shift && i.key_pressed(egui::Key::G)) {
                state.handle_g_press();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                state.selected_wad_id = None;
            }

            action = super::handle_action_keys(ui, state.selected_wad_id);
        }

        let thumb_width = card_w;
        let thumb_height = thumb_width * THUMB_ASPECT;
        let text_height = ui.text_style_height(&egui::TextStyle::Body);
        // Card body: title + author + footer (status + stars + playtime) + progress bar space
        let progress_bar_height = 6.0; // thin progress bar + spacing
        let body_height = 12.0 + text_height * 2.0 + 8.0 + 20.0 + 12.0 + progress_bar_height;
        let card_height = thumb_height + body_height;

        let wad_count = state.wads.len();
        let rows = wad_count.div_ceil(columns);

        for row in 0..rows {
            ui.horizontal(|ui| {
                for col in 0..columns {
                    let idx = row * columns + col;
                    if idx >= wad_count {
                        break;
                    }

                    let wad = &state.wads[idx];
                    let wad_id = wad.id;
                    let is_selected = state.selected_row == idx;
                    let stats = state.stats_map.get(&wad_id);

                    // Allocate card space
                    let (rect, response) = ui
                        .allocate_exact_size(Vec2::new(card_w, card_height), egui::Sense::click());

                    // Virtualization: skip painting + event handling for cards
                    // entirely outside the scroll viewport. Layout (scrollbar
                    // extents, selection indices) still lines up because
                    // allocate_exact_size ran.
                    let clip = ui.clip_rect();
                    if rect.max.y < clip.min.y || rect.min.y > clip.max.y {
                        continue;
                    }

                    // Handle clicks
                    if response.clicked() || response.secondary_clicked() {
                        state.selected_row = idx;
                        state.selected_wad_id = Some(wad_id);
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

                    let painter = ui.painter_at(rect);
                    let rounding = CornerRadius::same(CARD_ROUNDING);

                    // Card background
                    let bg = theme::BG_MEDIUM;
                    painter.rect_filled(rect, rounding, bg);

                    // Border painted at the end of the card body — see the
                    // matching block below the progress bar.

                    // Thumbnail area
                    let thumb_rect =
                        Rect::from_min_size(rect.min, Vec2::new(thumb_width, thumb_height));

                    // Clip thumbnail to card's top rounding
                    let thumb_rounding = CornerRadius {
                        nw: CARD_ROUNDING,
                        ne: CARD_ROUNDING,
                        sw: 0,
                        se: 0,
                    };

                    theme::paint_thumbnail(
                        &painter,
                        thumb_rect,
                        thumb_rounding,
                        thumbnails.and_then(|tm| tm.get(wad_id)),
                    );

                    // Card body
                    let body_x = rect.min.x + 14.0;
                    let body_top = thumb_rect.max.y + 10.0;

                    // Title
                    let title = caco_core::utils::truncate(&wad.title, 26, "..");
                    painter.text(
                        egui::pos2(body_x, body_top),
                        egui::Align2::LEFT_TOP,
                        &title,
                        egui::FontId::proportional(14.0),
                        theme::TEXT_PRIMARY,
                    );

                    // Author
                    let author = wad.author.as_deref().unwrap_or("");
                    let author = caco_core::utils::truncate(author, 28, "..");
                    painter.text(
                        egui::pos2(body_x, body_top + text_height + 2.0),
                        egui::Align2::LEFT_TOP,
                        &author,
                        egui::FontId::proportional(12.0),
                        theme::TEXT_SECONDARY,
                    );

                    // Footer row: status badge + stars + playtime
                    let footer_y = body_top + text_height * 2.0 + 10.0;

                    // Status, and how many times over if it has been finished more than
                    // once. The word is the mark: it used to be said twice, by a check mark
                    // over the picture too.
                    let times_beaten = stats.map_or(0, |s| s.times_beaten);
                    let replays =
                        if wad.status == caco_core::db::Status::Completed && times_beaten > 1 {
                            format!(" \u{00d7}{times_beaten}")
                        } else {
                            String::new()
                        };
                    let badge = theme::paint_status_pill(
                        &painter,
                        body_x,
                        footer_y + 9.0,
                        theme::status_style(wad.status),
                        &replays,
                    );
                    let badge_width = badge.width();

                    // Stars (right of status badge)
                    let stars = theme::rating_stars(wad.rating);
                    if !stars.is_empty() {
                        painter.text(
                            egui::pos2(body_x + badge_width + 8.0, footer_y + 2.0),
                            egui::Align2::LEFT_TOP,
                            &stars,
                            egui::FontId::proportional(11.0),
                            theme::TEXT_ACCENT,
                        );
                    }

                    // Playtime right-aligned
                    if let Some(s) = stats
                        && s.playtime > 0
                    {
                        let playtime_str = caco_core::player::format_duration(s.playtime);
                        painter.text(
                            egui::pos2(rect.right() - 14.0, footer_y + 2.0),
                            egui::Align2::RIGHT_TOP,
                            &playtime_str,
                            egui::FontId::proportional(11.0),
                            theme::TEXT_MUTED,
                        );
                    }

                    // Progress bar for in-progress WADs.
                    //
                    // Sourced from `state.analyses_map` (the single source of
                    // truth shared with the auto-completion verdict). When no
                    // fresh analysis is cached the bar is hidden — better than
                    // the old `wad_stats.maps.len()` fallback, which always
                    // showed 100% because levelstat snapshots only record maps
                    // the player has exited.
                    if wad.status == caco_core::db::Status::InProgress
                        && let Some(analysis) = state.analyses_map.get(&wad_id)
                        && analysis.required_maps > 0
                        && let Some(ref snapshot_json) = wad.stats_snapshot
                        && let Ok(wad_stats) = serde_json::from_str::<StatsData>(snapshot_json)
                    {
                        use caco_core::wad_analysis::MapClassification;
                        use std::collections::HashSet;
                        let required: HashSet<&str> = analysis
                            .maps
                            .iter()
                            .filter(|m| m.classification == MapClassification::Required)
                            .map(|m| m.lump.as_str())
                            .collect();
                        let played = wad_stats
                            .maps
                            .iter()
                            .filter(|m| m.total_exits >= 1 && required.contains(m.lump.as_str()))
                            .count();
                        let total = analysis.required_maps;
                        let pct = (played as f32 / total as f32).clamp(0.0, 1.0);

                        let bar_y = footer_y + 22.0;
                        let bar_h = 4.0;
                        let bar_x = body_x;
                        let bar_w = rect.right() - 14.0 - bar_x;
                        let bar_rect =
                            Rect::from_min_size(egui::pos2(bar_x, bar_y), Vec2::new(bar_w, bar_h));
                        painter.rect_filled(bar_rect, 2.0, theme::BG_LIGHT);
                        if pct > 0.0 {
                            let fill_rect =
                                Rect::from_min_size(bar_rect.min, Vec2::new(bar_w * pct, bar_h));
                            painter.rect_filled(
                                fill_rect,
                                2.0,
                                theme::status_color(caco_core::db::Status::InProgress),
                            );
                        }
                    }

                    // Border last — painted inside the rect so the parent
                    // scroll area doesn't clip the outer pixels, and on
                    // top of the thumbnail so the ring doesn't get
                    // hidden by the image.
                    if is_selected {
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
            });

            // Spacing between rows
            ui.add_space(CARD_SPACING);
        }
    });

    action
}
