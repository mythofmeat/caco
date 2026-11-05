//! The row above the WAD grid or table: the status filters, and how many are showing.

use crate::state::{ActionRequest, AppState};
use crate::theme;

pub(super) fn render_section_header(
    ui: &mut egui::Ui,
    state: &mut AppState,
    actions: &mut Vec<ActionRequest>,
) {
    let margin = egui::Margin::symmetric(20, 0);
    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
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

            // A search worth keeping is one click from being a collection, where the sidebar
            // has it. Not for a collection being shown, which already is one.
            if !state.filter.applied.is_empty() && state.active_collection.is_none() {
                ui.add_space(6.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Save as Collection")
                                .size(11.5)
                                .color(theme::TEXT_SECONDARY),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Keep this search in the sidebar")
                    .clicked()
                {
                    actions.push(ActionRequest::SaveFilterAsCollection);
                }
            }
        });
    });
}
