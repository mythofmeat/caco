//! Top bar — breadcrumbs (left) + filter bar + sort controls (right).

use crate::panels;
use crate::state::{ActionRequest, ActiveDialog, AppState, ViewMode};
use crate::theme;

pub(super) fn render_topbar(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _actions: &mut Vec<ActionRequest>,
) {
    ui.horizontal(|ui| {
        // Breadcrumbs
        render_breadcrumbs(ui, state);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Sort controls (right-aligned)
            panels::sort_controls::render(ui, state);

            // Search/filter
            panels::filter_bar::render(ui, state);
        });
    });
}

fn render_breadcrumbs(ui: &mut egui::Ui, state: &AppState) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        // Base crumb
        let base = if state.view_mode == ViewMode::Import {
            "Import"
        } else {
            "Library"
        };

        // If a dialog is open, the base is clickable (concept: navigate back)
        let has_detail = state.active_dialog.is_some();
        if has_detail {
            ui.colored_label(theme::TEXT_SECONDARY, base);
        } else {
            ui.colored_label(theme::TEXT_PRIMARY, egui::RichText::new(base).strong());
        }

        // WAD name crumb (when edit/sessions/etc dialog is open)
        if let Some(ref dialog) = state.active_dialog {
            let wad_title = match dialog {
                ActiveDialog::Edit(e) => Some(e.title()),
                _ => None,
            };
            if let Some(title) = wad_title {
                ui.colored_label(theme::TEXT_MUTED, "  /  ");
                ui.colored_label(theme::TEXT_SECONDARY, title);
                ui.colored_label(theme::TEXT_MUTED, "  /  ");
                ui.colored_label(theme::TEXT_ACCENT, egui::RichText::new("Edit").strong());
            }
        }
    });
}
