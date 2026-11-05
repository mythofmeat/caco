use crate::import::state::{SearchResultEntry, SearchSource, SearchSourceData, SearchState};
use crate::theme;

// ---------------------------------------------------------------------------
// Actions returned to the caller
// ---------------------------------------------------------------------------

/// ID source for the search box (shared with app.rs for Ctrl+F and `/`). The two searches
/// share it, since only one is on the screen.
pub const SEARCH_ID_SOURCE: &str = "import_search_input";

const ROW_HEIGHT: f32 = 22.0;

/// The check mark's column, which is what says a result is in the library already.
const MARK_COLUMN: f32 = 16.0;
const AUTHOR_COLUMN: f32 = 120.0;
const RATING_COLUMN: f32 = 44.0;
const DATE_COLUMN: f32 = 72.0;
const YEAR_COLUMN: f32 = 40.0;
const SOURCEPORT_COLUMN: f32 = 76.0;

pub enum SearchPanelAction {
    Search(String),
    Import(SearchResultEntry),
    ShowInLibrary(i64),
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

pub fn render(
    ui: &mut egui::Ui,
    state: &mut SearchState,
    source: SearchSource,
) -> Option<SearchPanelAction> {
    let mut action = None;

    // Search bar
    ui.horizontal(|ui| {
        let (name, hint) = match source {
            SearchSource::Idgames => ("Search idgames", "Title, author or file name"),
            SearchSource::Doomwiki => ("Search the Doom Wiki", "The name of a WAD"),
        };
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut state.query)
                    .id(egui::Id::unique(SEARCH_ID_SOURCE))
                    .desired_width(300.0)
                    .hint_text(hint),
            )
            .accessible_name(name);
        // Taken, or what is below would see it with the box no longer holding the keyboard, and
        // import whatever is selected as well as searching.
        let entered = response.lost_focus()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let can_search = !state.query.trim().is_empty() && !state.is_searching;
        let pressed = ui
            .add_enabled(can_search, egui::Button::new("Search"))
            .clicked();
        if (entered || pressed) && can_search {
            let q = state.query.trim().to_string();
            state.is_searching = true;
            state.status_text = "Searching...".to_string();
            action = Some(SearchPanelAction::Search(q));
        }

        // Status
        if state.is_searching {
            ui.spinner();
        }
        if !state.status_text.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, &state.status_text);
        }
    });

    if let Some(note) = &state.note {
        ui.add(
            egui::Label::new(
                egui::RichText::new(note)
                    .size(12.0)
                    .color(theme::COLOR_WARNING),
            )
            .truncate(),
        )
        .on_hover_text(note);
    }

    ui.separator();

    if state.results.is_empty() {
        render_nothing_yet(ui, state, source);
        return action;
    }

    if let Some(keyed) = keyboard(ui, state) {
        action = Some(keyed);
    }

    // Horizontal split: results table (left) + preview (right)
    let available = ui.available_size();
    let table_width = (available.x * 0.6).max(200.0);

    ui.horizontal_top(|ui| {
        // Results table
        ui.vertical(|ui| {
            ui.set_width(table_width);
            if let Some(a) = render_results_table(ui, state, source, available.y) {
                action = Some(a);
            }
        });

        ui.separator();

        // Preview panel
        ui.vertical(|ui| {
            if let Some(a) = render_preview(ui, state) {
                action = Some(a);
            }
        });
    });

    action
}

/// Up and down through the results, and Enter to import, while nothing is being typed. Marks
/// the selection to be brought into view.
fn keyboard(ui: &egui::Ui, state: &mut SearchState) -> Option<SearchPanelAction> {
    if ui.ctx().egui_wants_keyboard_input() {
        return None;
    }
    let pressed = |key| ui.input(|i| i.key_pressed(key));
    let last = state.results.len().saturating_sub(1);
    let from = state.selected_row.unwrap_or(0);
    let to = if pressed(egui::Key::ArrowDown) || pressed(egui::Key::J) {
        Some((from + 1).min(last))
    } else if pressed(egui::Key::ArrowUp) || pressed(egui::Key::K) {
        Some(from.saturating_sub(1))
    } else {
        None
    };
    if let Some(row) = to {
        state.selected_row = Some(row);
        state.reveal_row = true;
    }
    if pressed(egui::Key::Enter) {
        return state.selected_entry().and_then(entry_action);
    }
    None
}

/// What Enter or a double click does for a result: import it, unless it's there already.
fn entry_action(entry: &SearchResultEntry) -> Option<SearchPanelAction> {
    Some(match entry.in_library {
        Some(wad_id) => SearchPanelAction::ShowInLibrary(wad_id),
        None => SearchPanelAction::Import(entry.clone()),
    })
}

/// In place of a table with nothing in it, why there is nothing in it.
fn render_nothing_yet(ui: &mut egui::Ui, state: &SearchState, source: SearchSource) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        if state.is_searching {
            ui.colored_label(theme::TEXT_SECONDARY, "Searching...");
        } else if let Some(error) = &state.error {
            ui.colored_label(theme::COLOR_ERROR, error);
        } else if !state.status_text.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "No results");
            ui.colored_label(
                theme::TEXT_MUTED,
                egui::RichText::new("Fewer words, or part of a name, may find more.").size(12.0),
            );
        } else {
            let what = match source {
                SearchSource::Idgames => {
                    "Search the idgames archive for something to play. Importing adds it to your \
                     library, and it downloads when you first play it."
                }
                SearchSource::Doomwiki => {
                    "Search the Doom Wiki for a WAD. Importing adds what the wiki knows about it to \
                     your library, and you can get the file from there."
                }
            };
            ui.colored_label(theme::TEXT_SECONDARY, what);
        }
    });
}

fn render_results_table(
    ui: &mut egui::Ui,
    state: &mut SearchState,
    source: SearchSource,
    height: f32,
) -> Option<SearchPanelAction> {
    use egui_extras::{Column, TableBuilder};

    let extra_headers = match source {
        SearchSource::Idgames => ["Rating", "Date"],
        SearchSource::Doomwiki => ["Year", "Sourceport"],
    };
    let (first_width, second_width) = match source {
        SearchSource::Idgames => (RATING_COLUMN, DATE_COLUMN),
        SearchSource::Doomwiki => (YEAR_COLUMN, SOURCEPORT_COLUMN),
    };

    let mut clicked_row = None;
    let mut action = None;

    theme::calm_table_selection(ui);
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .auto_shrink([false, false])
        // Only what is left below the search box, which at 800x400 is less than a table's own
        // minimum.
        .min_scrolled_height(0.0)
        .max_scroll_height(height)
        .column(Column::exact(MARK_COLUMN))
        // Clipped, or one long title widens the table and takes the preview off the
        // window with it. Whole in the preview, and on hover.
        .column(Column::remainder().at_least(100.0).clip(true)) // Title
        .column(Column::exact(AUTHOR_COLUMN).clip(true)) // Author
        .column(Column::exact(first_width))
        .column(Column::exact(second_width));

    if std::mem::take(&mut state.reveal_row)
        && let Some(row) = state.selected_row
    {
        table = table.scroll_to_row(row, None);
    }

    table
        .header(20.0, |mut header| {
            header.col(|_| {});
            header.col(|ui| {
                ui.label(theme::column_header("Title"));
            });
            header.col(|ui| {
                ui.label(theme::column_header("Author"));
            });
            for label in extra_headers {
                header.col(|ui| {
                    ui.label(theme::column_header(label));
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, state.results.len(), |mut row| {
                let idx = row.index();
                row.set_selected(state.selected_row == Some(idx));

                let entry = &state.results[idx];
                row.col(|ui| {
                    if entry.in_library.is_some() {
                        ui.colored_label(theme::COLOR_SUCCESS, "\u{2713}")
                            .on_hover_text("In your library");
                    }
                });
                row.col(|ui| {
                    ui.label(&entry.title).on_hover_text(&entry.title);
                });
                row.col(|ui| {
                    ui.colored_label(theme::TEXT_SECONDARY, entry.author.as_deref().unwrap_or(""));
                });
                let [first, second] = entry.extra_columns();
                row.col(|ui| {
                    ui.colored_label(theme::TEXT_SECONDARY, first);
                });
                row.col(|ui| {
                    ui.colored_label(theme::TEXT_SECONDARY, second);
                });

                let response = row.response();
                if response.clicked() {
                    clicked_row = Some(idx);
                }
                if response.double_clicked() {
                    action = entry_action(entry);
                }
            });
        });

    if let Some(idx) = clicked_row {
        state.selected_row = Some(idx);
    }

    action
}

fn render_preview(ui: &mut egui::Ui, state: &SearchState) -> Option<SearchPanelAction> {
    let Some(idx) = state.selected_row else {
        ui.colored_label(theme::TEXT_SECONDARY, "Select a result to preview");
        return None;
    };
    let entry = state.results.get(idx)?;
    let mut action = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;

            // Title
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&entry.title)
                        .size(16.0)
                        .color(theme::TEXT_PRIMARY),
                )
                .wrap(),
            );

            // Author
            if let Some(author) = &entry.author {
                ui.colored_label(theme::TEXT_SECONDARY, format!("by {author}"));
            }

            // Above what could be pages long, where it can always be reached.
            ui.add_space(4.0);
            action = preview_button(ui, state, entry);
            ui.add_space(4.0);

            // Source-specific metadata
            match &entry.source_data {
                SearchSourceData::Idgames {
                    rating,
                    date,
                    dir,
                    filename,
                    ..
                } => {
                    if let Some(r) = rating {
                        preview_row(ui, "Rating", &format!("{r:.1}/5.0"));
                    }
                    if let Some(d) = date {
                        preview_row(ui, "Date", d);
                    }
                    if let Some(f) = filename {
                        preview_row(ui, "File", f);
                    }
                    if let Some(d) = dir {
                        preview_row(ui, "In", d);
                    }
                }
                SearchSourceData::Doomwiki {
                    year, iwad, port, ..
                } => {
                    if let Some(y) = year {
                        preview_row(ui, "Year", &y.to_string());
                    }
                    if let Some(i) = iwad {
                        preview_row(ui, "IWAD", i);
                    }
                    if let Some(p) = port {
                        preview_row(ui, "Sourceport", p);
                    }
                }
            }

            // Description
            if let Some(desc) = &entry.description {
                ui.separator();
                ui.add(
                    egui::Label::new(egui::RichText::new(desc).color(theme::TEXT_SECONDARY)).wrap(),
                );
            }
        });
    action
}

/// Import, or what stands in for it once that is under way or done.
fn preview_button(
    ui: &mut egui::Ui,
    state: &SearchState,
    entry: &SearchResultEntry,
) -> Option<SearchPanelAction> {
    if let Some(wad_id) = entry.in_library {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.colored_label(theme::COLOR_SUCCESS, "\u{2713} In your library");
            if ui.button("Show").clicked() {
                action = Some(SearchPanelAction::ShowInLibrary(wad_id));
            }
        });
        return action;
    }
    if state.is_importing(entry) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.colored_label(theme::TEXT_SECONDARY, "Importing\u{2026}");
        });
        return None;
    }
    ui.add(theme::primary_button("Import"))
        .clicked()
        .then(|| SearchPanelAction::Import(entry.clone()))
}

fn preview_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT_SECONDARY, format!("{label}:"));
        ui.colored_label(theme::TEXT_PRIMARY, value);
    });
}
