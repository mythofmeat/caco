use crate::state::{AppState, SORT_FIELDS};
use crate::theme;

/// Render the sort controls, which read `Sort by [field] [direction]`.
///
/// The top bar lays these out from its right edge, so they are added in
/// reverse: the last one added ends up leftmost.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let field = SORT_FIELDS[state.sort_field_index].0;
    // There is no direction to a shuffle.
    if field != "random"
        && ui
            .button(direction_arrow(state.sort_desc))
            .on_hover_text(direction_hint(field, state.sort_desc))
            .clicked()
    {
        state.sort_desc = !state.sort_desc;
        state.needs_reload = true;
    }

    let current_label = SORT_FIELDS[state.sort_field_index].1;
    let combo = egui::ComboBox::from_id_salt("sort_field")
        .selected_text(current_label)
        .width(100.0)
        .show_ui(ui, |ui| {
            for (i, (_key, label)) in SORT_FIELDS.iter().enumerate() {
                if ui
                    .selectable_value(&mut state.sort_field_index, i, *label)
                    .changed()
                {
                    state.needs_reload = true;
                }
            }
        });

    let label = ui.colored_label(theme::TEXT_SECONDARY, "Sort by");
    combo.response.labelled_by(label.id);
}

/// A column's heading in the list, which sorts by that column: the first click the way a person
/// would want it, and the next the other way.
pub fn column_header(ui: &mut egui::Ui, state: &mut AppState, label: &str, field: &str) {
    let active = SORT_FIELDS[state.sort_field_index].0 == field;
    let text = if active {
        format!("{label} {}", direction_arrow(state.sort_desc))
    } else {
        label.to_string()
    };
    let color = if active {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
    };
    let hint = if active {
        direction_hint(field, !state.sort_desc)
    } else {
        direction_hint(field, true)
    };
    let response = ui
        .add(egui::Button::new(egui::RichText::new(text).size(12.0).color(color)).frame(false))
        .on_hover_text(format!("Sort: {hint}"));
    if response.clicked() {
        sort_by_column(state, field);
    }
}

/// What clicking on a column's heading does.
fn sort_by_column(state: &mut AppState, field: &str) {
    if SORT_FIELDS[state.sort_field_index].0 == field {
        state.sort_desc = !state.sort_desc;
    } else if let Some(index) = SORT_FIELDS.iter().position(|(key, _)| *key == field) {
        state.sort_field_index = index;
        state.sort_desc = true;
    }
    state.needs_reload = true;
}

fn direction_arrow(desc: bool) -> &'static str {
    if desc { "\u{2193}" } else { "\u{2191}" }
}

/// What the arrow means for this field. `search_wads` reads "descending" as the way a person
/// would want each one by default, so for a name that is A to Z.
fn direction_hint(field: &str, desc: bool) -> &'static str {
    match (field, desc) {
        ("title" | "author", true) => "A to Z",
        ("title" | "author", false) => "Z to A",
        ("last_played" | "created" | "year", true) => "Most recent first",
        ("last_played" | "created" | "year", false) => "Oldest first",
        (_, true) => "Highest first",
        (_, false) => "Lowest first",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved position used to point into this list, so a test that says what comes first
    /// is what stops it being an internal row id again.
    #[test]
    fn the_library_sorts_by_what_was_played_last_unless_told_otherwise() {
        assert_eq!(SORT_FIELDS[0].0, "last_played");
        assert!(SORT_FIELDS.iter().all(|(key, _)| *key != "id"));
    }

    #[test]
    fn clicking_a_heading_sorts_by_it_and_clicking_again_turns_it_around() {
        let mut state = AppState::new(std::path::PathBuf::from("test-library.db"));
        let field = |state: &AppState| SORT_FIELDS[state.sort_field_index].0;
        state.sort_desc = false;
        state.needs_reload = false;

        sort_by_column(&mut state, "title");
        assert_eq!(field(&state), "title");
        assert!(state.sort_desc, "the way a person would want it first");
        assert!(state.needs_reload);

        sort_by_column(&mut state, "title");
        assert!(!state.sort_desc);

        sort_by_column(&mut state, "playtime");
        assert_eq!(field(&state), "playtime");
        assert!(state.sort_desc);
    }

    #[test]
    fn the_arrow_says_which_way_each_field_goes() {
        assert_eq!(direction_hint("title", true), "A to Z");
        assert_eq!(direction_hint("last_played", false), "Oldest first");
        assert_eq!(direction_hint("rating", true), "Highest first");
    }
}
