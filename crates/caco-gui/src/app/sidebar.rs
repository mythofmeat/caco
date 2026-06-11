//! Left navigation sidebar — logo, view toggle, collections, admin links.

use crate::state::{ActionRequest, AppState, ViewMode};
use crate::theme;

pub(super) fn render_sidebar(
    ui: &mut egui::Ui,
    state: &mut AppState,
    actions: &mut Vec<ActionRequest>,
) {
    ui.add_space(16.0);

    // Logo
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.colored_label(
            theme::TEXT_ACCENT,
            egui::RichText::new("caco").size(22.0).strong(),
        );
    });

    ui.add_space(24.0);

    // Navigation items. Library/Import/Collections are mutually exclusive
    // sidebar selections: clicking Library or Import clears any active
    // collection (and its filter) so the highlight matches the actual
    // scope being viewed. Library is highlighted only when no collection
    // is active — when a collection is selected, the collection row owns
    // the highlight instead.
    let library_active = state.view_mode == ViewMode::Library && state.active_collection.is_none();
    if theme::sidebar_nav_item(ui, "Library", library_active) {
        let cleared = state.clear_active_collection();
        state.view_mode = ViewMode::Library;
        if cleared || state.wads.is_empty() {
            state.needs_reload = true;
        }
    }
    if theme::sidebar_nav_item(ui, "Import", state.view_mode == ViewMode::Import) {
        if state.clear_active_collection() {
            state.needs_reload = true;
        }
        state.view_mode = ViewMode::Import;
    }
    if theme::sidebar_nav_item(ui, "Cacowards", state.view_mode == ViewMode::Cacowards) {
        if state.clear_active_collection() {
            state.needs_reload = true;
        }
        state.view_mode = ViewMode::Cacowards;
        // Re-pull on each entry; cheap relative to user-perceived latency
        // and keeps the year strip honest after an enrich or import.
        state.cacowards.needs_reload = true;
    }

    // Divider
    ui.add_space(12.0);
    let rect = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x + 20.0, rect.min.y),
            egui::pos2(rect.max.x - 20.0, rect.min.y),
        ],
        egui::Stroke::new(1.0, theme::BORDER),
    );
    ui.add_space(16.0);

    // Collections section
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.colored_label(
            theme::TEXT_MUTED,
            egui::RichText::new("COLLECTIONS").size(11.0).strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(16.0);
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("+").size(13.0).color(theme::TEXT_MUTED))
                        .frame(false),
                )
                .on_hover_text("Manage collections")
                .clicked()
            {
                actions.push(ActionRequest::Collections);
            }
        });
    });
    ui.add_space(4.0);

    if state.sidebar_collections.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new("No collections yet").size(12.0),
            );
        });
    } else {
        // Clone names to avoid borrow conflict
        let collection_names: Vec<String> = state
            .sidebar_collections
            .iter()
            .map(|c| c.name.clone())
            .collect();

        for name in &collection_names {
            // Only highlight while in Library view — collections are
            // mutually exclusive with the Import nav item.
            let is_active = state.view_mode == ViewMode::Library
                && state.active_collection.as_deref() == Some(name.as_str());
            let resp = theme::sidebar_collection_item(ui, name, is_active);

            if resp.clicked() {
                // Find the collection and load its query + sort
                if let Some(coll) = state.sidebar_collections.iter().find(|c| c.name == *name) {
                    state.active_collection = Some(name.clone());
                    state.filter.set_both(coll.query.clone());
                    // Apply collection sort settings
                    if let Some(ref sort_by) = coll.sort_by {
                        if let Some(idx) = crate::state::SORT_FIELDS
                            .iter()
                            .position(|(key, _)| *key == sort_by.as_str())
                        {
                            state.sort_field_index = idx;
                        }
                        state.sort_desc = coll.sort_desc;
                    }
                    state.view_mode = ViewMode::Library;
                    state.needs_reload = true;
                }
            }

            // Right-click context menu
            let ctx_name = name.clone();
            resp.context_menu(|ui| {
                if ui.button("Edit").clicked() {
                    actions.push(ActionRequest::EditCollection(ctx_name.clone()));
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    actions.push(ActionRequest::DeleteCollection(ctx_name.clone()));
                    ui.close_menu();
                }
            });
        }
    }

    // (Status filter pills are now rendered in the section header)

    // Bottom spacer + admin links
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(11.0),
            );
        });
        ui.add_space(4.0);

        // Divider above bottom links
        let rect = ui.available_rect_before_wrap();
        ui.painter().line_segment(
            [
                egui::pos2(rect.min.x + 20.0, rect.max.y),
                egui::pos2(rect.max.x - 20.0, rect.max.y),
            ],
            egui::Stroke::new(1.0, theme::BORDER),
        );
        ui.add_space(12.0);

        // Small action links
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Stats")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .frame(false),
                )
                .clicked()
            {
                actions.push(ActionRequest::Stats);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Cache")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .frame(false),
                )
                .clicked()
            {
                actions.push(ActionRequest::Cache);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("IWADs")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .frame(false),
                )
                .clicked()
            {
                actions.push(ActionRequest::Resources);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Settings")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .frame(false),
                )
                .clicked()
            {
                actions.push(ActionRequest::Settings);
            }
        });
    });
}
