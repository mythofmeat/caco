//! Bottom status bar — play-state indicator + notification toast.

use caco_core::db::Status;
use caco_core::downloads::page_host;
use std::time::Instant;

use crate::get::{Gets, PendingGet};
use crate::message::Severity;
use crate::state::{ActionRequest, AppState, PlayState, ViewMode};
use crate::theme;

pub(super) fn render_status_bar(
    ui: &mut egui::Ui,
    state: &mut AppState,
    actions: &mut Vec<ActionRequest>,
) {
    // From the right first, so that what is on the left gets what is left of the row.
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if state.view_mode == ViewMode::Library
            && let Some(shown) =
                shown_label(state.wads.len(), state.total_wad_count, state.is_filtered())
        {
            ui.colored_label(theme::TEXT_SECONDARY, shown);
            ui.add_space(8.0);
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            render_left(ui, state, actions);
        });
    });
}

fn render_left(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<ActionRequest>) {
    // Play state indicator
    if let PlayState::Playing {
        wad_title,
        downloading,
        ..
    } = &state.play_state
    {
        let doing = if *downloading {
            // Spinning is what says this one is getting somewhere. Not while playing, since
            // that would repaint for as long as the game is open, and the game wants the GPU.
            ui.spinner();
            "Downloading"
        } else {
            "Playing"
        };
        ui.colored_label(
            theme::status_color(Status::InProgress),
            format!("{doing} {wad_title}\u{2026}"),
        );
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

    render_notification(ui, state, actions);
}

/// The latest thing there is to say, and what to do about it. Dismissing it is always there
/// for what would otherwise stay until something replaced it.
fn render_notification(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<ActionRequest>) {
    let now = Instant::now();
    if state
        .notification
        .as_ref()
        .is_some_and(|n| n.is_expired(now))
    {
        state.notification = None;
    }
    let Some(notification) = &state.notification else {
        render_key_hints(ui, state);
        return;
    };
    // Nothing else repaints an idle window, so without this one would stay until the pointer
    // next moved.
    if let Some(left) = notification.remaining(now) {
        ui.ctx().request_repaint_after(left);
    }

    let mut dismissed = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let close = egui::RichText::new("\u{00d7}").color(theme::TEXT_SECONDARY);
        if ui
            .add(egui::Button::new(close).frame(false))
            .on_hover_text("Dismiss")
            .clicked()
        {
            dismissed = true;
        }
        if let Some(action) = notification.action
            && ui.small_button(action.label()).clicked()
        {
            actions.push(action.request());
            dismissed = true;
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            // Colour is never the only signal.
            let text = match notification.severity {
                Severity::Info => notification.text.clone(),
                Severity::Warning | Severity::Error => {
                    format!("\u{26a0} {}", notification.text)
                }
            };
            let color = theme::severity_color(notification.severity);
            ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate())
                .on_hover_text(&notification.text);
        });
    });
    if dismissed {
        state.notification = None;
    }
}

/// What there is to press, for when there is nothing else to say. Most of what caco can do
/// from the keyboard could only be found by opening Help.
fn render_key_hints(ui: &mut egui::Ui, state: &AppState) {
    if state.view_mode != ViewMode::Library || state.selected_wad_id.is_none() {
        return;
    }
    let details = if state.wide_window {
        "I Details"
    } else {
        "I More"
    };
    let hints = format!("Enter Play   E Edit   {details}   / Filter   ? Shortcuts");
    ui.add(
        egui::Label::new(
            egui::RichText::new(hints)
                .size(11.0)
                .color(theme::TEXT_MUTED),
        )
        .truncate()
        .selectable(false),
    );
}

/// How many WADs there are, and how many of them are showing when that is not all of them.
fn shown_label(shown: usize, total: usize, filtered: bool) -> Option<String> {
    match (total, filtered) {
        (0, _) => None,
        (total, true) => Some(format!(
            "{shown} of {}",
            crate::dialogs::count(total, "WAD", "WADs")
        )),
        (total, false) => Some(crate::dialogs::count(total, "WAD", "WADs")),
    }
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
    fn the_count_says_how_many_of_how_many_only_when_something_is_narrowing_it() {
        assert_eq!(shown_label(0, 0, false), None);
        assert_eq!(shown_label(1, 1, false).as_deref(), Some("1 WAD"));
        assert_eq!(shown_label(116, 116, false).as_deref(), Some("116 WADs"));
        assert_eq!(shown_label(3, 116, true).as_deref(), Some("3 of 116 WADs"));
        assert_eq!(shown_label(0, 116, true).as_deref(), Some("0 of 116 WADs"));
    }

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
