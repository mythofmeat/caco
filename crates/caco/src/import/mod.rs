pub mod form_panel;
pub mod search_panel;
pub mod state;
pub mod workers;

use state::{FormKind, IMPORT_SOURCES, ImportState, SearchResultEntry, SearchSource};

use crate::dialogs::tab_bar;
use crate::theme;

// ---------------------------------------------------------------------------
// Import actions (returned to app.rs for dispatch)
// ---------------------------------------------------------------------------

pub enum ImportAction {
    Search(SearchSource, String),
    /// Fill the idgames table with the newest uploads.
    Newest,
    /// Bring the local index of idgames up to date, now, whatever the setting says.
    UpdateIndex,
    ImportSearchResult(SearchSource, SearchResultEntry),
    ImportForm(FormKind, Vec<(String, String)>),
    /// Go to something that was already in the library.
    ShowWad(i64),
}

// ---------------------------------------------------------------------------
// Render the entire import view
// ---------------------------------------------------------------------------

pub fn render(ui: &mut egui::Ui, state: &mut ImportState) -> Option<ImportAction> {
    let mut action = None;

    // Source sub-tabs
    let tabs: Vec<(usize, &str)> = IMPORT_SOURCES.iter().copied().enumerate().collect();
    tab_bar(ui, &mut state.active_source, &tabs);
    ui.separator();

    // Dispatch to active source panel
    match state.active_source {
        0 => {
            action = render_index_bar(ui, state);
            if state.idgames.wants_newest(&state.index) {
                state.idgames.newest_asked = true;
                action = Some(ImportAction::Newest);
            }
            if let Some(a) = search_panel::render(ui, &mut state.idgames, SearchSource::Idgames) {
                action = Some(map_search_action(SearchSource::Idgames, a));
            }
        }
        1 => {
            if let Some(a) = search_panel::render(ui, &mut state.doomwiki, SearchSource::Doomwiki) {
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
    if crate::panels::keyboard_is_free(ui) {
        for (idx, key) in [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
        ]
        .into_iter()
        .enumerate()
        {
            if crate::panels::plain_key(ui, key) {
                state.active_source = idx;
            }
        }
    }

    action
}

/// One line about the local index of idgames, with what can be done about it. Clipped
/// rather than wrapped, since it sits above a table that needs the height at 800x400, with
/// all of it behind a hover.
fn render_index_bar(ui: &mut egui::Ui, state: &mut ImportState) -> Option<ImportAction> {
    let mut action = None;
    let index = &state.index;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            if index.is_running() {
                if ui
                    .button("Stop")
                    .on_hover_text("It picks up where it left off the next time caco starts")
                    .clicked()
                {
                    index.stop();
                }
                ui.spinner();
            } else if ui
                .button("Update")
                .on_hover_text("Ask a mirror for a new listing now, however recent this one is")
                .clicked()
            {
                action = Some(ImportAction::UpdateIndex);
            }
            if ui
                .button("Newest")
                .on_hover_text("The most recent uploads to idgames")
                .clicked()
            {
                state.idgames.query.clear();
                state.idgames.results.clear();
                state.idgames.showing_newest = true;
                action = Some(ImportAction::Newest);
            }

            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let (text, color) = match &index.problem {
                    Some(problem) if !index.is_running() => (problem.clone(), theme::COLOR_WARNING),
                    _ => (index.summary(), theme::TEXT_SECONDARY),
                };
                let label =
                    egui::Label::new(egui::RichText::new(&text).size(12.0).color(color)).truncate();
                ui.add(label).on_hover_text(&text);
            });
        });
    });
    action
}

fn map_search_action(source: SearchSource, a: search_panel::SearchPanelAction) -> ImportAction {
    match a {
        search_panel::SearchPanelAction::Search(q) => ImportAction::Search(source, q),
        search_panel::SearchPanelAction::Import(entry) => {
            ImportAction::ImportSearchResult(source, entry)
        }
        search_panel::SearchPanelAction::ShowInLibrary(wad_id) => ImportAction::ShowWad(wad_id),
    }
}

fn map_form_action(kind: FormKind, a: form_panel::FormPanelAction) -> ImportAction {
    match a {
        form_panel::FormPanelAction::Submit(values) => ImportAction::ImportForm(kind, values),
    }
}
