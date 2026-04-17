use egui::{Color32, CornerRadius, Rect, StrokeKind, Vec2};

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
        if !state.has_dialog() && !ui.ctx().wants_keyboard_input() {
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
                    if let Some(a) = super::wad_context_menu(&response, wad_id, wad.status) {
                        action = Some(a);
                    }

                    let painter = ui.painter_at(rect);
                    let rounding = CornerRadius::same(CARD_ROUNDING);

                    // Card background
                    let bg = theme::BG_MEDIUM;
                    painter.rect_filled(rect, rounding, bg);

                    // Border
                    if is_selected {
                        painter.rect_stroke(
                            rect,
                            rounding,
                            egui::Stroke::new(2.0, theme::TEXT_ACCENT),
                            StrokeKind::Outside,
                        );
                    } else if response.hovered() {
                        painter.rect_stroke(
                            rect,
                            rounding,
                            egui::Stroke::new(1.0, theme::BORDER_MED),
                            StrokeKind::Outside,
                        );
                    }

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

                    // Try to show a real thumbnail, fall back to placeholder
                    let mut drew_thumbnail = false;
                    if let Some(tm) = thumbnails
                        && let Some(tex) = tm.get(wad_id)
                    {
                        // Clip the image to rounded top corners
                        painter.rect_filled(thumb_rect, thumb_rounding, Color32::BLACK);
                        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        painter.image(tex.id(), thumb_rect, uv, Color32::WHITE);
                        drew_thumbnail = true;
                    }

                    if !drew_thumbnail {
                        // Smooth gradient placeholder from c1 (top) to c2 (bottom)
                        let (c1, c2, ci) = theme::thumb_colors(wad_id);
                        let steps = 32;
                        // Fill background with c1 for the rounded top corners
                        painter.rect_filled(thumb_rect, thumb_rounding, c1);
                        for i in 0..steps {
                            let t0 = i as f32 / steps as f32;
                            let t1 = (i + 1) as f32 / steps as f32;
                            let band_y0 = thumb_rect.min.y + thumb_rect.height() * t0;
                            let band_y1 = thumb_rect.min.y + thumb_rect.height() * t1;
                            let r = c1.r() as f32 + (c2.r() as f32 - c1.r() as f32) * t0;
                            let g = c1.g() as f32 + (c2.g() as f32 - c1.g() as f32) * t0;
                            let b = c1.b() as f32 + (c2.b() as f32 - c1.b() as f32) * t0;
                            let band = Rect::from_min_max(
                                egui::pos2(thumb_rect.min.x, band_y0),
                                egui::pos2(thumb_rect.max.x, band_y1),
                            );
                            painter.rect_filled(
                                band,
                                0,
                                Color32::from_rgb(r as u8, g as u8, b as u8),
                            );
                        }

                        // Initials
                        let initials: String = wad
                            .title
                            .chars()
                            .filter(|c| c.is_alphanumeric())
                            .take(2)
                            .flat_map(|c| c.to_uppercase())
                            .collect();
                        if !initials.is_empty() {
                            painter.text(
                                thumb_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &initials,
                                egui::FontId::proportional(28.0),
                                ci,
                            );
                        }
                    }

                    // Completion badge (top-right of thumbnail)
                    let times_completed = stats.map_or(0, |s| s.times_beaten);
                    if times_completed > 0 {
                        let badge_size = 26.0;
                        let badge_center = egui::pos2(
                            thumb_rect.right() - 8.0 - badge_size / 2.0,
                            thumb_rect.top() + 8.0 + badge_size / 2.0,
                        );
                        // Main circle
                        painter.circle_filled(
                            badge_center,
                            badge_size / 2.0,
                            Color32::from_black_alpha(178),
                        );
                        // Heavy check mark (U+2714) instead of plain check (U+2713).
                        // Noto Sans Symbols 2 renders plain U+2713 quite thin,
                        // and its visual centre sits low inside the circle.
                        // The heavy variant reads better at badge size.
                        painter.text(
                            badge_center,
                            egui::Align2::CENTER_CENTER,
                            "\u{2714}",
                            egui::FontId::proportional(16.0),
                            Color32::from_rgb(0x80, 0x80, 0x80),
                        );

                        // Count sub-badge for multiple completions
                        if times_completed > 1 {
                            let sub_size = 14.0;
                            let sub_center = egui::pos2(
                                badge_center.x + badge_size / 2.0 - sub_size / 3.0,
                                badge_center.y - badge_size / 2.0 + sub_size / 3.0,
                            );
                            painter.circle_filled(
                                sub_center,
                                sub_size / 2.0,
                                Color32::from_rgb(0x80, 0x80, 0x80),
                            );
                            painter.text(
                                sub_center,
                                egui::Align2::CENTER_CENTER,
                                times_completed.to_string(),
                                egui::FontId::proportional(9.0),
                                Color32::from_rgb(0x1a, 0x1a, 0x1a),
                            );
                        }
                    }

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

                    // Status badge (unified)
                    let status_label = theme::status_display(wad.status);
                    let status_color = theme::status_color(wad.status);
                    let status_bg = theme::status_bg(wad.status);

                    // Measure status text
                    let status_galley = painter.layout_no_wrap(
                        status_label.to_string(),
                        egui::FontId::proportional(11.0),
                        status_color,
                    );
                    let badge_width = status_galley.size().x + 16.0;
                    let badge_height = 18.0;
                    let badge_rect = Rect::from_min_size(
                        egui::pos2(body_x, footer_y),
                        Vec2::new(badge_width, badge_height),
                    );
                    painter.rect_filled(badge_rect, 6, status_bg);
                    painter.galley(
                        egui::pos2(body_x + 8.0, footer_y + 2.0),
                        status_galley,
                        Color32::TRANSPARENT,
                    );

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

                    // Progress bar for in-progress WADs with stats.
                    //
                    // Denominator comes from `wad_analysis.required_maps` when
                    // available. A levelstat-format snapshot only records maps
                    // the player has exited, so `wad_stats.maps.len()` would
                    // always equal `played_maps().len()` and the bar would read
                    // 100% for every in-progress WAD.
                    if wad.status == caco_core::db::Status::InProgress
                        && let Some(ref snapshot_json) = wad.stats_snapshot
                        && let Ok(wad_stats) = serde_json::from_str::<StatsData>(snapshot_json)
                        && !wad_stats.maps.is_empty()
                    {
                        let played = wad_stats.played_maps().len();
                        let total = state
                            .required_maps_map
                            .get(&wad_id)
                            .copied()
                            .filter(|n| *n > 0)
                            .unwrap_or(wad_stats.maps.len())
                            .max(played);
                        let pct = if total > 0 {
                            (played as f32 / total as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };

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
                            painter.rect_filled(fill_rect, 2.0, theme::TEXT_ACCENT);
                        }
                    }
                }
            });

            // Spacing between rows
            ui.add_space(CARD_SPACING);
        }
    });

    action
}
