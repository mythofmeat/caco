//! Bottom status bar — play-state indicator + notification toast.

use caco_core::downloads::page_host;

use crate::get::{Gets, PendingGet};
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
        if let Some(bulk) = &state.mirror_bulk {
            ui.colored_label(
                theme::TEXT_SECONDARY,
                format!(
                    "Uploading to {}: {} of {}...",
                    bulk.mirror, bulk.done, bulk.total
                ),
            );
            ui.separator();
        }
        if let Some(uploading) = in_flight_label("Uploading", &state.uploads) {
            ui.colored_label(theme::TEXT_SECONDARY, uploading);
            ui.separator();
        }

        // Files being got in a browser, for as long as they are out there.
        render_waiting(ui, &mut state.gets);
        if let Some(linking) = in_flight_label("Linking", state.gets.linking()) {
            ui.colored_label(theme::TEXT_SECONDARY, linking);
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

/// What's waiting for a download, with what can be done about it behind a click: every
/// page known for it, because the one that opened can turn out to be a dead share link,
/// and a way to stop.
fn render_waiting(ui: &mut egui::Ui, gets: &mut Gets) {
    let Some(label) = waiting_label(gets.waiting()) else {
        return;
    };
    let mut open = None;
    let mut stop = None;
    ui.menu_button(
        egui::RichText::new(label).color(theme::TEXT_SECONDARY),
        |ui| {
            egui::ScrollArea::vertical()
                .max_height(WAITING_MENU_HEIGHT)
                .show(ui, |ui| {
                    for get in gets.waiting() {
                        ui.colored_label(
                            theme::TEXT_PRIMARY,
                            egui::RichText::new(&get.title).strong(),
                        );
                        for page in &get.pages {
                            let button = ui
                                .button(format!("Open {}", page_host(page)))
                                .on_hover_text(page);
                            if button.clicked() {
                                open = Some(page.clone());
                                ui.close();
                            }
                        }
                        if ui.button("Stop Waiting").clicked() {
                            stop = Some(get.target);
                            ui.close();
                        }
                        ui.add_space(4.0);
                    }
                });
        },
    );
    ui.separator();
    if let Some(url) = open {
        gets.open_page(url);
    }
    if let Some(target) = stop {
        gets.stop(target);
    }
}

/// Short enough to fit above the status bar in the smallest window.
const WAITING_MENU_HEIGHT: f32 = 240.0;

/// As much of a title as the status bar has room for, with something else beside it.
const WAITING_TITLE_CHARS: usize = 40;

/// What the status bar says about what is waiting for a download.
fn waiting_label(waiting: &[PendingGet]) -> Option<String> {
    match waiting {
        [] => None,
        [get] => {
            let title: String = get.title.chars().take(WAITING_TITLE_CHARS).collect();
            Some(format!("Waiting for download: {title}..."))
        }
        many => Some(format!("Waiting for {} downloads...", many.len())),
    }
}

/// What the status bar says about work in flight, in the little room it has.
fn in_flight_label(doing: &str, titles: &[String]) -> Option<String> {
    match titles {
        [] => None,
        [title] => Some(format!("{doing}: {title}...")),
        [title, rest @ ..] => Some(format!("{doing}: {title} and {} more...", rest.len())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_what_is_waiting_fits_on_one_line() {
        let bg = crate::workers::BackgroundChannel::new();
        let label_after_getting = |titles: &[String]| {
            let mut gets = Gets::for_tests();
            for (id, title) in titles.iter().enumerate() {
                gets.arm(
                    &bg.sender(),
                    crate::get::GetTarget::Wad(id as i64),
                    title.clone(),
                    None,
                    Vec::new(),
                    std::time::Instant::now(),
                );
            }
            waiting_label(gets.waiting())
        };

        assert_eq!(label_after_getting(&[]), None);
        assert_eq!(
            label_after_getting(&["Road".to_string()]).as_deref(),
            Some("Waiting for download: Road...")
        );
        assert_eq!(
            label_after_getting(&["Road".to_string(), "Sunder".to_string()]).as_deref(),
            Some("Waiting for 2 downloads...")
        );
        assert_eq!(
            label_after_getting(&["A".repeat(200)]).unwrap(),
            format!(
                "Waiting for download: {}...",
                "A".repeat(WAITING_TITLE_CHARS)
            )
        );
    }

    #[test]
    fn test_work_in_flight_fits_on_one_line() {
        let titles =
            |names: &[&str]| -> Vec<String> { names.iter().map(|name| name.to_string()).collect() };
        assert_eq!(in_flight_label("Uploading", &[]), None);
        assert_eq!(
            in_flight_label("Uploading", &titles(&["Road"])).as_deref(),
            Some("Uploading: Road...")
        );
        assert_eq!(
            in_flight_label("Linking", &titles(&["Road", "Purgatory", "Sunder"])).as_deref(),
            Some("Linking: Road and 2 more...")
        );
    }
}
