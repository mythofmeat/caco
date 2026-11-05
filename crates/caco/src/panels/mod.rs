pub mod cacowards;
pub mod filter_bar;
pub mod sort_controls;
pub mod stats;
pub mod wad_grid;
pub mod wad_table;

use caco_core::db::Status;

use crate::get::GetTarget;
use crate::state::{ActionRequest, ViewLayout};
use crate::theme;

/// Grid or list, in a right-to-left row. The same control wherever a view offers both.
pub fn layout_toggle(ui: &mut egui::Ui, layout: &mut ViewLayout) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let side = |ui: &mut egui::Ui, layout: &mut ViewLayout, option, label, corners| {
            let active = *layout == option;
            let text = egui::RichText::new(label).size(12.0).color(if active {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            });
            let button = egui::Button::new(text)
                .fill(if active {
                    theme::BORDER_MED
                } else {
                    theme::BG_LIGHT
                })
                .corner_radius(corners)
                .selected(active);
            if ui.add(button).clicked() {
                *layout = option;
            }
        };
        side(
            ui,
            layout,
            ViewLayout::List,
            "List",
            egui::CornerRadius {
                nw: 0,
                ne: 6,
                se: 6,
                sw: 0,
            },
        );
        side(
            ui,
            layout,
            ViewLayout::Grid,
            "Grid",
            egui::CornerRadius {
                nw: 6,
                ne: 0,
                se: 0,
                sw: 6,
            },
        );
    });
}

/// What the library shows when it has nothing to show: where to get a WAD from if it is
/// empty, and that the filter is why if it isn't. Returns true if asked to go to Import.
pub fn render_empty_library(ui: &mut egui::Ui, total_wads: usize) -> bool {
    let mut import = false;
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        if total_wads > 0 {
            ui.colored_label(theme::TEXT_SECONDARY, "No WADs match.");
            return;
        }
        ui.colored_label(
            theme::TEXT_PRIMARY,
            egui::RichText::new("Your library is empty").size(16.0),
        );
        ui.add_space(4.0);
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "Find something on idgames or the Doom Wiki, paste a link, or add a file you have.",
        );
        ui.add_space(8.0);
        import = ui.add(theme::primary_button("Import a WAD")).clicked();
    });
    import
}

/// Show a right-click context menu for a WAD with standard actions.
/// Returns the chosen action, if any.
///
/// `lost` is [`caco_core::db::WadRecord::is_lost`]: the WAD has no file here and caco
/// cannot fetch one, so getting it through a browser is the next thing to do with it.
pub fn wad_context_menu(
    response: &egui::Response,
    wad_id: i64,
    status: Status,
    lost: bool,
) -> Option<ActionRequest> {
    let mut action = None;
    response.context_menu(|ui| {
        if lost && ui.button("Get in Browser").clicked() {
            action = Some(ActionRequest::Get(GetTarget::Wad(wad_id)));
            ui.close();
        }
        if ui.button("Play").clicked() {
            action = Some(ActionRequest::Play(wad_id));
            ui.close();
        }
        if matches!(status, Status::Completed | Status::Abandoned)
            && ui.button("Start New Playthrough").clicked()
        {
            action = Some(ActionRequest::StartNewPlaythrough(wad_id));
            ui.close();
        }
        if ui.button("Edit").clicked() {
            action = Some(ActionRequest::Edit(wad_id));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            action = Some(ActionRequest::Delete(wad_id));
            ui.close();
        }
        ui.separator();
        if ui.button("Sessions").clicked() {
            action = Some(ActionRequest::Sessions(wad_id));
            ui.close();
        }
        if ui.button("Map Stats").clicked() {
            action = Some(ActionRequest::MapStats(wad_id));
            ui.close();
        }
        if ui.button("Saves & Demos").clicked() {
            action = Some(ActionRequest::WadData(wad_id));
            ui.close();
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
    if ui.input(|i| i.key_pressed(egui::Key::F)) {
        return Some(ActionRequest::WadData(wad_id));
    }
    None
}
