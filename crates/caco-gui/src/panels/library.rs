use crate::state::{ActionRequest, AppState, ViewLayout, ViewMode, TABS};
use crate::theme;

/// Render the tab bar across the top.
pub fn render_tab_bar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // Library tabs
        for (i, tab) in TABS.iter().enumerate() {
            let is_active = state.view_mode == ViewMode::Library && state.active_tab == i;
            let text = egui::RichText::new(tab.label);
            let text = if is_active {
                text.strong().color(theme::TEXT_ACCENT)
            } else {
                text.color(theme::TEXT_SECONDARY)
            };

            if ui.selectable_label(is_active, text).clicked() {
                state.view_mode = ViewMode::Library;
                if state.active_tab != i {
                    state.active_tab = i;
                    state.needs_reload = true;
                }
            }
        }

        ui.separator();

        // Import tab
        let import_active = state.view_mode == ViewMode::Import;
        let import_text = egui::RichText::new("Import");
        let import_text = if import_active {
            import_text.strong().color(theme::TEXT_ACCENT)
        } else {
            import_text.color(theme::TEXT_SECONDARY)
        };
        if ui.selectable_label(import_active, import_text).clicked() {
            state.view_mode = ViewMode::Import;
        }

        // Right-aligned WAD count
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.view_mode == ViewMode::Library {
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    format!("{} WADs", state.wads.len()),
                );
            }
        });
    });
}

/// Render the toolbar (filter bar + sort controls + Stats/Cache/Detail buttons).
/// Returns an action request if a toolbar button was clicked.
pub fn render_toolbar(ui: &mut egui::Ui, state: &mut AppState) -> Option<ActionRequest> {
    let mut action = None;

    ui.horizontal(|ui| {
        super::filter_bar::render(ui, state);
        ui.separator();
        super::sort_controls::render(ui, state);

        // Right-aligned buttons
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if state.show_detail_panel { "Hide Detail" } else { "Show Detail" };
            if ui.button(label).clicked() {
                state.show_detail_panel = !state.show_detail_panel;
            }

            // List/Grid toggle
            let (icon, tooltip) = match state.view_layout {
                ViewLayout::List => ("\u{25a6}", "Switch to grid view"),
                ViewLayout::Grid => ("\u{2630}", "Switch to list view"),
            };
            if ui.button(icon).on_hover_text(tooltip).clicked() {
                state.view_layout = match state.view_layout {
                    ViewLayout::List => ViewLayout::Grid,
                    ViewLayout::Grid => ViewLayout::List,
                };
            }

            if ui.button("Cache").clicked() {
                action = Some(ActionRequest::Cache);
            }
            if ui.button("Resources").clicked() {
                action = Some(ActionRequest::Resources);
            }
            if ui.button("Stats").clicked() {
                action = Some(ActionRequest::Stats);
            }
        });
    });

    action
}
