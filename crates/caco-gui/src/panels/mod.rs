pub mod detail;
pub mod filter_bar;
pub mod library;
pub mod sort_controls;
pub mod wad_grid;
pub mod wad_table;

use crate::state::ActionRequest;

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
    None
}
