//! Top bar — breadcrumbs (left) + filter bar + sort controls (right).

use crate::panels;
use crate::state::{ActionRequest, AppState, ViewMode};
use crate::theme;

pub(super) fn render_topbar(
    ui: &mut egui::Ui,
    state: &mut AppState,
    actions: &mut Vec<ActionRequest>,
) {
    ui.horizontal(|ui| {
        // Breadcrumbs
        render_breadcrumbs(ui, state);

        // Filter and sort only make sense in Library, where they act on what is showing, and
        // are not drawn where they could do nothing. Cacowards has years to go by, and how
        // it lays them out.
        if state.view_mode == ViewMode::Library {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = if state.wide_window {
                    "Show or hide what is known about the selected WAD. Or press I."
                } else {
                    "What is known about the selected WAD. Or press I."
                };
                let open = state.wide_window && state.details_open;
                let fill = if open {
                    theme::CONTROL_FILL_HOVER
                } else {
                    egui::Color32::TRANSPARENT
                };
                let text = egui::RichText::new("Details").size(12.0).color(if open {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                });
                if ui
                    .add(egui::Button::new(text).selected(open).fill(fill))
                    .on_hover_text(hint)
                    .clicked()
                {
                    actions.push(ActionRequest::ToggleDetails);
                }
                ui.add_space(4.0);
                panels::layout_toggle(ui, &mut state.view_layout);
                ui.add_space(4.0);

                // Sort controls (right-aligned)
                panels::sort_controls::render(ui, state);

                // Search/filter
                panels::filter_bar::render(ui, state);
            });
        } else if state.view_mode == ViewMode::Cacowards {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                panels::layout_toggle(ui, &mut state.cacowards.layout);
            });
        }
    });
}

fn render_breadcrumbs(ui: &mut egui::Ui, state: &AppState) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        // Base crumb
        let base = match state.view_mode {
            ViewMode::Import => "Import",
            ViewMode::Cacowards => "Cacowards",
            ViewMode::Stats => "Stats",
            ViewMode::Library => "Library",
        };

        ui.colored_label(theme::TEXT_PRIMARY, egui::RichText::new(base).strong());

        // Year crumb on the Cacowards view, when one is selected.
        if state.view_mode == ViewMode::Cacowards
            && let Some(year) = state.cacowards.selected_year
        {
            ui.colored_label(theme::TEXT_MUTED, "  /  ");
            ui.colored_label(theme::TEXT_SECONDARY, year.to_string());
        }
    });
}
