//! Bottom status bar — play-state indicator + notification toast.

use crate::state::{AppState, PlayState};
use crate::theme;

pub(super) fn render_status_bar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // Play state indicator
        if let PlayState::Playing { wad_title, .. } = &state.play_state {
            ui.colored_label(theme::COLOR_SUCCESS, format!("Playing: {wad_title}..."));
            ui.separator();
        }

        // Uploads to a mirror, which outlive any one notification.
        if let Some(uploading) = uploading_label(&state.uploads) {
            ui.colored_label(theme::TEXT_SECONDARY, uploading);
            ui.separator();
        }

        // Notification
        if let Some(notif) = &state.notification {
            if notif.is_expired() {
                state.notification = None;
            } else {
                ui.colored_label(theme::severity_color(notif.severity), &notif.text);
            }
        }
    });
}

/// What the status bar says about uploads in flight, in the little room it has.
fn uploading_label(uploads: &[String]) -> Option<String> {
    match uploads {
        [] => None,
        [title] => Some(format!("Uploading: {title}...")),
        [title, rest @ ..] => Some(format!("Uploading: {title} and {} more...", rest.len())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uploads_in_flight_fit_on_one_line() {
        let titles =
            |names: &[&str]| -> Vec<String> { names.iter().map(|name| name.to_string()).collect() };
        assert_eq!(uploading_label(&[]), None);
        assert_eq!(
            uploading_label(&titles(&["Road"])).as_deref(),
            Some("Uploading: Road...")
        );
        assert_eq!(
            uploading_label(&titles(&["Road", "Purgatory", "Sunder"])).as_deref(),
            Some("Uploading: Road and 2 more...")
        );
    }
}
