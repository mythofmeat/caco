pub mod cacowards;
pub mod filter_bar;
pub mod sort_controls;
pub mod stats;
pub mod wad_details;
pub mod wad_grid;
pub mod wad_table;

use caco_core::db::{Status, WadRecord};

use crate::get::GetTarget;
use crate::state::{ActionRequest, AppState, ViewLayout, ViewMode};
use crate::theme;

/// Grid or list, in a right-to-left row. The same control wherever a view offers both: one
/// outline around the two, with what is showing filled in.
pub fn layout_toggle(ui: &mut egui::Ui, layout: &mut ViewLayout) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0_f32, theme::CONTROL_BORDER))
        .corner_radius(8)
        .show(ui, |ui| {
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
                        theme::CONTROL_FILL_HOVER
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .stroke(egui::Stroke::NONE)
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
                    ne: 7,
                    se: 7,
                    sw: 0,
                },
            );
            side(
                ui,
                layout,
                ViewLayout::Grid,
                "Grid",
                egui::CornerRadius {
                    nw: 7,
                    ne: 0,
                    se: 0,
                    sw: 7,
                },
            );
        });
}

/// A term for the filter: `field:value`, quoted if it has to be, as the filter expects.
pub fn field_query(field: &str, value: &str) -> String {
    if value.contains(char::is_whitespace) {
        format!("{field}:\"{}\"", value.replace('"', ""))
    } else {
        format!("{field}:{value}")
    }
}

/// How far along a new library is towards playing something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Setup {
    /// What will run what gets played, if anything has been found.
    pub sourceport: Option<String>,
    /// How many game files caco knows about.
    pub iwads: usize,
}

/// How wide the list of what to do first gets.
const SETUP_WIDTH: f32 = 480.0;

/// What the library shows when it has nothing to show. If it is really empty, what there is to
/// do before anything can be played, which nothing used to mention until Play failed. If not,
/// that the filter is why, with the way out of that.
pub fn render_empty_library(
    ui: &mut egui::Ui,
    total_wads: usize,
    setup: &Setup,
) -> Option<ActionRequest> {
    let mut action = None;
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        if total_wads > 0 {
            ui.colored_label(theme::TEXT_SECONDARY, "No WADs match.");
            ui.add_space(4.0);
            if ui.button("Show All WADs").clicked() {
                action = Some(ActionRequest::ClearFilters);
            }
            return;
        }
        ui.colored_label(
            theme::TEXT_PRIMARY,
            egui::RichText::new("Your library is empty").size(16.0),
        );
        ui.add_space(12.0);

        let width = ui.available_width().min(SETUP_WIDTH);
        ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                setup_steps(ui, setup, &mut action);
            },
        );
    });
    action
}

fn setup_steps(ui: &mut egui::Ui, setup: &Setup, action: &mut Option<ActionRequest>) {
    let (title, detail, button, go) = match &setup.sourceport {
        Some(name) => (
            format!("{name} will run what you play"),
            "Change it, or set one up for each WAD, in Settings.",
            "Settings",
            ActionRequest::Settings,
        ),
        None => (
            "No sourceport found".to_string(),
            "caco needs one to launch anything. Install one, or build one here.",
            "Sourceports",
            ActionRequest::Sourceports,
        ),
    };
    if setup_step(ui, setup.sourceport.is_some(), &title, detail, button) {
        *action = Some(go);
    }

    let iwads = if setup.iwads == 0 {
        "No game files yet".to_string()
    } else {
        format!(
            "{} ready",
            crate::dialogs::count(setup.iwads, "game file", "game files")
        )
    };
    if setup_step(
        ui,
        setup.iwads > 0,
        &iwads,
        "The IWADs you own, such as doom2.wad, which most WADs need to run.",
        "IWADs",
    ) {
        *action = Some(ActionRequest::Resources);
    }

    ui.add_space(8.0);
    ui.colored_label(theme::TEXT_PRIMARY, "Then find something to play");
    ui.add_space(2.0);
    ui.add(
        egui::Label::new(
            egui::RichText::new(
                "Search idgames or the Doom Wiki, paste a link, or add a file you have. The \
                 Cacowards are every year's best, and a good place to start.",
            )
            .size(12.0)
            .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.add(theme::primary_button("Import a WAD")).clicked() {
            *action = Some(ActionRequest::ShowView(ViewMode::Import));
        }
        if ui.button("See the Cacowards").clicked() {
            *action = Some(ActionRequest::ShowView(ViewMode::Cacowards));
        }
    });
}

/// One line of what there is to do first. True if its button was pressed.
fn setup_step(ui: &mut egui::Ui, done: bool, title: &str, detail: &str, button: &str) -> bool {
    let mut pressed = false;
    ui.horizontal(|ui| {
        // Colour is never the only signal, and these are different shapes as well.
        let (mark, color) = if done {
            ("\u{2713}", theme::COLOR_SUCCESS)
        } else {
            ("\u{25cb}", theme::COLOR_WARNING)
        };
        ui.colored_label(color, egui::RichText::new(mark).size(14.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            pressed = ui.button(button).clicked();
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.colored_label(theme::TEXT_PRIMARY, title);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(detail)
                            .size(12.0)
                            .color(theme::TEXT_SECONDARY),
                    )
                    .wrap(),
                );
            });
        });
    });
    ui.add_space(6.0);
    pressed
}

/// Show a right-click context menu for a WAD with standard actions, what you might do first at
/// the top and what cannot easily be undone at the bottom. Returns the chosen action, if any.
///
/// Get in Browser is there for [`WadRecord::is_lost`]: the WAD has no file here and caco cannot
/// fetch one, so getting it through a browser is the next thing to do with it.
pub fn wad_context_menu(response: &egui::Response, wad: &WadRecord) -> Option<ActionRequest> {
    let wad_id = wad.id;
    let mut action = None;
    response.context_menu(|ui| {
        if wad.is_lost() && ui.button("Get in Browser").clicked() {
            action = Some(ActionRequest::Get(GetTarget::Wad(wad_id)));
            ui.close();
        }
        if ui.button("Play").clicked() {
            action = Some(ActionRequest::Play(wad_id));
            ui.close();
        }
        if matches!(wad.status, Status::Completed | Status::Abandoned)
            && ui.button("Start New Playthrough").clicked()
        {
            action = Some(ActionRequest::StartNewPlaythrough(wad_id));
            ui.close();
        }
        ui.separator();
        if ui.button("Details").clicked() {
            action = Some(ActionRequest::ShowDetails(wad_id));
            ui.close();
        }
        ui.menu_button("Status", |ui| {
            for &status in Status::ALL {
                let style = theme::status_style(status);
                let text = egui::RichText::new(style.word).color(style.color);
                if ui.selectable_label(wad.status == status, text).clicked() {
                    if wad.status != status {
                        action = Some(ActionRequest::SetStatus(wad_id, status));
                    }
                    ui.close();
                }
            }
        });
        ui.menu_button("Rating", |ui| {
            for stars in (1..=5).rev() {
                let text = egui::RichText::new(theme::rating_stars(Some(stars))).color(theme::GOLD);
                if ui
                    .selectable_label(wad.rating == Some(stars), text)
                    .on_hover_text(crate::dialogs::count(stars as usize, "star", "stars"))
                    .clicked()
                {
                    action = Some(ActionRequest::SetRating(wad_id, Some(stars)));
                    ui.close();
                }
            }
            if wad.rating.is_some() && ui.button("No Rating").clicked() {
                action = Some(ActionRequest::SetRating(wad_id, None));
                ui.close();
            }
        });
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
        ui.separator();
        if ui.button("Edit").clicked() {
            action = Some(ActionRequest::Edit(wad_id));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            action = Some(ActionRequest::Delete(wad_id));
            ui.close();
        }
    });
    action
}

/// Bring a stretch of the library into view, which is in a row or on a card. The first row
/// goes all the way to `view_top`, taking the banner and the filters above it along, or going
/// back to the top would leave them scrolled away.
pub fn reveal(ui: &egui::Ui, stretch: egui::Rect, is_first_row: bool, view_top: f32) {
    if is_first_row {
        let top = egui::Rect::from_min_size(egui::pos2(stretch.left(), view_top), stretch.size());
        ui.scroll_to_rect(top, Some(egui::Align::TOP));
    } else {
        ui.scroll_to_rect(stretch, None);
    }
}

/// Whether a view's keys are its own this frame: not while something is being typed, and not
/// while a menu is open. egui's menus read Escape, the arrows and Enter as pressed, later in the
/// frame than the view, and a key the view has taken never reaches them: Escape left a WAD's
/// menu open and deselected the WAD behind it, and Enter in the menu played the WAD.
pub fn keyboard_is_free(ui: &egui::Ui) -> bool {
    !ui.ctx().egui_wants_keyboard_input() && !egui::Popup::is_any_open(ui.ctx())
}

/// Whether a key was pressed on its own, and if so take it. Ctrl+D is somebody's habit for
/// something else, and shouldn't ask to delete a WAD.
pub fn plain_key(ui: &egui::Ui, key: egui::Key) -> bool {
    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key))
}

/// How the library's two layouts differ to the arrow keys.
#[derive(Clone, Copy)]
pub struct LibraryShape {
    /// How many WADs across. Left and right only mean something with more than one.
    pub columns: usize,
    /// How many WADs one press of Page Down should go past.
    pub page: usize,
}

/// The keys for the library, which both of its layouts share. Nothing happens with a dialog
/// open, while something is being typed or while a menu is open (`keyboard_is_free`).
pub fn library_keys(
    ui: &egui::Ui,
    state: &mut AppState,
    shape: LibraryShape,
) -> Option<ActionRequest> {
    if state.has_dialog() || !keyboard_is_free(ui) {
        return None;
    }
    let LibraryShape { columns, page } = shape;
    let pressed = |key| plain_key(ui, key);

    if pressed(egui::Key::J) || pressed(egui::Key::ArrowDown) {
        state.select_down(columns);
    }
    if pressed(egui::Key::K) || pressed(egui::Key::ArrowUp) {
        state.select_up(columns);
    }
    if columns > 1 {
        if pressed(egui::Key::H) || pressed(egui::Key::ArrowLeft) {
            state.move_selection(-1);
        }
        if pressed(egui::Key::L) || pressed(egui::Key::ArrowRight) {
            state.move_selection(1);
        }
    }
    if pressed(egui::Key::PageDown) {
        state.move_selection(page as isize);
    }
    if pressed(egui::Key::PageUp) {
        state.move_selection(-(page as isize));
    }
    if pressed(egui::Key::Home) {
        state.select_first();
    }
    if pressed(egui::Key::End) {
        state.select_last();
    }
    // Shift first, since a key asked for without it is taken with it held as well.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::G)) {
        state.select_last();
    } else if pressed(egui::Key::G) {
        state.handle_g_press();
    }
    // Back out of whatever is narrowing the list first, and only then of the selection.
    if pressed(egui::Key::Escape) {
        if state.filter.input.is_empty() {
            state.deselect();
        } else {
            state.filter.clear(std::time::Instant::now());
        }
    }

    handle_action_keys(ui, state.selected_wad_id)
}

/// Handle shared action key shortcuts (Enter/P/E/D/S) for WAD views.
/// Returns an action if a key was pressed while a WAD is selected.
pub fn handle_action_keys(ui: &egui::Ui, selected_wad_id: Option<i64>) -> Option<ActionRequest> {
    let wad_id = selected_wad_id?;
    let pressed = |key| plain_key(ui, key);

    if pressed(egui::Key::Enter) || pressed(egui::Key::P) {
        return Some(ActionRequest::Play(wad_id));
    }
    if pressed(egui::Key::E) {
        return Some(ActionRequest::Edit(wad_id));
    }
    if pressed(egui::Key::D) || pressed(egui::Key::Delete) {
        return Some(ActionRequest::Delete(wad_id));
    }
    if pressed(egui::Key::S) {
        return Some(ActionRequest::Sessions(wad_id));
    }
    if pressed(egui::Key::M) {
        return Some(ActionRequest::MapStats(wad_id));
    }
    if pressed(egui::Key::F) {
        return Some(ActionRequest::WadData(wad_id));
    }
    if pressed(egui::Key::I) {
        return Some(ActionRequest::ToggleDetails);
    }
    None
}
