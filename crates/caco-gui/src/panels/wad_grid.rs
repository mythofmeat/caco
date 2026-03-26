use egui::{Color32, CornerRadius, Rect, StrokeKind, Vec2};

use crate::state::{ActionRequest, AppState};
use crate::theme;

/// Card dimensions.
const CARD_WIDTH: f32 = 200.0;
const CARD_SPACING: f32 = 8.0;
const CARD_PADDING: f32 = 8.0;
const CARD_ROUNDING: u8 = 6;
const THUMB_ASPECT: f32 = 0.6; // height = width * aspect

/// Placeholder colors cycled by WAD id.
const PLACEHOLDER_COLORS: &[Color32] = &[
    Color32::from_rgb(0x8b, 0x00, 0x00), // dark red
    Color32::from_rgb(0x00, 0x4b, 0x23), // dark green
    Color32::from_rgb(0x1a, 0x1a, 0x66), // dark blue
    Color32::from_rgb(0x66, 0x33, 0x00), // brown
    Color32::from_rgb(0x4a, 0x0e, 0x4e), // purple
    Color32::from_rgb(0x55, 0x44, 0x00), // dark yellow
];

/// Render the WAD library as a grid of cards.
/// Returns an action request if a keyboard shortcut was pressed.
pub fn render(
    ui: &mut egui::Ui,
    state: &mut AppState,
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
) -> Option<ActionRequest> {
    let available_width = ui.available_width();
    let columns = ((available_width + CARD_SPACING) / (CARD_WIDTH + CARD_SPACING))
        .floor()
        .max(1.0) as usize;

    // Handle keyboard shortcuts
    let mut action = None;
    if !state.has_dialog() && !ui.ctx().wants_keyboard_input() {
        if ui.input(|i| i.key_pressed(egui::Key::J)) {
            state.select_down_grid(columns);
        }
        if ui.input(|i| i.key_pressed(egui::Key::K)) {
            state.select_up_grid(columns);
        }
        if ui.input(|i| i.key_pressed(egui::Key::H)) {
            state.select_left(columns);
        }
        if ui.input(|i| i.key_pressed(egui::Key::L)) {
            state.select_right(columns);
        }

        action = super::handle_action_keys(ui, state.selected_wad_id);
    }

    let thumb_width = CARD_WIDTH - CARD_PADDING * 2.0;
    let thumb_height = thumb_width * THUMB_ASPECT;
    let text_height = ui.text_style_height(&egui::TextStyle::Body);
    let card_height = CARD_PADDING + thumb_height + 4.0 + text_height * 3.0 + CARD_PADDING;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
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
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(CARD_WIDTH, card_height),
                            egui::Sense::click(),
                        );

                        // Handle clicks
                        if response.clicked() {
                            state.selected_row = idx;
                            state.selected_wad_id = Some(wad_id);
                        }
                        if response.double_clicked() {
                            action = Some(ActionRequest::Play(wad_id));
                        }

                        let painter = ui.painter_at(rect);
                        let rounding = CornerRadius::same(CARD_ROUNDING);

                        // Card background
                        let bg = if is_selected {
                            theme::BG_SELECTED
                        } else if response.hovered() {
                            theme::BG_LIGHT
                        } else {
                            theme::BG_MEDIUM
                        };
                        painter.rect_filled(rect, rounding, bg);

                        // Border for selected card
                        if is_selected {
                            painter.rect_stroke(
                                rect,
                                rounding,
                                egui::Stroke::new(1.5, theme::TEXT_ACCENT),
                                StrokeKind::Outside,
                            );
                        }

                        // Thumbnail area
                        let thumb_rect = Rect::from_min_size(
                            rect.min + Vec2::new(CARD_PADDING, CARD_PADDING),
                            Vec2::new(thumb_width, thumb_height),
                        );

                        // Try to show a real thumbnail, fall back to placeholder
                        let mut drew_thumbnail = false;
                        if let Some(tm) = thumbnails
                            && let Some(tex) = tm.get(wad_id)
                        {
                            let uv = Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            );
                            painter.image(tex.id(), thumb_rect, uv, Color32::WHITE);
                            drew_thumbnail = true;
                        }

                        if !drew_thumbnail {
                            // Placeholder with color + initials
                            let color_idx = (wad_id as usize) % PLACEHOLDER_COLORS.len();
                            let placeholder_color = PLACEHOLDER_COLORS[color_idx];
                            painter.rect_filled(
                                thumb_rect,
                                CornerRadius::same(4),
                                placeholder_color,
                            );

                            // Initials (first two uppercase letters of title)
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
                                    egui::FontId::proportional(24.0),
                                    Color32::from_rgba_premultiplied(255, 255, 255, 180),
                                );
                            }
                        }

                        // Title (bold, elided)
                        let title_pos = thumb_rect.left_bottom() + Vec2::new(0.0, 4.0);
                        let title = caco_core::utils::truncate(&wad.title, 24, "..");
                        painter.text(
                            title_pos,
                            egui::Align2::LEFT_TOP,
                            &title,
                            egui::FontId::proportional(13.0),
                            theme::TEXT_PRIMARY,
                        );

                        // Author
                        let author_pos = title_pos + Vec2::new(0.0, text_height);
                        let author = wad.author.as_deref().unwrap_or("");
                        let author = caco_core::utils::truncate(author, 26, "..");
                        painter.text(
                            author_pos,
                            egui::Align2::LEFT_TOP,
                            &author,
                            egui::FontId::proportional(12.0),
                            theme::TEXT_SECONDARY,
                        );

                        // Status badge + playtime
                        let badge_y = author_pos.y + text_height;
                        let status_label = theme::status_display(&wad.status);
                        let status_color = theme::status_color(&wad.status);
                        painter.text(
                            egui::pos2(title_pos.x, badge_y),
                            egui::Align2::LEFT_TOP,
                            status_label,
                            egui::FontId::proportional(11.0),
                            status_color,
                        );

                        // Playtime right-aligned
                        if let Some(s) = stats
                            && s.playtime > 0
                        {
                            let playtime_str =
                                caco_core::player::format_duration(s.playtime);
                            painter.text(
                                egui::pos2(rect.right() - CARD_PADDING, badge_y),
                                egui::Align2::RIGHT_TOP,
                                &playtime_str,
                                egui::FontId::proportional(11.0),
                                theme::TEXT_SECONDARY,
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
