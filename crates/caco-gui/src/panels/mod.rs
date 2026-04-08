pub mod filter_bar;
pub mod library;
pub mod sort_controls;
pub mod wad_grid;
pub mod wad_table;

use crate::state::ActionRequest;

/// Show a right-click context menu for a WAD with standard actions.
/// Returns the chosen action, if any.
pub fn wad_context_menu(response: &egui::Response, wad_id: i64, status: &str) -> Option<ActionRequest> {
    let mut action = None;
    response.context_menu(|ui| {
        if ui.button("Play").clicked() {
            action = Some(ActionRequest::Play(wad_id));
            ui.close_menu();
        }
        if status == "completed" || status == "abandoned" {
            if ui.button("Start New Playthrough").clicked() {
                action = Some(ActionRequest::StartNewPlaythrough(wad_id));
                ui.close_menu();
            }
        }
        if ui.button("Edit").clicked() {
            action = Some(ActionRequest::Edit(wad_id));
            ui.close_menu();
        }
        if ui.button("Delete").clicked() {
            action = Some(ActionRequest::Delete(wad_id));
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Sessions").clicked() {
            action = Some(ActionRequest::Sessions(wad_id));
            ui.close_menu();
        }
        if ui.button("Map Stats").clicked() {
            action = Some(ActionRequest::MapStats(wad_id));
            ui.close_menu();
        }
    });
    action
}

/// Handle shared action key shortcuts (Enter/P/E/D/S) for WAD views.
/// Returns an action if a key was pressed while a WAD is selected.
pub fn handle_action_keys(ui: &egui::Ui, selected_wad_id: Option<i64>) -> Option<ActionRequest> {
    let wad_id = selected_wad_id?;

    if ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::P)) {
        return Some(ActionRequest::Play(wad_id));
    }
    if ui.input(|i| i.key_pressed(egui::Key::E)) {
        return Some(ActionRequest::Edit(wad_id));
    }
    if ui.input(|i| i.key_pressed(egui::Key::D)) {
        return Some(ActionRequest::Delete(wad_id));
    }
    if ui.input(|i| i.key_pressed(egui::Key::S)) {
        return Some(ActionRequest::Sessions(wad_id));
    }
    if ui.input(|i| i.key_pressed(egui::Key::M)) {
        return Some(ActionRequest::MapStats(wad_id));
    }
    None
}
