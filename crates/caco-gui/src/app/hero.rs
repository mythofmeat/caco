//! "Now Playing" / "Continue Playing" hero banner rendered at the top of the library view.

use egui::Color32;

use crate::panels;
use crate::state::{ActionRequest, AppState, PlayState};
use crate::theme;
use crate::thumbnails::ThumbnailManager;

/// Render the hero banner. Returns an action request if the user interacted with it.
pub(super) fn render_now_playing_hero(
    ui: &mut egui::Ui,
    state: &AppState,
    thumbnails: &ThumbnailManager,
) -> Option<ActionRequest> {
    // Find the first WAD with "playing" status, or show active play state
    let (wad_title, wad_author, wad_id, is_active) =
        if let PlayState::Playing {
            wad_id, wad_title, ..
        } = &state.play_state
        {
            let author = state
                .wads
                .iter()
                .find(|w| w.id == *wad_id)
                .and_then(|w| w.author.clone());
            (wad_title.clone(), author, *wad_id, true)
        } else {
            // Show the first WAD with in-progress status
            let playing_wad = state
                .wads
                .iter()
                .find(|w| w.status == caco_core::db::Status::InProgress);
            match playing_wad {
                Some(w) => (w.title.clone(), w.author.clone(), w.id, false),
                None => return None, // No hero to show
            }
        };

    let mut action = None;

    let stats = state.stats_map.get(&wad_id);

    ui.add_space(16.0);

    // Hero frame
    let hero_frame = egui::Frame::new()
        .fill(Color32::from_rgb(0x22, 0x18, 0x0c))
        .corner_radius(16)
        .inner_margin(egui::Margin::symmetric(24, 20))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x3a, 0x2e, 0x1a)))
        .outer_margin(egui::Margin::symmetric(20, 0));

    hero_frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            // Thumbnail (real or placeholder) — double-click to play, right-click for menu
            let thumb_size = egui::vec2(120.0, 90.0);
            let (thumb_rect, thumb_resp) =
                ui.allocate_exact_size(thumb_size, egui::Sense::click());

            if let Some(tex) = thumbnails.get(wad_id) {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .rect_filled(thumb_rect, 10.0, Color32::BLACK);
                ui.painter()
                    .image(tex.id(), thumb_rect, uv, Color32::WHITE);
            } else {
                let (c1, _c2, ci) = theme::thumb_colors(wad_id);
                ui.painter().rect_filled(thumb_rect, 10.0, c1);
                let initials: String = wad_title
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .take(2)
                    .flat_map(|c| c.to_uppercase())
                    .collect();
                ui.painter().text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &initials,
                    egui::FontId::proportional(28.0),
                    ci,
                );
            }

            // Hover outline on thumbnail
            if thumb_resp.hovered() {
                ui.painter().rect_stroke(
                    thumb_rect,
                    10.0,
                    egui::Stroke::new(1.5, theme::TEXT_ACCENT),
                    egui::StrokeKind::Outside,
                );
            }

            // Double-click to play
            if thumb_resp.double_clicked() {
                action = Some(ActionRequest::Play(wad_id));
            }

            // Right-click context menu
            let wad_status = state
                .wads
                .iter()
                .find(|w| w.id == wad_id)
                .map(|w| w.status)
                .unwrap_or(caco_core::db::Status::Unplayed);
            if let Some(a) = panels::wad_context_menu(&thumb_resp, wad_id, wad_status) {
                action = Some(a);
            }

            ui.add_space(16.0);

            // Info area. Bound the width so degenerately long titles or
            // author lists (e.g. community megawads with 10+ collaborators)
            // truncate with an ellipsis instead of overflowing into the
            // playtime/progress block on the right.
            const RIGHT_BLOCK_RESERVE: f32 = 240.0;
            let info_width = (ui.available_width() - RIGHT_BLOCK_RESERVE).max(120.0);
            ui.allocate_ui_with_layout(
                egui::vec2(info_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    let label_color = if is_active {
                        theme::COLOR_SUCCESS
                    } else {
                        Color32::from_rgb(0x55, 0x88, 0xdd)
                    };
                    let label_text = if is_active {
                        "NOW PLAYING"
                    } else {
                        "CONTINUE PLAYING"
                    };
                    ui.colored_label(
                        label_color,
                        egui::RichText::new(label_text).size(11.0).strong(),
                    );
                    ui.add_space(2.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&wad_title)
                                .size(20.0)
                                .strong()
                                .color(theme::TEXT_PRIMARY),
                        )
                        .truncate(),
                    );
                    ui.add_space(2.0);
                    let meta = wad_author.as_deref().unwrap_or("");
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(meta)
                                .size(13.0)
                                .color(theme::TEXT_SECONDARY),
                        )
                        .truncate(),
                    );
                },
            );

            // Right side: playtime + progress
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
                        if let Some(s) = stats
                            && s.playtime > 0
                        {
                            ui.colored_label(
                                theme::TEXT_PRIMARY,
                                egui::RichText::new(
                                    caco_core::player::format_duration(s.playtime),
                                )
                                .size(22.0)
                                .strong(),
                            );
                            ui.colored_label(
                                theme::TEXT_MUTED,
                                egui::RichText::new("TOTAL PLAYTIME").size(10.0),
                            );
                        }

                        // Progress bar — sourced from `state.analyses_map`
                        // (the same Required set the auto-completion verdict
                        // uses). Hidden when no fresh analysis is cached so
                        // we never show a misleading number derived from the
                        // levelstat snapshot's played-only map list.
                        let wad = state.wads.iter().find(|w| w.id == wad_id);
                        if let Some(wad) = wad
                            && let Some(analysis) = state.analyses_map.get(&wad_id)
                            && analysis.required_maps > 0
                            && let Some(ref snapshot_json) = wad.stats_snapshot
                            && let Ok(wad_stats) =
                                serde_json::from_str::<caco_core::wad_stats::WadStats>(
                                    snapshot_json,
                                )
                        {
                            use caco_core::wad_analysis::MapClassification;
                            let required_set: std::collections::HashSet<&str> = analysis
                                .maps
                                .iter()
                                .filter(|m| m.classification == MapClassification::Required)
                                .map(|m| m.lump.as_str())
                                .collect();
                            let secret_set: std::collections::HashSet<&str> = analysis
                                .secret_maps
                                .iter()
                                .map(|s| s.as_str())
                                .collect();

                            let exited: std::collections::HashSet<&str> = wad_stats
                                .maps
                                .iter()
                                .filter(|m| m.total_exits >= 1)
                                .map(|m| m.lump.as_str())
                                .collect();

                            let played_required =
                                required_set.iter().filter(|l| exited.contains(*l)).count();
                            let required_total = analysis.required_maps;
                            let secret_total = secret_set.len();
                            let played_secret =
                                secret_set.iter().filter(|l| exited.contains(*l)).count();

                            // Bar tracks required maps only
                            let pct = played_required as f32 / required_total as f32;

                            ui.add_space(8.0);

                            let bar_width = 200.0_f32;
                            let bar_height = 6.0;
                            let (bar_rect, _) = ui.allocate_exact_size(
                                egui::vec2(bar_width, bar_height),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(
                                bar_rect,
                                3.0,
                                Color32::from_rgb(0x3a, 0x2e, 0x1a),
                            );
                            if pct > 0.0 {
                                let fill_rect = egui::Rect::from_min_size(
                                    bar_rect.min,
                                    egui::vec2(
                                        bar_rect.width() * pct.min(1.0),
                                        bar_height,
                                    ),
                                );
                                ui.painter().rect_filled(
                                    fill_rect,
                                    3.0,
                                    theme::COLOR_SUCCESS,
                                );
                            }

                            let pct_display = (pct * 100.0).min(100.0) as u32;
                            // Label with secret badge when applicable
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    theme::TEXT_MUTED,
                                    egui::RichText::new(format!(
                                        "{played_required} / {required_total} maps \u{00b7} {pct_display}%"
                                    ))
                                    .size(11.0),
                                );
                                if secret_total > 0 {
                                    let badge = egui::RichText::new(format!(
                                        "{played_secret}/{secret_total} secret"
                                    ))
                                    .size(9.0)
                                    .color(theme::TEXT_PRIMARY);
                                    let badge_resp = ui.add(
                                        egui::Label::new(badge)
                                            .selectable(false),
                                    );
                                    let badge_rect = badge_resp.rect.expand2(
                                        egui::vec2(4.0, 1.0),
                                    );
                                    ui.painter_at(badge_rect).rect_filled(
                                        badge_rect,
                                        3.0,
                                        theme::COLOR_SECRET_FILL,
                                    );
                                    // Re-draw text on top of the background
                                    ui.painter_at(badge_rect).text(
                                        badge_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        format!("{played_secret}/{secret_total} secret"),
                                        egui::FontId::proportional(9.0),
                                        theme::TEXT_PRIMARY,
                                    );
                                }
                            });
                        }
                    });
                });
            });
        });
    });

    action
}
