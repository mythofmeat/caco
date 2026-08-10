use crate::import::state::{SearchResultEntry, SearchSource, SearchSourceData, SearchState};
use crate::theme;

// ---------------------------------------------------------------------------
// Actions returned to the caller
// ---------------------------------------------------------------------------

pub enum SearchPanelAction {
    Search(String),
    Import(SearchResultEntry),
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
        ui.label("Search:");
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .desired_width(300.0)
                .hint_text("Enter query and press Enter"),
        );
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let q = state.query.trim().to_string();
            if !q.is_empty() && !state.is_searching {
                state.is_searching = true;
                state.status_text = "Searching...".to_string();
                action = Some(SearchPanelAction::Search(q));
            }
        }

        // Status
        if !state.status_text.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, &state.status_text);
        }
    });

    ui.separator();

    // Horizontal split: results table (left) + preview (right)
    let available = ui.available_size();
    let table_width = (available.x * 0.6).max(200.0);

    ui.horizontal_top(|ui| {
        // Results table
        ui.vertical(|ui| {
            ui.set_width(table_width);
            if let Some(a) = render_results_table(ui, state, source) {
                action = Some(a);
            }
        });

        ui.separator();

        // Preview panel
        ui.vertical(|ui| {
            render_preview(ui, state, &mut action);
        });
    });

    action
}

fn render_results_table(
    ui: &mut egui::Ui,
    state: &mut SearchState,
    source: SearchSource,
) -> Option<SearchPanelAction> {
    use egui_extras::{Column, TableBuilder};

    if state.results.is_empty() {
        if state.is_searching {
            ui.colored_label(theme::TEXT_SECONDARY, "Searching...");
        } else if state.status_text.is_empty() {
            ui.colored_label(theme::TEXT_SECONDARY, "Enter a search query above");
        } else {
            ui.colored_label(theme::TEXT_SECONDARY, "No results");
        }
        return None;
    }

    let extra_header = match source {
        SearchSource::Idgames => "Rating/Date",
        SearchSource::Doomwiki => "Year/Port",
    };

    let mut clicked_row = None;

    let table = TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::remainder().at_least(100.0)) // Title
        .column(Column::exact(120.0)) // Author
        .column(Column::exact(100.0)); // Extra

    table
        .header(18.0, |mut header| {
            header.col(|ui| {
                ui.colored_label(theme::TEXT_ACCENT, "Title");
            });
            header.col(|ui| {
                ui.colored_label(theme::TEXT_ACCENT, "Author");
            });
            header.col(|ui| {
                ui.colored_label(theme::TEXT_ACCENT, extra_header);
            });
        })
        .body(|body| {
            body.rows(18.0, state.results.len(), |mut row| {
                let idx = row.index();
                let is_selected = state.selected_row == Some(idx);

                row.set_selected(is_selected);

                let entry = &state.results[idx];
                row.col(|ui| {
                    if ui.selectable_label(is_selected, &entry.title).clicked() {
                        clicked_row = Some(idx);
                    }
                });
                row.col(|ui| {
                    ui.colored_label(theme::TEXT_SECONDARY, entry.author.as_deref().unwrap_or(""));
                });
                row.col(|ui| {
                    ui.colored_label(theme::TEXT_SECONDARY, entry.extra_display());
                });
            });
        });

    if let Some(idx) = clicked_row {
        state.selected_row = Some(idx);
    }

    None
}

fn render_preview(ui: &mut egui::Ui, state: &SearchState, action: &mut Option<SearchPanelAction>) {
    let Some(idx) = state.selected_row else {
        ui.colored_label(theme::TEXT_SECONDARY, "Select a result to preview");
        return;
    };
    let Some(entry) = state.results.get(idx) else {
        return;
    };

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        // Title
        ui.colored_label(
            theme::TEXT_ACCENT,
            egui::RichText::new(&entry.title).heading().strong(),
        );

        // Author
        if let Some(author) = &entry.author {
            ui.colored_label(theme::TEXT_SECONDARY, format!("by {author}"));
        }

        // Source-specific metadata
        match &entry.source_data {
            SearchSourceData::Idgames {
                rating,
                date,
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
                    preview_row(ui, "Port", p);
                }
            }
        }

        ui.separator();

        // Description
        if let Some(desc) = &entry.description {
            let display = if desc.len() > 300 {
                let boundary = desc.floor_char_boundary(300);
                format!("{}...", &desc[..boundary])
            } else {
                desc.clone()
            };
            ui.colored_label(theme::TEXT_SECONDARY, display);
            ui.separator();
        }

        // Import button
        if ui.button("Import").clicked() {
            *action = Some(SearchPanelAction::Import(entry.clone()));
        }
    });
}

fn preview_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT_SECONDARY, format!("{label}:"));
        ui.colored_label(theme::TEXT_PRIMARY, value);
    });
}
