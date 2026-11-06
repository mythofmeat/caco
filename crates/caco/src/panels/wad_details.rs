//! What there is to know about the selected WAD, and what can be done with it.
//!
//! Selecting a WAD used to show nothing: its description, tags, awards and where its file was
//! could only be found by opening Edit, and most of what could be done with it only by a right
//! click. This goes beside the library where the window is wide enough for that, which is what
//! makes moving through the library with the keys a way to read about it, and in a dialog where
//! it is not, since the 800x400 minimum has no room for both.
//!
//! Nothing here is edited in place except what a person changes in passing: status and rating.

use caco_core::db::cacowards::CacowardRecord;
use caco_core::db::sessions::WadStats;
use caco_core::db::{Availability, Status, WadRecord};
use caco_core::utils::format_size;

use crate::get::GetTarget;
use crate::relative_time;
use crate::state::{ActionRequest, AppState, MapProgress};
use crate::theme;
use crate::thumbnails::ThumbnailManager;

/// How wide the panel is. With the sidebar, what leaves room for four cards at the default window.
pub const PANEL_WIDTH: f32 = 270.0;

/// How wide a window has to be for the panel to go beside the library.
pub const WIDE_WINDOW: f32 = 1040.0;

const MARGIN: f32 = 14.0;

/// The picture in the dialog, beside what matters most, since at 800x400 one the width of the
/// window would leave no room for anything else.
const DIALOG_PICTURE_WIDTH: f32 = 160.0;

const THUMB_ASPECT: f32 = 0.75;

/// As wide as the longest label, "Last played".
const LABEL_WIDTH: f32 = 66.0;

/// Whether the panel fits in this window.
pub fn fits(ctx: &egui::Context) -> bool {
    ctx.content_rect().width() >= WIDE_WINDOW
}

/// The panel beside the library.
pub fn render_panel(
    ui: &mut egui::Ui,
    state: &AppState,
    thumbnails: &ThumbnailManager,
) -> Option<ActionRequest> {
    let mut action = None;
    egui::ScrollArea::vertical()
        .id_salt("wad-details")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::same(MARGIN as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let Some(index) = state.selected_index() else {
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            ui.colored_label(
                                theme::TEXT_SECONDARY,
                                "Select a WAD to see more about it.",
                            );
                        });
                        return;
                    };
                    action = render_panel_contents(ui, &state.wads[index], state, thumbnails);
                });
        });
    action
}

/// Stacked, the picture across the top: the panel is narrow and tall.
fn render_panel_contents(
    ui: &mut egui::Ui,
    wad: &WadRecord,
    state: &AppState,
    thumbnails: &ThumbnailManager,
) -> Option<ActionRequest> {
    let mut action = None;
    ui.spacing_mut().item_spacing.y = 6.0;
    picture(ui, wad, thumbnails, ui.available_width());
    title_and_byline(ui, wad);
    ui.add_space(4.0);
    pick(&mut action, main_button(ui, wad, state));
    pick(&mut action, status_and_rating(ui, wad));
    pick(&mut action, the_rest(ui, wad, state));
    action
}

/// Side by side, what matters most beside the picture: the dialog is wide and, at the smallest
/// window, short. The dialog's title and the line under it are the WAD's, so they aren't here.
pub fn render_dialog_contents(
    ui: &mut egui::Ui,
    wad: &WadRecord,
    state: &AppState,
    thumbnails: &ThumbnailManager,
) -> Option<ActionRequest> {
    let mut action = None;
    ui.spacing_mut().item_spacing.y = 6.0;
    ui.horizontal_top(|ui| {
        picture(ui, wad, thumbnails, DIALOG_PICTURE_WIDTH);
        ui.vertical(|ui| {
            pick(&mut action, main_button(ui, wad, state));
            pick(&mut action, status_and_rating(ui, wad));
        });
    });
    pick(&mut action, the_rest(ui, wad, state));
    action
}

fn picture(ui: &mut egui::Ui, wad: &WadRecord, thumbnails: &ThumbnailManager, width: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, (width * THUMB_ASPECT).round()),
        egui::Sense::hover(),
    );
    theme::paint_thumbnail(
        ui.painter(),
        rect,
        egui::CornerRadius::same(8),
        thumbnails.get(wad.id),
    );
}

fn title_and_byline(ui: &mut egui::Ui, wad: &WadRecord) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(&wad.title)
                .size(17.0)
                .color(theme::TEXT_PRIMARY),
        )
        .wrap(),
    );
    if let Some(byline) = byline(wad) {
        ui.add(
            egui::Label::new(
                egui::RichText::new(byline)
                    .size(12.0)
                    .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    }
}

/// Everything below the first screenful, in the order of what is most worth knowing.
fn the_rest(ui: &mut egui::Ui, wad: &WadRecord, state: &AppState) -> Option<ActionRequest> {
    let mut action = None;
    let stats = state.stats_map.get(&wad.id);
    let progress = state.map_progress(wad);
    facts(ui, stats, progress.as_ref());

    ui.add_space(2.0);
    pick(&mut action, tool_buttons(ui, wad.id));

    let awards = state.details.as_ref().filter(|d| d.wad_id == wad.id);
    if let Some(details) = awards {
        pick(&mut action, award_chips(ui, &details.awards));
    }
    pick(&mut action, tag_chips(ui, &wad.tags));

    if let Some(notes) = text_of(&wad.notes) {
        section(ui, "Notes");
        body_text(ui, notes);
    }
    if let Some(description) = text_of(&wad.description) {
        section(ui, "About");
        body_text(ui, description);
    }
    setup(ui, wad);
    file(ui, wad);

    action
}

/// The first thing asked for wins: only one button can be pressed in a frame anyway.
fn pick(action: &mut Option<ActionRequest>, picked: Option<ActionRequest>) {
    if action.is_none() {
        *action = picked;
    }
}

/// Who made it and when, for the line under its title.
pub fn byline(wad: &WadRecord) -> Option<String> {
    let author = wad.author.as_deref().filter(|author| !author.is_empty());
    match (author, wad.year) {
        (Some(author), Some(year)) => Some(format!("{author} \u{00b7} {year}")),
        (Some(author), None) => Some(author.to_string()),
        (None, Some(year)) => Some(year.to_string()),
        (None, None) => None,
    }
}

fn text_of(text: &Option<String>) -> Option<&str> {
    text.as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

// ---------------------------------------------------------------------------
// What to do with it
// ---------------------------------------------------------------------------

/// What the big button is, which is whatever stands between this WAD and being played.
#[derive(Debug, PartialEq, Eq)]
enum MainAction {
    Play,
    /// There is no file here yet, and caco can fetch it.
    DownloadAndPlay,
    /// There is no file here, and only a browser can get one.
    GetInBrowser,
    /// A browser is already on it.
    WaitingForDownload,
    PlayingThis,
    /// Only one thing can run at a time.
    PlayingSomethingElse,
}

fn main_action(wad: &WadRecord, playing: Option<i64>, waiting: bool) -> MainAction {
    match playing {
        Some(id) if id == wad.id => MainAction::PlayingThis,
        Some(_) => MainAction::PlayingSomethingElse,
        None if wad.is_lost() && waiting => MainAction::WaitingForDownload,
        None if wad.is_lost() => MainAction::GetInBrowser,
        None if wad.availability() == Availability::Cached => MainAction::Play,
        None => MainAction::DownloadAndPlay,
    }
}

fn main_button(ui: &mut egui::Ui, wad: &WadRecord, state: &AppState) -> Option<ActionRequest> {
    let target = GetTarget::Wad(wad.id);
    let action = main_action(wad, state.playing_wad(), state.gets.is_waiting_for(target));
    // `add_sized` rather than a minimum size, which leaves the words at the left of a button
    // this wide in a layout that runs down the page.
    let wide = egui::vec2(ui.available_width(), 28.0);
    let filled = |text: &str| theme::primary_button(text);
    let quiet = |text: &str| egui::Button::new(text);
    match action {
        MainAction::Play => ui
            .add_sized(wide, filled("Play"))
            .on_hover_text("Enter")
            .clicked()
            .then_some(ActionRequest::Play(wad.id)),
        MainAction::DownloadAndPlay => ui
            .add_sized(wide, filled("Download & Play"))
            .on_hover_text("The file isn't on this machine yet, and caco can fetch it.")
            .clicked()
            .then_some(ActionRequest::Play(wad.id)),
        MainAction::GetInBrowser => {
            let mut action = ui
                .add_sized(wide, filled("Get in Browser"))
                .on_hover_text(
                    "Opens the page for it in your browser, and watches your Downloads folder \
                     for the file.",
                )
                .clicked()
                .then_some(ActionRequest::Get(target));
            if ui
                .add_sized(wide, quiet("Link a File You Have"))
                .on_hover_text("Asks where it is, then plays it.")
                .clicked()
            {
                action = Some(ActionRequest::Play(wad.id));
            }
            action
        }
        MainAction::WaitingForDownload => {
            ui.add_enabled_ui(false, |ui| {
                ui.add_sized(wide, quiet("Waiting for Download\u{2026}"));
            });
            None
        }
        MainAction::PlayingThis => {
            ui.add_enabled_ui(false, |ui| {
                ui.add_sized(wide, quiet("Playing\u{2026}"));
            });
            None
        }
        MainAction::PlayingSomethingElse => {
            ui.add_enabled_ui(false, |ui| {
                ui.add_sized(wide, quiet("Play"))
                    .on_disabled_hover_text("Something is already running.");
            });
            None
        }
    }
}

fn status_and_rating(ui: &mut egui::Ui, wad: &WadRecord) -> Option<ActionRequest> {
    let mut action = None;
    facts_grid("details-yours").show(ui, |ui| {
        let style = theme::status_style(wad.status);
        let label = ui.colored_label(theme::TEXT_SECONDARY, "Status");
        let combo = egui::ComboBox::from_id_salt("details-status")
            .selected_text(egui::RichText::new(style.word).color(style.color))
            .width(100.0)
            .show_ui(ui, |ui| {
                for &status in Status::ALL {
                    let style = theme::status_style(status);
                    let text = egui::RichText::new(style.word).color(style.color);
                    if ui.selectable_label(wad.status == status, text).clicked()
                        && wad.status != status
                    {
                        action = Some(ActionRequest::SetStatus(wad.id, status));
                    }
                }
            });
        combo.response.labelled_by(label.id);
        ui.end_row();

        ui.colored_label(theme::TEXT_SECONDARY, "Rating");
        if let Some(rating) = theme::rating_picker(ui, wad.rating, 18.0) {
            action = Some(ActionRequest::SetRating(wad.id, rating));
        }
        ui.end_row();
    });
    action
}

/// A button for a tool, with the key that does the same in its tooltip.
fn tool_button(ui: &mut egui::Ui, text: &str, key: &str, width: f32) -> bool {
    ui.add(egui::Button::new(text).min_size(egui::vec2(width, 0.0)))
        .on_hover_text(format!("Or press {key}"))
        .clicked()
}

fn tool_buttons(ui: &mut egui::Ui, wad_id: i64) -> Option<ActionRequest> {
    let mut action = None;
    let spacing = ui.spacing().item_spacing.x;
    let width = (ui.available_width() - spacing) / 2.0;
    ui.horizontal(|ui| {
        if tool_button(ui, "Sessions", "S", width) {
            action = Some(ActionRequest::Sessions(wad_id));
        }
        if tool_button(ui, "Map Stats", "M", width) {
            action = Some(ActionRequest::MapStats(wad_id));
        }
    });
    ui.horizontal(|ui| {
        if tool_button(ui, "Saves & Demos", "F", width) {
            action = Some(ActionRequest::WadData(wad_id));
        }
        if tool_button(ui, "Edit", "E", width) {
            action = Some(ActionRequest::Edit(wad_id));
        }
    });
    action
}

// ---------------------------------------------------------------------------
// What is known about it
// ---------------------------------------------------------------------------

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    theme::section_heading(ui, title);
}

fn body_text(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).color(theme::TEXT_PRIMARY))
            .wrap()
            .selectable(true),
    );
}

/// Labels and their values in two columns. A label's column is as wide as the widest of them, so
/// each of these has one of the same width to line up with the next.
fn facts_grid(id: &str) -> egui::Grid {
    egui::Grid::new(id)
        .num_columns(2)
        .min_col_width(LABEL_WIDTH)
        .spacing([10.0, 4.0])
        .striped(false)
}

/// A label and its value.
fn fact(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.colored_label(theme::TEXT_SECONDARY, label);
    ui.add(egui::Label::new(egui::RichText::new(value).color(theme::TEXT_PRIMARY)).wrap());
    ui.end_row();
}

fn facts(ui: &mut egui::Ui, stats: Option<&WadStats>, progress: Option<&MapProgress>) {
    if let Some(progress) = progress {
        ui.add_space(2.0);
        let (bar, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 5.0), egui::Sense::hover());
        theme::paint_progress(ui.painter(), bar, progress.fraction());
        ui.colored_label(
            theme::TEXT_SECONDARY,
            egui::RichText::new(progress.summary()).size(12.0),
        );
    }

    let lines = played_lines(stats);
    if lines.is_empty() {
        return;
    }
    ui.add_space(2.0);
    facts_grid("details-facts").show(ui, |ui| {
        for (label, value) in &lines {
            fact(ui, label, value);
        }
    });
}

/// What has happened with it so far, if anything has.
fn played_lines(stats: Option<&WadStats>) -> Vec<(&'static str, String)> {
    let mut lines = Vec::new();
    let Some(stats) = stats else {
        return lines;
    };
    if stats.playtime > 0 {
        let played = caco_core::player::format_duration(stats.playtime);
        lines.push((
            "Played",
            match stats.session_count {
                0 | 1 => played,
                n => format!("{played}, {n} sessions"),
            },
        ));
    }
    if let Some(when) = stats
        .last_played
        .as_deref()
        .and_then(relative_time::parse_timestamp)
    {
        lines.push(("Last played", relative_time::relative_time(&when)));
    }
    match stats.times_beaten {
        0 => {}
        1 => lines.push(("Finished", "Once".to_string())),
        2 => lines.push(("Finished", "Twice".to_string())),
        n => lines.push(("Finished", format!("{n} times"))),
    }
    lines
}

fn chip(ui: &mut egui::Ui, text: &str, color: egui::Color32, hint: &str) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(11.5).color(color))
            .fill(theme::BG_LIGHT)
            .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_MED))
            .corner_radius(10),
    )
    .on_hover_text(hint)
    .clicked()
}

fn award_chips(ui: &mut egui::Ui, awards: &[CacowardRecord]) -> Option<ActionRequest> {
    if awards.is_empty() {
        return None;
    }
    let mut action = None;
    section(ui, "Cacowards");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for award in awards {
            let label = crate::panels::cacowards::award_label(&award.category);
            let text = format!("{} {label}", award.year);
            let hint = format!("See the {} Cacowards", award.year);
            if chip(ui, &text, theme::GOLD, &hint) {
                action = Some(ActionRequest::ShowCacowards(award.year));
            }
        }
    });
    action
}

fn tag_chips(ui: &mut egui::Ui, tags: &[String]) -> Option<ActionRequest> {
    if tags.is_empty() {
        return None;
    }
    let mut action = None;
    section(ui, "Tags");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for tag in tags {
            if chip(ui, tag, theme::TEXT_TAG, "Show everything with this tag") {
                action = Some(ActionRequest::FilterLibrary(crate::panels::field_query(
                    "tag", tag,
                )));
            }
        }
    });
    action
}

/// What it takes to run, for a WAD where anything has been decided or worked out.
fn setup(ui: &mut egui::Ui, wad: &WadRecord) {
    let lines = setup_lines(wad);
    if lines.is_empty() {
        return;
    }
    section(ui, "Runs With");
    facts_grid("details-setup").show(ui, |ui| {
        for (label, value) in &lines {
            fact(ui, label, value);
        }
    });
}

fn setup_lines(wad: &WadRecord) -> Vec<(&'static str, String)> {
    let mut lines = Vec::new();
    if let Some(iwad) = text_of(&wad.custom_iwad) {
        lines.push(("IWAD", iwad.to_string()));
    }
    if wad.complevel.is_some() {
        let name = caco_core::complevel::complevel_name_string(wad.complevel);
        lines.push(("Complevel", name));
    }
    if let Some(sourceport) = text_of(&wad.custom_sourceport) {
        lines.push(("Sourceport", sourceport.to_string()));
    } else if wad.zdoom_required == Some(1) {
        lines.push(("Sourceport", "ZDoom family".to_string()));
    }
    if let Some(profile) = text_of(&wad.custom_config) {
        lines.push(("Profile", profile.to_string()));
    }
    lines
}

/// Where its file is, in a person's words. The path itself is on the edit dialog's Sources tab.
fn file_summary(wad: &WadRecord) -> (String, bool) {
    let size = wad
        .identity
        .as_ref()
        .map(|identity| format_size(identity.file.size));
    if wad.availability() == Availability::Cached {
        let text = match &size {
            Some(size) => format!("On this machine, {size}."),
            None => "On this machine.".to_string(),
        };
        return (text, false);
    }
    if wad.retrievability().is_automatic() {
        return (
            "Not here yet. Downloads when you play it.".to_string(),
            false,
        );
    }
    let folder = text_of(&wad.last_known_path)
        .and_then(|path| std::path::Path::new(path).parent())
        .map(|parent| crate::dialogs::short_path(&parent.display().to_string()));
    match folder {
        Some(folder) => (format!("Not on this machine. Last seen in {folder}."), true),
        None => (
            "Not on this machine, and not on a mirror or idgames.".to_string(),
            true,
        ),
    }
}

fn file(ui: &mut egui::Ui, wad: &WadRecord) {
    section(ui, "File");
    let (summary, trouble) = file_summary(wad);
    let color = if trouble {
        theme::COLOR_WARNING
    } else {
        theme::TEXT_PRIMARY
    };
    ui.add(egui::Label::new(egui::RichText::new(summary).color(color)).wrap());

    let pages = caco_core::downloads::pages_for_wad(wad);
    if let Some(page) = pages.first() {
        let host = caco_core::downloads::page_host(page);
        let text = format!("Open {host} \u{2197}");
        if ui
            .add(egui::Button::new(text).frame(false))
            .on_hover_text(page)
            .clicked()
        {
            // Nothing to do about a machine with no browser to hand this to.
            let _ = open::that(page);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{self, NewWad, SourceType, WadUpdate};
    use rusqlite::Connection;

    fn wad(conn: &Connection, source: SourceType, update: WadUpdate) -> WadRecord {
        let id = db::add_wad(conn, &NewWad::new("A WAD", source)).unwrap();
        db::update_wad(conn, id, &update).unwrap();
        db::get_wad(conn, id, false).unwrap().unwrap()
    }

    fn memory() -> Connection {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        conn
    }

    fn with_file() -> WadUpdate {
        WadUpdate::new().set_text("cached_path", Some("/keep/a.wad".to_string()))
    }

    #[test]
    fn the_big_button_is_whatever_stands_between_a_wad_and_being_played() {
        let conn = memory();

        let here = wad(&conn, SourceType::Local, with_file());
        assert_eq!(main_action(&here, None, false), MainAction::Play);

        let fetchable = wad(
            &conn,
            SourceType::Idgames,
            WadUpdate::new().set_text("idgames_id", Some("42".to_string())),
        );
        assert_eq!(
            main_action(&fetchable, None, false),
            MainAction::DownloadAndPlay
        );

        let lost = wad(&conn, SourceType::Doomworld, WadUpdate::new());
        assert_eq!(main_action(&lost, None, false), MainAction::GetInBrowser);
        assert_eq!(
            main_action(&lost, None, true),
            MainAction::WaitingForDownload
        );
    }

    #[test]
    fn only_one_thing_runs_at_a_time() {
        let conn = memory();
        let here = wad(&conn, SourceType::Local, with_file());
        assert_eq!(
            main_action(&here, Some(here.id), false),
            MainAction::PlayingThis
        );
        assert_eq!(
            main_action(&here, Some(here.id + 1), false),
            MainAction::PlayingSomethingElse
        );
    }

    #[test]
    fn a_file_is_described_by_where_it_is_and_only_trouble_reads_as_trouble() {
        let conn = memory();

        let (text, trouble) = file_summary(&wad(&conn, SourceType::Local, with_file()));
        assert_eq!(text, "On this machine.");
        assert!(!trouble);

        let fetchable = wad(
            &conn,
            SourceType::Idgames,
            WadUpdate::new().set_text("idgames_id", Some("42".to_string())),
        );
        let (text, trouble) = file_summary(&fetchable);
        assert!(text.contains("Downloads when you play"), "{text}");
        assert!(!trouble);

        let (text, trouble) = file_summary(&wad(&conn, SourceType::Doomworld, WadUpdate::new()));
        assert!(text.contains("not on a mirror or idgames"), "{text}");
        assert!(trouble);
    }

    #[test]
    fn a_wad_nothing_has_happened_to_has_nothing_to_say_about_it() {
        assert_eq!(played_lines(None), Vec::new());
        let stats = WadStats {
            playtime: 0,
            last_played: None,
            session_count: 0,
            times_beaten: 0,
        };
        assert_eq!(played_lines(Some(&stats)), Vec::new());
    }

    #[test]
    fn what_has_happened_reads_as_a_person_would_say_it() {
        let stats = WadStats {
            playtime: 3 * 3600,
            last_played: None,
            session_count: 2,
            times_beaten: 2,
        };
        assert_eq!(
            played_lines(Some(&stats)),
            vec![
                ("Played", "3h 0m, 2 sessions".to_string()),
                ("Finished", "Twice".to_string()),
            ]
        );
    }

    #[test]
    fn a_wad_says_nothing_about_a_setup_nobody_chose() {
        let conn = memory();
        let plain = wad(&conn, SourceType::Local, WadUpdate::new());
        assert_eq!(setup_lines(&plain), Vec::new());
        let chosen = wad(
            &conn,
            SourceType::Local,
            WadUpdate::new()
                .set_text("custom_iwad", Some("doom2".to_string()))
                .set_int("complevel", Some(21)),
        );
        let lines = setup_lines(&chosen);
        assert_eq!(lines[0], ("IWAD", "doom2".to_string()));
        assert_eq!(lines[1].0, "Complevel");
        assert!(lines[1].1.contains("MBF21"), "{:?}", lines[1]);
    }
}
