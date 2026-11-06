//! Left navigation sidebar: logo, views, collections, tools.

use crate::dialogs::storage::StorageTab;
use crate::state::{ActionRequest, AppState, ViewMode};
use crate::theme;

pub(super) fn render_sidebar(
    ui: &mut egui::Ui,
    state: &mut AppState,
    actions: &mut Vec<ActionRequest>,
) {
    let short = theme::is_short_window(ui.ctx());
    // The rows are spaced by their own height, or fewer of them fit in a short window.
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(if short { 8.0 } else { 16.0 });

    // Logo
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.colored_label(
            theme::TEXT_ACCENT,
            egui::RichText::new("caco").size(22.0).strong(),
        );
    });

    ui.add_space(if short { 6.0 } else { 20.0 });

    // Navigation items. Library/Import/Collections are mutually exclusive
    // sidebar selections: clicking Library or Import clears any active
    // collection (and its filter) so the highlight matches the actual
    // scope being viewed. Library is highlighted only when no collection
    // is active — when a collection is selected, the collection row owns
    // the highlight instead.
    let library_active = state.view_mode == ViewMode::Library && state.active_collection.is_none();
    if theme::sidebar_nav_item(ui, "Library", library_active) {
        state.show_view(ViewMode::Library);
    }
    if theme::sidebar_nav_item(ui, "Import", state.view_mode == ViewMode::Import) {
        state.show_view(ViewMode::Import);
    }
    if theme::sidebar_nav_item(ui, "Cacowards", state.view_mode == ViewMode::Cacowards) {
        state.show_view(ViewMode::Cacowards);
    }
    if theme::sidebar_nav_item(ui, "Stats", state.view_mode == ViewMode::Stats) {
        state.show_view(ViewMode::Stats);
    }

    ui.add_space(if short { 4.0 } else { 12.0 });
    divider(ui);

    // The rest scrolls, because at the 800x400 minimum there is not the room for all of it,
    // with what always has to be in reach pinned below. Solid rather than the floating bar
    // that only shows on hover, or tools below the fold would look as if there were none.
    let scrolling = (ui.available_height() - PINNED).max(0.0);
    ui.scope(|ui| {
        theme::solid_scrollbar(ui);
        egui::ScrollArea::vertical()
            .id_salt("sidebar-tools")
            .auto_shrink([false, false])
            .max_height(scrolling)
            .show(ui, |ui| {
                ui.add_space(8.0);
                render_collections(ui, state, actions);
                ui.add_space(12.0);
                section_heading(ui, "TOOLS");
                render_tools(ui, TOOLS, &state.sourceport_updates, actions);
                ui.add_space(8.0);
            });
    });

    divider(ui);
    ui.add_space(4.0);
    render_tools(ui, PINNED_TOOLS, &[], actions);
}

fn divider(ui: &mut egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x + 20.0, rect.min.y),
            egui::pos2(rect.max.x - 20.0, rect.min.y),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    ui.add_space(1.0);
}

fn section_heading(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.colored_label(
            theme::TEXT_MUTED,
            egui::RichText::new(title).size(11.0).strong(),
        );
    });
    ui.add_space(2.0);
}

fn render_collections(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<ActionRequest>) {
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
    ui.add_space(2.0);

    if state.sidebar_collections.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new("No collections yet").size(12.0),
            );
        });
        return;
    }

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

fn render_tools(
    ui: &mut egui::Ui,
    tools: &[Tool],
    updates: &[String],
    actions: &mut Vec<ActionRequest>,
) {
    for tool in tools {
        let row = theme::sidebar_tool_item(ui, tool.label);
        // Whether there is something new here is the kind of thing a notification says once,
        // and only this goes on saying.
        let row = if tool.label == "Sourceports" && !updates.is_empty() {
            theme::paint_row_dot(ui, row.rect);
            row.on_hover_text(format!("{} has an update.", updates.join(", ")))
        } else {
            row.on_hover_text(tool.hint)
        };
        if row.clicked() {
            actions.push((tool.action)());
        }
    }
}

/// Height of a row in the list of tools: see `theme::sidebar_tool_item`.
const TOOL_ROW: f32 = 26.0;

/// What has to stay below the part that scrolls: the rule, the gap and the rows.
const PINNED: f32 = 1.0 + 4.0 + TOOL_ROW * PINNED_TOOLS.len() as f32 + 6.0;

/// One entry in the sidebar's list of tools.
struct Tool {
    label: &'static str,
    /// A constructor rather than a value: `ActionRequest` is not `Clone`, and
    /// these are only ever built when the row is clicked.
    action: fn() -> ActionRequest,
    hint: &'static str,
}

/// The tools, in the order they appear in the sidebar: disk first, then library maintenance,
/// then setup. In the open, since the sidebar is where a person looks for what caco can do,
/// and they used to be folded away under a heading of their own.
const TOOLS: &[Tool] = &[
    Tool {
        label: "Storage",
        action: || ActionRequest::Storage(StorageTab::Cache),
        hint: "What is taking up space, and getting it back",
    },
    Tool {
        label: "Mirrors",
        action: || ActionRequest::Mirrors,
        hint: "Your own stores of WAD files",
    },
    Tool {
        label: "Sync",
        action: || ActionRequest::Sync,
        hint: "Keep this library on several machines",
    },
    Tool {
        label: "Enrich",
        action: || ActionRequest::Enrich,
        hint: "Fill in what is missing about your WADs",
    },
    Tool {
        label: "IWADs",
        action: || ActionRequest::Resources,
        hint: "The game files you own",
    },
    Tool {
        label: "Sourceports",
        action: || ActionRequest::Sourceports,
        hint: "Build sourceports from their source code",
    },
    Tool {
        label: "Profiles",
        action: || ActionRequest::Profiles,
        hint: "Sourceport settings you can give each WAD",
    },
];

/// In reach whatever the size of the window.
const PINNED_TOOLS: &[Tool] = &[
    Tool {
        label: "Settings",
        action: || ActionRequest::Settings,
        hint: "Sourceports, downloads, and where things go",
    },
    Tool {
        label: "Help",
        action: || ActionRequest::Help,
        hint: "Keyboard shortcuts, and which version this is",
    },
];
