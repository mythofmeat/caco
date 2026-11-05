use crate::state::{AppState, SORT_FIELDS};
use crate::theme;

/// Render the sort controls, which read `Sort by [field] [direction]`.
///
/// The top bar lays these out from its right edge, so they are added in
/// reverse: the last one added ends up leftmost.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let field = SORT_FIELDS[state.sort_field_index].0;
    // There is no direction to a shuffle.
    if field != "random" {
        let arrow = if state.sort_desc {
            "\u{2193}"
        } else {
            "\u{2191}"
        };
        if ui
            .button(arrow)
            .on_hover_text(direction_hint(field, state.sort_desc))
            .clicked()
        {
            state.sort_desc = !state.sort_desc;
            state.needs_reload = true;
        }
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
    fn the_arrow_says_which_way_each_field_goes() {
        assert_eq!(direction_hint("title", true), "A to Z");
        assert_eq!(direction_hint("last_played", false), "Oldest first");
        assert_eq!(direction_hint("rating", true), "Highest first");
    }
}
