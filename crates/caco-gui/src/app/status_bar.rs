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
