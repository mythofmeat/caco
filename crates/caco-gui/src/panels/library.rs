use std::time::Instant;

use crate::persist::SavedSearch;
use crate::state::{AppState, ViewMode, TABS};
use crate::theme;

/// Render the tab bar across the top.
pub fn render_tab_bar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // Library tabs with underline indicator
        for (i, tab) in TABS.iter().enumerate() {
            let is_active = state.view_mode == ViewMode::Library && state.active_tab == i;
            let text = egui::RichText::new(tab.label);
            let text = if is_active {
                text.strong().color(theme::TEXT_PRIMARY)
            } else {
                text.color(theme::TEXT_SECONDARY)
            };

            let response = ui.selectable_label(false, text);
            if is_active {
                let rect = response.rect;
                ui.painter().line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    egui::Stroke::new(2.0, theme::TEXT_ACCENT),
                );
            }
            if response.clicked() {
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
            import_text.strong().color(theme::TEXT_PRIMARY)
        } else {
            import_text.color(theme::TEXT_SECONDARY)
        };
        let response = ui.selectable_label(false, import_text);
        if import_active {
            let rect = response.rect;
            ui.painter().line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                egui::Stroke::new(2.0, theme::TEXT_ACCENT),
            );
        }
        if response.clicked() {
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

/// Render the toolbar (filter bar + searches + sort controls).
pub fn render_toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        super::filter_bar::render(ui, state);
        render_searches_button(ui, state);
        super::sort_controls::render(ui, state);
    });
}

/// Render the "Searches" dropdown button for saved searches.
fn render_searches_button(ui: &mut egui::Ui, state: &mut AppState) {
    let popup_id = ui.id().with("saved_searches_popup");
    let btn = ui.button("Searches");
    if btn.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    egui::popup_below_widget(ui, popup_id, &btn, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(200.0);

        // List saved searches
        if state.saved_searches.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "No saved searches");
        } else {
            let mut load_query: Option<String> = None;
            for search in &state.saved_searches {
                let resp = ui.button(&search.name);
                if !search.name.eq(&search.query) {
                    resp.clone().on_hover_text(&search.query);
                }
                if resp.clicked() {
                    load_query = Some(search.query.clone());
                }
            }
            if let Some(query) = load_query {
                state.filter_text = query;
                state.filter_changed_at = Some(Instant::now());
            }
        }

        ui.separator();

        // Save current filter
        let has_filter = !state.filter_text.trim().is_empty();
        if ui
            .add_enabled(has_filter, egui::Button::new("Save current filter..."))
            .clicked()
        {
            state.save_search_pending = true;
            state.save_search_name = state.filter_text.trim().to_string();
        }

        // Delete submenu
        if !state.saved_searches.is_empty() {
            let mut delete_idx: Option<usize> = None;
            ui.menu_button("Delete...", |ui| {
                for (i, search) in state.saved_searches.iter().enumerate() {
                    if ui.button(&search.name).clicked() {
                        delete_idx = Some(i);
                    }
                }
            });
            if let Some(idx) = delete_idx {
                state.saved_searches.remove(idx);
            }
        }
    });

    // Handle the save-search dialog (rendered as a floating window)
    if state.save_search_pending {
        let mut saved = false;
        let mut cancelled = false;
        egui::Window::new("Save Search")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut state.save_search_name);
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() && !state.save_search_name.trim().is_empty() {
                        saved = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        cancelled = true;
                    }
                });
            });

        if saved {
            let name = state.save_search_name.trim().to_string();
            let query = state.filter_text.trim().to_string();
            // Replace existing search with same name
            state.saved_searches.retain(|s| s.name != name);
            state.saved_searches.push(SavedSearch { name, query });
            state.save_search_pending = false;
            state.save_search_name.clear();
        } else if cancelled {
            state.save_search_pending = false;
            state.save_search_name.clear();
        }
    }
}
