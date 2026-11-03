//! Asked before launching a WAD whose file plays differently from the one it
//! was last played with.
//!
//! Saves and demos made with one version of a WAD often do not load in
//! another, and the usual way to arrive here is an upstream fix downloaded in
//! place of the copy the user had been playing. Whether to go ahead is the
//! user's call. `caco_core::player::play` refuses rather than deciding, and
//! this is where the question is put. Going ahead records the new file on the
//! session it starts, so the same file is not asked about again.

use caco_core::identity::ContentChange;

use crate::theme;

pub struct FileChangedDialogState {
    wad_id: i64,
    wad_title: String,
    change: ContentChange,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FileChangedResult {
    Open,
    PlayAnyway(i64),
    Cancelled,
}

impl FileChangedDialogState {
    pub fn new(wad_id: i64, wad_title: String, change: ContentChange) -> Self {
        Self {
            wad_id,
            wad_title,
            change,
        }
    }

    pub fn render(&self, ctx: &egui::Context) -> FileChangedResult {
        let mut result = FileChangedResult::Open;

        crate::dialogs::modal_window(ctx, "File Changed", [440.0, 230.0]).show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            ui.colored_label(
                theme::TEXT_PRIMARY,
                egui::RichText::new(&self.wad_title).heading(),
            );
            ui.add_space(4.0);
            ui.colored_label(theme::TEXT_SECONDARY, explanation(&self.change));
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "Saves and demos from before may not work with it.",
            );
            ui.label(
                egui::RichText::new(format!(
                    "played {}  \u{00b7}  now {}",
                    short_hash(&self.change.played),
                    short_hash(&self.change.current)
                ))
                .monospace()
                .small()
                .color(theme::TEXT_SECONDARY),
            );

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if ui.button("Play Anyway").clicked() {
                    result = FileChangedResult::PlayAnyway(self.wad_id);
                }
                if ui.button("Cancel").clicked() {
                    result = FileChangedResult::Cancelled;
                }
            });
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return FileChangedResult::Cancelled;
        }
        result
    }
}

/// "This WAD's file has changed since you last played it, 3 days ago
/// (2026-10-31)." The timestamp is shown raw if it does not parse, rather than
/// dropping the one fact that says how old the saves at risk are.
fn explanation(change: &ContentChange) -> String {
    let when = chrono::DateTime::parse_from_rfc3339(&change.last_played_at)
        .map(|at| {
            crate::relative_time::relative_time_full(
                &at.with_timezone(&chrono::Local).naive_local(),
            )
        })
        .unwrap_or_else(|_| change.last_played_at.clone());
    format!("This WAD's file has changed since you last played it, {when}.")
}

/// Enough of a hash to tell two apart at a glance. The full ones are in the
/// edit dialog's Sources tab.
fn short_hash(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(last_played_at: &str) -> ContentChange {
        ContentChange {
            last_played_at: last_played_at.to_string(),
            played: "3f2a9c1e8b7d6a5f4e3d2c1b0a998877".to_string(),
            current: "9b1c44d0aa".to_string(),
        }
    }

    #[test]
    fn test_explanation_dates_the_last_session() {
        // Midday UTC, so the local date has the same year in any timezone.
        let text = explanation(&change("2024-06-15T12:00:00+00:00"));
        assert!(text.starts_with("This WAD's file has changed since you last played it, "));
        assert!(text.contains(" ago (2024-06-"), "{text}");
        assert!(text.ends_with(")."), "{text}");
    }

    #[test]
    fn test_explanation_keeps_a_timestamp_it_cannot_parse() {
        assert_eq!(
            explanation(&change("yesterday")),
            "This WAD's file has changed since you last played it, yesterday."
        );
    }

    #[test]
    fn test_short_hash() {
        assert_eq!(short_hash("3f2a9c1e8b7d6a5f4e3d"), "3f2a9c1e8b7d");
        assert_eq!(short_hash("9b1c"), "9b1c");
    }
}
