use crate::relative_time;
use crate::state::{ActionRequest, AppState};
use crate::theme;
use rusqlite::Connection;

/// Type alias to disambiguate from caco_core::db::sessions::WadStats.
type StatsData = caco_core::wad_stats::WadStats;

/// Render the WAD detail sidebar panel. Returns an action request if a button was clicked.
pub fn render(ui: &mut egui::Ui, state: &AppState, conn: &Connection) -> Option<ActionRequest> {
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
        ui.spacing_mut().item_spacing.y = 4.0;

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

        // Status
        let status_label = theme::status_display(&wad.status);
        let status_color = theme::status_color(&wad.status);
        ui.colored_label(status_color, status_label);

        // Rating
        let stars = theme::rating_stars(wad.rating);
        if !stars.is_empty() {
            ui.colored_label(theme::TEXT_ACCENT, &stars);
        }

        ui.separator();

        // Stats section
        if let Some(s) = stats {
            detail_row(ui, "Playtime", &caco_core::player::format_duration(s.playtime));
            detail_row(ui, "Sessions", &s.session_count.to_string());
            if s.times_beaten > 0 {
                detail_row(ui, "Beaten", &format!("{}\u{00d7}", s.times_beaten));
            }
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
            let played = wad_stats.played_maps().len();
            let total = wad_stats.maps.len();
            let pct = if total > 0 {
                (played as f32) / (total as f32)
            } else {
                0.0
            };
            let pct_display = (pct * 100.0) as u32;

            ui.add_space(2.0);
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!("Progress: {played}/{total} maps ({pct_display}%)"),
            );
            ui.add(
                egui::ProgressBar::new(pct)
                    .desired_width(ui.available_width()),
            );
        }

        ui.separator();

        // Action buttons
        ui.horizontal(|ui| {
            let play_enabled = !state.is_playing();
            if ui.add_enabled(play_enabled, egui::Button::new("Play")).clicked() {
                action = Some(ActionRequest::Play(wad_id));
            }
            if ui.button("Edit").clicked() {
                action = Some(ActionRequest::Edit(wad_id));
            }
            if ui.button("Delete").clicked() {
                action = Some(ActionRequest::Delete(wad_id));
            }
            if ui.button("Sessions").clicked() {
                action = Some(ActionRequest::Sessions(wad_id));
            }
            if ui.button("Map Stats").clicked() {
                action = Some(ActionRequest::MapStats(wad_id));
            }
        });

        ui.separator();

        // Tags
        if !wad.tags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(theme::TEXT_SECONDARY, "Tags:");
                for tag in &wad.tags {
                    ui.colored_label(theme::TEXT_ACCENT, tag);
                }
            });
        }

        // Description
        if let Some(desc) = &wad.description {
            ui.separator();
            ui.add(
                egui::Label::new(
                    egui::RichText::new(desc.as_str()).color(theme::TEXT_SECONDARY),
                )
                .wrap(),
            );
        }

        // Source info
        ui.separator();
        detail_row(ui, "Source", &wad.source_type);
        if let Some(ref url) = source_url {
            ui.horizontal(|ui| {
                ui.colored_label(theme::TEXT_SECONDARY, "URL:");
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

        // Companion files section
        if !companions.is_empty() {
            ui.separator();
            ui.colored_label(theme::TEXT_SECONDARY, "Companion Files:");
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
        ui.colored_label(theme::TEXT_SECONDARY, format!("{label}:"));
        ui.colored_label(theme::TEXT_PRIMARY, value);
    });
}
