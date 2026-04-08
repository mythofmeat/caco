use crate::relative_time;
use crate::state::{ActionRequest, AppState};
use crate::theme;
use crate::thumbnails::ThumbnailManager;
use rusqlite::Connection;

/// Type alias to disambiguate from caco_core::db::sessions::WadStats.
type StatsData = caco_core::wad_stats::WadStats;

/// Render the WAD detail sidebar panel. Returns an action request if a button was clicked.
pub fn render(
    ui: &mut egui::Ui,
    state: &AppState,
    conn: &Connection,
    thumbnails: Option<&ThumbnailManager>,
) -> Option<ActionRequest> {
    let Some(wad) = state.selected_wad() else {
        ui.centered_and_justified(|ui| {
            ui.colored_label(theme::TEXT_SECONDARY, "No WAD selected");
        });
        return None;
    };
    let stats = state.selected_stats();
    let wad_id = wad.id;

    // Pre-extract data needed inside closures to avoid borrow issues
    let source_url = wad.source_url.clone();
    let idgames_id = wad.idgames_id.clone();
    let stats_snapshot = wad.stats_snapshot.clone();
    let companions =
        caco_core::db::companions::get_companions_for_wad(conn, wad_id).unwrap_or_default();

    let mut action = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;

        // Thumbnail
        if let Some(tm) = thumbnails
            && let Some(tex) = tm.get(wad_id)
        {
            let tex_size = tex.size_vec2();
            let available = ui.available_width();
            let scale = (available / tex_size.x).min(1.0);
            let display_size = egui::vec2(tex_size.x * scale, tex_size.y * scale);
            ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
            ui.add_space(4.0);
        }

        // Title
        ui.colored_label(
            theme::TEXT_ACCENT,
            egui::RichText::new(&wad.title).heading().strong(),
        );

        // Author + Year
        let mut subtitle = String::new();
        if let Some(author) = &wad.author {
            subtitle.push_str(author);
        }
        if let Some(year) = wad.year {
            if !subtitle.is_empty() {
                subtitle.push_str(" \u{2022} ");
            }
            subtitle.push_str(&year.to_string());
        }
        if !subtitle.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, &subtitle);
        }

        // Status pill + rating
        ui.horizontal_wrapped(|ui| {
            theme::status_pill(ui, &wad.status);
            let stars = theme::rating_stars(wad.rating);
            if !stars.is_empty() {
                ui.colored_label(theme::TEXT_ACCENT, &stars);
            }
        });

        // ── Play Stats ──
        theme::section_label(ui, "Play Stats");

        if let Some(s) = stats {
            detail_row(ui, "Playtime", &caco_core::player::format_duration(s.playtime));
            detail_row(ui, "Sessions", &s.session_count.to_string());
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(80.0, ui.spacing().interact_size.y), |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.colored_label(theme::TEXT_SECONDARY, "Beaten");
                    });
                });
                ui.colored_label(theme::TEXT_PRIMARY, format!("{}\u{00d7}", s.times_beaten));
                let small = egui::vec2(20.0, 18.0);
                if ui.add_sized(small, egui::Button::new("+")).clicked() {
                    action = Some(ActionRequest::BeatenAdd(wad_id));
                }
                if ui
                    .add_enabled_ui(s.times_beaten > 0, |ui| {
                        ui.add_sized(small, egui::Button::new("\u{2212}"))
                    })
                    .inner
                    .clicked()
                {
                    action = Some(ActionRequest::BeatenRemove(wad_id));
                }
            });
            if let Some(lp) = &s.last_played {
                let display = relative_time::parse_timestamp(lp)
                    .map(|dt| relative_time::relative_time_full(&dt))
                    .unwrap_or_else(|| lp.get(..10).unwrap_or(lp).to_string());
                detail_row(ui, "Last Played", &display);
            }
        } else {
            ui.colored_label(theme::TEXT_SECONDARY, "No play history");
        }

        // Map progress from stats_snapshot
        if let Some(ref snapshot_json) = stats_snapshot
            && let Ok(wad_stats) = serde_json::from_str::<StatsData>(snapshot_json)
            && !wad_stats.maps.is_empty()
        {
            let analysis = caco_core::db::analysis::get_analysis(conn, wad_id)
                .ok()
                .flatten();

            let secret_set: std::collections::HashSet<&str> = analysis
                .as_ref()
                .map(|a| a.secret_maps.iter().map(|s| s.as_str()).collect())
                .unwrap_or_default();

            let total = wad_stats.maps.len();
            let played_required = wad_stats
                .played_maps()
                .iter()
                .filter(|m| !secret_set.contains(m.lump.as_str()))
                .count();
            let required_total = analysis
                .as_ref()
                .map(|a| a.required_maps)
                .unwrap_or(total);
            let secret_total = secret_set.len();
            let played_secret = wad_stats
                .played_maps()
                .iter()
                .filter(|m| secret_set.contains(m.lump.as_str()))
                .count();

            // Bar tracks required maps only
            let pct = if required_total > 0 {
                played_required as f32 / required_total as f32
            } else {
                0.0
            };
            let pct_display = (pct * 100.0).min(100.0) as u32;

            ui.add_space(2.0);
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!("Progress: {played_required}/{required_total} maps ({pct_display}%)"),
            );
            // Custom progress bar with outlined track
            let bar_height = 14.0;
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), bar_height),
                egui::Sense::hover(),
            );
            let rounding = 4.0;
            let painter = ui.painter();
            painter.rect_filled(rect, rounding, theme::BG_MEDIUM);
            if pct > 0.0 {
                let fill_rect = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width() * pct.min(1.0), rect.height()),
                );
                painter.rect_filled(fill_rect, rounding, theme::TEXT_ACCENT);
            }
            painter.rect_stroke(
                rect,
                rounding,
                egui::Stroke::new(1.0, theme::BORDER),
                egui::StrokeKind::Outside,
            );
            // Secret badge
            if secret_total > 0 {
                ui.horizontal(|ui| {
                    let badge_text = format!("{played_secret}/{secret_total} secret");
                    let badge = egui::RichText::new(&badge_text)
                        .size(10.0)
                        .color(theme::TEXT_PRIMARY);
                    let badge_resp =
                        ui.add(egui::Label::new(badge).selectable(false));
                    let badge_rect = badge_resp.rect.expand2(egui::vec2(4.0, 1.0));
                    ui.painter_at(badge_rect)
                        .rect_filled(badge_rect, 3.0, theme::COLOR_SECRET_FILL);
                    ui.painter_at(badge_rect).text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &badge_text,
                        egui::FontId::proportional(10.0),
                        theme::TEXT_PRIMARY,
                    );
                });
            }
        }

        // ── Actions ──
        ui.add_space(4.0);
        let play_enabled = !state.is_playing();
        ui.columns(3, |cols| {
            if cols[0]
                .add_enabled(play_enabled, egui::Button::new("Play"))
                .clicked()
            {
                action = Some(ActionRequest::Play(wad_id));
            }
            if cols[1].button("Edit").clicked() {
                action = Some(ActionRequest::Edit(wad_id));
            }
            if cols[2].button("Sessions").clicked() {
                action = Some(ActionRequest::Sessions(wad_id));
            }
        });
        ui.columns(2, |cols| {
            if cols[0].button("Map Stats").clicked() {
                action = Some(ActionRequest::MapStats(wad_id));
            }
            if cols[1]
                .add(egui::Button::new(
                    egui::RichText::new("Delete").color(theme::COLOR_ERROR),
                ))
                .clicked()
            {
                action = Some(ActionRequest::Delete(wad_id));
            }
        });

        // ── Tags ──
        if !wad.tags.is_empty() {
            theme::section_label(ui, "Tags");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for tag in &wad.tags {
                    theme::tag_pill(ui, tag);
                }
            });
        }

        // ── Description ──
        if let Some(desc) = &wad.description {
            theme::section_label(ui, "Description");
            ui.add(
                egui::Label::new(
                    egui::RichText::new(desc.as_str()).color(theme::TEXT_SECONDARY),
                )
                .wrap(),
            );
        }

        // ── Source ──
        theme::section_label(ui, "Source");
        detail_row(ui, "Source", &wad.source_type);
        if let Some(ref url) = source_url {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.hyperlink_to(url, url);
            });
        }
        if let Some(ref ig_id) = idgames_id {
            let idgames_url = format!("https://www.doomworld.com/idgames/?id={ig_id}");
            ui.horizontal(|ui| {
                ui.colored_label(theme::TEXT_SECONDARY, "idgames:");
                ui.hyperlink_to(ig_id, &idgames_url);
            });
        }
        if let Some(filename) = &wad.filename {
            detail_row(ui, "File", filename);
        }
        if let Some(iwad) = &wad.custom_iwad {
            detail_row(ui, "IWAD", iwad);
        }
        if let Some(cl) = wad.complevel {
            detail_row(ui, "Complevel", caco_core::complevel::complevel_name(Some(cl)));
        }

        // ── Companion Files ──
        if !companions.is_empty() {
            theme::section_label(ui, "Companions");
            for comp in &companions {
                let label = if comp.enabled {
                    comp.filename.clone()
                } else {
                    format!("{} (off)", comp.filename)
                };
                ui.colored_label(theme::TEXT_PRIMARY, &label);
            }
        }
    });

    action
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        // Fixed-width right-aligned label for clean alignment
        ui.allocate_ui(egui::vec2(80.0, ui.spacing().interact_size.y), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(theme::TEXT_SECONDARY, label);
            });
        });
        ui.colored_label(theme::TEXT_PRIMARY, value);
    });
}
