//! Left navigation sidebar — logo, view toggle, collections, admin links.

use crate::dialogs::storage::StorageTab;
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
    if theme::sidebar_nav_item(ui, "Stats", state.view_mode == ViewMode::Stats) {
        if state.clear_active_collection() {
            state.needs_reload = true;
        }
        state.view_mode = ViewMode::Stats;
        // Same reasoning as Cacowards: one aggregate query, and a figure that
        // silently predates the session you just finished is worse than a
        // brief wait.
        state.stats.needs_reload = true;
    }

    // Divider
    ui.add_space(12.0);
    let rect = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x + 20.0, rect.min.y),
            egui::pos2(rect.max.x - 20.0, rect.min.y),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
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

            if resp.clicked() && state.activate_collection(name) {
                state.view_mode = ViewMode::Library;
                state.needs_reload = true;
            }

            // Right-click context menu
            let ctx_name = name.clone();
            resp.context_menu(|ui| {
                if ui.button("Edit").clicked() {
                    actions.push(ActionRequest::EditCollection(ctx_name.clone()));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    actions.push(ActionRequest::DeleteCollection(ctx_name.clone()));
                    ui.close();
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
                egui::RichText::new(format!("v{}", crate::VERSION)).size(11.0),
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
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
        ui.add_space(12.0);

        // Keep the maintenance controls out of the navigation hierarchy until
        // they are wanted. The open state belongs to this bit of UI, so egui's
        // persisted memory is enough; it does not need to become application
        // state or configuration.
        //
        // Emitted bottom-up, so the rows are written back to front: the first
        // one placed sits lowest. Wrapping them in a `ui.vertical` would read
        // in order, but a child region inside a bottom-up parent is handed the
        // whole remaining rect and then lays out top-down inside it — which is
        // exactly what parked this block under Collections instead of at the
        // panel's bottom edge.
        let open_id = ui.make_persistent_id("sidebar_manage_open");
        let open = ui.data_mut(|data| data.get_persisted::<bool>(open_id).unwrap_or(false));

        if open {
            for tool in TOOLS.iter().rev() {
                let resp = theme::sidebar_tool_item(ui, tool.label);
                let resp = if tool.hint.is_empty() {
                    resp
                } else {
                    resp.on_hover_text(tool.hint)
                };
                if resp.clicked() {
                    actions.push((tool.action)());
                }
            }
        }

        let label = if open { "Manage  ▾" } else { "Manage  ▸" };
        if theme::sidebar_tool_item(ui, label).clicked() {
            ui.data_mut(|data| data.insert_persisted(open_id, !open));
        }
        ui.add_space(4.0);
    });
}

/// One entry in the sidebar's management list.
struct Tool {
    label: &'static str,
    /// A constructor rather than a value: `ActionRequest` is not `Clone`, and
    /// these are only ever built when the row is clicked.
    action: fn() -> ActionRequest,
    /// Empty means no tooltip.
    hint: &'static str,
}

/// The management dialogs, in the order they appear in the sidebar: disk
/// first, then library maintenance, then setup.
const TOOLS: &[Tool] = &[
    Tool {
        label: "Storage",
        action: || ActionRequest::Storage(StorageTab::Cache),
        hint: "Cached files, cleanup, trash and companion files",
    },
    Tool {
        label: "Enrich",
        action: || ActionRequest::Enrich,
        hint: "Re-run metadata detection / refresh Cacowards",
    },
    Tool {
        label: "IWADs",
        action: || ActionRequest::Resources,
        hint: "Managed IWADs and id24",
    },
    Tool {
        label: "Sourceports",
        action: || ActionRequest::Sourceports,
        hint: "Build and manage sourceports from source",
    },
    Tool {
        label: "Profiles",
        action: || ActionRequest::Profiles,
        hint: "Sourceport config profiles",
    },
    Tool {
        label: "Settings",
        action: || ActionRequest::Settings,
        hint: "",
    },
];
