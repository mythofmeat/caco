pub mod form_panel;
pub mod search_panel;
pub mod state;
pub mod workers;

use state::{FormKind, ImportState, SearchResultEntry, SearchSource, IMPORT_SOURCES};

use crate::theme;

// ---------------------------------------------------------------------------
// Import actions (returned to app.rs for dispatch)
// ---------------------------------------------------------------------------

pub enum ImportAction {
    Search(SearchSource, String),
    ImportSearchResult(SearchSource, SearchResultEntry),
    ImportForm(FormKind, Vec<(String, String)>),
}

// ---------------------------------------------------------------------------
// Render the entire import view
// ---------------------------------------------------------------------------

pub fn render(ui: &mut egui::Ui, state: &mut ImportState) -> Option<ImportAction> {
    let mut action = None;

    // Source sub-tabs
    ui.horizontal(|ui| {
        for (i, label) in IMPORT_SOURCES.iter().enumerate() {
            let is_active = state.active_source == i;
            let text = egui::RichText::new(format!("{}. {label}", i + 1));
            let text = if is_active {
                text.strong().color(theme::TEXT_ACCENT)
            } else {
                text.color(theme::TEXT_SECONDARY)
            };
            if ui.selectable_label(is_active, text).clicked() {
                state.active_source = i;
            }
        }
    });
    ui.separator();

    // Dispatch to active source panel
    match state.active_source {
        0 => {
            if let Some(a) =
                search_panel::render(ui, &mut state.idgames, SearchSource::Idgames)
            {
                action = Some(map_search_action(SearchSource::Idgames, a));
            }
        }
        1 => {
            if let Some(a) =
                search_panel::render(ui, &mut state.doomwiki, SearchSource::Doomwiki)
            {
                action = Some(map_search_action(SearchSource::Doomwiki, a));
            }
        }
        2 => {
            if let Some(a) = form_panel::render(ui, &mut state.doomworld) {
                action = Some(map_form_action(FormKind::Doomworld, a));
            }
        }
        3 => {
            if let Some(a) = form_panel::render(ui, &mut state.url_form) {
                action = Some(map_form_action(FormKind::Url, a));
            }
        }
        4 => {
            if let Some(a) = form_panel::render(ui, &mut state.local_form) {
                action = Some(map_form_action(FormKind::Local, a));
            }
        }
        _ => {}
    }

    // Number key shortcuts for source switching (only when nothing focused)
    if !ui.ctx().wants_keyboard_input() {
        ui.input(|i| {
            for (idx, key) in [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
            ]
            .iter()
            .enumerate()
            {
                if i.key_pressed(*key) {
                    state.active_source = idx;
                }
            }
        });
    }

    action
}

fn map_search_action(
    source: SearchSource,
    a: search_panel::SearchPanelAction,
) -> ImportAction {
    match a {
        search_panel::SearchPanelAction::Search(q) => ImportAction::Search(source, q),
        search_panel::SearchPanelAction::Import(entry) => {
            ImportAction::ImportSearchResult(source, entry)
        }
    }
}

fn map_form_action(
    kind: FormKind,
    a: form_panel::FormPanelAction,
) -> ImportAction {
    match a {
        form_panel::FormPanelAction::Submit(values) => ImportAction::ImportForm(kind, values),
    }
}
