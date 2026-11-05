//! Top bar — breadcrumbs (left) + filter bar + sort controls (right).

use crate::panels;
use crate::state::{AppState, ViewMode};
use crate::theme;

pub(super) fn render_topbar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // Breadcrumbs
        render_breadcrumbs(ui, state);

        // Filter / sort widgets only make sense in Library; Cacowards has
        // its own year strip + category sections and Import has its own
        // search box, so suppress them here to avoid the dead-control
        // anti-affordance the user sees on those views.
        if state.view_mode == ViewMode::Library {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Sort controls (right-aligned)
                panels::sort_controls::render(ui, state);

                // Search/filter
                panels::filter_bar::render(ui, state);
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
            ui.colored_label(
                theme::TEXT_ACCENT,
                egui::RichText::new(year.to_string()).strong(),
            );
        }
    });
}
