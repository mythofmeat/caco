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

        // Filter and sort only make sense in a library with something in it, where they act on
        // what is showing, and are not drawn where they could do nothing. Cacowards has years to go by, and how
        // it lays them out.
        if state.view_mode == ViewMode::Library && state.total_wad_count > 0 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = if state.wide_window {
                    "Show or hide what is known about the selected WAD. Or press I."
                } else {
                    "What is known about the selected WAD. Or press I."
                };
                let open = state.wide_window && state.details_open;
                if toggle_button(ui, "Details", open)
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
                ui.add_space(4.0);
                let hiding = state.cacowards.hide_finished;
                if toggle_button(ui, "Hide Finished", hiding)
                    .on_hover_text("Show only what is left to play")
                    .clicked()
                {
                    state.cacowards.hide_finished = !hiding;
                }
            });
        }
    });
}

/// Something in the bar that stays on, filled in while it is.
fn toggle_button(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    let fill = if on {
        theme::CONTROL_FILL_HOVER
    } else {
        egui::Color32::TRANSPARENT
    };
    let text = egui::RichText::new(text).size(12.0).color(if on {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
    });
    ui.add(egui::Button::new(text).selected(on).fill(fill))
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
