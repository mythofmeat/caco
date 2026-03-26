use crate::state::{ActionRequest, AppState};
use crate::theme;

/// Render the WAD detail sidebar panel. Returns an action request if a button was clicked.
pub fn render(ui: &mut egui::Ui, state: &AppState) -> Option<ActionRequest> {
    let Some(wad) = state.selected_wad() else {
        ui.centered_and_justified(|ui| {
            ui.colored_label(theme::TEXT_SECONDARY, "No WAD selected");
        });
        return None;
    };
    let stats = state.selected_stats();
    let wad_id = wad.id;

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
                let date = lp.get(..10).unwrap_or(lp);
                detail_row(ui, "Last Played", date);
            }
        } else {
            ui.colored_label(theme::TEXT_SECONDARY, "No play history");
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
            // Truncate long descriptions
            let display = if desc.len() > 500 {
                let boundary = desc.floor_char_boundary(500);
                format!("{}...", &desc[..boundary])
            } else {
                desc.clone()
            };
            ui.colored_label(theme::TEXT_SECONDARY, display);
        }

        // Source info
        ui.separator();
        detail_row(ui, "Source", &wad.source_type);
        if let Some(url) = &wad.source_url {
            detail_row(ui, "URL", url);
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
    });

    action
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT_SECONDARY, format!("{label}:"));
        ui.colored_label(theme::TEXT_PRIMARY, value);
    });
}
