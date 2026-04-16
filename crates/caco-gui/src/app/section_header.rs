//! Section header above the WAD grid/table — title + list/grid toggle + status filter pills.

use crate::state::{AppState, ViewLayout};
use crate::theme;

pub(super) fn render_section_header(ui: &mut egui::Ui, state: &mut AppState) {
    let margin = egui::Margin::symmetric(20, 0);
    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new(format!("ALL WADS \u{00b7} {}", state.wads.len()))
                    .size(13.0)
                    .strong(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // View toggle buttons
                let list_selected = state.view_layout == ViewLayout::List;
                let grid_selected = state.view_layout == ViewLayout::Grid;

                // List button
                let list_text = if list_selected {
                    egui::RichText::new("List")
                        .size(12.0)
                        .color(theme::TEXT_PRIMARY)
                } else {
                    egui::RichText::new("List")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY)
                };
                let list_btn = ui.add(
                    egui::Button::new(list_text)
                        .fill(if list_selected {
                            theme::BORDER_MED
                        } else {
                            theme::BG_LIGHT
                        })
                        .corner_radius(egui::CornerRadius {
                            nw: 0,
                            ne: 6,
                            se: 6,
                            sw: 0,
                        }),
                );
                if list_btn.clicked() {
                    state.view_layout = ViewLayout::List;
                }

                // Grid button
                let grid_text = if grid_selected {
                    egui::RichText::new("Grid")
                        .size(12.0)
                        .color(theme::TEXT_PRIMARY)
                } else {
                    egui::RichText::new("Grid")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY)
                };
                let grid_btn = ui.add(
                    egui::Button::new(grid_text)
                        .fill(if grid_selected {
                            theme::BORDER_MED
                        } else {
                            theme::BG_LIGHT
                        })
                        .corner_radius(egui::CornerRadius {
                            nw: 6,
                            ne: 0,
                            se: 0,
                            sw: 6,
                        }),
                );
                if grid_btn.clicked() {
                    state.view_layout = ViewLayout::Grid;
                }
            });
        });

        // Status filter pills
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // "All" pill
            let all_active = state.status_filters.is_empty();
            if theme::filter_pill(ui, "All", all_active, None, state.total_wad_count) {
                state.status_filters.clear();
                state.needs_reload = true;
            }

            for &status in caco_core::db::Status::ALL {
                let status_str = status.as_str();
                let is_active = state.status_filters.contains(status_str);
                let count = state.status_count(Some(status_str));
                let color = theme::status_color(status);
                if theme::filter_pill(
                    ui,
                    theme::status_display(status),
                    is_active,
                    Some(color),
                    count,
                ) {
                    if is_active {
                        state.status_filters.remove(status_str);
                    } else {
                        state.status_filters.insert(status_str.to_string());
                    }
                    state.needs_reload = true;
                }
            }
        });
    });
}
